import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { resolve } from 'node:path'

// Compare actual entry adapters. No scientific or persistence behavior is implied.
execFileSync('cargo', ['build', '--workspace', '--locked'], { stdio: 'inherit' })
const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], {
    encoding: 'utf8',
  }),
)
const suffix = process.platform === 'win32' ? '.exe' : ''
const apiPath = resolve(metadata.target_directory, 'debug', `chemifree-api${suffix}`)
const cliPath = resolve(metadata.target_directory, 'debug', `chemifree${suffix}`)
const api = spawn(apiPath, ['--port', '0'], { stdio: ['ignore', 'ignore', 'pipe'] })
let log = ''

try {
  const baseUrl = await new Promise((resolveUrl, reject) => {
    const timer = setTimeout(() => reject(new Error(`API startup timed out. ${log}`)), 15_000)
    api.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    api.once('exit', (code) => {
      clearTimeout(timer)
      reject(new Error(`API exited before startup (${code}). ${log}`))
    })
    api.stderr.on('data', (data) => {
      log += data.toString()
      const match = log.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) {
        clearTimeout(timer)
        resolveUrl(match[1])
      }
    })
  })

  for (const [endpoint, command] of [
    ['health', 'status'],
    ['capabilities', 'capabilities'],
  ]) {
    const response = await fetch(`${baseUrl}/api/v1/${endpoint}`, {
      signal: AbortSignal.timeout(5_000),
    })
    assert.equal(response.status, 200)
    assert.match(response.headers.get('content-type'), /application\/json/)
    const http = await response.json()
    const cli = JSON.parse(execFileSync(cliPath, [command, '--json'], { encoding: 'utf8' }))
    assert.deepEqual(http, cli, `HTTP ${endpoint} and CLI ${command} must share a contract`)
    assert.equal(http.api_version, 'v1')

    if (endpoint === 'health') {
      assert.equal(http.status, 'ok')
      assert.equal(http.stage, 'scaffold')
    } else {
      assert.deepEqual(
        http.capabilities.filter((item) => item.status === 'available').map((item) => item.id),
        ['runtime_inspection'],
      )
      assert.equal(http.capabilities.length, 8)
      assert.equal(new Set(http.capabilities.map((item) => item.id)).size, 8)
      assert.ok(http.capabilities.every((item) => ['available', 'not_implemented'].includes(item.status)))
    }
  }

  for (const [path, method, status, code] of [
    ['/api/v1/capabilities', 'POST', 405, 'method_not_allowed'],
    ['/api/v1/documents', 'GET', 404, 'not_found'],
    ['/api/v2/health', 'GET', 404, 'not_found'],
  ]) {
    const response = await fetch(`${baseUrl}${path}`, {
      method,
      signal: AbortSignal.timeout(5_000),
    })
    assert.equal(response.status, status)
    const body = await response.json()
    assert.equal(body.code, code)
    assert.equal(body.api_version, 'v1')
  }

  const crossOrigin = await fetch(`${baseUrl}/api/v1/health`, {
    headers: { Origin: 'https://example.com' },
    signal: AbortSignal.timeout(5_000),
  })
  assert.equal(crossOrigin.headers.get('access-control-allow-origin'), null)
  console.log('Contract smoke passed: HTTP = CLI; scaffold capabilities, versioning, and read-only routes verified.')
} finally {
  if (api.pid && api.exitCode === null) {
    const stopped = once(api, 'exit')
    api.kill('SIGTERM')
    const force = setTimeout(() => api.kill('SIGKILL'), 5_000)
    await stopped
    clearTimeout(force)
  }
}
