// Real File/Download → production panel/hook/client → real application/domain.
// File reads and two dropped HTTP replies are delayed only at their I/O boundaries.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'
import { keepRequestInterception } from './lib/request-interception.mjs'

const root = fileURLToPath(new URL('../apps/web', import.meta.url))
const work = mkdtempSync(join(tmpdir(), 'chemifree-user-template-'))
let api
let apiLog = ''
let server
let browser
let page
let stage = 'build'
let passed = false

function fixture(ids) {
  return `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { UserTemplatePanel } from '/src/components/user-template-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
const ids = ${JSON.stringify(ids)}
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open(ids.source) }, [])
  return <main>
    <nav aria-label="Test documents">{Object.entries(ids).map(([name, id]) => <button key={id} disabled={editor.busy} onClick={() => void editor.open(id)}>Open {name}</button>)}</nav>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a1', 'a2', 'a3']))}>Select complete source</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a1']))}>Select crossing source</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a1', 's1']))}>Select mixed source</button>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo edit</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo edit</button>
    <button disabled={editor.busy} onClick={() => void editor.save()}>Save drawing</button>
    <button disabled={editor.busy} onClick={() => void editor.reload()}>Reload drawing</button>
    <output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    <output aria-label="Current snapshot">{JSON.stringify(editor.current)}</output>
    {editor.current && <UserTemplatePanel key={editor.current.document.id + '-' + editor.serviceGeneration} document={editor.current.document} editor={editor} activePlacement={editor.current.document.id === ids.source ? 's2' : 's1'} />}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
}

async function startApi(path, port = 0) {
  const env = { ...process.env, CHEMIFREE_DATA_DIR: work }
  delete env.CHEMIFREE_MODEL_DATA_ROOT
  delete env.CHEMIFREE_MODEL_TRUSTED_REGISTRY
  apiLog = ''
  api = spawn(path, ['--port', String(port)], { env, stdio: ['ignore', 'ignore', 'pipe'] })
  return new Promise((ready, reject) => {
    const timer = setTimeout(() => reject(new Error(`API startup timed out: ${apiLog}`)), 15_000)
    api.once('error', (error) => { clearTimeout(timer); reject(error) })
    api.once('exit', (code) => { clearTimeout(timer); reject(new Error(`API exited (${code}): ${apiLog}`)) })
    api.stderr.on('data', (data) => {
      apiLog += data.toString()
      const match = apiLog.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) { clearTimeout(timer); ready(match[1]) }
    })
  })
}

async function stopApi() {
  if (!api?.pid || api.exitCode !== null || api.signalCode !== null) return
  const exited = once(api, 'exit')
  api.kill('SIGTERM')
  const force = setTimeout(() => api.kill('SIGKILL'), 5_000)
  await exited
  clearTimeout(force)
}

function stable(snapshot) {
  return { document: snapshot.document, undo: snapshot.session.undo_depth, redo: snapshot.session.redo_depth }
}

function content(snapshot) {
  const { revision: _revision, ...document } = snapshot.document
  return document
}

try {
  execFileSync('cargo', ['build', '--locked', '--package', 'chemifree-api'], { stdio: 'inherit' })
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { encoding: 'utf8' }))
  const apiPath = resolve(metadata.target_directory, 'debug', `chemifree-api${process.platform === 'win32' ? '.exe' : ''}`)
  const apiBase = await startApi(apiPath)
  let session = await (await fetch(`${apiBase}/api/v1/session`)).json()
  let sequence = 0
  async function request(path, body) {
    const response = await fetch(`${apiBase}/api/v1${path}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { 'content-type': 'application/json', [session.header]: session.token, origin: apiBase },
      body: body === undefined ? undefined : JSON.stringify({ request_id: `template-${++sequence}`, ...body }),
      signal: AbortSignal.timeout(10_000),
    })
    const value = await response.json()
    assert.ok(response.ok, `${path}: ${response.status} ${JSON.stringify(value)}`)
    return value
  }
  async function seed(title, commands) {
    const created = await request('/documents', { title })
    const id = created.document.id
    await request(`/documents/${id}/transactions`, { expected_revision: 0, commands })
    await request(`/documents/${id}/save`, { expected_revision: 1 })
    return id
  }
  stage = 'seed independent source coordinates and graph states'
  const source = await seed('Reusable user fragment', [
    { op: 'add_atom', element: 'C', position: { x: 0, y: 0 }, hydrogens: { mode: 'specified', count: 1 }, radical: 'doublet', attachment_points: [1] },
    { op: 'add_atom', element: 'O', position: { x: 10, y: 0 } },
    { op: 'add_atom', element: 'N', position: { x: 10, y: 10 } },
    { op: 'add_bond', begin: 'a1', end: 'a2', order: 'single' },
    { op: 'add_bond', begin: 'a2', end: 'a3', order: 'double' },
    { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'begin' } },
    { op: 'set_bond_configuration', bond: 'b2', configuration: { kind: 'unknown' } },
    { op: 'update_atom', atom: 'a1', isotope: 13, charge: 1 },
    { op: 'add_instance', source: 's1', offset: { x: 100, y: 20 } },
  ])
  const destination = await seed('Template destination', [
    { op: 'add_atom', element: 'F', position: { x: -30, y: -40 } },
    { op: 'add_reference', source: 's1', target: { kind: 'atom', id: 'a1' } },
  ])
  const stale = await seed('Stale download source', [
    { op: 'add_atom', element: 'C', position: { x: 0, y: 0 } },
    { op: 'add_atom', element: 'O', position: { x: 10, y: 0 } },
    { op: 'add_atom', element: 'N', position: { x: 10, y: 10 } },
  ])
  const multiple = await seed('Multiple placements', [
    { op: 'add_atom', element: 'C', position: { x: 0, y: 0 } },
    { op: 'add_instance', source: 's1', offset: { x: 10, y: 20 } },
  ])
  const snapshot = (id) => request(`/documents/${id}`)
  const sourceBefore = await snapshot(source)
  const before = await snapshot(destination)
  server = await createServer({
    root,
    configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
    // The API's default web-origin allowlist includes 5173. A random Vite port
    // would make real browser POSTs fail the Origin guard before copy/paste.
    server: { host: '127.0.0.1', port: 5173, strictPort: true, proxy: { '/api': apiBase } },
    plugins: [{ name: 'user-template-harness', configureServer(vite) {
      vite.middlewares.use('/user-template-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/user-template-check', '<!doctype html><html><head><title>User template check</title></head><body><div id="root"></div><script type="module" src="/user-template-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    }, resolveId(id) { if (id === '/user-template-harness.tsx') return id },
    load(id) { if (id === '/user-template-harness.tsx') return fixture({ source, destination, multiple, stale }) } }],
  })
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const context = await browser.newContext({ acceptDownloads: true })
  await keepRequestInterception(context)
  page = await context.newPage()
  const errors = []
  const inputWarnings = []
  const mutations = []
  page.on('pageerror', (error) => errors.push(error.message))
  page.on('console', (message) => {
    if (/controlled.*uncontrolled|uncontrolled.*controlled/i.test(message.text())) inputWarnings.push(message.text())
  })
  page.on('request', (request) => {
    if (request.url().endsWith('/api/v1/operations') && request.postDataJSON()?.operation === 'document.clipboard_paste') mutations.push(request.postDataJSON())
  })
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText() { throw new Error('Template files must not read the OS clipboard.') },
      writeText() { throw new Error('Template files must not write the OS clipboard.') },
    } })
    const original = File.prototype.arrayBuffer
    window.heldFileReads = []
    File.prototype.arrayBuffer = function () {
      if (this.name.startsWith('held-')) return new Promise((resolveRead, rejectRead) => {
        window.heldFileReads.push(() => original.call(this).then(resolveRead, rejectRead))
      })
      return original.call(this)
    }
  })
  const button = (name) => page.getByRole('button', { name, exact: true })
  // Modal accessibility correctly hides these harness-only external-event controls.
  // Dispatching them exercises incoming snapshot/navigation changes, not a user's
  // ability to press background toolbar buttons through the modal.
  const externalEvent = (name) => page.getByRole('button', { name, exact: true, includeHidden: true }).evaluate((element) => element.click())
  const dialog = () => page.getByRole('dialog')
  const templatePanel = page.getByRole('region', { name: 'User graph templates', exact: true })
  const fileInput = () => page.getByLabel('Template file', { exact: true })
  const shown = async () => JSON.parse(await page.getByLabel('Current snapshot').innerText())
  async function open(name, id) {
    await button(`Open ${name}`).click()
    await page.waitForFunction((expected) => document.querySelector('[aria-label="Current document"]')?.textContent === expected, id)
  }
  async function revision(expected) {
    await page.waitForFunction((value) => JSON.parse(document.querySelector('[aria-label="Current snapshot"]').textContent)?.document.revision === value, expected)
  }
  async function selectBytes(name, bytes) {
    await fileInput().setInputFiles({ name, mimeType: 'application/json', buffer: Buffer.from(bytes) })
    await page.getByRole('status').filter({ hasText: `Selected ${name}.` }).waitFor()
  }
  async function operation(name, action, outcome = 'ok') {
    const pending = page.waitForResponse((response) => response.url().endsWith('/api/v1/operations') && response.request().postDataJSON()?.operation === name)
    await action()
    const response = await pending
    const body = await response.json()
    assert.equal(response.status(), 200, `${name}: HTTP ${response.status()} ${JSON.stringify(body)}`)
    assert.equal(body.outcome, outcome, `${name}: ${JSON.stringify(body)}`)
    return body
  }
  await page.goto(`http://127.0.0.1:${address.port}/user-template-check`)
  await page.waitForFunction((id) => document.querySelector('[aria-label="Current document"]')?.textContent === id, source)

  stage = 'crossing and mixed selections refuse template saving'
  await button('Select crossing source').click()
  const crossingCopy = await operation('document.clipboard_copy', () => button('Save template file…').click())
  assert.deepEqual(crossingCopy.result.copied, { objects: [], atoms: ['a1'], bonds: [] })
  assert.deepEqual(crossingCopy.result.boundary_bonds, ['b1'], 'the template-specific crossing check follows a successful self-contained native copy')
  await page.getByRole('alert').filter({ hasText: 'Select complete graph components' }).waitFor()
  assert.equal(await button('Download template').isDisabled(), true)
  await button('Close').click()
  await button('Select mixed source').click()
  const mixedCopy = await operation('document.clipboard_copy', () => button('Save template file…').click(), 'error')
  assert.equal(mixedCopy.diagnostics[0].details.reason, 'clipboard_scene_selection_unsupported')
  await page.getByRole('alert').waitFor()
  assert.equal(await button('Download template').isDisabled(), true)
  await button('Close').click()
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore))

  stage = 'save selected source as an actual download'
  await button('Select complete source').click()
  await button('Save template file…').click()
  await page.getByRole('status').filter({ hasText: '3 atoms, 2 bonds from s2' }).waitFor()
  await page.getByLabel('Template filename', { exact: true }).fill('my-fragment')
  const downloading = page.waitForEvent('download')
  await button('Download template').click()
  const download = await downloading
  assert.equal(download.suggestedFilename(), 'my-fragment.chemifree-fragment.json')
  const templatePath = join(work, download.suggestedFilename())
  await download.saveAs(templatePath)
  const bytes = readFileSync(templatePath)
  const payload = JSON.parse(bytes.toString('utf8'))
  assert.equal(payload.format, 'chemifree.clipboard')
  assert.equal(payload.version, 1)
  assert.deepEqual(payload.atoms.map(({ position }) => position), [{ x: 100, y: 20 }, { x: 110, y: 20 }, { x: 110, y: 30 }])
  assert.equal(mutations.length, 0)
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore))
  await button('Close').click()

  stage = 'a changed source snapshot invalidates a prepared download'
  await open('stale', stale)
  await button('Select complete source').click()
  await button('Save template file…').click()
  await page.getByRole('status').filter({ hasText: '3 atoms, 0 bonds from s1' }).waitFor()
  await request(`/documents/${stale}/transactions`, { expected_revision: 1, commands: [{ op: 'rename', title: 'Changed source snapshot' }] })
  await externalEvent('Reload drawing')
  await page.getByText('The drawing changed. Close this dialog and prepare the template again.', { exact: true }).waitFor()
  assert.equal(await button('Download template').isDisabled(), true)
  await button('Close').click()

  stage = 'file reading is explicit, bounded and resistant to stale completion'
  await open('destination', destination)
  await button('Insert template file…').click()
  await page.setViewportSize({ width: 390, height: 844 })
  await fileInput().focus()
  await page.keyboard.press('Tab')
  assert.equal(await page.getByLabel('Template offset x (pt)', { exact: true }).evaluate((element) => element === document.activeElement), true)
  assert.equal(await dialog().evaluate((element) => element.scrollWidth <= element.clientWidth), true)
  await page.setViewportSize({ width: 1280, height: 1000 })
  await fileInput().setInputFiles({ name: 'held-old.json', mimeType: 'application/json', buffer: Buffer.from('{bad old file') })
  await page.waitForFunction(() => window.heldFileReads.length === 1)
  await selectBytes('latest.json', bytes)
  await page.evaluate(() => window.heldFileReads.shift()())
  assert.equal(await page.getByRole('status').filter({ hasText: 'Selected latest.json.' }).count(), 1)
  assert.equal(mutations.length, 0)
  await fileInput().setInputFiles({ name: 'oversized.json', mimeType: 'application/json', buffer: Buffer.alloc(64 * 1024 + 1, 32) })
  await page.getByRole('alert').filter({ hasText: 'at most 64 KiB' }).waitFor()
  assert.equal(await button('Insert template').isDisabled(), true)
  await selectBytes('exact-limit.json', Buffer.concat([bytes, Buffer.alloc(64 * 1024 - bytes.length, 32)]))
  assert.equal(await button('Insert template').isEnabled(), true, 'exact 64 KiB file is accepted for later server validation')
  await fileInput().setInputFiles({ name: 'invalid-utf8.json', mimeType: 'application/json', buffer: Buffer.from([0xc3, 0x28]) })
  await page.getByRole('alert').waitFor()
  assert.equal(await button('Insert template').isDisabled(), true)
  await fileInput().setInputFiles({ name: 'held-closed.json', mimeType: 'application/json', buffer: bytes })
  await page.waitForFunction(() => window.heldFileReads.length === 1)
  await button('Close').click()
  await button('Insert template file…').click()
  await page.evaluate(() => window.heldFileReads.shift()())
  assert.equal(await button('Insert template').isDisabled(), true)
  await button('Close').click()
  assert.deepEqual(stable(await snapshot(destination)), stable(before))

  stage = 'document switch and service restart invalidate pending file reads'
  await button('Insert template file…').click()
  await fileInput().setInputFiles({ name: 'held-document.json', mimeType: 'application/json', buffer: bytes })
  await page.waitForFunction(() => window.heldFileReads.length === 1)
  await externalEvent('Open source')
  await page.waitForFunction((id) => document.querySelector('[aria-label="Current document"]')?.textContent === id, source)
  await page.evaluate(() => window.heldFileReads.shift()())
  assert.equal(await dialog().count(), 0)
  await open('destination', destination)
  await button('Insert template file…').click()
  await fileInput().setInputFiles({ name: 'held-restart.json', mimeType: 'application/json', buffer: bytes })
  await page.waitForFunction(() => window.heldFileReads.length === 1)
  await stopApi()
  await startApi(apiPath, Number(new URL(apiBase).port))
  session = await (await fetch(`${apiBase}/api/v1/session`)).json()
  await externalEvent('Reload drawing')
  await dialog().waitFor({ state: 'detached' })
  await page.evaluate(() => window.heldFileReads.shift()())
  await button('Insert template file…').click()
  assert.equal(await button('Insert template').isDisabled(), true)
  await button('Close').click()
  assert.deepEqual(stable(await snapshot(destination)), stable(before))

  stage = 'file content refusals preserve document and history'
  async function rejectFile(name, value, reason) {
    await button('Insert template file…').click()
    await selectBytes(name, value)
    const response = page.waitForResponse((reply) => reply.url().endsWith('/api/v1/operations') && reply.request().postDataJSON()?.operation === 'document.clipboard_paste')
    await button('Insert template').click()
    const refusal = await (await response).json()
    assert.equal(refusal.outcome, 'error')
    assert.equal(refusal.diagnostics[0].details.reason, reason)
    await page.getByRole('alert').waitFor()
    assert.deepEqual(stable(await snapshot(destination)), stable(before))
    await button('Close').click()
  }
  await rejectFile('malformed.json', '{', 'clipboard_fragment_invalid')
  await rejectFile('future.json', JSON.stringify({ ...payload, version: 2 }), 'clipboard_format_unsupported')
  await rejectFile('unknown-field.json', JSON.stringify({ ...payload, surprise: true }), 'clipboard_fragment_invalid')
  await rejectFile('style.json', JSON.stringify({ ...payload, settings: { ...payload.settings, bond_length_pt: 31 } }), 'clipboard_style_mismatch')
  await open('multiple', multiple)
  const multipleBefore = await snapshot(multiple)
  await button('Insert template file…').click()
  await selectBytes('valid.json', bytes)
  assert.equal(await button('Insert template').isDisabled(), true)
  assert.deepEqual(stable(await snapshot(multiple)), stable(multipleBefore))
  await button('Close').click()
  await open('destination', destination)

  stage = 'actual saved file insertion with immutable lost-reply recovery'
  await button('Insert template file…').click()
  await fileInput().setInputFiles(templatePath)
  await page.getByRole('status').filter({ hasText: 'Selected my-fragment.chemifree-fragment.json.' }).waitFor()
  await page.getByLabel('Template offset x (pt)', { exact: true }).fill('30')
  await page.getByLabel('Template offset y (pt)', { exact: true }).fill('40')
  const lostBodies = []
  await page.route('**/api/v1/operations', async (route) => {
    if (route.request().postDataJSON()?.operation !== 'document.clipboard_paste') return route.continue()
    lostBodies.push(route.request().postDataJSON())
    const response = await route.fetch()
    if (lostBodies.length <= 2) return route.abort('failed')
    return route.fulfill({ response })
  })
  await button('Insert template').click()
  await dialog().getByRole('button', { name: 'Check template insertion outcome', exact: true }).waitFor()
  assert.equal(await button('Insert template').isDisabled(), true)
  const committed = await snapshot(destination)
  assert.equal(committed.document.revision, 2)
  assert.equal(committed.session.undo_depth, before.session.undo_depth + 1)
  await button('Close').click()
  // Confirm dismissal of the portal before using the persistent panel recovery.
  // The test proves recovery survives closing, rather than clicking a closing dialog.
  await dialog().waitFor({ state: 'detached' })
  const persistentRecovery = templatePanel.getByRole('button', { name: 'Check template insertion outcome', exact: true })
  await persistentRecovery.waitFor()
  assert.equal(await persistentRecovery.count(), 1)
  await persistentRecovery.click()
  await persistentRecovery.waitFor({ state: 'detached' })
  assert.deepEqual(lostBodies, [lostBodies[0], lostBodies[0], lostBodies[0]], 'confirmation must keep request ID, payload and offsets')
  const after = await shown()
  assert.deepEqual(stable(after), stable(committed))
  assert.deepEqual(after.document.atoms, [before.document.atoms[0],
    { ...sourceBefore.document.atoms[0], id: 'a2', position: { x: 130, y: 60 } },
    { ...sourceBefore.document.atoms[1], id: 'a3', position: { x: 140, y: 60 } },
    { ...sourceBefore.document.atoms[2], id: 'a4', position: { x: 140, y: 70 } },
  ])
  assert.deepEqual(after.document.bonds, [
    { id: 'b1', begin: 'a2', end: 'a3', order: 'single', query: null, mark: { kind: 'wedge', narrow: 'begin' } },
    { id: 'b2', begin: 'a3', end: 'a4', order: 'double', query: null, mark: null },
  ])
  assert.deepEqual(after.document.selection, { objects: [], atoms: ['a2', 'a3', 'a4'], bonds: ['b1', 'b2'] })
  assert.deepEqual(after.document.settings, before.document.settings)
  assert.deepEqual(after.document.stereo, { atoms: [], bonds: [{ bond: 'b2', configuration: { kind: 'unknown' } }], groups: [] })
  assert.deepEqual(after.document.scene.references, [{ source: 's1', target: { kind: 'atom', id: 'a1' } }])
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore))
  await button('Undo edit').click()
  await revision(3)
  assert.deepEqual(content(await shown()), content(before))
  await button('Redo edit').click()
  await revision(4)
  assert.deepEqual(content(await shown()), content(after))
  await button('Save drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Current snapshot"]').textContent)?.session.unsaved_changes === false)
  await page.reload()
  await page.waitForFunction((id) => document.querySelector('[aria-label="Current document"]')?.textContent === id, source)
  await open('destination', destination)
  assert.deepEqual(content(await shown()), content(after), 'saved template insertion survives a page reload and reopen')
  stage = 'explicit source anchor inspection and read-only connection preview'
  const attachmentBefore = await shown()
  await button('Attach template file…').click()
  await page.getByLabel('Template file for attachment', { exact: true }).setInputFiles(templatePath)
  await page.getByRole('status').filter({ hasText: 'Validated my-fragment.chemifree-fragment.json: 3 local atoms.' }).waitFor()
  const sourceAnchor = page.getByLabel('Source anchor', { exact: true })
  await sourceAnchor.click()
  assert.equal(await page.getByRole('option', { name: /^a2 · O/ }).isDisabled(), true, 'incident source double configuration cannot be silently dropped')
  await page.getByRole('option', { name: /^a1 · C/ }).click()
  await page.getByLabel('Destination atom', { exact: true }).click()
  await page.getByRole('option', { name: /^a1 · F/ }).click()
  assert.equal(await button('Preview template connection').isDisabled(), true, 'direction cannot be inferred')
  await page.getByLabel('Attachment direction dx', { exact: true }).fill('1')
  await page.getByLabel('Attachment direction dy', { exact: true }).fill('0')
  await operation('document.template_plan', () => button('Preview template connection').click())
  await button('Attach template').waitFor()
  assert.deepEqual(content(await snapshot(destination)), content(attachmentBefore), 'preview changes nothing')
  await page.getByLabel('Attachment direction dx', { exact: true }).fill('0')
  assert.equal(await button('Attach template').count(), 0, 'changing an input invalidates the preview')
  assert.equal(await button('Preview template connection').isDisabled(), true, 'zero direction is refused before preview')
  await page.getByLabel('Attachment direction dx', { exact: true }).fill('1')
  await operation('document.template_plan', () => button('Preview template connection').click())
  stage = 'lost attached-template replies retain the exact target, source, direction and request'
  const attachBodies = []
  await page.route('**/api/v1/operations', async (route) => {
    if (route.request().postDataJSON()?.operation !== 'document.template_attach') return route.fallback()
    attachBodies.push(route.request().postDataJSON())
    const response = await route.fetch()
    if (attachBodies.length <= 2) return route.abort('failed')
    return route.fulfill({ response })
  })
  await button('Attach template').click()
  await button('Check template insertion outcome').waitFor()
  assert.equal(await button('Attach template').isDisabled(), true)
  const attached = await snapshot(destination)
  assert.equal(attached.document.revision, 5)
  assert.equal(attached.document.atoms.length, 7)
  assert.equal(attached.document.bonds.length, 5)
  assert.deepEqual(attached.document.atoms.slice(0, 4), attachmentBefore.document.atoms)
  assert.deepEqual(attached.document.atoms.slice(4).map((atom) => atom.position), [{ x: 0, y: -40 }, { x: 10, y: -40 }, { x: 10, y: -30 }])
  assert.deepEqual(attached.document.selection, { objects: [], atoms: ['a5', 'a6', 'a7'], bonds: ['b3', 'b4'] })
  assert.deepEqual(attached.document.bonds.at(-1), { id: 'b5', begin: 'a1', end: 'a5', order: 'single', query: null, mark: null })
  await button('Close').click()
  const persistentAttachRecovery = templatePanel.getByRole('button', { name: 'Check template insertion outcome', exact: true })
  assert.equal(await persistentAttachRecovery.count(), 1, 'the attachment has one recovery control after dialog dismissal')
  await persistentAttachRecovery.click()
  await page.waitForFunction(() => !document.querySelector('section[aria-label="User graph templates"]')?.textContent.includes('Check template insertion outcome'))
  assert.deepEqual(attachBodies, [attachBodies[0], attachBodies[0], attachBodies[0]])
  assert.equal(attachBodies[0].parameters.source_anchor, 'a1')
  assert.equal(attachBodies[0].parameters.atom, 'a1')
  assert.deepEqual(attachBodies[0].parameters.direction, { dx: 1, dy: 0 })
  await button('Undo edit').click()
  await revision(6)
  assert.deepEqual(content(await shown()), content(attachmentBefore))
  await button('Redo edit').click()
  await revision(7)
  assert.deepEqual(content(await shown()), content(attached))
  assert.deepEqual(stable(await snapshot(source)), stable(sourceBefore))
  await button('Save drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Current snapshot"]').textContent)?.session.unsaved_changes === false)
  await page.reload()
  await page.waitForFunction((id) => document.querySelector('[aria-label="Current document"]')?.textContent === id, source)
  await open('destination', destination)
  assert.deepEqual(content(await shown()), content(attached), 'attached template persists across reload and reopen')
  assert.deepEqual(errors, [])
  assert.deepEqual(inputWarnings, [], 'closing and switching template forms must not reuse a controlled text input as an uncontrolled file input')
  passed = true
  console.log('User graph template download, file selection, real backend insertion, refusals and recovery passed.')
} catch (error) {
  console.error(`User template check failed at ${stage}. Data retained at ${work}.\n${apiLog}`)
  try {
    if (page && !page.isClosed()) {
      console.error('Template UI at failure:', JSON.stringify(await page.locator('[role="dialog"], [role="alert"], [aria-label="Current document"]').allTextContents()))
    }
  } catch (diagnosticError) {
    console.error('Could not capture template UI diagnostics:', diagnosticError)
  }
  throw error
} finally {
  await browser?.close()
  await server?.close()
  await stopApi()
  if (passed) rmSync(work, { recursive: true, force: true })
}
