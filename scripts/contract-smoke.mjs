import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { once } from 'node:events'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

// Compare the real entry adapters over a throwaway data directory. Document editing and
// local persistence are exercised; no chemistry perception or analysis is implied.
execFileSync('cargo', ['build', '--workspace', '--locked'], { stdio: 'inherit' })
const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], {
    encoding: 'utf8',
  }),
)
const suffix = process.platform === 'win32' ? '.exe' : ''
const apiPath = resolve(metadata.target_directory, 'debug', `chemifree-api${suffix}`)
const cliPath = resolve(metadata.target_directory, 'debug', `chemifree${suffix}`)
const dataDir = mkdtempSync(join(tmpdir(), 'chemifree-contract-'))
const env = { ...process.env, CHEMIFREE_DATA_DIR: dataDir }
const running = new Set()

async function startApi() {
  const child = spawn(apiPath, ['--port', '0'], { env, stdio: ['ignore', 'ignore', 'pipe'] })
  running.add(child)
  let log = ''
  const baseUrl = await new Promise((resolveUrl, reject) => {
    const timer = setTimeout(() => reject(new Error(`API startup timed out. ${log}`)), 15_000)
    child.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    child.once('exit', (code) => {
      clearTimeout(timer)
      reject(new Error(`API exited before startup (${code}). ${log}`))
    })
    child.stderr.on('data', (data) => {
      log += data.toString()
      const match = log.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) {
        clearTimeout(timer)
        resolveUrl(match[1])
      }
    })
  })
  return { child, baseUrl }
}

async function stop(child, signal = 'SIGTERM') {
  if (child.pid && child.exitCode === null && child.signalCode === null) {
    const stopped = once(child, 'exit')
    child.kill(signal)
    const force = setTimeout(() => child.kill('SIGKILL'), 5_000)
    await stopped
    clearTimeout(force)
  }
  running.delete(child)
}

function cli(...args) {
  const result = spawnSync(cliPath, args, { env, encoding: 'utf8' })
  return { status: result.status, body: JSON.parse(result.stdout) }
}

let api
try {
  api = await startApi()
  const { baseUrl } = api
  const origin = new URL(baseUrl).origin

  async function call(path, { method = 'GET', body, headers = {} } = {}) {
    const response = await fetch(`${baseUrl}${path}`, {
      method,
      headers: body === undefined ? headers : { 'content-type': 'application/json', ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(5_000),
    })
    assert.match(response.headers.get('content-type'), /application\/json/)
    assert.equal(response.headers.get('access-control-allow-origin'), null)
    const json = await response.json()
    assert.equal(json.api_version, 'v1')
    return { status: response.status, body: json }
  }

  // Inspection parity.
  for (const [endpoint, command] of [
    ['health', 'status'],
    ['capabilities', 'capabilities'],
  ]) {
    const http = await call(`/api/v1/${endpoint}`)
    assert.equal(http.status, 200)
    assert.deepEqual(http.body, cli(command, '--json').body, `HTTP ${endpoint} = CLI ${command}`)
  }
  const capabilities = (await call('/api/v1/capabilities')).body.capabilities
  assert.deepEqual(
    capabilities.filter((item) => item.status === 'available').map((item) => item.id),
    ['runtime_inspection', 'document_editing', 'persistence'],
  )
  assert.equal(new Set(capabilities.map((item) => item.id)).size, 8)

  // Write protection.
  const { token, header } = (await call('/api/v1/session')).body
  assert.equal(header, 'x-chemifree-session')
  assert.match(token, /^[0-9a-f]{64}$/)
  const auth = { [header]: token, origin }
  const unauthenticated = await call('/api/v1/documents', { method: 'POST', body: { request_id: 'c0' } })
  assert.deepEqual([unauthenticated.status, unauthenticated.body.code], [401, 'unauthorized'])
  const crossSite = await call('/api/v1/documents', {
    method: 'POST',
    body: { request_id: 'c0' },
    headers: { ...auth, origin: 'https://example.com' },
  })
  assert.deepEqual([crossSite.status, crossSite.body.code], [403, 'forbidden_origin'])
  const tokenLeak = await call('/api/v1/session', { headers: { origin: 'https://example.com' } })
  assert.equal(tokenLeak.status, 403)
  assert.deepEqual((await call('/api/v1/documents')).body.documents, [])

  // Create → edit → undo/redo → save → close → reopen over HTTP.
  const write = (path, body) => call(path, { method: 'POST', body, headers: auth })
  const created = await write('/api/v1/documents', { request_id: 'create-1', title: 'Contract ethanol' })
  assert.equal(created.status, 201)
  const id = created.body.document.id
  const doc = `/api/v1/documents/${id}`
  const draw = {
    request_id: 'edit-1',
    expected_revision: 0,
    commands: [
      { op: 'add_atom', element: 'C', position: { x: 0, y: 0 } },
      { op: 'add_atom', element: 'C', position: { x: 26, y: 15 }, bond_to: { atom: 'a1', order: 'single' } },
      { op: 'add_atom', element: 'O', position: { x: 52, y: 0 }, bond_to: { atom: 'a2', order: 'single' } },
    ],
  }
  const edited = await write(`${doc}/transactions`, draw)
  assert.equal(edited.status, 200)
  assert.equal(edited.body.document.revision, 1)
  const replayed = await write(`${doc}/transactions`, draw)
  assert.deepEqual(replayed.body, edited.body, 'a retried request_id replays its result')
  const reused = await write(`${doc}/transactions`, { ...draw, commands: draw.commands.slice(0, 1) })
  assert.deepEqual([reused.status, reused.body.code], [409, 'request_id_reused'])
  const stale = await write(`${doc}/transactions`, { ...draw, request_id: 'edit-stale' })
  assert.deepEqual([stale.status, stale.body.code, stale.body.current_revision], [409, 'revision_conflict', 1])
  const invalid = await write(`${doc}/transactions`, {
    request_id: 'edit-bad',
    expected_revision: 1,
    commands: [{ op: 'add_bond', begin: 'a1', end: 'a2', order: 'double' }],
  })
  assert.deepEqual([invalid.status, invalid.body.code, invalid.body.command_index], [422, 'invalid_command', 0])

  const changed = await write(`${doc}/transactions`, {
    request_id: 'edit-2',
    expected_revision: 1,
    commands: [
      { op: 'update_bond', bond: 'b2', order: 'double' },
      { op: 'update_atom', atom: 'a3', charge: -1, isotope: 18 },
    ],
  })
  assert.equal(changed.body.document.revision, 2)
  const undone = await write(`${doc}/undo`, { request_id: 'undo-1', expected_revision: 2 })
  assert.deepEqual(undone.body.document.atoms, edited.body.document.atoms)
  assert.deepEqual(undone.body.document.bonds, edited.body.document.bonds)
  const redone = await write(`${doc}/redo`, { request_id: 'redo-1', expected_revision: 3 })
  assert.deepEqual(redone.body.document.atoms, changed.body.document.atoms)
  const unsavedClose = await write(`${doc}/close`, { request_id: 'close-0' })
  assert.deepEqual([unsavedClose.status, unsavedClose.body.code], [409, 'unsaved_changes'])

  // Save failure: make the target path a directory so the atomic replace cannot commit.
  const file = join(dataDir, 'documents', `${id}.json`)
  const committed = readFileSync(file)
  rmSync(file)
  mkdirSync(file)
  const failed = await write(`${doc}/save`, { request_id: 'save-1', expected_revision: 4 })
  assert.deepEqual([failed.status, failed.body.code], [503, 'storage_failed'])
  assert.equal((await call(doc)).body.session.unsaved_changes, true)
  rmSync(file, { recursive: true })
  writeFileSync(file, committed)
  const saved = await write(`${doc}/save`, { request_id: 'save-1', expected_revision: 4 })
  assert.equal(saved.status, 200)
  assert.deepEqual(saved.body.session, { saved_revision: 4, unsaved_changes: false, undo_depth: 2, redo_depth: 0 })
  const closed = await write(`${doc}/close`, { request_id: 'close-1' })
  assert.deepEqual(closed.body, { api_version: 'v1', id, discarded_changes: false })
  const reopened = await call(doc)
  assert.deepEqual(reopened.body, saved.body, 'reopening returns the committed document')

  // CLI sees the same committed document and edits it through the same application.
  assert.deepEqual(cli('documents', 'show', id, '--json').body, saved.body, 'HTTP GET = CLI show')
  const listed = (await call('/api/v1/documents')).body.documents
  assert.deepEqual(cli('documents', 'list', '--json').body.documents, listed.map((item) => ({ ...item, open: false })))

  // A concurrent CLI save makes the HTTP session's later save a conflict, not a lost update.
  await write(`${doc}/transactions`, {
    request_id: 'edit-3',
    expected_revision: 4,
    commands: [{ op: 'rename', title: 'HTTP title' }],
  })
  const cliEdit = cli('documents', 'apply', id, '--expected-revision', '4', '--json', '--commands',
    JSON.stringify([{ op: 'move_atoms', atoms: ['a1'], delta: { x: 5, y: 5 } }]))
  assert.equal(cliEdit.status, 0)
  assert.equal(cliEdit.body.document.revision, 5)
  assert.equal(cliEdit.body.session.unsaved_changes, false)
  const cliStale = cli('documents', 'undo', id, '--expected-revision', '4', '--json')
  assert.deepEqual([cliStale.status, cliStale.body.code], [1, 'revision_conflict'])
  const conflict = await write(`${doc}/save`, { request_id: 'save-2', expected_revision: 5 })
  assert.deepEqual([conflict.status, conflict.body.code], [409, 'save_conflict'])
  const discarded = await write(`${doc}/close`, { request_id: 'close-2', discard_unsaved: true })
  assert.equal(discarded.body.discarded_changes, true)
  assert.deepEqual((await call(doc)).body.document, cliEdit.body.document)

  // Restart recovery: unsaved work in a killed process is gone; the last commit survives.
  await write(`${doc}/transactions`, {
    request_id: 'edit-4',
    expected_revision: 5,
    commands: [{ op: 'delete_atoms', atoms: ['a1', 'a2', 'a3'] }],
  })
  writeFileSync(join(dataDir, 'documents', `.${id}.99999.0.tmp`), '{"partial":')
  await stop(api.child, 'SIGKILL')
  api = await startApi()
  const recovered = await fetch(`${api.baseUrl}${doc}`).then((response) => response.json())
  assert.deepEqual(recovered.document, cliEdit.body.document)
  assert.equal(recovered.session.unsaved_changes, false)

  for (const [path, method, status, code] of [
    ['/api/v1/capabilities', 'POST', 405, 'method_not_allowed'],
    ['/api/v1/unknown', 'GET', 404, 'not_found'],
    ['/api/v2/health', 'GET', 404, 'not_found'],
  ]) {
    const response = await fetch(`${api.baseUrl}${path}`, { method, signal: AbortSignal.timeout(5_000) })
    assert.equal(response.status, status)
    const body = await response.json()
    assert.deepEqual([body.code, body.api_version], [code, 'v1'])
  }

  console.log(
    'Contract smoke passed: HTTP = CLI; document create/edit/undo/redo/save/reopen, write guards, ' +
      'revision and save conflicts, save failure, and restart recovery verified.',
  )
} finally {
  for (const child of running) await stop(child)
  rmSync(dataDir, { recursive: true, force: true })
}
