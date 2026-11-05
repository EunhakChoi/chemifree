// Browser smoke for the document editor: create → edit → undo/redo → save → reopen, plus
// conflict, save-failure, autosave and crash recovery, save as copy, mobile, and dark-mode
// checks. It starts the API and Vite on a temporary data directory (ports 3001 and 5173 must
// be free) and drives a real browser. The API runs as its own process so it can be killed.
//
//   npm run test:browser
//
// Uses playwright-core with an installed Chrome/Chromium: set CHEMIFREE_BROWSER to its
// executable, or install Playwright's build with `npx playwright-core install chromium`.
//
// Records go to CHEMIFREE_SMOKE_OUT, or to the temporary directory, which only a passing run
// removes. Every run, passed or failed, writes:
//   results.json      the steps that passed, the last step reached, and the error, if any
//   build.log         cargo's diagnostics
//   environment.json  browser, driver, Node, and platform (once the browser has started)
//   requests.log      every browser request with its outcome and duration
//   api.log           the request log of every API process the run started
//   web.log           the Vite dev server's output
//   editor-*.png      screenshots of the steps reached
// A failed run also writes failure.png. Records of an earlier run in CHEMIFREE_SMOKE_OUT are
// removed first; other files there are left alone.
//
// Steps that make drafts/ read-only cannot work as root, whose writes permissions do not
// stop, so they are skipped there and listed in results.skipped_steps. With
// CHEMIFREE_SMOKE_NO_SKIPS=1 (set in CI) a skipped step fails the run.
import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { stripVTControlCharacters } from 'node:util'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const BASE = 'http://127.0.0.1:5173'
const results = { skipped_steps: [] }
const runningAsRoot = process.getuid?.() === 0
const consoleErrors = []
const work = mkdtempSync(join(tmpdir(), 'chemifree-browser-'))
const dataDir = join(work, 'data')
const outDir = process.env.CHEMIFREE_SMOKE_OUT ?? work
mkdirSync(outDir, { recursive: true })
const RECORD_NAME = /^(?:editor-[a-z0-9-]+\.png|failure\.png|results\.json|build\.log|environment\.json|requests\.log|api\.log|web\.log)$/
for (const name of readdirSync(outDir)) {
  if (RECORD_NAME.test(name)) rmSync(join(outDir, name))
}

// What the failure handler needs exists before anything else runs, so a run that fails to
// build, to start its servers, or to launch the browser still leaves its records.
let step = 'build'
let apiServer = null
let web = null
let browser = null
let page = null
/** Output of API processes that have ended; some steps kill the API and start a new one. */
const endedApiLogs = []
/** Every browser request: start offset and duration in ms (monotonic clock), method, path, outcome. */
const requestLog = []
const buildLog = []
/** Stops whatever has started. The temporary directory is removed unless `keepWork` is set. */
async function cleanup({ keepWork = false } = {}) {
  await browser?.close().catch(() => {})
  // The API holds files in the temporary directory until it has exited.
  const apiChild = apiServer?.child
  const apiExited =
    apiChild && apiChild.exitCode === null && apiChild.signalCode === null
      ? new Promise((resolveExit) => apiChild.once('exit', resolveExit))
      : Promise.resolve()
  apiChild?.kill('SIGTERM')
  if (web) {
    try {
      process.kill(-web.child.pid, 'SIGTERM')
    } catch {
      // Already stopped.
    }
  }
  await apiExited
  if (!keepWork && !process.env.CHEMIFREE_SMOKE_OUT) rmSync(work, { recursive: true, force: true })
}
/** Writes the run's records; `error` is what failed it, if anything. */
function writeRecords(error) {
  const outcome = { step, error: error === undefined ? null : String(error?.message ?? error), results }
  writeFileSync(join(outDir, 'results.json'), `${JSON.stringify(outcome, null, 2)}\n`)
  writeFileSync(join(outDir, 'build.log'), buildLog.join('\n'))
  writeFileSync(join(outDir, 'requests.log'), `${requestLog.join('\n')}\n`)
  writeFileSync(join(outDir, 'api.log'), [...endedApiLogs, apiServer?.log() ?? ''].join('\n--- next API process\n'))
  writeFileSync(join(outDir, 'web.log'), web?.log() ?? '')
}
process.on('uncaughtException', async (error) => {
  console.error(`Browser smoke FAILED at step ${step}:`, error.message)
  // No page (for example, the browser did not launch) means no screenshot.
  const shot = await page?.screenshot({ path: join(outDir, 'failure.png') }).then(
    () => true,
    () => false,
  )
  console.error(requestLog.slice(-40).join('\n'))
  writeRecords(error)
  const written = readdirSync(outDir).filter((name) => RECORD_NAME.test(name) && (shot || name !== 'failure.png'))
  console.error(`Records in ${outDir}: ${written.sort().join(', ')}`)
  // The records may be in the temporary directory, so a failed run keeps it.
  await cleanup({ keepWork: true })
  process.exit(1)
})

/** Runs cargo, keeping its diagnostics (standard error) for build.log; returns its output. */
function cargo(args) {
  const run = spawnSync('cargo', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 })
  buildLog.push(`$ cargo ${args.join(' ')}\n${run.stderr ?? ''}`)
  process.stderr.write(run.stderr ?? '')
  if (run.error) throw run.error
  if (run.status !== 0) throw new Error(`cargo ${args[0]} failed with exit status ${run.status}; see build.log`)
  return run.stdout
}
cargo(['build', '--workspace', '--locked'])
const metadata = JSON.parse(cargo(['metadata', '--no-deps', '--format-version', '1', '--locked']))
const suffix = process.platform === 'win32' ? '.exe' : ''
const cliPath = resolve(metadata.target_directory, 'debug', `chemifree${suffix}`)
const apiPath = resolve(metadata.target_directory, 'debug', `chemifree-api${suffix}`)
const env = { ...process.env, CHEMIFREE_DATA_DIR: dataDir }
delete env.CHEMIFREE_MODEL_DATA_ROOT
delete env.CHEMIFREE_MODEL_TRUSTED_REGISTRY

/**
 * Starts a process and resolves once its output matches `ready`. The match ignores terminal
 * colors, which tools add when they detect CI even though their output is piped.
 */
function start(command, args, ready, options = {}) {
  const child = spawn(command, args, { env, stdio: ['ignore', 'pipe', 'pipe'], ...options })
  let log = ''
  const started = new Promise((resolveReady, reject) => {
    const timer = setTimeout(() => reject(new Error(`${command} did not start. ${log}`)), 120_000)
    const read = (data) => {
      log += data.toString()
      if (ready.test(stripVTControlCharacters(log))) {
        clearTimeout(timer)
        resolveReady(child)
      }
    }
    child.stdout.on('data', read)
    child.stderr.on('data', read)
    child.once('exit', (code) => reject(new Error(`${command} exited (${code}). ${log}`)))
  })
  return { child, started, log: () => log }
}
const startApi = () => start(apiPath, ['--port', '3001', '--log-requests'], /listening on http:\/\/127\.0\.0\.1:3001/)
step = 'start'
apiServer = startApi()
web = start('npm', ['run', 'dev:web'], /Local:\s+http:\/\/127\.0\.0\.1:5173/, { detached: true })
await Promise.all([apiServer.started, web.started])
/** Kills the API the way a crash would and starts a new process on the same port. */
async function crashApi() {
  const exited = new Promise((resolveExit) => apiServer.child.once('exit', resolveExit))
  apiServer.child.kill('SIGKILL')
  await exited
  endedApiLogs.push(apiServer.log())
  apiServer = startApi()
  await apiServer.started
}

step = 'launch'
browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
const environment = {
  browser: `${browser.browserType().name()} ${browser.version()}`,
  // Without CHEMIFREE_BROWSER, Playwright picks its own build for the version it was locked to.
  executable: process.env.CHEMIFREE_BROWSER ?? `Playwright's build for playwright-core`,
  playwright_core: JSON.parse(readFileSync(resolve('node_modules/playwright-core/package.json'), 'utf8')).version,
  node: process.version,
  platform: `${process.platform} ${process.arch}`,
}
writeFileSync(join(outDir, 'environment.json'), `${JSON.stringify(environment, null, 2)}\n`)
console.log(`Browser ${environment.browser} (${environment.executable}), playwright-core ${environment.playwright_core}`)
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } })
// Steps add and remove page routes; without this, a request sent just as the last one goes
// away can stall in the browser (see scripts/lib/request-interception.mjs and #8).
await keepRequestInterception(context)
page = await context.newPage()
page.on('console', (message) => message.type() === 'error' && consoleErrors.push(message.text()))
// Durations use the monotonic clock: the wall clock can be stepped by time synchronization.
const runStarted = performance.now()
const started = new Map()
page.on('request', (request) => started.set(request, performance.now()))
const note = (request, outcome) => {
  const at = started.get(request) ?? performance.now()
  const { pathname, search } = new URL(request.url())
  const path = `${pathname}${search}`
  requestLog.push(`+${Math.round(at - runStarted)} ${request.method()} ${path} ${outcome} ${Math.round(performance.now() - at)}ms`)
}
page.on('requestfinished', async (request) => note(request, (await request.response())?.status()))
page.on('requestfailed', (request) => note(request, request.failure()?.errorText))
page.on('pageerror', (error) => consoleErrors.push(String(error)))

const api = (path) => page.evaluate(async (p) => (await fetch(p)).json(), path)
let docId = null
const openDoc = () => api(`/api/v1/documents/${docId}`)
/** Screen coordinates of a document point. */
const screen = (point) =>
  page.evaluate(({ x, y }) => {
    const svg = document.querySelector('svg[data-structure-canvas]')
    const p = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM())
    return { x: p.x, y: p.y }
  }, point)
const atomScreen = async (doc, id) => screen(doc.document.atoms.find((atom) => atom.id === id).position)
const waitIdle = () => page.waitForFunction(() => !document.querySelector('[data-slot="spinner"]'))
/**
 * The page's next read of the document list, which it sends after discarding a draft or
 * making a copy. A step that kills the API waits for it first: a read cut off by the kill is
 * answered 502 by the development proxy, which is not what such a step is about.
 */
const listRead = () =>
  page.waitForResponse((r) => r.request().method() === 'GET' && new URL(r.url()).pathname === '/api/v1/documents')
const revision = async () => (await openDoc()).document.revision
const saveState = () => page.getByTestId('save-state').textContent()
const tool = (label) => page.getByRole('button', { name: label, exact: true }).first().click()

// The service may not be ready for the first preference read. Once it is reachable, the
// dialog can retry without reloading the page and applies the already-stored theme.
execFileSync(cliPath, [
  'preferences', 'set', '--expected-revision', '0', '--preferences',
  JSON.stringify({ ui: { theme: 'dark' }, new_documents: {} }),
], { env })
await page.emulateMedia({ colorScheme: 'light' })
let preferencesAvailable = false
const unavailablePreferences = (route) =>
  !preferencesAvailable && route.request().method() === 'GET'
    ? route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ api_version: 'v1', code: 'storage_failed', message: 'temporarily unavailable' }) })
    : route.continue()
await page.route('**/api/v1/preferences', unavailablePreferences)
const unavailableData = (route) => route.fulfill({ status: 503, contentType: 'application/json', body: '{}' })
await page.route('**/api/v1/data', unavailableData)
await page.goto(BASE)
await page.getByText('No document open').waitFor()
await page.getByText('Data status is unavailable.').waitFor()
await page.unroute('**/api/v1/data', unavailableData)
const refreshedData = page.waitForResponse((response) => response.url().endsWith('/api/v1/data') && response.status() === 200)
await page.getByRole('button', { name: 'Refresh status' }).click()
await refreshedData
await page.getByText('No external data selected.').waitFor()
assert.equal((await api('/api/v1/data')).engine, 'not_implemented')
await page.getByText('Chemistry engine not implemented').waitFor()
const dataWithExtraColumns = (route) => route.fulfill({
  status: 200,
  contentType: 'application/json',
  body: JSON.stringify({
    api_version: 'v1', state: 'checked', reason: null, engine: 'not_implemented',
    registrations: [{ registration_id: 'independent-fixture', state: 'loaded', reason: null, retained_extra_columns: 2 }],
  }),
})
await page.route('**/api/v1/data', dataWithExtraColumns)
const extraColumnsResponse = page.waitForResponse((response) => response.url().endsWith('/api/v1/data') && response.status() === 200)
await page.getByRole('button', { name: 'Refresh status' }).click()
await extraColumnsResponse
await page.getByText('independent-fixture: 2 extra source columns retained but not used for prediction.').waitFor()
await page.unroute('**/api/v1/data', dataWithExtraColumns)
const originalDataResponse = page.waitForResponse((response) => response.url().endsWith('/api/v1/data') && response.status() === 200)
await page.getByRole('button', { name: 'Refresh status' }).click()
await originalDataResponse
await page.getByText('No external data selected.').waitFor()
results.data_status = true
await page.getByRole('button', { name: 'Preferences', exact: true }).click()
const unavailableDialog = page.getByRole('dialog', { name: 'Preferences' })
await unavailableDialog.getByText(/Preferences could not be read/).waitFor()
const failedRetry = page.waitForResponse((response) => response.url().endsWith('/api/v1/preferences') && response.request().method() === 'GET' && response.status() === 503)
await unavailableDialog.getByRole('button', { name: 'Retry preferences' }).click()
assert.equal((await failedRetry).status(), 503)
await unavailableDialog.getByText(/Preferences could not be read/).waitFor()
preferencesAvailable = true
const retryPreferences = page.waitForResponse((response) => response.url().endsWith('/api/v1/preferences') && response.request().method() === 'GET' && response.status() === 200)
await unavailableDialog.getByRole('button', { name: 'Retry preferences' }).click()
await retryPreferences
await unavailableDialog.getByLabel('Theme').waitFor()
assert.match(await unavailableDialog.getByLabel('Theme').textContent(), /^Dark/)
await page.waitForFunction(() => document.documentElement.classList.contains('dark'))
const resetInitialPreferences = page.waitForResponse((response) => response.url().endsWith('/api/v1/preferences/reset'))
await unavailableDialog.getByRole('button', { name: 'Reset preferences' }).click()
assert.equal((await resetInitialPreferences).status(), 200)
await page.waitForFunction(() => !document.documentElement.classList.contains('dark'))
await unavailableDialog.getByRole('button', { name: 'Done' }).click()
await unavailableDialog.waitFor({ state: 'hidden' })
await page.unroute('**/api/v1/preferences', unavailablePreferences)
results.preferences_read_can_be_retried = true

// Select a page and fragment, inspect losses and IDs, then explicitly create a new drawing.
{
  const source = '<CDXML><page id="1"><fragment id="2"><n id="3" p="1 2"/></fragment><t id="4"><s>α</s></t></page><page id="5"><fragment id="6"><n id="7" p="10 20" Element="8"/></fragment></page></CDXML>'
  const before = (await api('/api/v1/documents')).documents.length
  await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name: 'two-pages.cdxml', mimeType: 'application/xml', buffer: Buffer.from(source) })
  await page.getByLabel('Page and molecule').selectOption('1')
  await page.getByRole('button', { name: 'Preview import' }).click()
  const preview = page.getByTestId('cdxml-preview')
  await preview.getByText('Preview: 1 atom (O), 0 bonds.').waitFor()
  await preview.getByText(/#7 → a1/).waitFor()
  await preview.getByText(/not represented in the editor projection/i).first().waitFor()
  await preview.locator('..').screenshot({ path: join(outDir, 'editor-cdxml-preview.png') })
  assert.equal((await api('/api/v1/documents')).documents.length, before, 'preview creates no document')
  const committed = page.waitForResponse((response) => response.url().endsWith('/api/v1/operations') &&
    response.request().postDataJSON()?.parameters?.mode === 'create')
  await preview.getByRole('button', { name: 'Accept losses and create drawing' }).click()
  const body = await (await committed).json()
  assert.equal(body.result.document.atoms[0].element, 'O')
  await page.getByLabel('Document title').waitFor()
  assert.equal((await api('/api/v1/documents')).documents.length, before + 1)
  await page.getByRole('button', { name: 'Close document' }).click()
  await page.getByText('No document open').waitFor()
  for (const [name, unsafe, message] of [
    ['missing.cdxml', '<CDXML><page id="1"><fragment id="2"><n id="3"/></fragment></page></CDXML>', /has no position/],
    ['label.cdxml', '<CDXML><page id="1"><fragment id="2"><n id="3" p="0 0"><t id="4"><s>O</s></t></n></fragment></page></CDXML>', /has a label or child object/],
  ]) {
    await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name, mimeType: 'application/xml', buffer: Buffer.from(unsafe) })
    await page.getByLabel('Page and molecule').waitFor()
    await page.getByRole('button', { name: 'Preview import' }).click()
    await page.getByRole('alert').filter({ hasText: message }).waitFor()
    assert.equal(await page.getByTestId('cdxml-preview').count(), 0, 'unsafe chemistry is not previewed as a different graph')
    assert.equal((await api('/api/v1/documents')).documents.length, before + 1, 'refused preview creates no drawing')
  }
}
// CDX: the same picker sends base64 and shows the same preview, mapping and consent.
{
  step = 'CDX import'
  const u16 = (value) => Buffer.from([value & 0xff, value >> 8])
  const i32 = (value) => { const buffer = Buffer.alloc(4); buffer.writeInt32LE(value); return buffer }
  const obj = (tag, id, ...parts) => Buffer.concat([u16(tag), i32(id), ...parts, u16(0)])
  const prop = (tag, data) => Buffer.concat([u16(tag), u16(data.length), data])
  const node = obj(0x8004, 3, prop(0x0200, Buffer.concat([i32(20 * 65536), i32(10 * 65536)])), prop(0x0402, u16(8)))
  const document = obj(0x8000, 0, obj(0x8001, 1, obj(0x8003, 2, node)))
  const header = Buffer.concat([Buffer.from('VjCD0100'), Buffer.from([4, 3, 2, 1]), Buffer.alloc(16)])
  const cdx = Buffer.concat([header, document, u16(0)])
  const before = (await api('/api/v1/documents')).documents.length
  await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name: 'molecule.cdx', mimeType: 'application/octet-stream', buffer: cdx })
  await page.getByLabel('Page and molecule').waitFor()
  assert.equal(await page.getByLabel('New drawing title').inputValue(), 'molecule', 'the .cdx extension is stripped from the title')
  await page.getByRole('button', { name: 'Preview import' }).click()
  const preview = page.getByTestId('cdxml-preview')
  await preview.getByText('Preview: 1 atom (O), 0 bonds.').waitFor()
  await preview.getByText(/#3 → a1/).waitFor()
  assert.equal((await api('/api/v1/documents')).documents.length, before, 'CDX preview creates no document')
  const committed = page.waitForResponse((response) => response.url().endsWith('/api/v1/operations') &&
    response.request().postDataJSON()?.parameters?.mode === 'create')
  await preview.getByRole('button', { name: 'Accept losses and create drawing' }).click()
  const request = (await committed).request().postDataJSON()
  assert.equal(request.parameters.format, 'cdx')
  assert.equal(request.parameters.source, cdx.toString('base64'), 'CDX travels as canonical base64')
  await page.getByLabel('Document title').waitFor()
  assert.equal((await api('/api/v1/documents')).documents.length, before + 1)
  await page.getByRole('button', { name: 'Close document' }).click()
  await page.getByText('No document open').waitFor()
  // A truncated file is refused by the service; an oversized one never leaves the browser.
  await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name: 'cut.cdx', mimeType: 'application/octet-stream', buffer: cdx.subarray(0, cdx.length - 5) })
  await page.getByRole('alert').filter({ hasText: /ends in the middle of an item\. \(byte offset \d+\)/ }).waitFor()
  await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name: 'big.cdx', mimeType: 'application/octet-stream', buffer: Buffer.alloc(121 * 1024) })
  await page.getByRole('alert').filter({ hasText: /up to 120 KiB/ }).waitFor()
  assert.equal((await api('/api/v1/documents')).documents.length, before + 1, 'refused CDX creates no drawing')
  results.cdx_import_with_consent = true
}
// A preview from an unmounted picker cannot replace a newer file's preview.
{
  step = 'CDXML delayed preview after picker changes'
  const first = '<CDXML><page id="1"><fragment id="2"><n id="3" p="1 2"/></fragment></page></CDXML>'
  const second = '<CDXML><page id="5"><fragment id="6"><n id="7" p="3 4" Element="8"/></fragment></page></CDXML>'
  const before = (await api('/api/v1/documents')).documents.length
  await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name: 'old.cdxml', mimeType: 'application/xml', buffer: Buffer.from(first) })
  await page.getByLabel('Page and molecule').waitFor()
  let arrived
  const responseArrived = new Promise((resolve) => { arrived = resolve })
  let release
  const released = new Promise((resolve) => { release = resolve })
  let finished
  const responseFinished = new Promise((resolve) => { finished = resolve })
  let intercepted = false
  const holdPreview = async (route) => {
    const request = route.request().postDataJSON()
    if (intercepted || request?.operation !== 'document.import' || request.parameters?.mode !== 'preview') return route.continue()
    intercepted = true
    const response = await route.fetch()
    arrived()
    await released
    await route.fulfill({ response })
    finished()
  }
  await page.route('**/api/v1/operations', holdPreview)
  await page.getByRole('button', { name: 'Preview import' }).click()
  await responseArrived
  await page.getByRole('button', { name: 'New document' }).click()
  await page.getByLabel('Document title').waitFor()
  await page.getByRole('button', { name: 'Close document' }).click()
  await page.getByText('No document open').waitFor()
  await page.getByLabel(/CDXML(?: or CDX)? file/).setInputFiles({ name: 'new.cdxml', mimeType: 'application/xml', buffer: Buffer.from(second) })
  await page.getByLabel('Page and molecule').waitFor()
  await page.getByRole('button', { name: 'Preview import' }).click()
  const currentPreview = page.getByTestId('cdxml-preview')
  await currentPreview.getByText('Preview: 1 atom (O), 0 bonds.').waitFor()
  release()
  await responseFinished
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)))
  await currentPreview.getByText('Preview: 1 atom (O), 0 bonds.').waitFor()
  assert.equal(await currentPreview.getByText('Preview: 1 atom (C), 0 bonds.').count(), 0)
  assert.equal((await api('/api/v1/documents')).documents.length, before + 1, 'only the deliberately created document exists')
  await page.unroute('**/api/v1/operations', holdPreview)
  results.cdxml_delayed_preview_is_stale = true
}
results.cdxml_projection_with_loss_consent = true
results.cdxml_unsafe_projection_refused = true

// Create.
const created = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
await page.getByRole('button', { name: 'New document' }).click()
docId = (await (await created).json()).document.id
await page.getByLabel('Document title').waitFor()
assert.equal(await page.getByLabel('Document title').inputValue(), 'Untitled')
assert.equal(await saveState(), 'Saved')
results.create = true
step = 'after create'

// Bond tool click on empty canvas: two carbons and one bond.
const canvas = page.locator('svg[data-structure-canvas]')
const box = await canvas.boundingBox()
await page.mouse.click(box.x + box.width * 0.3, box.y + box.height * 0.55)
await waitIdle()
let doc = await openDoc()
assert.equal(doc.document.atoms.length, 2)
assert.equal(doc.document.bonds.length, 1)
assert.equal(await saveState(), 'Unsaved changes')

// Click the end atom to grow a chain; drag from it to make another bond.
let a2 = await atomScreen(doc, 'a2')
await page.mouse.click(a2.x, a2.y)
await waitIdle()
doc = await openDoc()
assert.equal(doc.document.atoms.length, 3)
const a3 = await atomScreen(doc, 'a3')
await page.mouse.move(a3.x, a3.y)
await page.mouse.down()
await page.mouse.move(a3.x + 40, a3.y + 5, { steps: 5 })
await page.mouse.move(a3.x + 70, a3.y + 10, { steps: 5 })
await page.mouse.up()
await waitIdle()
doc = await openDoc()
assert.equal(doc.document.atoms.length, 4)
assert.equal(doc.document.bonds.length, 3)
const a4 = doc.document.atoms.find((atom) => atom.id === 'a4')
const a3pos = doc.document.atoms.find((atom) => atom.id === 'a3').position
assert.ok(Math.abs(Math.hypot(a4.position.x - a3pos.x, a4.position.y - a3pos.y) - 30) < 0.05, 'fixed bond length')
results.add_atoms_and_bonds = true
step = 'after add_atoms_and_bonds'

// The property panel reads the same graph without editing it, and invalidates an answer
// when that graph changes. The service has no external mass data in this smoke run.
const properties = page.getByRole('region', { name: 'Molecular properties' })
const beforeFormula = await openDoc()
await properties.getByRole('button', { name: 'Calculate', exact: true }).click()
await properties.getByTestId('properties-result').getByText('C4H10', { exact: false }).waitFor()
assert.deepEqual(await openDoc(), beforeFormula, 'calculation does not change document or history')
await properties.getByText('Method and data', { exact: true }).click()
assert.equal(await properties.getByText(/display: .* decimal places/).count(), 0, 'formula does not claim a mass precision')
await page.getByRole('button', { name: 'Preferences', exact: true }).click()
const propertyPreferences = page.getByRole('dialog', { name: 'Preferences' })
await propertyPreferences.waitFor()
let propertyPreferenceWrite = page.waitForResponse((response) => new URL(response.url()).pathname === '/api/v1/preferences' && response.request().method() === 'PUT')
await propertyPreferences.getByLabel('Mass display decimal places').click()
await page.getByRole('option', { name: '5', exact: true }).click()
assert.equal((await propertyPreferenceWrite).status(), 200)
await propertyPreferences.getByRole('button', { name: 'Done' }).click()
await propertyPreferences.waitFor({ state: 'hidden' })
assert.equal((await api('/api/v1/preferences')).computation.mass_precision.decimals, 5)
await properties.getByTestId('properties-result').getByText('C4H10', { exact: false }).waitFor()
await properties.getByRole('button', { name: 'Calculate', exact: true }).click()
await properties.getByTestId('properties-result').getByText('C4H10', { exact: false }).waitFor()
await properties.getByText('Method and data', { exact: true }).click()
assert.equal(await properties.getByText(/display: .* decimal places/).count(), 0, 'a new formula also has no mass precision')
await properties.screenshot({ path: join(outDir, 'editor-properties.png') })
await page.getByRole('button', { name: 'Preferences', exact: true }).click()
await propertyPreferences.waitFor()
propertyPreferenceWrite = page.waitForResponse((response) => response.url().endsWith('/api/v1/preferences/reset'))
await propertyPreferences.getByRole('button', { name: 'Reset preferences' }).click()
assert.equal((await propertyPreferenceWrite).status(), 200)
await propertyPreferences.getByRole('button', { name: 'Done' }).click()
await propertyPreferences.waitFor({ state: 'hidden' })

// Atom tool: O element, click a4 to relabel it.
await page.getByRole('button', { name: 'Element O' }).click()
await canvas.scrollIntoViewIfNeeded()
let a4s = await atomScreen(doc, 'a4')
await page.mouse.click(a4s.x, a4s.y)
await waitIdle()
doc = await openDoc()
assert.equal(doc.document.atoms.find((atom) => atom.id === 'a4').element, 'O')
await page.locator('[data-atom-label="a4"]').waitFor()
assert.equal(await properties.getByTestId('properties-result').count(), 0, 'a changed graph hides the earlier formula')
await properties.getByRole('button', { name: 'Calculate', exact: true }).click()
await properties.getByTestId('properties-result').getByText('C3H8O', { exact: false }).waitFor()
await properties.getByLabel('Result').click()
await page.getByRole('option', { name: 'Formula and mass', exact: true }).click()
await properties.getByRole('button', { name: 'Calculate', exact: true }).click()
await properties.getByText('No selected, compatible external mass dataset is available.', { exact: true }).waitFor()
await properties.getByLabel('Result').click()
await page.getByRole('option', { name: 'Formula', exact: true }).click()
results.property_formula_and_missing_data = true

// Select a4, set charge and isotope in the inspector.
await tool('Select and move')
await canvas.scrollIntoViewIfNeeded()
a4s = await atomScreen(doc, 'a4')
await page.mouse.click(a4s.x, a4s.y)
await page.getByRole('heading', { name: 'Atom a4' }).waitFor()
await page.getByRole('button', { name: 'Decrease charge' }).click()
await waitIdle()
await page.getByLabel('Isotope mass number').fill('18')
await page.getByLabel('Isotope mass number').press('Enter')
await waitIdle()
doc = await openDoc()
const oxygen = doc.document.atoms.find((atom) => atom.id === 'a4')
assert.deepEqual([oxygen.charge, oxygen.isotope], [-1, 18])
assert.equal((await page.locator('text[data-atom="a4"]').allTextContents()).join('').replace(/\s/g, ''), '18O−')
results.edit_atom = true
step = 'after edit_atom'

// Bond tool: click bond b1 to cycle single → double.
await tool('Bond')
const b1 = doc.document.bonds.find((bond) => bond.id === 'b1')
const p1 = doc.document.atoms.find((atom) => atom.id === b1.begin).position
const p2 = doc.document.atoms.find((atom) => atom.id === b1.end).position
const mid = await screen({ x: (p1.x + p2.x) / 2, y: (p1.y + p2.y) / 2 })
await page.mouse.click(mid.x, mid.y)
await waitIdle()
assert.equal(await page.locator('line[data-bond="b1"]').count(), 2)
results.edit_bond = true
step = 'after edit_bond'

// Undo/redo via buttons and keyboard.
const before = await revision()
await page.getByRole('button', { name: 'Undo' }).click()
await waitIdle()
assert.equal(await page.locator('line[data-bond="b1"]').count(), 1)
await page.getByRole('button', { name: 'Redo' }).click()
await waitIdle()
assert.equal(await page.locator('line[data-bond="b1"]').count(), 2)
await page.keyboard.press('Control+z')
await waitIdle()
assert.equal(await page.locator('line[data-bond="b1"]').count(), 1)
await page.keyboard.press('Control+Shift+z')
await waitIdle()
assert.equal(await page.locator('line[data-bond="b1"]').count(), 2)
assert.equal(await revision(), before + 4, 'each undo/redo is a new revision')
results.undo_redo = true
step = 'after undo_redo'

// Move a1 with the select tool.
await tool('Select and move')
doc = await openDoc()
const a1 = await atomScreen(doc, 'a1')
const a1Before = doc.document.atoms.find((atom) => atom.id === 'a1').position
await page.mouse.move(a1.x, a1.y)
await page.mouse.down()
await page.mouse.move(a1.x - 20, a1.y + 20, { steps: 6 })
await page.mouse.up()
await waitIdle()
doc = await openDoc()
const a1After = doc.document.atoms.find((atom) => atom.id === 'a1').position
assert.ok(a1After.x < a1Before.x && a1After.y > a1Before.y, 'atom moved')
results.move = true
step = 'after move'

// Rename and save with Ctrl+S.
await page.getByLabel('Document title').fill('Browser ethanol')
await page.getByLabel('Document title').press('Enter')
await waitIdle()
await canvas.click({ position: { x: 5, y: 5 } }) // blur input; select tool on empty does nothing
await page.keyboard.press('Control+s')
await waitIdle()
assert.equal(await saveState(), 'Saved')
const saved = await openDoc()
assert.equal(saved.document.title, 'Browser ethanol')
await page.waitForTimeout(300)
await page.screenshot({ path: join(outDir, 'editor-desktop.png') })
results.save = true
step = 'after save'

// Close and reopen: identical committed document.
await page.getByRole('button', { name: 'Close document' }).click()
await page.getByText('No document open').waitFor()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
const reopened = await openDoc()
assert.deepEqual(reopened.document, saved.document)
assert.deepEqual(reopened.session, saved.session)
results.close_and_reopen = true
step = 'after close_and_reopen'

// Unsaved close asks first; discarding returns to the saved revision.
await tool('Erase')
let eraseDoc = await openDoc()
const a2s = await atomScreen(eraseDoc, 'a2')
await page.mouse.click(a2s.x, a2s.y)
await waitIdle()
assert.equal((await openDoc()).document.atoms.length, 3)
// A discard confirmed on a stale view is refused: another client edited after it.
await page.getByRole('button', { name: 'Close document' }).click()
await page.getByRole('alertdialog').waitFor()
const beforeOther = await openDoc()
await page.evaluate(async ({ id, rev }) => {
  const session = await (await fetch('/api/v1/session')).json()
  await fetch(`/api/v1/documents/${id}/transactions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', [session.header]: session.token },
    body: JSON.stringify({ request_id: 'discard-other', expected_revision: rev, commands: [{ op: 'rename', title: 'Other tab' }] }),
  })
}, { id: docId, rev: beforeOther.document.revision })
await page.getByRole('button', { name: 'Discard and close' }).click()
await page.getByText('The document changed', { exact: true }).waitFor()
assert.equal((await openDoc()).document.title, 'Other tab', "the other client's edit survives")
await page.getByRole('button', { name: 'Reload' }).click()
await waitIdle()
results.stale_discard_is_refused = true

await page.getByRole('button', { name: 'Close document' }).click()
await page.getByRole('alertdialog').waitFor()
// Editor shortcuts are inert while the dialog is open.
await page.keyboard.press('Control+s')
await page.waitForTimeout(300)
assert.equal(await saveState(), 'Unsaved changes', 'Ctrl+S behind the dialog does nothing')
await page.getByRole('button', { name: 'Discard and close' }).click()
await page.getByText('No document open').waitFor()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
const afterDiscard = await openDoc()
assert.deepEqual(
  { ...afterDiscard.document, revision: saved.document.revision },
  saved.document,
  'discarding returns to the saved content',
)
assert.ok(afterDiscard.document.revision > saved.document.revision + 1, 'discarded revisions are not reused')
results.discard_requires_confirmation = true
step = 'after discard_requires_confirmation'

// Revision conflict: another client edits the same session; the next UI edit is rejected.
const id = saved.document.id
await page.evaluate(async ({ id, rev }) => {
  const session = await (await fetch('/api/v1/session')).json()
  await fetch(`/api/v1/documents/${id}/transactions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', [session.header]: session.token },
    body: JSON.stringify({ request_id: 'other-client', expected_revision: rev, commands: [{ op: 'rename', title: 'Other client' }] }),
  })
}, { id, rev: afterDiscard.document.revision })
await page.getByRole('button', { name: 'Undo' }).click()
await page.getByText('The document changed', { exact: true }).waitFor()
await page.getByRole('button', { name: 'Reload' }).click()
await waitIdle()
assert.equal(await page.getByLabel('Document title').inputValue(), 'Other client')
results.revision_conflict_and_reload = true
step = 'after revision_conflict_and_reload'

// Save failure: the file path becomes a directory, then is restored.
const file = join(dataDir, 'documents', `${id}.json`)
const committed = readFileSync(file)
rmSync(file)
mkdirSync(file)
await page.keyboard.press('Control+s')
await page.getByText('Storage failed', { exact: true }).waitFor()
assert.equal(await saveState(), 'Unsaved changes')
rmSync(file, { recursive: true })
writeFileSync(file, committed)
await page.getByRole('button', { name: 'Save', exact: true }).click()
await waitIdle()
assert.equal(await saveState(), 'Saved')
assert.equal(await page.getByText('Storage failed', { exact: true }).count(), 0)
results.save_failure_and_retry = true
step = 'after save_failure_and_retry'

// ── Storage feedback ──────────────────────────────────────────────────────────────────────
const stored = () => JSON.parse(readFileSync(file, 'utf8')).document
const retitle = async (title) => {
  await page.getByLabel('Document title').fill(title)
  await page.getByLabel('Document title').press('Enter')
  await waitIdle()
}
const saveAgain = async () => {
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  await waitIdle()
  assert.equal(await saveState(), 'Saved')
}
/** Request IDs of the page's saves, in the order sent. */
const saveIds = []
const recordSaveId = (request) => {
  if (request.method() === 'POST' && request.url().endsWith('/save')) saveIds.push(request.postDataJSON().request_id)
}
page.on('request', recordSaveId)

// Read-only storage is named as such, and nothing is written.
if (runningAsRoot) {
  results.skipped_steps.push('read_only_storage_is_named')
} else {
  await retitle('Read-only attempt')
  const documentsDir = join(dataDir, 'documents')
  const before = readFileSync(file)
  chmodSync(documentsDir, 0o500)
  try {
    await page.keyboard.press('Control+s')
    await page.getByText('Storage is read-only', { exact: true }).waitFor()
  } finally {
    chmodSync(documentsDir, 0o700)
  }
  assert.equal(await saveState(), 'Unsaved changes')
  assert.deepEqual(readFileSync(file), before, 'nothing was written')
  await saveAgain()
  results.read_only_storage_is_named = true
}
step = 'after read_only_storage_is_named'

// A CLI process holds the document's save lock (its commit pauses inside the lock): the save
// is refused as busy within the page's deadline, and saving later works.
await retitle('Busy attempt')
const lockFile = join(dataDir, 'locks', `${id}.lock`)
const holder = spawn(cliPath, [
  'documents', 'apply', id, '--expected-revision', String(stored().revision),
  '--commands', JSON.stringify([{ op: 'rename', title: 'Held by the CLI' }]),
], { env: { ...env, CHEMIFREE_TEST_COMMIT_DELAY_MS: '60000' }, stdio: 'ignore' })
const holderExit = new Promise((resolveExit) => holder.once('exit', resolveExit))
/** Held once a non-blocking attempt to take the lock is refused. */
const lockHeld = () => {
  try {
    execFileSync('flock', ['--nonblock', '--conflict-exit-code', '9', lockFile, 'true'])
    return false
  } catch (error) {
    if (error.status === 9) return true
    throw error
  }
}
while (!existsSync(lockFile) || !lockHeld()) await new Promise((resolvePoll) => setTimeout(resolvePoll, 20))
const beforeBusy = readFileSync(file)
await page.keyboard.press('Control+s')
await page.getByText('Document busy', { exact: true }).waitFor({ timeout: 15_000 })
assert.equal(await saveState(), 'Unsaved changes')
holder.kill('SIGKILL')
await holderExit
assert.deepEqual(readFileSync(file), beforeBusy, 'neither save committed')
await saveAgain()
assert.equal(stored().title, 'Busy attempt')
results.busy_document_is_named_and_retried = true
step = 'after busy_document_is_named_and_retried'

// The service commits, but the reply arrives after the page's 10-second deadline. The page
// asks again with the same request ID and gets the committed result, not an error. The route
// stays until the step ends, so later saves pass through it untouched.
await retitle('Late reply')
const lateRevision = await revision()
let lateSaves = 0
const held = []
await page.route('**/save', async (route) => {
  lateSaves += 1
  if (lateSaves > 1) return route.fallback()
  const response = await route.fetch() // The service commits now.
  held.push(response.status())
  await new Promise((resolveHold) => setTimeout(resolveHold, 11_000))
  await route.fulfill({ response }).catch(() => {}) // The page gave up on it.
})
saveIds.length = 0
const confirmed = page.waitForResponse((r) => r.url().endsWith('/save') && r.status() === 200, { timeout: 20_000 })
await page.keyboard.press('Control+s')
await confirmed
await waitIdle()
await page.unroute('**/save')
assert.deepEqual(held, [200], 'the first attempt committed')
assert.equal(saveIds.length, 2, `asked again once (${saveIds.length} requests)`)
assert.equal(saveIds[1], saveIds[0], 'with the same request ID')
assert.equal(await saveState(), 'Saved')
for (const title of ['Storage failed', 'Save not confirmed']) {
  assert.equal(await page.getByText(title, { exact: true }).count(), 0, `no "${title}" shown`)
}
assert.deepEqual([stored().title, stored().revision], ['Late reply', lateRevision], 'committed once')
results.late_save_reply_is_confirmed_not_failed = true
step = 'after late_save_reply_is_confirmed_not_failed'

// The connection drops after the service answers, for the save and for the immediate check:
// the outcome is unknown, and saying so is honest. Checking again asks about the same save
// and finds it committed.
await retitle('Unconfirmed')
const unconfirmedRevision = await revision()
const dropped = []
await page.route('**/save', async (route) => {
  if (dropped.length === 2) return route.fallback()
  dropped.push((await route.fetch()).status()) // Answered by the service…
  await route.abort('connectionreset') // …but the page never gets the reply.
})
saveIds.length = 0
await page.keyboard.press('Control+s')
await page.getByText('Save not confirmed', { exact: true }).waitFor()
assert.deepEqual(dropped, [200, 200], 'committed, then replayed')
assert.equal(stored().title, 'Unconfirmed', 'the service committed it meanwhile')
await page.getByRole('alert').getByRole('button', { name: 'Check again' }).click()
await waitIdle()
await page.unroute('**/save')
assert.equal(await saveState(), 'Saved')
assert.equal(saveIds.length, 3, `two attempts, then the check (${saveIds.length} requests)`)
assert.ok(saveIds.every((saveId) => saveId === saveIds[0]), `one request ID throughout: ${saveIds.join(', ')}`)
assert.deepEqual([stored().title, stored().revision], ['Unconfirmed', unconfirmedRevision], 'committed once')
page.off('request', recordSaveId)
results.unconfirmed_save_is_checked_not_repeated = true
step = 'after unconfirmed_save_is_checked_not_repeated'

// Save conflict: the CLI commits a change first.
await tool('Atom')
const conflictDoc = await openDoc()
const a1c = await atomScreen(conflictDoc, 'a1')
await page.getByRole('button', { name: 'Element N' }).click()
await page.mouse.click(a1c.x, a1c.y)
await waitIdle()
execFileSync(cliPath, [
  'documents', 'apply', id, '--expected-revision', String(conflictDoc.document.revision),
  '--commands', JSON.stringify([{ op: 'rename', title: 'CLI title' }]),
], { env })
await page.keyboard.press('Control+s')
await page.getByText('Saved elsewhere', { exact: true }).waitFor()
await page.getByRole('button', { name: 'Close document' }).click()
await page.getByRole('button', { name: 'Discard and close' }).click()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
assert.equal(await page.getByLabel('Document title').inputValue(), 'CLI title')
results.save_conflict_with_cli = true
step = 'after save_conflict_with_cli'

// Input while an edit is in flight: slow transactions so the view still shows the old revision.
await page.route('**/transactions', async (route) => {
  await new Promise((resolve) => setTimeout(resolve, 400))
  await route.continue()
})
await tool('Bond')
let raceDoc = await openDoc()
const a1r = await atomScreen(raceDoc, 'a1')
await page.mouse.click(a1r.x, a1r.y)
await page.mouse.click(a1r.x, a1r.y)
await waitIdle()
raceDoc = await openDoc()
const positions = raceDoc.document.atoms.map(({ position }) => `${position.x},${position.y}`)
assert.equal(new Set(positions).size, positions.length, 'no atoms drawn on top of each other')

await tool('Select and move')
await page.mouse.click(a1r.x, a1r.y)
await page.getByRole('heading', { name: 'Atom a1' }).waitFor()
const charge = (await openDoc()).document.atoms.find((atom) => atom.id === 'a1').charge
await page.getByRole('button', { name: 'Increase charge' }).click()
await page.getByRole('button', { name: 'Increase charge' }).click()
await waitIdle()
assert.equal((await openDoc()).document.atoms.find((atom) => atom.id === 'a1').charge, charge + 2, 'both clicks count')
results.edits_while_a_request_is_pending = true
step = 'after edits_while_a_request_is_pending'

const isotope = page.getByLabel('Isotope mass number')
await isotope.fill('13')
await isotope.press('Enter')
await waitIdle()
await isotope.fill('abc')
await isotope.press('Enter')
await page.getByText('Enter a whole number from 1 to 999', { exact: false }).waitFor()
await waitIdle()
assert.equal((await openDoc()).document.atoms.find((atom) => atom.id === 'a1').isotope, 13, 'invalid text is not sent')
// A rejected element followed at once by a successful edit: the rejection stays visible and the
// field shows the atom's element again.
const elementField = page.getByLabel('Element', { exact: true })
const elementBefore = (await openDoc()).document.atoms.find((atom) => atom.id === 'a1').element
await elementField.fill('Xx')
await elementField.press('Enter')
await page.getByRole('button', { name: 'Increase charge' }).click()
await waitIdle()
await page.getByText('Edit rejected', { exact: true }).waitFor()
assert.equal(await elementField.inputValue(), elementBefore, 'the field is reset to the stored element')
await page.getByRole('button', { name: 'Dismiss' }).click()
await page.unroute('**/transactions')
results.rejected_edit_stays_reported_and_field_resets = true

// An API restart: the page's token is refused. If the restarted service reopened the same
// drawing the edit is sent again; if the drawing changed, the page asks for a reload.
const refuseToken = () =>
  page.route(
    '**/transactions',
    (route) =>
      route.fulfill({
        status: 401,
        contentType: 'application/json',
        body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'missing or invalid session token' }),
      }),
    { times: 1 },
  )
const a1Charge = async () => (await openDoc()).document.atoms.find((atom) => atom.id === 'a1').charge
const chargeBeforeRestart = await a1Charge()
await refuseToken()
const retriedEdit = page.waitForResponse(
  (r) => r.request().method() === 'POST' && r.url().endsWith('/transactions') && r.status() === 200,
)
await page.getByRole('button', { name: 'Increase charge' }).click()
await retriedEdit
await waitIdle()
assert.equal(await a1Charge(), chargeBeforeRestart + 1, 'same drawing: the edit is retried')
assert.equal(await page.getByText('Local service restarted', { exact: true }).count(), 0)

const changedElsewhere = await openDoc()
await page.evaluate(async ({ id, rev }) => {
  const session = await (await fetch('/api/v1/session')).json()
  await fetch(`/api/v1/documents/${id}/transactions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', [session.header]: session.token },
    body: JSON.stringify({ request_id: 'restart-other', expected_revision: rev, commands: [{ op: 'rename', title: 'Changed elsewhere' }] }),
  })
}, { id: changedElsewhere.document.id, rev: changedElsewhere.document.revision })
const beforeRestart = await revision()
await refuseToken()
await page.getByRole('button', { name: 'Increase charge' }).click()
await page.getByText('Local service restarted', { exact: true }).waitFor()
assert.equal(await revision(), beforeRestart, 'changed drawing: nothing was applied')
await page.getByRole('button', { name: 'Reload' }).click()
await waitIdle()
assert.equal(await page.getByLabel('Document title').inputValue(), 'Changed elsewhere')
results.api_restart_retries_only_unchanged_drawings = true
results.invalid_isotope_is_rejected = true
step = 'after invalid_isotope_is_rejected'

// A create whose response is lost: the server commits the document, the page sees a network
// failure, and retrying must return that document instead of creating a second one.
await page.getByRole('button', { name: 'Close document' }).click()
await page.getByRole('button', { name: 'Discard and close' }).click()
await page.getByText('No document open').waitFor()
const documentsBefore = (await api('/api/v1/documents')).documents.length
let lostCreate = null
await page.route('**/api/v1/documents', async (route) => {
  if (route.request().method() !== 'POST') return route.continue()
  const response = await route.fetch()
  if (lostCreate) return route.fulfill({ response })
  lostCreate = await response.json()
  return route.abort('connectionreset')
})
await page.getByRole('button', { name: 'New document' }).click()
await page.getByText('could not be reached', { exact: false }).waitFor()
const retried = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
await page.getByRole('button', { name: 'New document' }).click()
assert.equal((await (await retried).json()).document.id, lostCreate.document.id, 'the retry replays the lost create')
await page.getByLabel('Document title').waitFor()
assert.equal((await api('/api/v1/documents')).documents.length, documentsBefore + 1, 'exactly one document created')
await page.unroute('**/api/v1/documents')
results.lost_create_response_is_retried_without_duplicates = true
step = 'after lost_create_response_is_retried_without_duplicates'

// The same lost create, but the service restarts before the retry: it has forgotten the
// request_id, so the page does not resend it and asks the user to check the list instead.
await page.getByRole('button', { name: 'Close document' }).click()
await page.getByText('No document open').waitFor()
const beforeUnconfirmed = (await api('/api/v1/documents')).documents.length
let createsCommitted = 0
let createsRefused = 0
await page.route('**/api/v1/documents', async (route) => {
  if (route.request().method() !== 'POST') return route.continue()
  if (createsCommitted === 0) {
    createsCommitted += 1
    await route.fetch()
    return route.abort('connectionreset')
  }
  createsRefused += 1
  return route.fulfill({
    status: 401,
    contentType: 'application/json',
    body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'missing or invalid session token' }),
  })
})
await page.getByRole('button', { name: 'New document' }).click()
await page.getByText('could not be reached', { exact: false }).waitFor()
await page.getByRole('button', { name: 'New document' }).click()
await page.getByText('New document not confirmed', { exact: true }).waitFor()
await waitIdle()
assert.equal(createsRefused, 1, 'the unconfirmed create is not resent after the restart')
assert.equal((await api('/api/v1/documents')).documents.length, beforeUnconfirmed + 1, 'no duplicate')
await page.unroute('**/api/v1/documents')
await page.getByRole('button', { name: 'New document' }).click()
await page.getByLabel('Document title').waitFor()
assert.equal((await api('/api/v1/documents')).documents.length, beforeUnconfirmed + 2, 'a deliberate new create works')
results.unconfirmed_create_is_not_resent_after_restart = true
step = 'after unconfirmed_create_is_not_resent_after_restart'

// ── Autosave and crash recovery ─────────────────────────────────────────────────────────
const draftState = page.getByTestId('draft-state')
const draftKind = () => draftState.getAttribute('data-state')
const waitDraft = (kind) =>
  page.waitForFunction((value) => document.querySelector('[data-testid="draft-state"]')?.getAttribute('data-state') === value, kind, {
    timeout: 15_000,
  })
const draftFile = () => join(dataDir, 'drafts', `${docId}.json`)
/** Clicks and waits for the POST it causes, so the next check sees its result. */
const clickFor = async (locator, path) => {
  const response = page.waitForResponse((r) => r.request().method() === 'POST' && r.url().endsWith(path))
  await locator.click()
  await response
  await waitIdle()
}
/**
 * Draws a bond and waits until the page has taken in the result. The response arrives before
 * the page renders it, and until then the draft state may still read "saved" for the revision
 * before, which `waitDraft('saved')` would take for the draft of this edit.
 */
const drawPair = async () => {
  await tool('Bond')
  const area = await page.locator('svg[data-structure-canvas]').boundingBox()
  const response = page.waitForResponse((r) => r.request().method() === 'POST' && r.url().endsWith('/transactions'))
  await page.mouse.click(area.x + area.width * 0.3, area.y + area.height * 0.7)
  const answered = await response
  if (answered.ok()) {
    const { revision } = (await answered.json()).document
    await page.waitForFunction((value) => {
      const draft = document.querySelector('[data-testid="draft-state"]')
      const saved = draft?.getAttribute('data-state') === 'saved' ? /\(revision (\d+)\)/.exec(draft.textContent) : null
      return saved === null || Number(saved[1]) >= value
    }, revision)
  }
  await waitIdle()
}
const waitForTitle = (title) =>
  page.waitForFunction((value) => document.querySelector('input[aria-label="Document title"]')?.value === value, title)
const closeShown = async () => {
  await page.getByRole('button', { name: 'Close document' }).click()
  const discard = page.getByRole('button', { name: 'Discard and close' })
  if (await discard.isVisible().catch(() => false)) await discard.click()
  await page.getByText('No document open').waitFor()
}

await closeShown()
const recoveryCreated = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
await page.getByRole('button', { name: 'New document' }).click()
docId = (await (await recoveryCreated).json()).document.id
await page.getByLabel('Document title').waitFor()
assert.equal(await draftKind(), 'clean')
await drawPair()
assert.match(await page.getByTestId('file-state').textContent(), /last saved at revision 0/)
await waitDraft('saved')
assert.ok(existsSync(draftFile()), 'the draft is written apart from the document file')
const drawnBeforeCrash = (await openDoc()).document
results.autosave_writes_a_draft_and_shows_its_state = true
step = 'after autosave_writes_a_draft_and_shows_its_state'

// The API crashes. The page notices on its next request and asks for a reload; the reloaded
// document offers the draft, and recovering it restores the drawing with its undo history.
await crashApi()
await drawPair()
await page.getByText('Local service restarted', { exact: true }).waitFor()
await page.getByRole('button', { name: 'Reload' }).click()
await page.getByTestId('recovery-notice').waitFor()
assert.equal(await page.getByTestId('recovery-notice').getAttribute('data-state'), 'recoverable')
assert.equal(await saveState(), 'Saved', 'storage still holds the empty commit')
await page.screenshot({ path: join(outDir, 'editor-recovery.png') })
await clickFor(page.getByRole('button', { name: 'Recover', exact: true }), '/recover')
const recoveredDoc = await openDoc()
assert.deepEqual(recoveredDoc.document.atoms, drawnBeforeCrash.atoms)
assert.deepEqual(recoveredDoc.document.bonds, drawnBeforeCrash.bonds)
assert.equal(await saveState(), 'Unsaved changes')
assert.equal(await page.getByTestId('recovery-notice').count(), 0)
await waitDraft('saved')
await clickFor(page.getByRole('button', { name: 'Undo' }), '/undo')
assert.equal((await openDoc()).document.atoms.length, 0, 'undo history came back with the draft')
await clickFor(page.getByRole('button', { name: 'Redo' }), '/redo')
await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
assert.equal(await saveState(), 'Saved')
assert.ok(!existsSync(draftFile()), 'saving removes the draft')
await crashApi()
await page.reload()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
assert.equal(await page.getByTestId('recovery-notice').count(), 0, 'a saved draft never comes back')
results.crash_recovery_restores_drawing_and_history = true
step = 'after crash_recovery_restores_drawing_and_history'

// Discarding a draft after a crash asks first, then removes it for good.
await drawPair()
await waitDraft('saved')
await crashApi()
await page.reload()
const listItem = page.locator(`li[data-document-id="${docId}"]`)
await listItem.getByText('Unsaved work found').waitFor()
await listItem.getByRole('button', { name: 'Open' }).click()
await page.getByTestId('recovery-notice').waitFor()
await page.getByTestId('recovery-notice').getByRole('button', { name: 'Discard draft' }).click()
const relistedAfterDiscard = listRead()
await clickFor(page.getByRole('alertdialog').getByRole('button', { name: 'Discard draft' }), '/discard-draft')
await page.getByTestId('recovery-notice').waitFor({ state: 'detached' })
assert.ok(!existsSync(draftFile()))
await relistedAfterDiscard
await crashApi()
await page.reload()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
assert.equal(await page.getByTestId('recovery-notice').count(), 0, 'a discarded draft never comes back')
results.discarded_draft_stays_gone = true
step = 'after discarded_draft_stays_gone'

// A draft whose base the CLI saved over is recovered as a separate copy.
await drawPair()
await waitDraft('saved')
const savedRevision = (await openDoc()).session.saved_revision
await crashApi()
const cliSaved = JSON.parse(
  execFileSync(cliPath, ['documents', 'apply', docId, '--expected-revision', String(savedRevision), '--json', '--commands',
    JSON.stringify([{ op: 'rename', title: 'Saved by the CLI' }])], { env, encoding: 'utf8' }),
)
assert.equal(cliSaved.document.title, 'Saved by the CLI')
await page.reload()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByTestId('recovery-notice').waitFor()
assert.equal(await page.getByTestId('recovery-notice').getAttribute('data-state'), 'outdated')
const outdatedDraftAtoms = JSON.parse(readFileSync(draftFile(), 'utf8')).document.molecule.atoms.length
const recoveredCopyResponse = page.waitForResponse((r) => r.url().endsWith('/recover-as-copy'))
await page.getByRole('button', { name: 'Recover as copy' }).click()
const recoveredCopy = await (await recoveredCopyResponse).json()
await page.getByTestId('copy-notice').waitFor()
assert.equal(recoveredCopy.document.atoms.length, outdatedDraftAtoms, 'the copy holds the draft')
assert.equal(await page.getByLabel('Document title').inputValue(), recoveredCopy.document.title)
assert.match(recoveredCopy.document.title, /\(recovered\)$/)
assert.equal((await api(`/api/v1/documents/${docId}`)).document.title, 'Saved by the CLI', 'the original keeps the CLI commit')
assert.ok(!existsSync(draftFile()))
await page.getByRole('button', { name: 'Open original' }).click()
await waitForTitle('Saved by the CLI')
results.outdated_draft_is_recovered_as_a_copy = true
step = 'after outdated_draft_is_recovered_as_a_copy'

// After a restart the page trusts only what the new service says about drafts: work at
// revision numbers the old process had drafted is drafted again, and an earlier draft shows
// as blocking autosave for as long as it exists, saved or not.
const draftRevision = () => JSON.parse(readFileSync(draftFile(), 'utf8')).document.revision
for (let i = 0; i < 3; i += 1) await drawPair()
await waitDraft('saved')
const draftedBeforeRestart = (await openDoc()).document.revision
await crashApi()
await drawPair()
await page.getByText('Local service restarted', { exact: true }).waitFor()
await page.getByRole('button', { name: 'Reload' }).click()
await page.getByTestId('recovery-notice').waitFor()
await drawPair()
await waitDraft('blocked')
await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
assert.equal(await page.getByTestId('recovery-notice').getAttribute('data-state'), 'outdated')
await drawPair()
await waitDraft('blocked')
await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
await page.getByTestId('recovery-notice').getByRole('button', { name: 'Discard draft' }).click()
await clickFor(page.getByRole('alertdialog').getByRole('button', { name: 'Discard draft' }), '/discard-draft')
await page.getByTestId('recovery-notice').waitFor({ state: 'detached' })
await drawPair()
const afterRestart = (await openDoc()).document.revision
assert.ok(afterRestart <= draftedBeforeRestart, `a revision the old process drafted (${afterRestart} <= ${draftedBeforeRestart})`)
await waitDraft('saved')
assert.ok(existsSync(draftFile()), 'the new service holds a draft of the work')
assert.equal(draftRevision(), afterRestart)
results.drafts_after_a_restart_follow_the_new_service = true
step = 'after drafts_after_a_restart_follow_the_new_service'

// A failed autosave is shown and retried; the saved file is untouched meanwhile. This
// service records its drafting failures, so the page shows that record and leaves the retries
// to the service; a service from before the record leaves them to the page.
const failureSteps = [
  'autosave_retry_is_not_postponed_by_edits',
  'autosave_failure_is_shown_and_retried',
  'a_recorded_failure_is_retried_by_the_service_alone',
  'a_failed_first_read_of_the_record_still_leaves_the_retry_to_the_service',
  'a_failed_read_of_the_outcome_is_made_again',
  'another_document_never_shows_a_draft_failure',
  'a_restart_while_drafts_fail_stops_drafting_and_keeps_the_drawing',
  'a_service_without_failure_records_leaves_retries_to_the_page',
]
if (runningAsRoot) {
  results.skipped_steps.push(...failureSteps)
} else {
  const draftsDir = join(dataDir, 'drafts')
  const isAutosave = (request) => request.method() === 'POST' && request.url().endsWith(`/${docId}/autosave`)
  const isDocumentRead = (request) => request.method() === 'GET' && request.url().endsWith(`/api/v1/documents/${docId}`)
  /** Counts the page's draft requests until the returned function is called, which gives the count. */
  const countDrafts = () => {
    let count = 0
    const counted = (request) => {
      if (isAutosave(request)) count += 1
    }
    page.on('request', counted)
    return () => {
      page.off('request', counted)
      return count
    }
  }
  /** Aborts the next `count` reads of the open document; the returned function removes the route and gives how many reads there were. */
  const cutReads = async (count) => {
    let reads = 0
    const route = (routed) => {
      if (!isDocumentRead(routed.request())) return routed.fallback()
      reads += 1
      return reads <= count ? routed.abort('connectionreset') : routed.fallback()
    }
    await page.route(`**/api/v1/documents/${docId}`, route)
    return async () => {
      await page.unroute(`**/api/v1/documents/${docId}`, route)
      return reads
    }
  }
  const failedBy = (by, attemptsAtLeast = 1) =>
    page.waitForFunction(
      ([who, least]) => {
        const draft = document.querySelector('[data-testid="draft-state"]')
        if (draft?.getAttribute('data-state') !== 'failed' || draft.getAttribute('data-by') !== who) return false
        const times = /failed (\d+) times/.exec(draft.textContent ?? '')
        return (times ? Number(times[1]) : 1) >= least
      },
      [by, attemptsAtLeast],
      { timeout: 20_000 },
    )

  step = 'autosave_retry_is_not_postponed_by_edits'
  // The retry keeps its schedule (2 s after the first failure) while edits keep coming.
  const draftedBefore = draftRevision()
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await waitDraft('failed')
  chmodSync(draftsDir, 0o700)
  const drawingUntil = performance.now() + 8_000
  while (draftRevision() <= draftedBefore && performance.now() < drawingUntil) {
    await drawPair()
    await page.waitForTimeout(300)
  }
  assert.ok(draftRevision() > draftedBefore, 'a retry drafted the work while edits kept coming')
  await waitDraft('saved')
  results.autosave_retry_is_not_postponed_by_edits = true

  step = 'autosave_failure_is_shown_and_retried'
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await waitDraft('failed')
  assert.equal(await saveState(), 'Unsaved changes')
  chmodSync(draftsDir, 0o700)
  await clickFor(page.getByRole('button', { name: 'Retry now' }), '/autosave')
  await waitDraft('saved')
  assert.ok(existsSync(draftFile()))
  results.autosave_failure_is_shown_and_retried = true

  step = 'a_recorded_failure_is_retried_by_the_service_alone'
  // The service's record is shown with what to do about it, and its retry is the only one.
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await failedBy('service', 2)
  const shown = await draftState.textContent()
  assert.match(shown, /the service retries at/, shown)
  assert.match(shown, /Make the data directory writable/, shown)
  const pageDrafts = countDrafts()
  chmodSync(draftsDir, 0o700)
  await waitDraft('saved')
  assert.equal(pageDrafts(), 0, 'the service drafted on its own retry; the page only read the outcome')
  assert.equal(draftRevision(), (await openDoc()).document.revision)
  results.a_recorded_failure_is_retried_by_the_service_alone = true

  step = 'a_failed_first_read_of_the_record_still_leaves_the_retry_to_the_service'
  // The page's own draft fails, and its read of the service's record of that failure fails
  // too. The page must not fall back to drafting beside the service, which would count two
  // attempts and skip ahead in the service's backoff.
  let readsCut = await cutReads(1)
  chmodSync(draftsDir, 0o500)
  const draftsSinceEdit = countDrafts()
  await drawPair()
  await failedBy('service', 2)
  assert.equal(draftsSinceEdit(), 1, 'only the page’s first draft was sent')
  assert.ok((await readsCut()) >= 2, 'the record was read again after the failed read')
  chmodSync(draftsDir, 0o700)
  await waitDraft('saved')
  results.a_failed_first_read_of_the_record_still_leaves_the_retry_to_the_service = true

  step = 'a_failed_read_of_the_outcome_is_made_again'
  // Once the service's retry is due the page reads its outcome; when that read fails it reads
  // again rather than leave an expired failure on screen.
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await failedBy('service')
  readsCut = await cutReads(1)
  chmodSync(draftsDir, 0o700)
  await waitDraft('saved')
  assert.ok((await readsCut()) >= 2, 'the outcome was read again after a failed read')
  results.a_failed_read_of_the_outcome_is_made_again = true

  step = 'another_document_never_shows_a_draft_failure'
  // Another document never shows that failure, and the one left behind is not read meanwhile.
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await failedBy('service')
  const failingId = docId
  const readAgainBy = Date.now() + (await openDoc()).session.draft_failure.retry_in_ms + 2_000
  chmodSync(draftsDir, 0o700)
  await closeShown()
  let failingReads = 0
  const countFailingReads = (request) => {
    if (request.method() === 'GET' && request.url().endsWith(`/api/v1/documents/${failingId}`)) failingReads += 1
  }
  page.on('request', countFailingReads)
  const otherDocument = page.locator(
    `section[aria-labelledby="saved-documents"] li[data-document-id]:not([data-document-id="${failingId}"])`,
  )
  await otherDocument.first().getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  await waitDraft('clean')
  await page.waitForFunction((at) => Date.now() > at, readAgainBy, { timeout: 60_000 })
  page.off('request', countFailingReads)
  assert.equal(await draftKind(), 'clean', 'the other document shows its own state')
  assert.equal(failingReads, 0, 'the document left behind is not read again')
  await closeShown()
  await page.locator(`li[data-document-id="${failingId}"]`).getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  await drawPair()
  await waitDraft('saved')
  results.another_document_never_shows_a_draft_failure = true

  step = 'a_restart_while_drafts_fail_stops_drafting_and_keeps_the_drawing'
  // The service restarts while it cannot draft, so the drawing is in no draft. The page's read
  // of the outcome meets the new process, which holds only the stored document: the page stops
  // drafting and keeps the drawing on screen rather than showing what the new process holds.
  // The page is loaded afresh first and only opens the document, so it has written nothing to
  // the old process; it must still tell the processes apart.
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await failedBy('service')
  await page.reload()
  await page.locator(`section[aria-labelledby="saved-documents"] li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  await failedBy('service')
  const canvas = page.locator('svg[data-structure-canvas]')
  const drawn = await canvas.getAttribute('aria-label')
  // Reads of the document wait while the API is down, so none of them meets the proxy's 502.
  let apiBack
  const restarted = new Promise((resolve) => (apiBack = resolve))
  const holdReads = async (route) => {
    if (!isDocumentRead(route.request())) return route.fallback()
    await restarted
    return route.fallback()
  }
  await page.route(`**/api/v1/documents/${docId}`, holdReads)
  await crashApi()
  apiBack()
  await waitDraft('stopped')
  await page.unroute(`**/api/v1/documents/${docId}`, holdReads)
  await page.getByText('Local service restarted', { exact: true }).waitFor()
  assert.equal(await canvas.getAttribute('aria-label'), drawn, 'the unsaved drawing stays on screen')
  chmodSync(draftsDir, 0o700)
  // The reloaded document offers the old process's last draft, from before drafting failed.
  await page.getByRole('button', { name: 'Reload' }).click()
  await page.getByTestId('recovery-notice').waitFor()
  await page.getByTestId('recovery-notice').getByRole('button', { name: 'Discard draft' }).click()
  await clickFor(page.getByRole('alertdialog').getByRole('button', { name: 'Discard draft' }), '/discard-draft')
  await page.getByTestId('recovery-notice').waitFor({ state: 'detached' })
  await waitDraft('clean')
  results.a_restart_while_drafts_fail_stops_drafting_and_keeps_the_drawing = true

  step = 'a_service_without_failure_records_leaves_retries_to_the_page'
  // A service from before it recorded failures (no draft_failure) leaves retries to the page.
  const withoutFailureRecord = async (route) => {
    const response = await route.fetch()
    const body = await response.json().catch(() => null)
    if (body?.session) delete body.session.draft_failure
    await route.fulfill(body ? { response, json: body } : { response })
  }
  await page.route(`**/api/v1/documents/${docId}**`, withoutFailureRecord)
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await failedBy('page')
  const pageRetry = page.waitForResponse((response) => isAutosave(response.request()) && response.status() === 200, {
    timeout: 15_000,
  })
  chmodSync(draftsDir, 0o700)
  await pageRetry
  await waitDraft('saved')
  await page.unroute(`**/api/v1/documents/${docId}**`, withoutFailureRecord)
  results.a_service_without_failure_records_leaves_retries_to_the_page = true
}
step = 'after autosave_failure_is_shown_and_retried'

// Save as copy: the unsaved drawing becomes a new document; the original keeps its save.
const originalFile = join(dataDir, 'documents', `${docId}.json`)
const originalBytes = readFileSync(originalFile)
const originalId = docId
// The first copy attempt gets no reply, and asked again, the service says the copy is still
// running. The page keeps asking with the same request ID, so one copy is made. This route is
// removed only after the autosave held below is answered: removing a route while another
// request is held let that request through to the service in a trial run.
const copyIds = []
const recordCopyId = (request) => {
  if (request.url().endsWith('/save-as')) copyIds.push(request.postDataJSON().request_id)
}
page.on('request', recordCopyId)
let copyAttempts = 0
await page.route('**/save-as', async (route) => {
  copyAttempts += 1
  if (copyAttempts === 1) return route.abort('connectionreset')
  if (copyAttempts > 2) return route.fallback()
  return route.fulfill({
    status: 409,
    contentType: 'application/json',
    body: JSON.stringify({ api_version: 'v1', code: 'request_in_progress', message: 'still running' }),
  })
})
const documentFiles = () => readdirSync(join(dataDir, 'documents')).filter((name) => name.endsWith('.json')).length
// An autosave of the original is held until the page has moved to the copy, then answered
// with a pausing error. It must not pause autosave for the copy.
let releaseStale
const staleReleased = new Promise((resolve) => (releaseStale = resolve))
let staleHeld
const staleArrived = new Promise((resolve) => (staleHeld = resolve))
await page.route(
  `**/documents/${originalId}/autosave`,
  async (route) => {
    staleHeld()
    await staleReleased
    await route.fulfill({
      status: 409,
      contentType: 'application/json',
      body: JSON.stringify({ api_version: 'v1', code: 'recovery_pending', message: 'an earlier draft is waiting' }),
    })
  },
  { times: 1 },
)
await drawPair()
await staleArrived
const filesBeforeCopy = documentFiles()
const saveCopy = async () => {
  await page.getByRole('button', { name: 'Save as copy' }).click()
  await page.getByRole('dialog').getByLabel('Title of the copy').fill('Branch')
  await page.getByRole('dialog').getByRole('button', { name: 'Save copy' }).click()
}
await saveCopy()
await page.getByText('Local service unavailable', { exact: true }).waitFor()
const copyResponse = page.waitForResponse((r) => r.url().endsWith('/save-as') && r.ok(), { timeout: 15_000 })
await saveCopy()
const copyBody = await (await copyResponse).json()
page.off('request', recordCopyId)
assert.equal(copyIds.length, 3, `no reply, in progress, then the copy (${copyIds.length} requests)`)
assert.ok(copyIds.every((copyId) => copyId === copyIds[0]), `one request ID throughout: ${copyIds.join(', ')}`)
assert.equal(documentFiles(), filesBeforeCopy + 1, 'one copy was made')
results.copy_in_progress_keeps_its_request_id = true
docId = copyBody.document.id
await page.getByTestId('copy-notice').waitFor()
assert.match(await page.getByTestId('copy-notice').textContent(), /Now editing the copy “Branch”/)
await page.getByRole('dialog').waitFor({ state: 'detached' })
await page.screenshot({ path: join(outDir, 'editor-copy.png') })
assert.equal(await page.getByLabel('Document title').inputValue(), 'Branch')
assert.equal(await saveState(), 'Saved')
assert.notEqual(docId, originalId)
assert.deepEqual(readFileSync(originalFile), originalBytes, 'the original file is untouched')
assert.ok(!existsSync(join(dataDir, 'drafts', `${originalId}.json`)), "the original's draft went with its changes")
const staleAnswered = page.waitForResponse((r) => r.url().endsWith(`/${originalId}/autosave`))
releaseStale()
await staleAnswered
await page.unroute('**/save-as')
await drawPair()
await waitDraft('saved')
results.late_autosave_reply_leaves_the_next_document_autosaving = true
await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
assert.deepEqual(readFileSync(originalFile), originalBytes, 'editing the copy leaves the original alone')
results.save_as_copy_is_independent = true
step = 'after save_as_copy_is_independent'

// Leaving a document inside the autosave delay drafts its pending work first.
await drawPair()
await page.getByRole('button', { name: 'Open original' }).click()
await page.getByTestId('copy-notice').waitFor({ state: 'detached' })
assert.ok(existsSync(draftFile()), 'the copy, left with pending work, has a draft')
assert.equal(draftRevision(), (await api(`/api/v1/documents/${docId}`)).document.revision)
assert.equal((await api(`/api/v1/documents/${originalId}`)).session.unsaved_changes, false)
docId = originalId
results.leaving_a_document_drafts_its_pending_work = true
step = 'after leaving_a_document_drafts_its_pending_work'

// Another client moves the session past the page's view. The page's autosave drafts that
// newer state once and then waits for a newer view, instead of drafting every second.
let autosaves = 0
const countAutosave = (r) => {
  if (r.request().method() === 'POST' && r.url().endsWith(`/${docId}/autosave`)) autosaves += 1
}
page.on('response', countAutosave)
await drawPair()
const shownRevision = (await openDoc()).document.revision
await page.evaluate(async ({ id, rev }) => {
  const session = await (await fetch('/api/v1/session')).json()
  await fetch(`/api/v1/documents/${id}/transactions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', [session.header]: session.token },
    body: JSON.stringify({ request_id: 'moved-elsewhere', expected_revision: rev, commands: [{ op: 'rename', title: 'Elsewhere' }] }),
  })
}, { id: docId, rev: shownRevision })
await page.waitForTimeout(6_000)
page.off('response', countAutosave)
assert.ok(autosaves >= 1 && autosaves <= 2, `drafted once, not every second (${autosaves} autosaves)`)
assert.equal(await draftKind(), 'saved')
assert.equal(draftRevision(), shownRevision + 1, 'the draft holds the newer state')
await drawPair()
await page.getByText('The document changed', { exact: true }).waitFor()
await page.getByRole('button', { name: 'Reload' }).click()
await waitForTitle('Elsewhere')
results.a_session_moved_elsewhere_is_drafted_once = true
step = 'after a_session_moved_elsewhere_is_drafted_once'

// Save as copy while the original's draft cannot be removed (drafts/ is briefly read-only):
// the copy is committed and shown, the original stays open with its unsaved changes and its
// draft, and the notice says so and leads back to it. The notice describes that moment only,
// so it is gone once the page leaves the copy, and once the service restarts while the copy is
// still shown (the restart ends the original's session).
if (runningAsRoot) {
  results.skipped_steps.push('save_as_copy_reports_an_original_left_open', 'copy_notice_ends_when_the_service_restarts')
} else {
  const draftsDir = join(dataDir, 'drafts')
  const notice = page.getByTestId('copy-notice')
  const copyLeavingOpen = async (title) => {
    await drawPair()
    await waitDraft('saved')
    chmodSync(draftsDir, 0o500)
    try {
      const answered = page.waitForResponse((r) => r.url().endsWith('/save-as'))
      await page.getByRole('button', { name: 'Save as copy' }).click()
      await page.getByRole('dialog').getByLabel('Title of the copy').fill(title)
      await page.getByRole('dialog').getByRole('button', { name: 'Save copy' }).click()
      return await (await answered).json()
    } finally {
      chmodSync(draftsDir, 0o700)
    }
  }

  const sourceId = docId
  const sourceTitle = await page.getByLabel('Document title').inputValue()
  const beside = await copyLeavingOpen('Beside')
  const sourceView = await openDoc()
  assert.deepEqual(beside.source, { id: sourceId, closed: false, discarded_changes: false })
  await waitForTitle('Beside')
  await notice.waitFor()
  assert.equal(await notice.getAttribute('data-source'), 'left_open')
  assert.match(await notice.textContent(), new RegExp(`“${sourceTitle}” was left open with its unsaved changes`))
  await page.screenshot({ path: join(outDir, 'editor-copy-left-open.png') })
  assert.ok(existsSync(draftFile()), "the original's draft is kept")
  assert.deepEqual(
    [sourceView.session.unsaved_changes, sourceView.session.draft_revision],
    [true, sourceView.document.revision],
    'the original is still open with its changes and its draft',
  )
  await page.getByRole('button', { name: 'Open original' }).click()
  await waitForTitle(sourceTitle)
  await notice.waitFor({ state: 'detached' })
  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
  assert.ok(!existsSync(draftFile()), "saving the original removes its draft")
  await closeShown()
  docId = beside.document.id
  await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
  await waitForTitle('Beside')
  assert.equal(await notice.count(), 0, 'returning to the copy shows no notice from before')
  results.save_as_copy_reports_an_original_left_open = true
  step = 'after save_as_copy_reports_an_original_left_open'

  const relistedAfterCopy = listRead()
  const after = await copyLeavingOpen('After restart')
  assert.equal(after.source.closed, false)
  await waitForTitle('After restart')
  await notice.waitFor()
  await relistedAfterCopy
  await crashApi()
  docId = after.document.id
  const restartedTitle = 'After restart edited'
  const isRename = (response) => response.request().method() === 'POST' &&
    response.url().endsWith(`/${docId}/transactions`) &&
    response.request().postDataJSON().commands.some((command) => command.op === 'rename' && command.title === restartedTitle)
  const unauthorized = page.waitForResponse((response) => isRename(response) && response.status() === 401)
  const edited = page.waitForResponse((response) => isRename(response) && response.status() === 200)
  // A fixed canvas click can cycle an existing bond instead of creating atoms.
  // Rename makes this lifecycle probe independent of the current drawing and viewport.
  await retitle(restartedTitle)
  await unauthorized
  const committed = await (await edited).json()
  assert.equal(committed.document.title, restartedTitle, 'the explicit edit was retried after the old session was refused')
  assert.equal(committed.document.revision, after.document.revision + 1, 'the restart edit commits exactly once')
  await notice.waitFor({ state: 'detached' })
  const restartedCopy = await openDoc()
  assert.deepEqual(restartedCopy.document, committed.document, 'the restarted service retains the committed edit')
  results.copy_notice_ends_when_the_service_restarts = true
  step = 'after copy_notice_ends_when_the_service_restarts'
}


// Document list: search, sort, rename, a refused and a confirmed move to the trash, restore.
const apiWrite = (path, body) =>
  page.evaluate(async ({ path, body }) => {
    const session = await (await fetch('/api/v1/session')).json()
    const response = await fetch(path, {
      method: 'POST',
      headers: { 'content-type': 'application/json', [session.header]: session.token },
      body: JSON.stringify(body),
    })
    return { status: response.status, body: await response.json() }
  }, { path, body })
await page.keyboard.press('Control+s')
await waitIdle()
await page.getByRole('button', { name: 'Close document' }).click()
const listed = {}
for (const [index, title] of ['Smoke list alpha', 'Smoke list beta'].entries()) {
  listed[title] = (await apiWrite('/api/v1/documents', { request_id: `smoke-list-${index}`, title })).body.document.id
}
const shownTitles = () =>
  page.locator('section[aria-labelledby="saved-documents"] li[data-document-id] span.truncate').allTextContents()
const waitForTitles = (expected) =>
  page.waitForFunction(
    (titles) =>
      JSON.stringify(
        [...document.querySelectorAll('section[aria-labelledby="saved-documents"] li[data-document-id] span.truncate')].map(
          (node) => node.textContent,
        ),
      ) === JSON.stringify(titles),
    expected,
  )
// Recent sorts by the stored file's modification time, a wall-clock time that a clock
// adjustment can move backward (WSL2's clock did, by about 0.7 s, in 2 of 8 local runs). Alpha
// is therefore made an hour old, so the order below shows the rename moving beta first and not
// the clock between the two writes.
const anHourAgo = new Date(Date.now() - 3_600_000)
utimesSync(join(dataDir, 'documents', `${listed['Smoke list alpha']}.json`), anHourAgo, anHourAgo)
utimesSync(join(dataDir, 'documents', `${listed['Smoke list beta']}.json`), anHourAgo, anHourAgo)
await page.getByLabel('Search titles').fill('SMOKE LIST')
await waitForTitles(['Smoke list alpha', 'Smoke list beta'])
const alphaItem = page.locator(`li[data-document-id="${listed['Smoke list alpha']}"]`)
const betaItem = page.locator(`li[data-document-id="${listed['Smoke list beta']}"]`)
await betaItem.getByRole('button', { name: 'Rename Smoke list beta' }).click()
await page.getByLabel('Title', { exact: true }).fill('Smoke list beta, renamed')
await page.getByRole('button', { name: 'Rename', exact: true }).click()
await page.getByRole('dialog').waitFor({ state: 'detached' })
await waitForTitles(['Smoke list alpha', 'Smoke list beta, renamed'])
const fileTitle = (id, folder = 'documents') => JSON.parse(readFileSync(join(dataDir, folder, `${id}.json`), 'utf8')).document.title
assert.equal(fileTitle(listed['Smoke list beta']), 'Smoke list beta, renamed', 'renamed in storage at once')
const sortedByRecent = page.waitForResponse((r) => new URL(r.url()).searchParams.get('sort') === 'recent')
await page.getByRole('radio', { name: 'Recent' }).or(page.getByRole('button', { name: 'Recent', exact: true })).click()
await sortedByRecent
await waitForTitles(['Smoke list beta, renamed', 'Smoke list alpha'])
results.list_search_sort_and_rename = true
step = 'after list_search_sort_and_rename'

// Unsaved work in the service's session is never moved to the trash.
await apiWrite(`/api/v1/documents/${listed['Smoke list alpha']}/transactions`, {
  request_id: 'smoke-list-unsaved',
  expected_revision: 0,
  commands: [{ op: 'rename', title: 'Smoke list alpha, unsaved' }],
})
await page.getByRole('button', { name: 'Refresh', exact: true }).click()
await alphaItem.getByText('Unsaved', { exact: true }).waitFor()
await alphaItem.getByRole('button', { name: /^Move .* to trash$/ }).click()
await page.getByRole('alertdialog').getByRole('button', { name: 'Move to trash' }).click()
await page.getByText('Unsaved changes', { exact: true }).waitFor()
assert.ok(existsSync(join(dataDir, 'documents', `${listed['Smoke list alpha']}.json`)), 'still stored')
await page.getByRole('button', { name: 'Dismiss' }).click()
await apiWrite(`/api/v1/documents/${listed['Smoke list alpha']}/close`, {
  request_id: 'smoke-list-discard',
  discard_unsaved: true,
  expected_revision: 1,
})
await page.getByRole('button', { name: 'Refresh', exact: true }).click()
await alphaItem.getByText('Unsaved', { exact: true }).waitFor({ state: 'detached' })
results.unsaved_document_is_not_trashed = true
step = 'after unsaved_document_is_not_trashed'

// Confirmed: the document moves to the trash, and Restore brings it back.
await alphaItem.getByRole('button', { name: 'Move Smoke list alpha to trash' }).click()
await page.getByRole('alertdialog').getByRole('button', { name: 'Move to trash' }).click()
await waitForTitles(['Smoke list beta, renamed'])
const trashedItem = page.locator(`li[data-trashed-id="${listed['Smoke list alpha']}"]`)
await trashedItem.waitFor()
assert.equal(fileTitle(listed['Smoke list alpha'], 'trash'), 'Smoke list alpha')
assert.ok(!existsSync(join(dataDir, 'documents', `${listed['Smoke list alpha']}.json`)))
await trashedItem.getByRole('button', { name: 'Restore' }).click()
await waitForTitles(['Smoke list beta, renamed', 'Smoke list alpha'])
await page.getByText('The trash is empty.', { exact: true }).waitFor()
assert.equal(fileTitle(listed['Smoke list alpha']), 'Smoke list alpha', 'restored as trashed')
// A rename refused because the document changed elsewhere: the list then shows the document
// as it is now, and Reload, with no document open, only reads the list again.
await apiWrite(`/api/v1/documents/${listed['Smoke list beta']}/rename`, {
  request_id: 'smoke-list-elsewhere',
  expected_revision: 1,
  title: 'Smoke list beta, elsewhere',
})
await betaItem.getByRole('button', { name: 'Rename Smoke list beta, renamed' }).click()
await page.getByLabel('Title', { exact: true }).fill('Smoke list beta, mine')
await page.getByRole('button', { name: 'Rename', exact: true }).click()
await page.getByText('The document changed', { exact: true }).waitFor()
await betaItem.getByText('Smoke list beta, elsewhere', { exact: true }).waitFor()
await page.getByRole('button', { name: 'Reload' }).click()
await page.getByText('The document changed', { exact: true }).waitFor({ state: 'detached' })
assert.equal(await page.getByRole('alert').count(), 0, 'Reload leaves no error behind')
assert.equal(fileTitle(listed['Smoke list beta']), 'Smoke list beta, elsewhere', 'the refused rename changed nothing')
await page.getByLabel('Search titles').fill('')
// The list with its search, sort, and per-document actions fits a narrow screen.
await page.setViewportSize({ width: 320, height: 800 })
const listOverflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
assert.ok(listOverflow <= 0, `no horizontal overflow in the document list at 320px (${listOverflow})`)
await page.screenshot({ path: join(outDir, 'list-mobile.png'), fullPage: true })
await page.setViewportSize({ width: 1280, height: 900 })
await alphaItem.getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
results.trash_and_restore = true
step = 'after trash_and_restore'

// The capabilities card shows per-operation support from the operation catalog, with
// representation apart from execution; the layout checks below run with it open.
const operations = page.locator('#capabilities')
await operations.getByTestId('operation-summary').filter({ hasText: '21 of 31 operations run' }).waitFor()
await operations.getByRole('button', { name: 'Operations' }).click()
const operationRow = (id) => operations.locator(`[data-operation="${id}"]`)
await operationRow('document.transaction').getByText('Runs', { exact: true }).waitFor()
assert.equal(await operationRow('chem.properties').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.properties/formula').locator('[data-execution]').getAttribute('data-execution'), 'executable')
for (const id of ['chem.properties/mass', 'chem.properties/isotope_distribution']) {
  assert.equal(await operationRow(id).locator('[data-execution]').getAttribute('data-execution'), 'requires_data', `${id} needs compatible data`)
}
assert.equal(await operationRow('chem.validate').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.aromaticity').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.aromaticity_convert').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.identity').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.substructure').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.stereo').locator('[data-execution]').getAttribute('data-execution'), 'executable')
assert.equal(await operationRow('chem.cip').locator('[data-execution]').getAttribute('data-execution'), 'executable')
const valenceCapability = operations.getByRole('listitem').filter({ has: page.getByRole('heading', { name: 'Valence inspection' }) })
await valenceCapability.getByText('Available', { exact: true }).waitFor()
for (const id of [
  'document.transaction/scene_placement',
  'document.transaction/add_instance',
  'document.transaction/hydrogen_state',
  'document.transaction/radical',
]) {
  assert.equal(await operationRow(id).locator('[data-execution]').getAttribute('data-execution'), 'executable', `${id} runs`)
}
assert.equal(
  await operations.locator('[data-profile="legacy-observed"] [data-execution]').getAttribute('data-execution'),
  'not_implemented',
  'the legacy-observed profile is not shown as available',
)
results.operation_catalog = true
step = 'after operation_catalog'

// Placements: the open document (alpha, empty) is drawn, its drawing is placed a second time,
// and that placement moves on its own; the chemistry and the first placement stay. A new atom
// is placed in both, and an atom dragged in the second placement moves only there.
{
  const placedDoc = async () => (await api(`/api/v1/documents/${listed['Smoke list alpha']}`)).document
  const positionsOf = (document, instance) =>
    Object.fromEntries(document.scene.instances.find((item) => item.id === instance).positions.map(({ atom, x, y }) => [atom, { x, y }]))
  await tool('Bond')
  const frame = await canvas.boundingBox()
  await page.mouse.click(frame.x + frame.width * 0.25, frame.y + frame.height * 0.3)
  await waitIdle()
  const drawn = await placedDoc()
  assert.deepEqual([drawn.atoms.length, drawn.bonds.length, drawn.scene.instances.length], [2, 1, 1])
  const first = positionsOf(drawn, 's1')

  await page.getByRole('button', { name: 'Duplicate placement' }).click()
  await waitIdle()
  await page.locator('li[data-placement="s2"]').waitFor()
  // Two presses while the first move is still in flight: the first request is held until the
  // second press is in, and both moves count.
  let releaseFirstMove
  const firstMoveHeld = new Promise((resolveHeld) => {
    releaseFirstMove = resolveHeld
  })
  let movesSeen = 0
  const holdFirstMove = async (route) => {
    movesSeen += 1
    if (movesSeen === 1) await firstMoveHeld
    await route.continue()
  }
  await page.route('**/transactions', holdFirstMove)
  const moveRight = page.getByRole('button', { name: 'Move placement s2 right' })
  await moveRight.click()
  await page.waitForFunction(() => document.querySelector('[data-slot="spinner"]'))
  await moveRight.click()
  releaseFirstMove()
  await waitIdle()
  await page.unroute('**/transactions', holdFirstMove)
  assert.equal(await page.getByText('Edit rejected', { exact: true }).count(), 0, 'neither press is refused')
  const placed = await placedDoc()
  assert.deepEqual(placed.scene.instances.map((item) => item.id), ['s1', 's2'])
  assert.deepEqual(positionsOf(placed, 's1'), first, 'the first placement stays')
  for (const atom of ['a1', 'a2']) {
    assert.deepEqual(positionsOf(placed, 's2')[atom], { x: first[atom].x + 60, y: first[atom].y + 80 }, `${atom} in s2`)
  }
  assert.deepEqual([placed.atoms, placed.bonds], [drawn.atoms, drawn.bonds], 'chemistry is shared and unchanged')
  assert.equal(await page.locator('g[data-instance] line[data-bond="b1"]').count(), 2, 'the bond is drawn in both placements')

  // A new atom clicked beside a1 in s2 lands beside a1 in s1 too.
  await page.getByRole('button', { name: 'Element N' }).click()
  const besideA1 = { x: positionsOf(placed, 's2').a1.x - 20, y: positionsOf(placed, 's2').a1.y + 20 }
  const besideA1Screen = await screen(besideA1)
  await page.mouse.click(besideA1Screen.x, besideA1Screen.y)
  await waitIdle()
  const grown = await placedDoc()
  assert.equal(grown.atoms.length, 3)
  const [inFirst, inSecond] = [positionsOf(grown, 's1').a3, positionsOf(grown, 's2').a3]
  assert.ok(Math.abs(inSecond.x - besideA1.x) < 1 && Math.abs(inSecond.y - besideA1.y) < 1, 'placed where clicked')
  assert.ok(
    Math.abs(inFirst.x - (inSecond.x - 60)) < 0.02 && Math.abs(inFirst.y - (inSecond.y - 80)) < 0.02,
    'placed at the same spot relative to a1 in s1',
  )

  // Dragging a1 in s2 moves it there only.
  await tool('Select and move')
  const a1InSecond = await screen(positionsOf(grown, 's2').a1)
  await page.mouse.move(a1InSecond.x, a1InSecond.y)
  await page.mouse.down()
  await page.mouse.move(a1InSecond.x + 15, a1InSecond.y + 15, { steps: 6 })
  await page.mouse.up()
  await waitIdle()
  const dragged = await placedDoc()
  assert.deepEqual(positionsOf(dragged, 's1'), positionsOf(grown, 's1'), 'the first placement stays')
  assert.ok(positionsOf(dragged, 's2').a1.x > positionsOf(grown, 's2').a1.x, 'a1 moved in s2')
  assert.deepEqual(dragged.atoms.map((atom) => atom.position), grown.atoms.map((atom) => atom.position))
  await page.screenshot({ path: join(outDir, 'editor-placements.png') })
  await page.getByRole('button', { name: 'Undo' }).click()
  await waitIdle()
  assert.deepEqual(positionsOf(await placedDoc(), 's2'), positionsOf(grown, 's2'), 'undo puts it back')

  // Another copy is made from the last placement, the moved s2, so it does not land on it.
  await page.getByRole('button', { name: 'Duplicate placement' }).click()
  await waitIdle()
  await page.locator('li[data-placement="s3"]').waitFor()
  const copied = await placedDoc()
  for (const [atom, position] of Object.entries(positionsOf(copied, 's2'))) {
    assert.deepEqual(positionsOf(copied, 's3')[atom], { x: position.x + 40, y: position.y + 80 }, `${atom} in s3`)
  }
}
results.placements_move_independently = true
step = 'after placements_move_independently'

// Recorded states: stated hydrogens and a radical on a bonded carbon, an attachment point on
// another, an aromatic bond, and an inferred atom that shows no count. They are drawn with one
// box per label (bonds stop outside it, the view box holds it) and kept through save, a page
// reload, and reopening from storage. Quick presses built from the latest document are never
// refused, whichever request is held.
{
  const alpha = listed['Smoke list alpha']
  const statesDoc = async () => (await api(`/api/v1/documents/${alpha}`)).document
  const atomOf = async (id) => (await statesDoc()).atoms.find((atom) => atom.id === id)
  const inspector = page.locator('aside[aria-label="Selection"]')
  const press = (name) => inspector.getByRole('button', { name, exact: true }).click()
  const select = async (id, kind = 'atom') => {
    const document = await statesDoc()
    let point
    if (kind === 'atom') {
      point = document.atoms.find((atom) => atom.id === id).position
    } else {
      const bond = document.bonds.find((item) => item.id === id)
      const [begin, end] = [bond.begin, bond.end].map((atom) => document.atoms.find((item) => item.id === atom).position)
      point = { x: (begin.x + end.x) / 2, y: (begin.y + end.y) / 2 }
    }
    // Pressing inspector buttons may have scrolled the canvas out of view.
    await page.evaluate(() => window.scrollTo(0, 0))
    const at = await screen(point)
    await page.mouse.click(at.x, at.y)
    await page.getByRole('heading', { name: `${kind === 'atom' ? 'Atom' : 'Bond'} ${id}` }).waitFor()
  }
  await tool('Select and move')
  await select('a1')
  await press('Hydrogens stated')
  await press('Increase stated hydrogens')
  await press('Increase stated hydrogens')
  await press('Doublet radical')
  await waitIdle()
  await select('a2')
  await press('Attachment point 1')
  await waitIdle()
  await select('b1', 'bond')
  await press('Aromatic bond')
  await waitIdle()
  const a1 = await atomOf('a1')
  assert.deepEqual([a1.hydrogens, a1.radical], [{ mode: 'specified', count: 3 }, 'doublet'], 'quick presses all count')

  /**
   * Runs `first`, holds the request it sends until `second` has been done too, then lets both
   * run. No request may be refused: each is built from the latest document when it is sent.
   */
  const heldRace = async (name, first, second) => {
    let release
    const held = new Promise((resolveHeld) => {
      release = resolveHeld
    })
    let seen = 0
    const holdFirst = async (route) => {
      seen += 1
      if (seen === 1) await held
      await route.continue()
    }
    const refused = []
    const watch = (response) => {
      if (response.url().endsWith('/transactions') && response.status() !== 200) refused.push(response.status())
    }
    page.on('response', watch)
    await page.route('**/transactions', holdFirst)
    await first()
    await page.waitForFunction(() => document.querySelector('[data-slot="spinner"]'))
    await second()
    release()
    await waitIdle()
    await page.unroute('**/transactions', holdFirst)
    page.off('response', watch)
    assert.deepEqual(refused, [], `${name}: no request is refused`)
    assert.equal(await page.getByText('Edit rejected', { exact: true }).count(), 0, name)
  }

  // Charge then query, and query then charge: the query atom carries no charge.
  for (const [first, second] of [
    ['Increase charge', 'Query: any atom'],
    ['Query: any atom', 'Increase charge'],
  ]) {
    await select('a3')
    await heldRace(`${first}, then ${second}`, () => press(first), () => press(second))
    const a3 = await atomOf('a3')
    assert.deepEqual([a3.kind, a3.element, a3.charge], ['query', '*', 0], `${first}, then ${second}`)
    const elementField = page.getByLabel('Element', { exact: true })
    await elementField.fill('N')
    await elementField.press('Enter')
    await waitIdle()
    const nitrogen = await atomOf('a3')
    assert.deepEqual([nitrogen.kind, nitrogen.element, nitrogen.charge], ['element', 'N', 0], 'back to nitrogen')
    assert.equal(
      await inspector.getByRole('button', { name: 'Query: any atom', exact: true }).getAttribute('aria-pressed'),
      'false',
      'no query is shown as chosen for an element atom',
    )
  }

  // Atoms whose symbols are narrow and wide (I, Cl) and a long query list, placed left of a1
  // in s1 (and with it in every placement), so the label checks below cover them; the chlorine
  // gets a charge and the query stated hydrogens, so text follows a short and a long symbol.
  const placeAt = async (symbol, offset) => {
    const origin = (await atomOf('a1')).position
    await page.evaluate(() => window.scrollTo(0, 0))
    await page.getByRole('button', { name: `Element ${symbol}`, exact: true }).click()
    const at = await screen({ x: origin.x + offset.x, y: origin.y + offset.y })
    const before = (await statesDoc()).atoms.length
    await page.mouse.click(at.x, at.y)
    await waitIdle()
    const atoms = (await statesDoc()).atoms
    assert.equal(atoms.length, before + 1, `${symbol} placed`)
    return atoms.at(-1).id
  }
  const iodine = await placeAt('I', { x: -45, y: 0 })
  const chlorine = await placeAt('Cl', { x: -45, y: 35 })
  const queried = await placeAt('N', { x: -115, y: 18 })
  await tool('Select and move')
  await select(queried)
  const listField = inspector.getByLabel('Query element list')
  await listField.fill('N, O')
  await listField.press('Enter')
  await waitIdle()
  // A typed list and the negation pressed while it is on its way: both are kept.
  await heldRace(
    'a typed list, then the negation',
    async () => {
      await listField.fill('N, O, S')
      await listField.press('Enter')
    },
    () => press('Negate the list: any atom except these'),
  )
  assert.deepEqual((await atomOf(queried)).query, { kind: 'list', elements: ['N', 'O', 'S'], negated: true })
  await listField.fill('N, O, S, P, Se')
  await listField.press('Enter')
  await waitIdle()
  assert.equal((await atomOf(queried)).element, '![N,O,S,P,Se]')
  await press('Hydrogens stated')
  await press('Increase stated hydrogens')
  await waitIdle()
  await select(chlorine)
  await press('Increase charge')
  await waitIdle()

  /** Problems with the drawing, measured on the rendered SVG. */
  const drawingProblems = ({ fitted = true } = {}) =>
    page.evaluate(async ({ fitted }) => {
      await document.fonts.ready
      const svg = document.querySelector('svg[data-structure-canvas]')
      const view = svg.viewBox.baseVal
      const problems = []
      const boxOf = (element) => {
        const [x, y, width, height] = element.getAttribute('data-box').split(' ').map(Number)
        return { x, y, width, height }
      }
      // Within, beyond rounding.
      const within = (inner, outer, extraX = 0, extraY = 0) =>
        inner.x >= outer.x - 0.05 - extraX &&
        inner.y >= outer.y - 0.05 - extraY &&
        inner.x + inner.width <= outer.x + outer.width + 0.05 + extraX &&
        inner.y + inner.height <= outer.y + outer.height + 0.05 + extraY
      const matrix = svg.getScreenCTM()
      const scaleX = Math.hypot(matrix.a, matrix.b)
      const scaleY = Math.hypot(matrix.c, matrix.d)
      if (!(scaleX > 0 && scaleY > 0 && Number.isFinite(scaleX) && Number.isFinite(scaleY))) throw new Error('Invalid canvas scale')
      // Chromium quantizes small SVG text bounds in CSS pixels. Position and extent
      // rounding can each contribute half a pixel to the measured far edge. At fitted
      // zoom only, allow their combined one-pixel budget for text, converted through
      // the actual SVG scale. Model boxes, dots, strokes and view containment stay strict.
      // A separate 100% probe below checks the text with no pixel allowance.
      const textExtraX = fitted ? 1 / scaleX : 0
      const textExtraY = fitted ? 1 / scaleY : 0
      const describeBox = ({ x, y, width, height }) =>
        [x, y, width, height].map((value) => Number(value.toFixed(4))).join(',')
      const inView = (box) => within(box, view)
      if (svg.querySelector('[textLength], [lengthAdjust]')) problems.push('text is drawn at a forced length')
      for (const part of fitted ? svg.querySelectorAll('[data-frame], [data-frame-id]') : []) {
        const name = part.getAttribute('data-frame') ?? `${part.getAttribute('data-frame-id')} ID`
        if (!inView(part.getBBox())) problems.push(`frame ${name} outside the view box`)
      }
      // Strictly inside, beyond rounding.
      const inside = (point, box) =>
        point.x > box.x + 0.05 &&
        point.x < box.x + box.width - 0.05 &&
        point.y > box.y + 0.05 &&
        point.y < box.y + box.height - 0.05
      for (const placement of svg.querySelectorAll('g[data-instance]')) {
        const where = placement.getAttribute('data-instance')
        // The box the canvas reserves for each label, which bonds stop outside of.
        const labels = [...placement.querySelectorAll('[data-atom-label]')].map((label) => ({
          id: label.getAttribute('data-atom-label'),
          box: boxOf(label),
          drawn: label.getBBox(),
          // Each text chunk against the box reserved for it; dots against the label's box.
          parts: [...label.querySelectorAll('text, circle')].map((part) => ({
            name: part.getAttribute('data-chunk') ?? part.getAttribute('data-mark'),
            box: part.getBBox(),
            reserved: part.hasAttribute('data-box') ? boxOf(part) : boxOf(label),
            font: getComputedStyle(part).fontFamily,
            text: part.tagName === 'text',
          })),
          // Where each run of the label's text (not its marks) is drawn, left and right, in
          // reading order: from its first character's cell to its last's.
          runs: [...label.querySelectorAll('text[data-chunk] tspan')].map((tspan) => {
            const text = tspan.closest('text')
            let offset = 0
            for (const other of text.querySelectorAll('tspan')) {
              if (other === tspan) break
              offset += other.textContent.length
            }
            const first = text.getExtentOfChar(offset)
            const last = text.getExtentOfChar(offset + tspan.textContent.length - 1)
            return { part: tspan.getAttribute('data-part'), left: first.x, right: last.x + last.width }
          }),
        }))
        for (const { id, box, drawn, parts, runs } of labels) {
          // What precedes and follows the symbol is drawn against it, not apart from it.
          const at = runs.findIndex((item) => item.part === 'symbol')
          for (const [before, after] of [
            [runs[at - 1], runs[at]],
            [runs[at], runs[at + 1]],
          ]) {
            if (!before || !after) continue
            const gap = after.left - before.right
            if (gap > 2 || gap < -1) problems.push(`${where} ${id}: ${gap.toFixed(1)} pt between ${before.part} and ${after.part}`)
          }
          if (drawn.width === 0 || drawn.height === 0) problems.push(`${where} ${id}: empty label`)
          if (fitted && !inView(box)) problems.push(`${where} ${id}: label outside the view box`)
          for (const part of parts) {
            if (!within(part.box, part.reserved, part.text ? textExtraX : 0, part.text ? textExtraY : 0) || !within(part.reserved, box)) {
              problems.push(`${where} ${id}: ${part.name} drawn outside the box reserved for it; actual=[${describeBox(part.box)}] reserved=[${describeBox(part.reserved)}] label=[${describeBox(box)}] scale=${svg.getAttribute('data-viewport-scale')} font=${part.font}`)
            }
          }
        }
        for (const stroke of placement.querySelectorAll('[data-bond]')) {
          const bond = stroke.getAttribute('data-bond')
          const ends =
            stroke.tagName === 'line'
              ? [
                  { x: stroke.x1.baseVal.value, y: stroke.y1.baseVal.value },
                  { x: stroke.x2.baseVal.value, y: stroke.y2.baseVal.value },
                ]
              : [...stroke.points]
          for (const end of ends) {
            for (const { id, box } of labels) {
              if (inside(end, box)) problems.push(`${where} ${bond}: a stroke ends inside the label of ${id}`)
            }
          }
          const [first, last] = [ends[0], ends[ends.length - 1]]
          if (Math.hypot(last.x - first.x, last.y - first.y) < 1) problems.push(`${where} ${bond}: a stroke is not drawn`)
        }
      }
      return problems
    }, { fitted })
  const assertDrawingContainment = async () => {
    await page.getByRole('button', { name: 'Reset zoom (100%)', exact: true }).click()
    await page.waitForFunction(() => {
      const scale = Number(document.querySelector('svg[data-structure-canvas]')?.getAttribute('data-viewport-scale'))
      // Reset multiplies the previous scale by its reciprocal; binary64 may leave
      // the result a few ulps from one. This only tolerates numerical reset noise.
      return Number.isFinite(scale) && Math.abs(scale - 1) <= 1e-12
    })
    // At 100% the complete drawing may extend beyond the viewport. Check all label
    // geometry without viewport containment, then restore Fit and check that as well.
    assert.deepEqual(await drawingProblems({ fitted: false }), [], 'strict label containment at 100%')
    await page.getByRole('button', { name: 'Fit drawing', exact: true }).click()
    assert.deepEqual(await drawingProblems(), [], 'fitted drawing containment')
  }
  /** The states as drawn in the first placement and named for assistive technology. */
  const drawnStates = async () => {
    const first = page.locator('g[data-instance="s1"]')
    await first.locator('[data-atom-label="a1"]').waitFor()
    return {
      a1: (await first.locator('text[data-atom="a1"]').allTextContents()).join('').replace(/\s/g, ''),
      a1Radical: await first.locator('[data-atom-label="a1"] circle[data-mark="radical"]').count(),
      a2: (await first.locator('text[data-atom="a2"]').allTextContents()).join('').replace(/\s/g, ''),
      a2Attachment: await first.locator('[data-atom-label="a2"] [data-mark="attachment"]').textContent(),
      a3: (await first.locator('text[data-atom="a3"]').allTextContents()).join('').replace(/\s/g, ''),
      palette: await Promise.all(
        [iodine, chlorine, queried].map(async (id) => (await first.locator(`text[data-atom="${id}"]`).allTextContents()).join('')),
      ),
      // Labels with text against their symbol, which the gap check above measures.
      adjacent: await page.evaluate(() =>
        [...document.querySelectorAll('g[data-instance="s1"] [data-atom-label]')]
          .filter((label) => label.querySelectorAll('text[data-chunk] tspan').length > 1)
          .map((label) => label.getAttribute('data-atom-label'))
          .sort(),
      ),
      b1Dashed: await first.locator('g[data-bond-kind="aromatic"] line[data-pattern="dashed"][data-bond="b1"]').count(),
      a1Name: await first.locator('[data-atom-label="a1"]').getAttribute('aria-label'),
      b1Name: await first.locator('g[data-bond-kind="aromatic"]').getAttribute('aria-label'),
    }
  }
  const expected = {
    a1: 'H3C',
    a1Radical: 1,
    a2: 'C',
    a2Attachment: 'ap1',
    a3: 'N',
    palette: ['I', 'Cl+', '![N,O,S,…]H2'],
    adjacent: ['a1', chlorine, queried].sort(),
    b1Dashed: 1,
    a1Name: 'Atom a1 in s1: C; 3 stated hydrogens; doublet radical',
    b1Name: 'Bond b1 in s1: aromatic bond between a1 and a2',
  }
  assert.deepEqual(await drawnStates(), expected, 'drawn as recorded; the inferred nitrogen shows no count')
  await assertDrawingContainment()
  await page.screenshot({ path: join(outDir, 'editor-states.png') })
  const recorded = await statesDoc()

  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
  await page.reload()
  await alphaItem.getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  assert.deepEqual(await drawnStates(), expected, 'after a page reload')
  await assertDrawingContainment()
  await closeShown()
  await alphaItem.getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  assert.deepEqual(await drawnStates(), expected, 'reopened from storage')
  await assertDrawingContainment()
  const reopened = await statesDoc()
  assert.deepEqual([reopened.atoms, reopened.bonds], [recorded.atoms, recorded.bonds])
}
results.recorded_states_drawn_and_kept = true
step = 'after recorded_states_drawn_and_kept'

// Hold replies with explicit gates: selection made while a group is in flight must survive,
// and Delete queued behind a move must use the selection captured at the Delete keypress.
{
  await closeShown()
  const createdScene = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
  await page.getByRole('button', { name: 'New document' }).click()
  docId = (await (await createdScene).json()).document.id
  await page.getByLabel('Document title').waitFor()
  for (const shape of ['line', 'arrow', 'rectangle']) {
    await clickFor(page.getByRole('button', { name: `Add ${shape}`, exact: true }), '/transactions')
  }
  assert.equal(await page.locator('[data-scene-object="s2"]').count(), 1)
  assert.equal(await page.locator('[data-scene-object="s3"]').count(), 1)
  assert.equal(await page.locator('[data-scene-object="s4"]').count(), 1)
  const toggle = (id) => page.getByRole('button', { name: `Select scene object ${id}`, exact: true })
  await toggle('s2').click()
  await toggle('s3').click()

  let releaseGroup
  let groupReceived
  const groupGate = new Promise((resolve) => { releaseGroup = resolve })
  const groupStarted = new Promise((resolve) => { groupReceived = resolve })
  const holdGroup = async (route) => {
    if (!route.request().postDataJSON().commands.some((command) => command.op === 'group')) return route.fallback()
    const response = await route.fetch()
    assert.equal(response.status(), 200)
    groupReceived()
    await groupGate
    await route.fulfill({ response })
  }
  await page.route('**/transactions', holdGroup)
  await page.getByRole('button', { name: 'Group', exact: true }).click()
  await groupStarted
  await toggle('s4').click()
  releaseGroup()
  await toggle('s5').waitFor()
  await waitIdle()
  await page.unroute('**/transactions', holdGroup)
  assert.equal(await toggle('s4').getAttribute('aria-pressed'), 'true', 'the unrelated selection stays')
  assert.equal(await toggle('s5').getAttribute('aria-pressed'), 'true', 'the new group is selected')

  await toggle('s4').click()
  let releaseMove
  let moveReceived
  const moveGate = new Promise((resolve) => { releaseMove = resolve })
  const moveStarted = new Promise((resolve) => { moveReceived = resolve })
  const holdMove = async (route) => {
    if (!route.request().postDataJSON().commands.some((command) => command.op === 'move_objects')) return route.fallback()
    const response = await route.fetch()
    assert.equal(response.status(), 200)
    moveReceived()
    await moveGate
    await route.fulfill({ response })
  }
  await page.route('**/transactions', holdMove)
  await page.getByRole('button', { name: 'Move selected scene objects right' }).click()
  await moveStarted
  await page.keyboard.press('Delete')
  await toggle('s5').click()
  await toggle('s4').click()
  releaseMove()
  await toggle('s5').waitFor({ state: 'detached' })
  await waitIdle()
  await page.unroute('**/transactions', holdMove)
  assert.equal(await toggle('s4').getAttribute('aria-pressed'), 'true', 'the later selection was not deleted')
  await clickFor(page.getByRole('button', { name: 'Undo', exact: true }), '/undo')
  await toggle('s5').waitFor()
  assert.equal(await toggle('s5').getAttribute('aria-pressed'), 'true', 'Undo restores the selection from the Delete press')
  assert.equal(await toggle('s4').getAttribute('aria-pressed'), 'false')
  // React has not rendered the toggle's new value yet when the keypress is delivered.
  const quickDelete = page.waitForResponse((r) => r.url().endsWith('/transactions') && r.request().method() === 'POST')
  await page.evaluate(() => {
    document.querySelector('[aria-label="Select scene object s4"]').click()
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }))
  })
  assert.equal((await quickDelete).status(), 200)
  await toggle('s5').waitFor({ state: 'detached' })
  await toggle('s4').waitFor({ state: 'detached' })
  // Deleting a reference owner and its target together removes the owner first.
  await clickFor(page.getByRole('button', { name: 'Add line', exact: true }), '/transactions')
  await drawPair()
  await tool('Select and move')
  const selectedAtom = await atomScreen(await openDoc(), 'a1')
  await page.mouse.click(selectedAtom.x, selectedAtom.y)
  await page.getByRole('heading', { name: 'Atom a1' }).waitFor()
  await toggle('s6').click()
  await clickFor(page.getByRole('button', { name: 'Add reference', exact: true }), '/transactions')
  assert.deepEqual((await openDoc()).document.scene.references, [{ source: 's6', target: { kind: 'atom', id: 'a1' } }])
  const deleteOwnerAndTarget = page.waitForResponse((r) => r.url().endsWith('/transactions') && r.request().method() === 'POST')
  await page.keyboard.press('Delete')
  assert.equal((await deleteOwnerAndTarget).status(), 200)
  await waitIdle()
  const deleted = (await openDoc()).document
  assert.equal(deleted.atoms.some((atom) => atom.id === 'a1'), false)
  assert.equal(deleted.scene.objects.some((object) => object.id === 's6'), false)
  assert.deepEqual(deleted.scene.references, [])
  await clickFor(page.getByRole('button', { name: 'Undo', exact: true }), '/undo')
  assert.deepEqual((await openDoc()).document.scene.references, [{ source: 's6', target: { kind: 'atom', id: 'a1' } }])
  results.scene_group_and_queued_delete_selection = true
  results.scene_selection_before_render_is_used = true
  results.scene_reference_owner_and_target_delete = true
}
step = 'after scene_group_and_queued_delete_selection'

// Scene geometry controls act on whole objects, preserving grouped units and
// using one explicit geometry-center pivot for quarter turns and axis mirrors.
{
  await closeShown()
  const created = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
  await page.getByRole('button', { name: 'New document' }).click()
  docId = (await (await created).json()).document.id
  await page.getByLabel('Document title').waitFor()
  const toggle = (id) => page.getByRole('button', { name: `Select scene object ${id}`, exact: true })
  const action = (name) => page.getByRole('button', { name, exact: true })
  const graphics = async () => (await openDoc()).document.scene.objects
    .filter((object) => object.kind === 'graphic')
    .map((object) => ({ id: object.id, points: object.points }))
  assert.equal(await action('Rotate 90° clockwise').isDisabled(), true, 'no selected geometry')
  for (const shape of ['line', 'arrow', 'rectangle']) {
    await clickFor(action(`Add ${shape}`), '/transactions')
  }
  await toggle('s3').click()
  assert.equal(await action('Align left').isDisabled(), true, 'alignment needs two outermost objects')
  await clickFor(action('Move selected scene objects right'), '/transactions')
  await toggle('s2').click()
  assert.equal(await action('Distribute horizontal gaps').isDisabled(), true, 'distribution needs three outermost objects')
  await toggle('s4').click()
  await clickFor(action('Distribute horizontal gaps'), '/transactions')
  assert.deepEqual(await graphics(), [
    { id: 's2', points: [{ x: 20, y: 20 }, { x: 44, y: 20 }] },
    { id: 's3', points: [{ x: 55, y: 20 }, { x: 79, y: 20 }] },
    { id: 's4', points: [{ x: 90, y: 20 }, { x: 114, y: 36 }] },
  ], 'equal edge gaps restore the middle object while retaining endpoints')
  const beforeAlign = await graphics()
  await clickFor(action('Align bottom'), '/transactions')
  const aligned = [
    { id: 's2', points: [{ x: 20, y: 36 }, { x: 44, y: 36 }] },
    { id: 's3', points: [{ x: 55, y: 36 }, { x: 79, y: 36 }] },
    { id: 's4', points: [{ x: 90, y: 20 }, { x: 114, y: 36 }] },
  ]
  assert.deepEqual(await graphics(), aligned)
  await clickFor(action('Undo'), '/undo')
  assert.deepEqual(await graphics(), beforeAlign)
  await clickFor(action('Redo'), '/redo')
  assert.deepEqual(await graphics(), aligned)
  await clickFor(action('Group'), '/transactions')
  await toggle('s5').waitFor()
  await toggle('s2').click()
  assert.equal(await action('Align left').isDisabled(), true, 'a selected group and its child count as one unit')
  await clickFor(action('Rotate 90° clockwise'), '/transactions')
  assert.deepEqual(await graphics(), [
    { id: 's2', points: [{ x: 59, y: -19 }, { x: 59, y: 5 }] },
    { id: 's3', points: [{ x: 59, y: 16 }, { x: 59, y: 40 }] },
    { id: 's4', points: [{ x: 75, y: 51 }, { x: 59, y: 75 }] },
  ], 'group plus selected descendant rotates each endpoint once around (67,28)')
  await clickFor(action('Reflect across horizontal axis'), '/transactions')
  assert.deepEqual(await graphics(), [
    { id: 's2', points: [{ x: 59, y: 75 }, { x: 59, y: 51 }] },
    { id: 's3', points: [{ x: 59, y: 40 }, { x: 59, y: 16 }] },
    { id: 's4', points: [{ x: 75, y: 5 }, { x: 59, y: -19 }] },
  ], 'horizontal-axis reflection reverses y and preserves x and corner identities')
  await drawPair()
  await tool('Select and move')
  const at = await atomScreen(await openDoc(), 'a1')
  await page.mouse.click(at.x, at.y)
  await page.getByRole('heading', { name: 'Atom a1', exact: true }).waitFor()
  await toggle('s5').click()
  assert.equal(await action('Rotate 90° clockwise').isDisabled(), true)
  assert.equal(await action('Reflect across vertical axis').isDisabled(), true)
  await page.getByText('Choose scene objects to arrange or transform. Atom and bond selection is not supported by these controls yet.', { exact: true }).waitFor()
  results.scene_geometry_controls = true
}
step = 'after scene_geometry_controls'


// Mixed selection has one explicit graph movement placement. A marquee spanning
// several drawings takes graph targets only from that placement, and group/atom/
// bond/instance overlap contributes each geometry point just once to the drag.
{
  step = 'mixed marquee and group drag in one movement placement'
  await closeShown()
  const created = await apiWrite('/api/v1/documents', { request_id: 'mixed-selection-create', title: 'Mixed selection gestures' })
  docId = created.body.document.id
  const id = docId
  const seeded = await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'mixed-selection-fixture', expected_revision: 0, commands: [
      { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
      { op: 'add_atom', element: 'O', position: { x: 130, y: 100 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_instance', source: 's1', offset: { x: 0, y: 120 } },
      { op: 'add_graphic', shape: 'line', points: [{ x: 180, y: 220 }, { x: 210, y: 220 }] },
      { op: 'group', objects: ['s2', 's3'] },
    ],
  })
  assert.equal(seeded.status, 200, JSON.stringify(seeded.body))
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open', exact: true }).click()
  await page.getByLabel('Revision 1').waitFor()
  await tool('Select and move')
  const canvas = page.locator('svg[data-structure-canvas]')
  const controls = page.getByRole('toolbar', { name: 'Canvas view' })
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  const toggle = (object) => page.getByRole('button', { name: `Select scene object ${object}`, exact: true })
  const placement = page.getByRole('combobox', { name: 'Atom and bond movement placement', exact: true })
  const assertPlacementReachable = async () => {
    await placement.scrollIntoViewIfNeeded()
    const reachable = await placement.evaluate((trigger) => {
      const canvasRoot = trigger.closest('[data-slot="field"]')?.parentElement
      if (!canvasRoot) throw new Error('The movement field has no canvas container.')
      const container = canvasRoot.getBoundingClientRect()
      const box = trigger.getBoundingClientRect()
      const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)
      return {
        fitsCanvas: box.left >= container.left - 1 && box.right <= container.right + 1,
        receivesPointer: hit !== null && trigger.contains(hit),
      }
    })
    assert.deepEqual(reachable, { fitsCanvas: true, receivesPointer: true }, 'the movement selector stays inside the canvas column and receives its own pointer input')
  }
  const choosePlacement = async (instance) => {
    await assertPlacementReachable()
    await placement.click()
    await page.getByRole('option', { name: instance, exact: true }).click()
  }
  const gesturePoint = async (point) => {
    // Scene-list toggles may scroll the long inspector below the canvas. Raw
    // mouse coordinates do not scroll automatically like locator clicks do.
    await canvas.scrollIntoViewIfNeeded()
    const at = await screen(point)
    const reachable = await canvas.evaluate((svg, pointer) => {
      const box = svg.getBoundingClientRect()
      const hit = document.elementFromPoint(pointer.x, pointer.y)
      return pointer.x >= box.left && pointer.x <= box.right && pointer.y >= box.top && pointer.y <= box.bottom &&
        pointer.x >= 0 && pointer.x < window.innerWidth && pointer.y >= 0 && pointer.y < window.innerHeight &&
        hit !== null && svg.contains(hit)
    }, at)
    assert.equal(reachable, true, `gesture point (${point.x}, ${point.y}) must receive pointer input in the visible canvas`)
    return at
  }
  const clickPoint = async (point) => {
    const at = await gesturePoint(point)
    await page.mouse.click(at.x, at.y)
  }
  const dragPoint = async (from, to) => {
    const start = await gesturePoint(from)
    const end = await gesturePoint(to)
    await page.mouse.move(start.x, start.y)
    await page.mouse.down()
    await page.mouse.move(end.x, end.y, { steps: 5 })
    await page.mouse.up()
  }
  const moveGeometry = (document) => ({
    instances: document.scene.instances.map((instance) => ({ id: instance.id, positions: instance.positions })),
    line: document.scene.objects.find((object) => object.id === 's3').points,
  })
  const original = (await openDoc()).document
  await clickPoint({ x: 100, y: 220 })
  await page.getByRole('heading', { name: 'Atom a1', exact: true }).waitFor()
  await toggle('s3').click()
  await choosePlacement('s1')
  assert.equal(await page.getByRole('heading', { name: 'Atom a1', exact: true }).count(), 0, 'placement change clears explicit graph targets')
  assert.equal(await toggle('s3').getAttribute('aria-pressed'), 'true', 'placement change preserves independent scene selection')
  assert.equal(await page.getByRole('button', { name: 'Rotate 90° clockwise', exact: true }).isDisabled(), false, 'clearing the graph targets re-enables scene geometry actions')
  await choosePlacement('s2')
  await page.setViewportSize({ width: 320, height: 800 })
  await assertPlacementReachable()
  await page.setViewportSize({ width: 1280, height: 900 })
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()

  await dragPoint({ x: 90, y: 90 }, { x: 220, y: 230 })
  assert.equal(await toggle('s3').getAttribute('aria-pressed'), 'true', 'complete graphic geometry is enclosed')
  assert.equal(await toggle('s1').getAttribute('aria-pressed'), 'false', 'marquee does not promote graph atoms to a placement')
  assert.equal(await toggle('s2').getAttribute('aria-pressed'), 'false')
  assert.equal(await toggle('s4').getAttribute('aria-pressed'), 'false', 'marquee does not promote contained objects to a group')
  assert.equal((await openDoc()).document.revision, 1, 'selection changes make no edit')
  const firstMove = page.waitForResponse((response) => response.url().endsWith(`/documents/${id}/transactions`) && response.request().method() === 'POST')
  await dragPoint({ x: 100, y: 220 }, { x: 110, y: 235 })
  const firstResponse = await firstMove
  assert.equal(firstResponse.status(), 200)
  await page.getByLabel('Revision 2').waitFor()
  assert.deepEqual(firstResponse.request().postDataJSON().commands, [{
    op: 'move_selection', selection: { objects: ['s3'], atoms: ['a1', 'a2'], bonds: [] }, instance: 's2', delta: { x: 10, y: 15 },
  }], 'marquee captures active-placement atoms and full graphics in one transaction command')
  const expectedMoved = {
    instances: [
      { id: 's1', positions: [{ atom: 'a1', x: 100, y: 100 }, { atom: 'a2', x: 130, y: 100 }] },
      { id: 's2', positions: [{ atom: 'a1', x: 110, y: 235 }, { atom: 'a2', x: 140, y: 235 }] },
    ],
    line: [{ x: 190, y: 235 }, { x: 220, y: 235 }],
  }
  assert.deepEqual(moveGeometry((await openDoc()).document), expectedMoved)
  await clickFor(page.getByRole('button', { name: 'Undo', exact: true }), '/undo')
  assert.deepEqual(moveGeometry((await openDoc()).document), moveGeometry(original))

  await canvas.focus()
  await page.keyboard.press('Escape')
  await clickPoint({ x: 100, y: 220 })
  await page.keyboard.down('Shift')
  await clickPoint({ x: 115, y: 220 })
  await page.keyboard.up('Shift')
  await toggle('s4').click()
  await toggle('s2').click()
  const groupMove = page.waitForResponse((response) => response.url().endsWith(`/documents/${id}/transactions`) && response.request().method() === 'POST')
  await dragPoint({ x: 195, y: 220 }, { x: 205, y: 235 })
  const groupResponse = await groupMove
  assert.equal(groupResponse.status(), 200)
  await waitIdle()
  assert.deepEqual(groupResponse.request().postDataJSON().commands, [{
    op: 'move_selection', selection: { objects: ['s2', 's4'], atoms: ['a1'], bonds: ['b1'] }, instance: 's2', delta: { x: 10, y: 15 },
  }], 'dragging an effectively selected graphic preserves its selected parent instead of replacing selection')
  assert.deepEqual(moveGeometry((await openDoc()).document), expectedMoved, 'group, instance, atom and bond overlap moves every target once')

  const beforeCancel = (await openDoc()).document
  let writes = 0
  const countWrite = (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname === `/api/v1/documents/${id}/transactions`) writes += 1
  }
  page.on('request', countWrite)
  const lineAt = await screen({ x: 205, y: 235 })
  await page.mouse.move(lineAt.x, lineAt.y)
  await page.mouse.down()
  await page.mouse.move(lineAt.x + 1, lineAt.y + 1)
  await page.mouse.up()
  await canvas.evaluate((svg) => svg.addEventListener('pointerdown', (event) => { svg.dataset.smokePointer = String(event.pointerId) }, { once: true }))
  await page.mouse.move(lineAt.x, lineAt.y)
  await page.mouse.down()
  await page.mouse.move(lineAt.x + 35, lineAt.y + 10, { steps: 4 })
  await canvas.dispatchEvent('pointercancel', { pointerId: Number(await canvas.getAttribute('data-smoke-pointer')), pointerType: 'mouse', isPrimary: true })
  await page.mouse.up()
  await controls.getByRole('button', { name: 'Zoom in', exact: true }).click()
  assert.equal(writes, 0, 'subthreshold graphic presses and cancelled mixed drags make no transaction')
  assert.deepEqual((await openDoc()).document, beforeCancel)
  page.off('request', countWrite)
  results.mixed_marquee_placement_and_group_drag = true
}
step = 'after mixed_marquee_placement_and_group_drag'

// Document style and preferences: a document's style is changed in its Document style dialog (two quick
// presses while the first request is held both count), the canvas follows it (measured from the
// DOM: font size and family, carbon labels, stroke width, label boxes holding the drawn text, the
// bond length of a new bond), and it is saved and reopened. New-document preferences reach a
// document created afterwards, not the first one, and the first document's own edits do not
// reach the second.
{
  const styledCreated = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
  await closeShown()
  await page.getByRole('button', { name: 'New document' }).click()
  const styledId = (await (await styledCreated).json()).document.id
  await page.getByLabel('Document title').waitFor()
  const styledDoc = async (id = styledId) => (await api(`/api/v1/documents/${id}`)).document
  await tool('Bond')
  const frame = await canvas.boundingBox()
  await page.mouse.click(frame.x + frame.width * 0.35, frame.y + frame.height * 0.55)
  await waitIdle()
  await page.getByRole('button', { name: 'Element N' }).click()
  const drawn = await styledDoc()
  const a2At = await screen(drawn.atoms.find((atom) => atom.id === 'a2').position)
  await page.mouse.click(a2At.x, a2At.y)
  await waitIdle()
  await page.locator('[data-atom-label="a2"]').waitFor()
  /** What the canvas draws, read from the DOM. */
  const drawnStyle = () =>
    page.evaluate(() => {
      const svg = document.querySelector('svg[data-structure-canvas]')
      const labels = svg.querySelector('g[data-labels]')
      const symbol = svg.querySelector('[data-atom-label="a2"] tspan[data-part="symbol"]')
      const box = (element) => element.getAttribute('data-box').split(' ').map(Number)
      // Every label's drawn text lies inside the box the canvas reserves for it.
      const overflowing = [...svg.querySelectorAll('[data-atom-label] text[data-box]')].filter((text) => {
        const [x, y, width, height] = box(text)
        const drawnBox = text.getBBox()
        return drawnBox.x < x - 0.05 || drawnBox.y < y - 0.05 || drawnBox.x + drawnBox.width > x + width + 0.05 || drawnBox.y + drawnBox.height > y + height + 0.05
      })
      return {
        family: labels.getAttribute('data-labels'),
        computedFamily: getComputedStyle(symbol).fontFamily,
        size: Number(symbol.getAttribute('font-size')),
        stroke: Number(svg.querySelector('g[data-bonds]').getAttribute('stroke-width')),
        labelled: [...svg.querySelectorAll('[data-atom-label]')].map((element) => element.getAttribute('data-atom-label')).sort(),
        a2Box: box(svg.querySelector('[data-atom-label="a2"]')),
        overflowing: overflowing.map((text) => text.outerHTML.slice(0, 80)),
      }
    })
  const plain = await drawnStyle()
  assert.deepEqual(
    [plain.family, plain.size, plain.stroke, plain.labelled],
    ['sans-serif', 12, 1.2, ['a2']],
    'the built-in style: a bonded carbon is a bare vertex',
  )

  await page.getByRole('button', { name: 'Document style', exact: true }).click()
  const style = page.getByRole('dialog', { name: 'Document style' })
  await style.waitFor()
  // Re-entering a displayed value is a no-op, and displayed limits are accepted after conversion.
  await style.getByLabel('Display unit').click()
  await page.getByRole('option', { name: 'Millimetres (mm)', exact: true }).click()
  await waitIdle()
  const beforeSameValue = (await styledDoc()).revision
  const bondLengthInput = style.getByRole('textbox', { name: 'Bond length', exact: true })
  await bondLengthInput.fill('10.580')
  await bondLengthInput.press('Enter')
  await waitIdle()
  assert.equal((await styledDoc()).revision, beforeSameValue, 'equivalent displayed values make no edit')
  await bondLengthInput.fill('1.76')
  await bondLengthInput.press('Enter')
  await waitIdle()
  assert.equal((await styledDoc()).settings.bond_length_pt, 5, 'the lower bound as displayed is accepted')

  // A later choice of the currently displayed value must survive an earlier pending choice.
  let releaseUnit
  const unitHeld = new Promise((resolveHeld) => { releaseUnit = resolveHeld })
  const holdUnit = async (route) => { await unitHeld; await route.continue() }
  await page.route('**/transactions', holdUnit)
  await style.getByLabel('Display unit').click()
  await page.getByRole('option', { name: 'Centimetres (cm)', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('[data-slot="spinner"]'))
  await style.getByLabel('Display unit').click()
  await page.getByRole('option', { name: 'Millimetres (mm)', exact: true }).click()
  releaseUnit()
  await waitIdle()
  await page.unroute('**/transactions', holdUnit)
  assert.equal((await styledDoc()).settings.display_unit, 'mm', 'the last choice wins')
  await style.getByLabel('Display unit').click()
  await page.getByRole('option', { name: 'Points (pt)', exact: true }).click()
  await waitIdle()
  await bondLengthInput.fill('30')
  await bondLengthInput.press('Enter')
  await waitIdle()
  assert.equal((await styledDoc()).settings.bond_length_pt, 30)
  // The displayed value may be chosen again while a different typed value is in flight.
  let releaseTyped
  let typedReceived
  const typedGate = new Promise((resolve) => { releaseTyped = resolve })
  const typedStarted = new Promise((resolve) => { typedReceived = resolve })
  let firstTyped = true
  const holdTyped = async (route) => {
    if (firstTyped) { firstTyped = false; typedReceived(); await typedGate }
    await route.continue()
  }
  await page.route('**/transactions', holdTyped)
  await bondLengthInput.fill('40')
  await bondLengthInput.press('Enter')
  await typedStarted
  await bondLengthInput.fill('30')
  await bondLengthInput.press('Enter')
  releaseTyped()
  await waitIdle()
  await page.unroute('**/transactions', holdTyped)
  assert.equal((await styledDoc()).settings.bond_length_pt, 30, 'the last typed value wins')
  // Merely focusing and leaving an unchanged field is not an edit and cannot undo a step
  // already waiting in the queue.
  let releaseFocus
  let focusReceived
  const focusGate = new Promise((resolve) => { releaseFocus = resolve })
  const focusStarted = new Promise((resolve) => { focusReceived = resolve })
  let firstFocus = true
  const holdFocus = async (route) => {
    if (firstFocus) { firstFocus = false; focusReceived(); await focusGate }
    await route.continue()
  }
  await page.route('**/transactions', holdFocus)
  await bondLengthInput.focus()
  await style.getByRole('button', { name: 'Increase bond length' }).click()
  await focusStarted
  await bondLengthInput.focus()
  await style.getByRole('textbox', { name: 'Line width', exact: true }).focus()
  releaseFocus()
  await waitIdle()
  await page.unroute('**/transactions', holdFocus)
  assert.equal((await styledDoc()).settings.bond_length_pt, 31, 'an unchanged field does not undo a pending step')
  await style.getByRole('button', { name: 'Decrease bond length' }).click()
  await waitIdle()
  // A response arriving while the next number is being typed must not replace that draft.
  let releaseDraft
  let draftReceived
  const draftGate = new Promise((resolve) => { releaseDraft = resolve })
  const draftStarted = new Promise((resolve) => { draftReceived = resolve })
  let firstDraft = true
  const holdDraft = async (route) => {
    if (firstDraft) { firstDraft = false; draftReceived(); await draftGate }
    await route.continue()
  }
  await page.route('**/transactions', holdDraft)
  await bondLengthInput.fill('40')
  await bondLengthInput.press('Enter')
  await draftStarted
  await bondLengthInput.fill('45')
  releaseDraft()
  await waitIdle()
  assert.equal(await bondLengthInput.inputValue(), '45', 'the uncommitted input survives the earlier reply')
  await bondLengthInput.press('Enter')
  await waitIdle()
  await page.unroute('**/transactions', holdDraft)
  assert.equal((await styledDoc()).settings.bond_length_pt, 45)
  await bondLengthInput.fill('30')
  await bondLengthInput.press('Enter')
  await waitIdle()
  // The unit in force when typing began remains attached to that draft if a queued unit
  // choice completes while the user is still typing.
  let releaseDraftUnit
  let draftUnitReceived
  const draftUnitGate = new Promise((resolve) => { releaseDraftUnit = resolve })
  const draftUnitStarted = new Promise((resolve) => { draftUnitReceived = resolve })
  let firstDraftUnit = true
  const holdDraftUnit = async (route) => {
    if (firstDraftUnit) { firstDraftUnit = false; draftUnitReceived(); await draftUnitGate }
    await route.continue()
  }
  await page.route('**/transactions', holdDraftUnit)
  await style.getByLabel('Display unit').click()
  await page.getByRole('option', { name: 'Millimetres (mm)', exact: true }).click()
  await draftUnitStarted
  await bondLengthInput.fill('45')
  releaseDraftUnit()
  await waitIdle()
  assert.equal(await bondLengthInput.inputValue(), '45')
  assert.match(await bondLengthInput.locator('..').textContent(), /pt/, 'the draft is still shown in the unit it was typed in')
  await bondLengthInput.press('Enter')
  await waitIdle()
  await page.unroute('**/transactions', holdDraftUnit)
  assert.deepEqual([(await styledDoc()).settings.bond_length_pt, (await styledDoc()).settings.display_unit], [45, 'mm'])
  await style.getByLabel('Display unit').click()
  await page.getByRole('option', { name: 'Points (pt)', exact: true }).click()
  await waitIdle()
  await bondLengthInput.fill('30')
  await bondLengthInput.press('Enter')
  await waitIdle()
  // Two presses while the first request is held: each is built from the latest document.
  let releaseFirst
  const firstHeld = new Promise((resolveHeld) => {
    releaseFirst = resolveHeld
  })
  let seen = 0
  const holdFirst = async (route) => {
    seen += 1
    if (seen === 1) await firstHeld
    await route.continue()
  }
  await page.route('**/transactions', holdFirst)
  const larger = style.getByRole('button', { name: 'Increase label size' })
  await larger.click()
  await page.waitForFunction(() => document.querySelector('[data-slot="spinner"]'))
  await larger.click()
  releaseFirst()
  await waitIdle()
  await page.unroute('**/transactions', holdFirst)
  assert.equal(await page.getByText('Edit rejected', { exact: true }).count(), 0, 'neither press is refused')
  assert.equal((await styledDoc()).settings.label_size_pt, 14, 'both presses count')

  const choose = async (label, option) => {
    await style.getByLabel(label).click()
    await page.getByRole('option', { name: option, exact: true }).click()
    await waitIdle()
  }
  await choose('Label font', 'Serif')
  await choose('Carbon labels', 'Every carbon')
  await style.getByRole('textbox', { name: 'Line width', exact: true }).fill('2.4')
  await style.getByRole('textbox', { name: 'Line width', exact: true }).press('Enter')
  await waitIdle()
  await style.getByRole('textbox', { name: 'Bond length', exact: true }).fill('45')
  await style.getByRole('textbox', { name: 'Bond length', exact: true }).press('Enter')
  await waitIdle()
  await choose('Display unit', 'Millimetres (mm)')
  assert.equal(await style.getByRole('textbox', { name: 'Bond length', exact: true }).inputValue(), '15.88', '45 pt shown in mm')
  let edited = { bond_length_pt: 45, line_width_pt: 2.4, label_font: 'serif', label_size_pt: 14, carbon_labels: 'all', display_unit: 'mm' }
  assert.deepEqual((await styledDoc()).settings, edited)
  await style.getByRole('button', { name: 'Done' }).click()
  await style.waitFor({ state: 'hidden' })

  const restyled = await drawnStyle()
  assert.deepEqual(
    [restyled.family, restyled.size, restyled.stroke, restyled.labelled],
    ['serif', 14, 2.4, ['a1', 'a2']],
    'the canvas draws the document style',
  )
  assert.match(restyled.computedFamily, /serif/)
  assert.ok(restyled.a2Box[3] > plain.a2Box[3] * 1.1, `the label box grows with the label size: ${restyled.a2Box} vs ${plain.a2Box}`)
  assert.deepEqual(restyled.overflowing, [], 'every label lies inside its box')
  // A new bond from a1 is drawn at the document's bond length.
  await tool('Bond')
  const a1At = await screen(drawn.atoms.find((atom) => atom.id === 'a1').position)
  await page.mouse.click(a1At.x, a1At.y)
  await waitIdle()
  const grown = await styledDoc()
  const [from, to] = ['a1', 'a3'].map((id) => grown.atoms.find((atom) => atom.id === id).position)
  assert.ok(Math.abs(Math.hypot(to.x - from.x, to.y - from.y) - 45) < 0.05, 'the new bond is 45 pt long')

  // A style reset records the selection at its button press, including a selection made since
  // the last server edit. Undo restores that snapshot with the previous style.
  await tool('Select and move')
  const firstAtom = await screen(grown.atoms.find((atom) => atom.id === 'a1').position)
  await page.mouse.click(firstAtom.x, firstAtom.y)
  await page.getByRole('heading', { name: 'Atom a1' }).waitFor()
  await page.getByRole('button', { name: 'Document style', exact: true }).click()
  await style.waitFor()
  const resetting = page.waitForRequest((request) => request.url().endsWith('/settings/reset'))
  await style.getByRole('button', { name: 'Built-in defaults' }).click()
  assert.deepEqual((await resetting).postDataJSON().selection_before.atoms, ['a1'])
  await waitIdle()
  assert.deepEqual((await styledDoc()).selection.atoms, ['a1'])
  await style.getByRole('button', { name: 'Done' }).click()
  await style.waitFor({ state: 'hidden' })
  await clickFor(page.getByRole('button', { name: 'Undo', exact: true }), '/undo')
  assert.deepEqual((await styledDoc()).selection.atoms, ['a1'])
  assert.deepEqual((await styledDoc()).settings, edited)

  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
  await page.reload()
  await page.locator(`li[data-document-id="${styledId}"]`).getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  assert.deepEqual((await styledDoc()).settings, edited, 'saved and reopened')
  const reopened = await drawnStyle()
  assert.deepEqual([reopened.family, reopened.size, reopened.stroke], ['serif', 14, 2.4], 'drawn the same after reopening')

  // A preference import is ordered after earlier document edits, and later preference writes
  // cannot overtake it. Instrument fetch calls synchronously while the edit route is held.
  await page.evaluate(() => {
    window.__p12OriginalFetch = window.fetch
    window.__p12PreferenceWrites = []
    window.fetch = (...args) => {
      const path = String(args[0])
      const method = args[1]?.method ?? 'GET'
      if (path.includes('/api/v1/preferences') && method !== 'GET') window.__p12PreferenceWrites.push(path)
      return window.__p12OriginalFetch(...args)
    }
  })
  await page.getByRole('button', { name: 'Document style', exact: true }).click()
  await style.waitFor()
  let releaseStyle
  const styleHeld = new Promise((resolveHeld) => { releaseStyle = resolveHeld })
  const holdStyle = async (route) => { await styleHeld; await route.continue() }
  await page.route('**/transactions', holdStyle)
  await style.getByRole('button', { name: 'Increase label size' }).click()
  await page.waitForFunction(() => document.querySelector('[data-slot="spinner"]'))
  await style.getByRole('button', { name: 'Done' }).click()
  await style.waitFor({ state: 'hidden' })
  await page.getByRole('button', { name: 'Preferences', exact: true }).click()
  const adopting = page.getByRole('dialog', { name: 'Preferences' })
  await adopting.waitFor()
  // Establish the preference client's session while the document edit is still held. This
  // makes any incorrectly ordered import observable without a network-dependent delay.
  const warmed = page.waitForResponse((r) => r.url().endsWith('/api/v1/preferences/reset'))
  await adopting.getByRole('button', { name: 'Reset preferences' }).click()
  assert.equal((await warmed).status(), 200)
  await page.evaluate(() => { window.__p12PreferenceWrites = [] })
  const adopted = page.waitForResponse((r) => r.url().endsWith('/api/v1/preferences/import'))
  await adopting.getByRole('button', { name: /^Use the style of/ }).click()
  const themeChanged = page.waitForResponse((r) => new URL(r.url()).pathname === '/api/v1/preferences' && r.request().method() === 'PUT')
  await adopting.getByLabel('Theme').click()
  await page.getByRole('option', { name: 'Dark', exact: true }).click()
  const beforeRelease = await page.evaluate(() => [...window.__p12PreferenceWrites])
  releaseStyle()
  assert.equal((await adopted).status(), 200)
  assert.equal((await themeChanged).status(), 200)
  await waitIdle()
  await page.unroute('**/transactions', holdStyle)
  assert.deepEqual(beforeRelease, [], 'both preference writes wait for the earlier document edit')
  assert.deepEqual(await page.evaluate(() => window.__p12PreferenceWrites), ['/api/v1/preferences/import', '/api/v1/preferences'], 'later preferences follow the import')
  edited = { ...edited, label_size_pt: 15 }
  assert.deepEqual((await api('/api/v1/preferences')).new_document_settings, edited, 'the imported style includes the held edit')
  const cleared = page.waitForResponse((r) => r.url().endsWith('/api/v1/preferences/reset'))
  await adopting.getByRole('button', { name: 'Reset preferences' }).click()
  assert.equal((await cleared).status(), 200)
  await adopting.getByRole('button', { name: 'Done' }).click()
  await adopting.waitFor({ state: 'hidden' })
  await page.evaluate(() => { window.fetch = window.__p12OriginalFetch })
  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')

  // Preferences: new documents start in monospace; the open document keeps its own style.
  await page.getByRole('button', { name: 'Preferences', exact: true }).click()
  const prefs = page.getByRole('dialog', { name: 'Preferences' })
  await prefs.waitFor()
  assert.equal(await prefs.getByLabel('NMR prediction').isDisabled(), true, 'computation preferences cannot be set')
  assert.equal(await prefs.getByLabel('Numeric precision').inputValue(), 'Not implemented')
  const preferenceWrite = () => page.waitForResponse((r) => new URL(r.url()).pathname === '/api/v1/preferences' && r.request().method() === 'PUT')
  let written = preferenceWrite()
  await prefs.getByLabel('Label font').click()
  await page.getByRole('option', { name: 'Monospace', exact: true }).click()
  assert.equal((await written).status(), 200)
  written = preferenceWrite()
  await prefs.getByLabel('Theme').click()
  await page.getByRole('option', { name: 'Dark', exact: true }).click()
  assert.equal((await written).status(), 200)
  written = preferenceWrite()
  await prefs.getByLabel('Mass display decimal places').click()
  await page.getByRole('option', { name: '5', exact: true }).click()
  assert.equal((await written).status(), 200)
  await page.waitForFunction(() => document.documentElement.classList.contains('dark'))
  const stored = await api('/api/v1/preferences')
  assert.deepEqual([stored.ui.theme, stored.new_documents], ['dark', { label_font: 'monospace' }])
  assert.equal(stored.computation.mass_precision.decimals, 5)
  await prefs.getByRole('button', { name: 'Done' }).click()
  await prefs.waitFor({ state: 'hidden' })
  assert.deepEqual((await styledDoc()).settings, edited, 'preferences never change an open document')

  const secondCreated = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
  await closeShown()
  await page.getByRole('button', { name: 'New document' }).click()
  const secondId = (await (await secondCreated).json()).document.id
  await page.getByLabel('Document title').waitFor()
  assert.deepEqual(
    (await styledDoc(secondId)).settings,
    { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'monospace', label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' },
    'the preferred font and nothing of the first document',
  )
  await closeShown()
  assert.deepEqual((await styledDoc()).settings, edited, 'the first document is unchanged')

  // Back to the built-in preferences, so later steps see the system theme.
  await page.getByRole('button', { name: 'Preferences', exact: true }).click()
  await prefs.waitFor()
  const reset = page.waitForResponse((r) => r.url().endsWith('/api/v1/preferences/reset'))
  await prefs.getByRole('button', { name: 'Reset preferences' }).click()
  assert.equal((await reset).status(), 200)
  await page.waitForFunction(() => !document.documentElement.classList.contains('dark'))
  await page.screenshot({ path: join(outDir, 'editor-preferences.png') })
  await prefs.getByRole('button', { name: 'Done' }).click()
  await prefs.waitFor({ state: 'hidden' })
  assert.deepEqual((await api('/api/v1/preferences')).new_documents, {})
  assert.equal((await api('/api/v1/preferences')).computation.mass_precision.decimals, 2)
}
results.document_style_and_preferences = true
step = 'after document_style_and_preferences'

// Even when the two labels leave no room to draw a 10 pt line, a 5 pt bond is still
// hoverable and selectable. Its selection underlay must fit inside the canvas view box.
{
  const createdShort = page.waitForResponse((r) => r.url().endsWith('/api/v1/documents') && r.request().method() === 'POST')
  await page.getByRole('button', { name: 'New document' }).click()
  const shortId = (await (await createdShort).json()).document.id
  await page.getByLabel('Document title').waitFor()
  const status = await page.evaluate(async (id) => {
    const session = await (await fetch('/api/v1/session')).json()
    const response = await fetch(`/api/v1/documents/${id}/transactions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', [session.header]: session.token },
      body: JSON.stringify({ request_id: 'short-wide-fixture', expected_revision: 0, commands: [
        { op: 'add_atom', element: 'N', position: { x: -30, y: 0 } },
        { op: 'add_atom', element: 'O', position: { x: -25, y: 0 }, bond_to: { atom: 'a1', order: 'single' } },
        { op: 'update_settings', bond_length_pt: 5, line_width_pt: 10 },
      ] }),
    })
    return response.status
  }, shortId)
  assert.equal(status, 200)
  await page.reload()
  await page.locator(`li[data-document-id="${shortId}"]`).getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  await tool('Select and move')
  const middle = await screen({ x: -27.5, y: 0 })
  await page.mouse.move(middle.x, middle.y)
  await page.locator('[data-underlay="b1"]').waitFor({ state: 'attached' })
  await page.mouse.click(middle.x, middle.y)
  await page.getByRole('heading', { name: 'Bond b1' }).waitFor()
  assert.equal(await page.locator('[data-bond="b1"]').count(), 0, 'no bond ink crosses the overlapping label boxes')
  await page.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  const highlightFits = await page.evaluate(() => {
    const svg = document.querySelector('svg[data-structure-canvas]')
    const line = svg.querySelector('[data-underlay="b1"]')
    const [x, y, width, height] = svg.getAttribute('viewBox').split(/\s+/).map(Number)
    const radius = Number(line.getAttribute('stroke-width')) / 2
    const xs = [Number(line.getAttribute('x1')), Number(line.getAttribute('x2'))]
    const ys = [Number(line.getAttribute('y1')), Number(line.getAttribute('y2'))]
    return Math.min(...xs) - radius >= x && Math.max(...xs) + radius <= x + width &&
      Math.min(...ys) - radius >= y && Math.max(...ys) + radius <= y + height
  })
  assert.equal(highlightFits, true, 'the selected bond and its round caps fit the view box')
}
results.short_wide_bond_is_selectable = true
step = 'after short_wide_bond_is_selectable'

// A process restart can reuse the same document ID and revision for a different graph.
// Verify that the inspection is tied to its actual snapshot, not just that revision.
{
  const created = await apiWrite('/api/v1/documents', { request_id: 'valence-branch-create', title: 'Valence branch' })
  const id = created.body.document.id
  const carbon = await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'valence-branch-carbon', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'C', position: { x: 0, y: 0 } }],
  })
  assert.equal(carbon.status, 200)
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const panel = page.getByRole('complementary', { name: 'Valence inspection' })
  await panel.getByRole('button', { name: 'Inspect valence' }).click()
  await panel.getByText('a1: valid; hydrogens 4', { exact: true }).waitFor()
  const unchanged = await api(`/api/v1/documents/${id}`)
  assert.deepEqual(unchanged.document, carbon.body.document)
  assert.equal(unchanged.session.undo_depth, carbon.body.session.undo_depth)
  await crashApi()
  const oxygen = await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'valence-branch-oxygen', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'O', position: { x: 0, y: 0 } }],
  })
  assert.equal(oxygen.status, 200)
  assert.equal(oxygen.body.document.revision, carbon.body.document.revision)
  await tool('Select and move')
  const carbonPoint = await atomScreen(carbon.body, 'a1')
  await page.mouse.click(carbonPoint.x, carbonPoint.y)
  await page.getByRole('button', { name: 'Increase charge' }).click()
  await page.getByText('Local service restarted', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Reload' }).click()
  await panel.getByText('The inspected drawing differs from this one. Inspect again.', { exact: true }).waitFor()
  assert.equal(await panel.getByText('a1: valid; hydrogens 4', { exact: true }).count(), 0)
  await panel.getByRole('button', { name: 'Inspect valence' }).click()
  await panel.getByText('a1: valid; hydrogens 2', { exact: true }).waitFor()
  await page.screenshot({ path: join(outDir, 'editor-valence.png') })
  results.valence_snapshot_after_restart = true
}

// Compare two captured graph snapshots, and do not show an error for a different target.
{
  const source = await apiWrite('/api/v1/documents', { request_id: 'identity-source-create', title: 'Identity source' })
  const target = await apiWrite('/api/v1/documents', { request_id: 'identity-target-create', title: 'Identity iron' })
  const id = source.body.document.id
  const targetId = target.body.document.id
  const sourceDraw = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'identity-source-draw', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'C', position: { x: 0, y: 0 } }] })
  const targetDraw = await apiWrite(`/api/v1/documents/${targetId}/transactions`, { request_id: 'identity-target-draw', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'Fe', position: { x: 0, y: 0 } }] })
  assert.equal(sourceDraw.status, 200)
  assert.equal(targetDraw.status, 200)
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const identity = page.getByRole('region', { name: 'Graph identity' })
  await identity.getByLabel('Compare with').click()
  await page.getByRole('option', { name: `Identity source (${id})`, exact: true }).click()
  const before = await api(`/api/v1/documents/${id}`)
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  await identity.getByTestId('identity-result').getByText(/Equivalent under this model/).waitFor()
  const afterInspection = await api(`/api/v1/documents/${id}`)
  assert.deepEqual(afterInspection.document, before.document, 'identity inspection leaves the drawing unchanged')
  assert.equal(afterInspection.session.undo_depth, before.session.undo_depth, 'identity inspection adds no history')
  assert.equal(afterInspection.session.redo_depth, before.session.redo_depth, 'identity inspection preserves redo')
  await page.screenshot({ path: join(outDir, 'editor-identity.png') })
  await identity.getByLabel('Comparison model').click()
  await page.getByRole('option', { name: 'Bounded chemical model', exact: true }).click()
  await identity.getByLabel('Compare with').click()
  await page.getByRole('option', { name: `Identity iron (${targetId})`, exact: true }).click()
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  const targetError = `Graph identity was not determined under this model. Comparison snapshot ${targetId}, revision ${targetDraw.body.document.revision}: element or charge. Inspect again to include later edits.`
  await identity.getByText(targetError, { exact: true }).waitFor()
  let errorArrived
  const errorReady = new Promise((resolve) => { errorArrived = resolve })
  let releaseError
  const errorHeld = new Promise((resolve) => { releaseError = resolve })
  const holdIdentityError = async (route) => {
    if (route.request().postDataJSON()?.operation !== 'chem.identity') return route.continue()
    const response = await route.fetch()
    errorArrived()
    await errorHeld
    await route.fulfill({ response })
  }
  await page.route('**/api/v1/operations', holdIdentityError)
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  await errorReady
  await identity.getByRole('button', { name: 'Refresh documents' }).click()
  releaseError()
  await identity.getByRole('button', { name: 'Inspect identity' }).waitFor()
  await page.unroute('**/api/v1/operations', holdIdentityError)
  assert.equal(await identity.getByText(targetError, { exact: true }).count(), 0, 'refresh invalidates an in-flight comparison error')
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  await identity.getByText(targetError, { exact: true }).waitFor()
  await identity.getByLabel('Compare with').click()
  await page.getByRole('option', { name: `Identity source (${id})`, exact: true }).click()
  assert.equal(await identity.getByText(targetError, { exact: true }).count(), 0, 'an error for another comparison is hidden')
  await identity.getByLabel('Compare with').click()
  await page.getByRole('option', { name: `Identity iron (${targetId})`, exact: true }).click()
  await identity.getByText(targetError, { exact: true }).waitFor()
  const changedTarget = await apiWrite(`/api/v1/documents/${targetId}/transactions`, {
    request_id: 'identity-target-now-carbon', expected_revision: targetDraw.body.document.revision,
    commands: [{ op: 'update_atom', atom: 'a1', element: 'C' }],
  })
  assert.equal(changedTarget.status, 200)
  await identity.getByRole('button', { name: 'Refresh documents' }).click()
  await identity.getByText(targetError, { exact: true }).waitFor({ state: 'hidden' })
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  await identity.getByTestId('identity-result').getByText(/Equivalent under this model/).waitFor()
  await identity.getByLabel('Compare with').click()
  await page.getByRole('option', { name: `Identity source (${id})`, exact: true }).click()
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  await identity.getByTestId('identity-result').getByText(/Equivalent under this model/).waitFor()
  await crashApi()
  const recreated = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'identity-recreate', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'C', position: { x: 0, y: 0 } }] })
  assert.equal(recreated.status, 200)
  assert.deepEqual(recreated.body.document, sourceDraw.body.document, 'the restarted service recreated the same graph and revision')
  await page.getByRole('complementary', { name: 'Valence inspection' }).getByRole('button', { name: 'Inspect valence' }).click()
  await identity.getByText('The drawing, model, comparison or service changed. Inspect again.', { exact: true }).waitFor()
  assert.equal(await identity.getByTestId('identity-result').count(), 0, 'a restart invalidates the old identity even for the same snapshot')
  const afterRestart = await api(`/api/v1/documents/${id}`)
  assert.deepEqual(afterRestart.document, recreated.body.document, 'the restart check leaves the graph unchanged')
  assert.equal(afterRestart.session.undo_depth, recreated.body.session.undo_depth, 'the restart check leaves history unchanged')
  await identity.getByRole('button', { name: 'Inspect identity' }).click()
  await identity.getByTestId('identity-result').getByText(/Equivalent under this model/).waitFor()
  await tool('Select and move')
  await canvas.scrollIntoViewIfNeeded()
  const point = await atomScreen(sourceDraw.body, 'a1')
  await page.mouse.click(point.x, point.y)
  await page.getByRole('button', { name: 'Increase charge' }).click()
  await waitIdle()
  await identity.getByText('The drawing, model, comparison or service changed. Inspect again.', { exact: true }).waitFor()
  assert.equal(await identity.getByTestId('identity-result').count(), 0, 'an edit hides the earlier identity')
  results.graph_identity_snapshots_and_comparison = true
}

// A verified export response can arrive after an editor transaction has completed. Its
// bytes belong to the older drawing and must not trigger a download of that stale preview.
{
  step = 'MOL export delayed response after edit'
  const created = await apiWrite('/api/v1/documents', { request_id: 'ct-export-race-create', title: 'Export race' })
  const id = created.body.document.id
  const carbon = await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'ct-export-race-carbon', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'C', position: { x: 0, y: 0 } }],
  })
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const panel = page.getByRole('complementary', { name: 'MOL and SDF' })
  await panel.getByRole('button', { name: 'Preview MOL export' }).click()
  await panel.getByRole('button', { name: 'Download MOL' }).waitFor()
  let arrived
  const responseArrived = new Promise((resolve) => { arrived = resolve })
  let release
  const released = new Promise((resolve) => { release = resolve })
  const holdExport = async (route) => {
    const request = route.request().postDataJSON()
    if (request?.operation !== 'document.export') return route.continue()
    const response = await route.fetch()
    arrived()
    await released
    await route.fulfill({ response })
  }
  await page.route('**/api/v1/operations', holdExport)
  let downloads = 0
  const countDownload = () => { downloads += 1 }
  page.on('download', countDownload)
  await panel.getByRole('button', { name: 'Download MOL' }).click()
  await responseArrived
  await tool('Select and move')
  const position = await atomScreen(carbon.body, 'a1')
  await page.mouse.click(position.x, position.y)
  const edit = page.waitForResponse((response) => response.url().endsWith(`/documents/${id}/transactions`) && response.request().method() === 'POST')
  await page.getByRole('button', { name: 'Increase charge' }).click()
  assert.equal((await edit).status(), 200)
  await page.getByLabel('Revision 2').waitFor()
  const settled = Promise.race([
    page.waitForEvent('download').then(() => 'downloaded'),
    panel.getByText('The export preview changed. Preview the export again.', { exact: true }).waitFor().then(() => 'refused'),
  ])
  release()
  assert.equal(await settled, 'refused', 'a stale verified export is refused after the edit')
  assert.equal(downloads, 0, 'an export from before the edit is not downloaded')
  await page.unroute('**/api/v1/operations', holdExport)
  page.off('download', countDownload)
  results.ctfile_export_response_race = true
}

// Import previews a selected record and its losses before one atomic edit. Export gives a
// separate preview and download containing the resulting graph.
{
  step = 'MOL preview, import and export'
  const created = await apiWrite('/api/v1/documents', { request_id: 'ct-browser-create', title: 'CT browser' })
  const id = created.body.document.id
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const panel = page.getByRole('complementary', { name: 'MOL and SDF' })
  const mol = 'Carbon\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n' +
    '    0.0000    1.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n'
  await panel.getByLabel('Import file').setInputFiles({ name: 'carbon.mol', mimeType: 'text/plain', buffer: Buffer.from(mol) })
  await panel.getByRole('button', { name: 'Preview import' }).click()
  await panel.getByText('1 record(s); importing record 1.', { exact: true }).waitFor()
  assert.equal((await api(`/api/v1/documents/${id}`)).document.atoms.length, 0, 'preview does not change the drawing')
  await panel.getByText(/does not replace its title/).waitFor()
  await panel.getByRole('button', { name: 'Accept losses and import' }).click()
  await panel.getByRole('button', { name: 'Accept losses and import' }).waitFor({ state: 'hidden' })
  const imported = await api(`/api/v1/documents/${id}`)
  assert.deepEqual(imported.document.atoms.map((atom) => [atom.element, atom.position.x, atom.position.y]), [['C', 0, -20]])
  await panel.getByRole('button', { name: 'Preview MOL export' }).click()
  await panel.getByText(/Native atom and bond IDs/).waitFor()
  const downloadPromise = page.waitForEvent('download')
  await panel.getByRole('button', { name: 'Download MOL' }).click()
  const download = await downloadPromise
  const path = join(work, 'ct-browser.mol')
  await download.saveAs(path)
  const written = readFileSync(path, 'utf8')
  assert.match(written, /V2000/)
  assert.match(written, /M  END/)

  // The page still shows the imported carbon at revision 1. A restarted service can reuse
  // that revision for a different unsaved graph. First check an old export cannot download;
  // then the refreshed token must not make the oxygen snapshot look like the carbon preview.
  await crashApi()
  const replacement = await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'ct-browser-replacement', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'O', position: { x: 0, y: 0 } }],
  })
  assert.equal(replacement.body.document.revision, 1)
  assert.equal(replacement.body.document.atoms[0].element, 'O')
  let extraDownloads = 0
  const countDownload = () => { extraDownloads += 1 }
  page.on('download', countDownload)
  await panel.getByRole('button', { name: 'Download MOL' }).click()
  await panel.getByText(/The local service restarted/).waitFor()
  await panel.getByRole('button', { name: 'Download MOL' }).waitFor({ state: 'hidden' })
  assert.equal(extraDownloads, 0, 'an old export is not downloaded after restart')
  page.off('download', countDownload)
  await panel.getByRole('button', { name: 'Preview import' }).click()
  await panel.getByText('The service previewed a different drawing. Reload and preview again.', { exact: true }).waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Accept losses and import' }).count(), 0)
  assert.equal((await api(`/api/v1/documents/${id}`)).document.atoms[0].element, 'O')
  results.ctfile_preview_import_export = true
}

// MOL version choice: V2000 is the default and asks consent for the group it cannot carry; V3000
// writes the group as a COLLECTION after consent for the centers it does not state, the written
// file reads back to the same group, and a refusal is worded and leaves the drawing unchanged.
{
  step = 'MOL V3000 export, consent and re-import'
  const stereoCommands = [
    { op: 'add_atom', element: 'F', position: { x: 0, y: 40 } },
    { op: 'add_atom', element: 'C', position: { x: 0, y: 0 }, bond_to: { atom: '$0', order: 'single' } },
    { op: 'add_atom', element: 'Cl', position: { x: -35, y: -18 }, bond_to: { atom: '$1', order: 'single' } },
    { op: 'add_atom', element: 'C', position: { x: 35, y: -18 }, bond_to: { atom: '$1', order: 'single' } },
    { op: 'add_atom', element: 'Br', position: { x: 35, y: -58 }, bond_to: { atom: '$3', order: 'single' } },
    { op: 'add_atom', element: 'I', position: { x: 70, y: 0 }, bond_to: { atom: '$3', order: 'single' } },
    { op: 'set_bond_mark', bond: '$1', mark: { kind: 'hash', narrow: 'end' } },
    { op: 'set_bond_mark', bond: '$4', mark: { kind: 'hash', narrow: 'begin' } },
  ]
  const build = async (key, title, commands) => {
    const created = await apiWrite('/api/v1/documents', { request_id: `v3k-${key}-create`, title })
    const id = created.body.document.id
    const drawn = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: `v3k-${key}-draw`, expected_revision: 0, commands })
    assert.equal(drawn.status, 200, JSON.stringify(drawn.body))
    return id
  }
  const openPanel = async (id) => {
    await page.reload()
    await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
    return page.getByRole('complementary', { name: 'MOL and SDF' })
  }
  const id = await build('source', 'V3000 source', stereoCommands)
  const stored = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'v3k-source-derive', expected_revision: 1, commands: [
    { op: 'set_configuration_from_drawing', atoms: ['a2', 'a4'] },
  ] })
  assert.equal(stored.status, 200, JSON.stringify(stored.body))
  const grouped = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'v3k-source-group', expected_revision: 2, commands: [
    { op: 'create_stereo_group', kind: 'or', atoms: ['a2'], number: 1 },
  ] })
  assert.equal(grouped.status, 200, JSON.stringify(grouped.body))
  const heldGroups = (await api(`/api/v1/documents/${id}`)).document.stereo.groups
  assert.equal(heldGroups.length, 1)
  const panel = await openPanel(id)
  const version = panel.getByLabel('MOL version to write')
  assert.equal(await version.inputValue(), 'v2000', 'V2000 is the default')
  assert.match(await panel.getByTestId('ctfile-note').textContent(), /enhanced stereo groups as COLLECTION entries/)

  // V2000 cannot carry the group: nothing is offered for download until the loss is accepted.
  await panel.getByRole('button', { name: 'Preview MOL export' }).click()
  const offer = panel.getByRole('group', { name: 'Losses to accept before writing' })
  await offer.waitFor()
  await offer.getByText('One or more enhanced stereo groups are omitted; the reported reason identifies the affected groups.').waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Download MOL' }).count(), 0, 'no file before consent')

  // Choosing V3000 voids that offer; the group is written, the center outside it needs consent.
  await version.selectOption('v3000')
  await offer.waitFor({ state: 'hidden' })
  await panel.getByRole('button', { name: 'Preview MOL export' }).click()
  await offer.waitFor()
  await offer.getByText('Stored tetrahedral centers without an enhanced stereo group: the drawing may carry their configuration, but another reader’s enhanced stereo interpretation is uncertain.').waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Download MOL' }).count(), 0, 'no file before consent')
  await offer.getByRole('button', { name: 'Accept these losses and preview MOL' }).click()
  await panel.getByRole('button', { name: 'Download MOL' }).waitFor()
  assert.equal(await panel.getByTestId('ctfile-written').textContent(), 'Written as MOL V3000.')
  // The consent goes stale between the accept and the download (the Download click rechecks with a
  // digest the service no longer knows): the preview is dropped and the losses are offered again.
  let staled = false
  const staleDigest = async (route) => {
    const request = route.request().postDataJSON()
    if (staled || request?.operation !== 'document.export' || request.parameters.accepted_losses === undefined) return route.continue()
    staled = true
    const parameters = { ...request.parameters, accepted_losses: '0'.repeat(64) }
    await route.continue({ postData: JSON.stringify({ ...request, parameters }) })
  }
  await page.route('**/api/v1/operations', staleDigest)
  await panel.getByRole('button', { name: 'Download MOL' }).click()
  await offer.waitFor()
  await offer.getByText('The losses changed since you accepted them. Review them again.').waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Download MOL' }).count(), 0, 'the stale preview is gone')
  await page.unroute('**/api/v1/operations', staleDigest)
  await offer.getByRole('button', { name: 'Accept these losses and preview MOL' }).click()
  await panel.getByRole('button', { name: 'Download MOL' }).waitFor()
  const downloadPromise = page.waitForEvent('download')
  await panel.getByRole('button', { name: 'Download MOL' }).click()
  const path = join(work, 'v3000-groups.mol')
  await (await downloadPromise).saveAs(path)
  const text = readFileSync(path, 'utf8')
  assert.match(text.split('\n')[3], /V3000$/, 'the counts line names V3000')
  assert.match(text, /M  V30 BEGIN COLLECTION\nM  V30 MDLV30\/STEREL1 ATOMS=\(1 2\)\nM  V30 END COLLECTION/)

  // The written file reads back into the same group, partition and number.
  const copyId = (await apiWrite('/api/v1/documents', { request_id: 'v3k-copy-create', title: 'V3000 copy' })).body.document.id
  const importPanel = await openPanel(copyId)
  await importPanel.getByLabel('Import file').setInputFiles({ name: 'v3000-groups.mol', mimeType: 'text/plain', buffer: Buffer.from(text) })
  await importPanel.getByRole('button', { name: 'Preview import' }).click()
  await importPanel.getByText('1 record(s); importing record 1.', { exact: true }).waitFor()
  await importPanel.getByRole('button', { name: 'Accept losses and import' }).click()
  await importPanel.getByRole('button', { name: 'Accept losses and import' }).waitFor({ state: 'hidden' })
  let backGroups
  for (let attempt = 0; attempt < 50; attempt += 1) {
    backGroups = (await api(`/api/v1/documents/${copyId}`)).document.stereo.groups
    if (backGroups.length > 0) break
    await page.waitForTimeout(100)
  }
  assert.deepEqual(backGroups.map((group) => [group.kind, group.stored_number, group.members]),
    heldGroups.map((group) => [group.kind, group.stored_number, group.members]), 'the group survives the round trip')

  // A group member the depiction does not derive is refused with its sentence; nothing changes.
  const refusedId = await build('refused', 'V3000 refused', [
    ...stereoCommands.slice(0, 6),
    { op: 'set_atom_configuration', atom: '$1', configuration: { kind: 'tetrahedral', neighbors: ['$0', '$2', '$3', 'implicit_hydrogen'], turn: 'clockwise' } },
  ])
  const refusedGroup = await apiWrite(`/api/v1/documents/${refusedId}/transactions`, { request_id: 'v3k-refused-group', expected_revision: 1, commands: [
    { op: 'create_stereo_group', kind: 'absolute', atoms: ['a2'], number: null },
  ] })
  assert.equal(refusedGroup.status, 200, JSON.stringify(refusedGroup.body))
  const before = (await api(`/api/v1/documents/${refusedId}`)).document
  const refusedPanel = await openPanel(refusedId)
  await refusedPanel.getByLabel('MOL version to write').selectOption('v3000')
  await refusedPanel.getByRole('button', { name: 'Preview MOL export' }).click()
  await refusedPanel.getByText(/^A stereo group holds atoms that the wedges or hashes would not derive/).waitFor()
  assert.match(await refusedPanel.getByRole('alert').textContent(), /Nothing was changed\. Service message: /)
  assert.equal(await refusedPanel.getByRole('button', { name: 'Download MOL' }).count(), 0)
  assert.deepEqual((await api(`/api/v1/documents/${refusedId}`)).document, before, 'a refused export leaves the drawing unchanged')
  results.ctfile_v3000_version_consent_roundtrip = true
}

// Drawing export preserves supported stereo, asks before losing it, and downloads binary CDX.
{
  step = 'CDXML and CDX drawing export consent and binary download'
  const build = async (key, keepMark) => {
    const created = await apiWrite('/api/v1/documents', { request_id: `ix-${key}-create`, title: `Interchange ${key}` })
    const id = created.body.document.id
    const commands = [
      { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
      { op: 'add_atom', element: 'F', position: { x: 100, y: 70 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Cl', position: { x: 126, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Br', position: { x: 74, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'set_bond_mark', bond: '$2', mark: { kind: 'wedge', narrow: 'begin' } },
      { op: 'set_configuration_from_drawing', atoms: ['$0'] },
      { op: 'create_stereo_group', kind: 'absolute', atoms: ['$0'], number: null },
      ...(keepMark ? [] : [{ op: 'set_bond_mark', bond: '$2', mark: null }]),
    ]
    const drawn = await apiWrite(`/api/v1/documents/${id}/transactions`, {
      request_id: `ix-${key}-draw`, expected_revision: 0, commands,
    })
    assert.equal(drawn.status, 200, JSON.stringify(drawn.body))
    return { id, drawing: drawn.body }
  }
  const open = async (id) => {
    await page.reload()
    await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
    return page.getByRole('complementary', { name: 'CDXML and CDX export' })
  }
  const supported = await build('supported', true)
  let panel = await open(supported.id)
  assert.match(await panel.getByTestId('interchange-export-note').textContent(), /exporter does not add wedges/)
  assert.match(await panel.getByTestId('interchange-export-note').textContent(), /whole group is omitted/)
  const before = (await api(`/api/v1/documents/${supported.id}`)).document
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  await panel.getByRole('button', { name: 'Download CDXML', exact: true }).waitFor()
  assert.equal(await panel.getByTestId('interchange-export-offer').count(), 0, 'representable grouped stereo needs no loss acceptance')
  await panel.getByRole('button', { name: 'Preview CDX export', exact: true }).click()
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Download CDXML', exact: true }).count(), 0, 'a different format replaces the old preview')
  const downloadReady = page.waitForEvent('download')
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).click()
  const binaryFile = await downloadReady
  assert.equal(binaryFile.suggestedFilename(), 'chemifree.cdx')
  const binaryPath = join(work, 'interchange-supported.cdx')
  await binaryFile.saveAs(binaryPath)
  const bytes = readFileSync(binaryPath)
  assert.equal(bytes.subarray(0, 8).toString('ascii'), 'VjCD0100', 'download contains CDX bytes, never the base64 transport text')
  const writtenRead = await apiWrite('/api/v1/operations', {
    contract: 1, request_id: 'ix-supported-read', profile: 'preserve-semantics', operation: 'document.import',
    parameters: { mode: 'read', format: 'cdx', source: bytes.toString('base64') },
  })
  assert.equal(writtenRead.status, 200, JSON.stringify(writtenRead.body))
  const choice = writtenRead.body.result.choices[0]
  const writtenPreview = await apiWrite('/api/v1/operations', {
    contract: 1, request_id: 'ix-supported-preview', profile: 'preserve-semantics', operation: 'document.import',
    parameters: { mode: 'preview', format: 'cdx', source: bytes.toString('base64'), title: 'Read back', page_id: choice.page_id, fragment_id: choice.fragment_id },
  })
  assert.deepEqual(writtenPreview.body.result.document.stereo, before.stereo, 'downloaded binary retains the center and absolute group')
  assert.deepEqual((await api(`/api/v1/documents/${supported.id}`)).document, before, 'export changes no drawing')

  const omitted = await build('stored-only', false)
  panel = await open(omitted.id)
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  const offer = panel.getByTestId('interchange-export-offer')
  await offer.waitFor()
  await offer.locator('[data-feature="stereo_configuration"]').waitFor()
  await offer.locator('[data-feature="stereo_group"]').waitFor()
  assert.doesNotMatch(await offer.textContent(), /groups themselves are|which this output cannot carry/, 'headings must not claim all formats lose all groups, or omitted groups survived')
  assert.equal(await panel.getByRole('button', { name: /^Download / }).count(), 0, 'no download before loss consent')
  await panel.getByRole('button', { name: 'Preview CDX export', exact: true }).click()
  await offer.getByRole('button', { name: 'Accept these losses and preview CDX', exact: true }).waitFor()
  assert.equal(await offer.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).count(), 0, 'acceptance is scoped to the selected format')
  await offer.getByRole('button', { name: 'Accept these losses and preview CDX', exact: true }).click()
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).waitFor()
  let changedDigest = false
  const staleDigest = async (route) => {
    const request = route.request().postDataJSON()
    if (changedDigest || request?.operation !== 'document.export' || request.parameters.document_id !== omitted.id || request.parameters.accepted_losses === undefined) return route.continue()
    changedDigest = true
    await route.continue({ postData: JSON.stringify({ ...request, parameters: { ...request.parameters, accepted_losses: '0'.repeat(64) } }) })
  }
  await page.route('**/api/v1/operations', staleDigest)
  let unexpectedDownloads = 0
  const countDownload = () => { unexpectedDownloads += 1 }
  page.on('download', countDownload)
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).click()
  await offer.getByText('The losses changed since you accepted them. Review them again.', { exact: true }).waitFor()
  assert.equal(unexpectedDownloads, 0, 'stale consent never downloads a file')
  assert.equal(await panel.getByRole('button', { name: 'Download CDX', exact: true }).count(), 0)
  await page.unroute('**/api/v1/operations', staleDigest)
  page.off('download', countDownload)
  results.interchange_export_consent_binary_and_groups = true

  // Hold a verified response while the user edits; its already-produced bytes must not download.
  step = 'CDX drawing export delayed response after edit'
  panel = await open(supported.id)
  await panel.getByRole('button', { name: 'Preview CDX export', exact: true }).click()
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).waitFor()
  let arrive
  const arrived = new Promise((resolve) => { arrive = resolve })
  let release
  const released = new Promise((resolve) => { release = resolve })
  const holdExport = async (route) => {
    const request = route.request().postDataJSON()
    if (request?.operation !== 'document.export' || request.parameters.document_id !== supported.id) return route.continue()
    const response = await route.fetch()
    arrive()
    await released
    await route.fulfill({ response })
  }
  await page.route('**/api/v1/operations', holdExport)
  unexpectedDownloads = 0
  page.on('download', countDownload)
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).click()
  await arrived
  await tool('Select and move')
  const point = await atomScreen(supported.drawing, 'a2')
  await page.mouse.click(point.x, point.y)
  const edit = page.waitForResponse((response) => response.url().endsWith(`/documents/${supported.id}/transactions`) && response.request().method() === 'POST')
  await page.getByRole('button', { name: 'Increase charge' }).click()
  assert.equal((await edit).status(), 200)
  await page.getByLabel('Revision 2').waitFor()
  const settled = Promise.race([
    page.waitForEvent('download').then(() => 'downloaded'),
    panel.getByRole('alert').filter({ hasText: 'The drawing or service changed. Preview the export again.' }).waitFor().then(() => 'refused'),
  ])
  release()
  assert.equal(await settled, 'refused')
  assert.equal(unexpectedDownloads, 0, 'a delayed pre-edit result cannot download')
  await page.unroute('**/api/v1/operations', holdExport)
  page.off('download', countDownload)
  results.interchange_export_response_race = true

  // A prepared result from a dead process is invalid, even if the document ID is reused.
  step = 'CDXML drawing export service restart'
  const restart = await build('restart', true)
  panel = await open(restart.id)
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  await panel.getByRole('button', { name: 'Download CDXML', exact: true }).waitFor()
  await crashApi()
  unexpectedDownloads = 0
  page.on('download', countDownload)
  await panel.getByRole('button', { name: 'Download CDXML', exact: true }).click()
  await panel.getByRole('alert').filter({ hasText: 'The drawing or service changed. Preview the export again.' }).waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Download CDXML', exact: true }).count(), 0)
  assert.equal(unexpectedDownloads, 0, 'a new service process must not download an older prepared result')
  page.off('download', countDownload)
  results.interchange_export_service_generation = true
}

// SMILES text keeps its line breaks, previews before anything changes, asks for consent only for
// losses that need it, and never offers a result for a form or drawing it was not made for.
{
  step = 'SMILES panel'
  const created = await apiWrite('/api/v1/documents', { request_id: 'smiles-browser-create', title: 'SMILES browser' })
  const id = created.body.document.id
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const panel = page.getByRole('complementary', { name: 'SMILES' })
  const source = panel.getByLabel('SMILES text')
  const atomCount = async () => (await api(`/api/v1/documents/${id}`)).document.atoms.length

  // A pasted second line is not lost silently: the preview shows ethanol and the ignored text.
  await source.focus()
  await page.keyboard.insertText('CCO\nCC')
  assert.equal(await source.inputValue(), 'CCO\nCC', 'the line break reaches the reader')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByTestId('smiles-preview-summary').getByText('3 atom(s), 2 bond(s) would be added.').waitFor()
  await panel.getByText('Not read: characters 5 to 6', { exact: false }).waitFor()
  await panel.getByText(/Text after the first whitespace is not read/).waitFor()
  assert.equal(await atomCount(), 0, 'a preview changes nothing')

  // That loss drops text, so the import asks for it to be accepted; nothing is added before.
  await panel.getByRole('button', { name: 'Import SMILES' }).click()
  await panel.getByRole('button', { name: 'Accept these losses and import SMILES' }).waitFor()
  assert.equal(await atomCount(), 0, 'a consent refusal changes nothing')

  // Editing the drawing makes the offer stale: it cannot be clicked.
  await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'smiles-browser-edit', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'N', position: { x: 300, y: 300 } }],
  })
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  assert.equal(await panel.getByRole('button', { name: 'Accept these losses and import SMILES' }).count(), 0)
  assert.equal(await panel.getByLabel('SMILES text').inputValue(), '', 'a reopened panel starts empty')

  // A plain string needs no acceptance.
  await source.fill('CCO')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByTestId('smiles-preview-summary').waitFor()
  await panel.getByRole('button', { name: 'Import SMILES' }).click()
  await panel.getByText(/Imported\./).waitFor()
  assert.equal(await atomCount(), 4)
  await panel.getByRole('button', { name: 'Import SMILES' }).waitFor({ state: 'hidden' })

  // An edit of the text while a preview is running is said out loud and its result is not used.
  let arrived
  const reached = new Promise((resolve) => { arrived = resolve })
  let release
  const released = new Promise((resolve) => { release = resolve })
  let finished
  const answered = new Promise((resolve) => { finished = resolve })
  let intercepted = false
  const hold = async (route) => {
    const request = route.request().postDataJSON()
    if (intercepted || request?.operation !== 'document.import' || request?.parameters?.preview !== true) return route.continue()
    intercepted = true
    const response = await route.fetch()
    arrived()
    await released
    await route.fulfill({ response })
    finished()
  }
  await page.route('**/api/v1/operations', hold)
  await source.fill('CC')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await reached
  await source.fill('CCC')
  await panel.getByText('The text changed while a request was running; its result was not used. Run it again.').waitFor()
  release()
  await answered
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)))
  await page.unroute('**/api/v1/operations', hold)
  assert.equal(await panel.getByTestId('smiles-preview-summary').count(), 0, 'the replaced preview is not shown')

  // Errors are shown in characters and bytes, in the panel itself.
  await source.fill('C\u00e9O')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByRole('alert').getByText(/character 2 \(bytes 1 to 2\)/).waitFor()
  await source.fill('c1cccc1')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByRole('alert').getByText(/Reason: /).waitFor()

  // Writing: the output belongs to the form it was written for.
  await panel.getByLabel('Write the drawing as').selectOption('canonical')
  await panel.getByRole('button', { name: 'Write SMILES' }).click()
  const output = panel.getByLabel('Written SMILES')
  await output.waitFor()
  assert.notEqual(await output.inputValue(), '')
  assert.equal((await api(`/api/v1/documents/${id}`)).document.atoms.length, 4, 'writing changes nothing')
  await panel.getByLabel('Write the drawing as').selectOption('recorded')
  assert.equal(await panel.getByLabel('Written SMILES').count(), 0, 'the output is not offered for another form')

  // A drawing whose text needs assumptions asks for them to be accepted before it is written.
  const metal = await apiWrite('/api/v1/documents', { request_id: 'smiles-browser-metal', title: 'Metal' })
  const metalId = metal.body.document.id
  await apiWrite(`/api/v1/documents/${metalId}/transactions`, {
    request_id: 'smiles-browser-fe', expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'Fe', position: { x: 0, y: 0 } }],
  })
  await page.reload()
  await page.locator(`li[data-document-id="${metalId}"]`).getByRole('button', { name: 'Open' }).click()
  await panel.getByRole('button', { name: 'Write SMILES' }).click()
  await panel.getByRole('button', { name: 'Accept these losses and write SMILES' }).waitFor()
  assert.equal(await panel.getByLabel('Written SMILES').count(), 0, 'nothing is written before consent')
  await panel.getByLabel('Write the drawing as').selectOption('canonical')
  assert.equal(await panel.getByRole('button', { name: 'Accept these losses and write SMILES' }).count(), 0, 'an offer for another form is gone')
  await panel.getByLabel('Write the drawing as').selectOption('recorded')
  await panel.getByRole('button', { name: 'Write SMILES' }).click()
  await panel.getByRole('button', { name: 'Accept these losses and write SMILES' }).click()
  await panel.getByLabel('Written SMILES').waitFor()
  assert.equal(await panel.getByLabel('Written SMILES').inputValue(), '[Fe]')

  // A commit that the service fails (503) leaves the panel usable: the error shows in the panel, the
  // buttons work again, and a new preview and import go through.
  await source.fill('CC')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByTestId('smiles-preview-summary').waitFor()
  const failCommit = async (route) => {
    const request = route.request().postDataJSON()
    if (request?.operation === 'document.import' && request?.parameters?.preview !== true) return route.fulfill({ status: 503, contentType: 'application/json', body: '{}' })
    return route.continue()
  }
  await page.route('**/api/v1/operations', failCommit)
  await panel.getByRole('button', { name: 'Import SMILES' }).click()
  await panel.getByRole('alert').waitFor()
  await page.unroute('**/api/v1/operations', failCommit)
  assert.equal(await panel.getByRole('button', { name: 'Preview SMILES import' }).isEnabled(), true, 'the panel is usable again')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByTestId('smiles-preview-summary').waitFor()
  await panel.getByRole('button', { name: 'Import SMILES' }).isEnabled()

  // A commit whose reply is lost may have happened. The preview stays, and the same request
  // is retried: the service answers with what it committed, so nothing is imported twice.
  // The panel is on the Metal document by now.
  const metalAtoms = async () => (await api(`/api/v1/documents/${metalId}`)).document.atoms.length
  const before = await metalAtoms()
  const loseReply = async (route) => {
    const request = route.request().postDataJSON()
    if (request?.operation !== 'document.import' || request?.parameters?.preview === true) return route.continue()
    await route.fetch()
    return route.abort('connectionreset')
  }
  await page.route('**/api/v1/operations', loseReply)
  await panel.getByRole('button', { name: 'Import SMILES' }).click()
  await panel.getByRole('button', { name: 'Retry the same import' }).waitFor()
  await page.unroute('**/api/v1/operations', loseReply)
  assert.equal(await metalAtoms(), before + 2, 'the first request did commit')
  // The first commit went through, so a new preview names a stale revision and is refused. That
  // must not take the retry away: it is the only way left to learn what the first request did.
  const stalePreview = page.waitForResponse((response) => response.url().includes('/api/v1/operations') &&
    response.request().postDataJSON()?.parameters?.preview === true)
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await stalePreview
  await panel.getByRole('button', { name: 'Preview SMILES import' }).isEnabled()
  assert.equal(await panel.getByRole('button', { name: 'Retry the same import' }).count(), 1, 'a refused re-preview keeps the retry')
  assert.equal(await metalAtoms(), before + 2, 'the refused preview imported nothing')
  await panel.getByRole('button', { name: 'Retry the same import' }).click()
  await panel.getByText(/Imported\./).waitFor()
  assert.equal(await metalAtoms(), before + 2, 'the retry was answered from the committed reply, not imported again')
  await panel.getByRole('button', { name: 'Retry the same import' }).waitFor({ state: 'hidden' })

  // A restart between the preview and the import refuses the import in the panel.
  await source.fill('CC')
  await panel.getByRole('button', { name: 'Preview SMILES import' }).click()
  await panel.getByTestId('smiles-preview-summary').waitFor()
  // The successful import above schedules a recovery draft. Let that request finish
  // before deliberately killing the service so its unrelated response is not cut off.
  const metalRevision = (await api(`/api/v1/documents/${metalId}`)).document.revision
  await page.waitForFunction(
    (revision) => document.querySelector('[data-testid="draft-state"]')?.textContent?.includes(`(revision ${revision})`),
    metalRevision,
  )
  await crashApi()
  await panel.getByRole('button', { name: 'Import SMILES' }).click()
  await panel.getByRole('alert').getByText(/The local service restarted/).waitFor()
  assert.equal(await panel.getByRole('button', { name: 'Import SMILES' }).count(), 0)
  results.smiles_panel = true
}

// Inspect an independently drawn benzene graph, explicitly convert it, and undo the edit.
{
  const created = await apiWrite('/api/v1/documents', { request_id: 'aromatic-create', title: 'Aromaticity' })
  const id = created.body.document.id
  const commands = []
  for (let i = 0; i < 6; i += 1) commands.push({ op: 'add_atom', element: 'C', position: { x: 40 * Math.cos(i * Math.PI / 3), y: 40 * Math.sin(i * Math.PI / 3) } })
  for (let i = 0; i < 6; i += 1) commands.push({ op: 'add_bond', begin: `$${i}`, end: `$${(i + 1) % 6}`, order: i % 2 === 0 ? 'double' : 'single' })
  const drawn = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'aromatic-draw', expected_revision: 0, commands })
  assert.equal(drawn.status, 200)
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const panel = page.getByRole('region', { name: 'Rings and aromaticity' })
  await panel.getByRole('button', { name: 'Inspect rings' }).click()
  await panel.getByTestId('aromaticity-result').getByText(/aromatic; cycle rank 1; 6 π electrons/).waitFor()
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, drawn.body.document, 'inspection leaves every recorded bond unchanged')
  await crashApi()
  const identical = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'aromatic-identical', expected_revision: 0, commands })
  assert.equal(identical.status, 200)
  assert.deepEqual(identical.body.document, drawn.body.document, 'the restarted service recreated the same ID, revision and graph')
  const valencePanel = page.getByRole('complementary', { name: 'Valence inspection' })
  await valencePanel.getByRole('button', { name: 'Inspect valence' }).click()
  await panel.getByText('The drawing or service changed. Inspect again.', { exact: true }).waitFor()
  assert.equal(await panel.getByTestId('aromaticity-result').count(), 0, 'a restart invalidates even an identical snapshot')
  const afterValence = await api(`/api/v1/documents/${id}`)
  assert.deepEqual(afterValence.document, identical.body.document, 'the restart check does not change the drawing')
  assert.equal(afterValence.session.undo_depth, identical.body.session.undo_depth, 'the restart check does not change history')
  await panel.getByRole('button', { name: 'Inspect rings' }).click()
  await panel.getByTestId('aromaticity-result').getByText(/aromatic; cycle rank 1; 6 π electrons/).waitFor()
  await crashApi()
  const replacementCommands = commands.map((command, index) => index === 0 ? { ...command, element: 'N' } : command)
  const replacement = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: 'aromatic-replacement', expected_revision: 0, commands: replacementCommands })
  assert.equal(replacement.status, 200)
  assert.equal(replacement.body.document.revision, drawn.body.document.revision, 'the restarted service reused the revision')
  await tool('Select and move')
  await canvas.scrollIntoViewIfNeeded()
  const oldAtom = await atomScreen(drawn.body, 'a1')
  await page.mouse.click(oldAtom.x, oldAtom.y)
  await page.getByRole('button', { name: 'Increase charge' }).click()
  await page.getByText('Local service restarted', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Reload' }).click()
  await panel.getByText('The drawing or service changed. Inspect again.', { exact: true }).waitFor()
  assert.equal(await panel.getByTestId('aromaticity-result').count(), 0, 'the old same-revision result is hidden')
  await panel.getByRole('button', { name: 'Inspect rings' }).click()
  await panel.getByTestId('aromaticity-result').getByText(/aromatic; cycle rank 1; 6 π electrons/).waitFor()
  const convertedReply = page.waitForResponse((response) => response.url().endsWith('/api/v1/operations') && response.request().postData()?.includes('chem.aromaticity_convert'))
  await panel.getByRole('button', { name: 'Convert to aromatic' }).click()
  assert.equal((await convertedReply).status(), 200)
  await waitIdle()
  assert.ok((await api(`/api/v1/documents/${id}`)).document.bonds.every((bond) => bond.order === 'aromatic'))
  await panel.getByText('The drawing or service changed. Inspect again.', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await waitIdle()
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document.bonds, replacement.body.document.bonds, 'one undo restores the original Kekulé bonds')
  await panel.getByRole('button', { name: 'Inspect rings' }).click()
  await panel.getByTestId('aromaticity-result').getByText(/6 π electrons/).waitFor()
  await page.screenshot({ path: join(outDir, 'editor-aromaticity.png') })
  results.aromaticity_inspection_and_atomic_conversion = true
}

// Substructure search: the two drawings' IDs overlap, the result describes the snapshots it
// searched, survives a refresh of state that is not the graph, and is hidden (not dropped)
// when this drawing or the service changes. A refusal appears in the panel's own error area.
{
  step = 'substructure search'
  const make = async (title, name, atoms, bonds) => {
    const created = await apiWrite('/api/v1/documents', { request_id: `${name}-create`, title })
    const id = created.body.document.id
    const commands = [
      ...atoms.map((element, i) => ({ op: 'add_atom', element, position: { x: 40 * Math.cos(i * Math.PI / 3), y: 40 * Math.sin(i * Math.PI / 3) } })),
      ...bonds.map(([begin, end, order]) => ({ op: 'add_bond', begin: `$${begin}`, end: `$${end}`, order })),
    ]
    const drawn = await apiWrite(`/api/v1/documents/${id}/transactions`, { request_id: `${name}-draw`, expected_revision: 0, commands })
    assert.equal(drawn.status, 200)
    return { id, drawn: drawn.body, commands }
  }
  const ring = (kinds) => kinds.map((order, i) => [i, (i + 1) % kinds.length, order])
  const query = await make('Match query C-O', 'match-query', ['C', 'O'], [[0, 1, 'single']])
  const ethanol = await make('Match target ethanol', 'match-ethanol', ['C', 'C', 'O'], [[0, 1, 'single'], [1, 2, 'single']])
  await page.reload()
  const open = async (id) => {
    await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
    return page.getByRole('region', { name: 'Substructure search' })
  }
  let panel = await open(query.id)
  const search = panel.getByRole('button', { name: 'Search', exact: true })
  assert.equal(await search.isDisabled(), true, 'no search until the other drawing is chosen')
  await panel.getByLabel('Other drawing').click()
  await page.getByRole('option', { name: `Match target ethanol (${ethanol.id})`, exact: true }).click()
  const before = await api(`/api/v1/documents/${query.id}`)
  const targetBefore = await api(`/api/v1/documents/${ethanol.id}`)
  await search.click()
  const result = panel.getByTestId('substructure-result')
  await result.getByText('1 mapping(s) found.', { exact: true }).waitFor()
  await result.getByText('Atoms (query→target): a1→a2, a2→a3', { exact: true }).waitFor()
  await panel.getByTestId('substructure-asked').getByText(`Asked with this drawing as the query; the other drawing is ${ethanol.id}.`, { exact: true }).waitFor()
  await result.getByText('Bonds (query→target): b1→b2', { exact: true }).waitFor()
  await result.getByText(new RegExp(`Query ${query.id} revision 1; target ${ethanol.id} revision 1`)).waitFor()
  const after = await api(`/api/v1/documents/${query.id}`)
  assert.deepEqual(after.document, before.document, 'a search leaves the drawing unchanged')
  assert.deepEqual([after.session.undo_depth, after.session.redo_depth], [before.session.undo_depth, before.session.redo_depth], 'a search adds no history')
  assert.deepEqual(await api(`/api/v1/documents/${ethanol.id}`), targetBefore, 'a search leaves the target unchanged')
  await page.screenshot({ path: join(outDir, 'editor-substructure.png') })

  // A refresh of state that is not the graph keeps the result: the document list, and a save,
  // which replaces the page's document without changing a bond or an atom.
  await panel.getByRole('button', { name: 'Refresh documents' }).click()
  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
  await result.getByText('1 mapping(s) found.', { exact: true }).waitFor()
  assert.equal(await panel.getByTestId('substructure-stale').count(), 0, 'a refresh or save does not mark the result stale')

  // Selecting a mapping selects its query atoms and bond in this drawing.
  await panel.getByRole('button', { name: 'Select mapping 1 in this drawing' }).click()
  await page.getByText('3 selected', { exact: true }).waitFor()
  await result.getByText('1 mapping(s) found.', { exact: true }).waitFor()
  assert.equal(await panel.getByTestId('substructure-stale').count(), 0, 'selecting does not mark the result stale')

  // An edit to the OTHER drawing, made elsewhere, is noticed once the document list is refreshed:
  // the result is hidden with a notice, as for this drawing, and a new search sees the edit.
  const otherEdit = await apiWrite(`/api/v1/documents/${ethanol.id}/transactions`, {
    request_id: 'match-ethanol-edit', expected_revision: 1,
    commands: [{ op: 'add_atom', element: 'N', position: { x: 0, y: 90 } }],
  })
  assert.equal(otherEdit.status, 200)
  await panel.getByRole('button', { name: 'Refresh documents' }).click()
  await panel.getByTestId('substructure-stale').getByText('The other drawing changed since the last search. Search again.', { exact: true }).waitFor()
  assert.equal(await result.count(), 0, 'an edit to the other drawing hides the earlier result')
  await search.click()
  await result.getByText(new RegExp(`target ${ethanol.id} revision 2`)).waitFor()
  assert.equal(await panel.getByTestId('substructure-stale').count(), 0, 'the new search is about the edited drawing')

  // A charge on the query atom is a condition: C+ does not occur in an uncharged target.
  await tool('Select and move')
  await canvas.scrollIntoViewIfNeeded()
  const carbonPoint = await atomScreen(query.drawn, 'a1')
  const area = await canvas.boundingBox()
  await page.mouse.click(area.x + area.width * 0.8, area.y + area.height * 0.8)
  await page.getByText('3 selected', { exact: true }).waitFor({ state: 'hidden' })
  await page.mouse.click(carbonPoint.x, carbonPoint.y)
  await page.getByRole('button', { name: 'Increase charge' }).click()
  await waitIdle()
  await panel.getByTestId('substructure-stale').getByText('This drawing changed since the last search. Search again.', { exact: true }).waitFor()
  assert.equal(await result.count(), 0, 'an edit hides the earlier result')
  await search.click()
  await result.getByText('The query does not occur in the target: a complete search found no mapping.', { exact: true }).waitFor()

  // A response that arrives after an edit is kept, and marked as belonging to the older drawing.
  let held
  const heldArrived = new Promise((resolve) => { held = resolve })
  let release
  const releaseHeld = new Promise((resolve) => { release = resolve })
  const hold = async (route) => {
    if (route.request().postDataJSON()?.operation !== 'chem.substructure') return route.continue()
    const response = await route.fetch()
    held()
    await releaseHeld
    await route.fulfill({ response })
  }
  await page.route('**/api/v1/operations', hold)
  await search.click()
  await heldArrived
  await page.mouse.click(carbonPoint.x, carbonPoint.y)
  await page.getByRole('button', { name: 'Decrease charge' }).click()
  release()
  await panel.getByTestId('substructure-stale').waitFor()
  await page.unroute('**/api/v1/operations', hold)
  assert.equal(await result.count(), 0, 'a response for an older drawing is not shown as current')
  await search.click()
  await result.getByText('1 mapping(s) found.', { exact: true }).waitFor()

  // A service restart hides the result even when the same graph and revision come back.
  await crashApi()
  await page.getByRole('complementary', { name: 'Valence inspection' }).getByRole('button', { name: 'Inspect valence' }).click()
  await panel.getByTestId('substructure-stale').getByText('The service restarted since the last search. Search again.', { exact: true }).waitFor()
  assert.equal(await result.count(), 0, 'a restart hides the earlier result')

  // Another document has its own panel state; coming back does not resurrect an old result.
  await closeShown()
  panel = await open(ethanol.id)
  assert.equal(await panel.getByTestId('substructure-result').count(), 0)
  assert.equal(await panel.getByTestId('substructure-stale').count(), 0)
  await closeShown()
  panel = await open(query.id)
  assert.equal(await panel.getByTestId('substructure-result').count(), 0, 'a reopened document starts without the result of its earlier panel')
  assert.equal(await panel.getByTestId('substructure-stale').count(), 0)
  await closeShown()

  // A refusal names its side and reason in the panel's error area, with both snapshots. These
  // drawings are made after the restart, which discards drawings the service held unsaved.
  const benzene = await make('Match query benzene', 'match-benzene', ['C', 'C', 'C', 'C', 'C', 'C'], ring(['double', 'single', 'double', 'single', 'double', 'single']))
  const tetralin = await make('Match target tetralin', 'match-tetralin', ['C', 'C', 'C', 'C', 'C', 'C', 'C', 'C', 'C', 'C'],
    [...ring(['double', 'single', 'double', 'single', 'double', 'single']), [0, 6, 'single'], [6, 7, 'single'], [7, 8, 'single'], [8, 9, 'single'], [9, 5, 'single']])
  await page.reload()
  panel = await open(benzene.id)
  await panel.getByLabel('Other drawing').click()
  await page.getByRole('option', { name: `Match target tetralin (${tetralin.id})`, exact: true }).click()
  await panel.getByRole('button', { name: 'Search', exact: true }).click()
  const refusal = panel.getByTestId('substructure-error')
  await refusal.getByText('No answer: match unsupported', { exact: true }).waitFor()
  await refusal.getByText(/The target drawing, bond b\d+: fused partially aromatic\./).waitFor()
  await refusal.getByText(new RegExp(`Query ${benzene.id} revision 1; target ${tetralin.id} revision 1`)).waitFor()
  assert.equal(await panel.getByTestId('substructure-result').count(), 0, 'a refusal is not shown as an empty answer')
  await panel.getByTestId('substructure-asked').getByText(`Asked with this drawing as the query; the other drawing is ${tetralin.id}.`, { exact: true }).waitFor()

  // The document changes (a copy opens) while a search is running. The form starts again for the
  // copy, and the copy can be searched while the first document's search is still out; that
  // search's late outcome belongs to its own document and changes nothing shown for the copy.
  let heldCopy
  const copyArrived = new Promise((resolve) => { heldCopy = resolve })
  let releaseCopy
  const copyHeld = new Promise((resolve) => { releaseCopy = resolve })
  let holdingFirst = true
  const holdCopy = async (route) => {
    if (route.request().postDataJSON()?.operation !== 'chem.substructure' || !holdingFirst) return route.continue()
    holdingFirst = false
    const response = await route.fetch()
    heldCopy()
    await copyHeld
    await route.fulfill({ response })
  }
  await page.route('**/api/v1/operations', holdCopy)
  // The direction is part of the form too: choose the other one for this drawing, and see below
  // that the copy starts again from the default.
  await panel.getByLabel('This drawing is the').click()
  await page.getByRole('option', { name: 'target (search another drawing in it)', exact: true }).click()
  await panel.getByLabel('This drawing is the').getByText('target (search another drawing in it)', { exact: true }).waitFor()
  await panel.getByRole('button', { name: 'Search', exact: true }).click()
  await copyArrived
  await panel.getByRole('button', { name: 'Searching…' }).waitFor()
  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
  await page.getByRole('button', { name: 'Save as copy' }).click()
  await page.getByRole('dialog').getByLabel('Title of the copy').fill('Benzene copy')
  await page.getByRole('dialog').getByRole('button', { name: 'Save copy' }).click()
  await page.getByTestId('copy-notice').waitFor()
  await panel.getByLabel('Other drawing').getByText('Choose a drawing', { exact: true }).waitFor()
  await panel.getByLabel('This drawing is the').getByText('query (search it in another drawing)', { exact: true }).waitFor()
  assert.equal(await panel.getByTestId('substructure-result').count(), 0, 'the copy shows no result')
  assert.equal(await panel.getByTestId('substructure-error').count(), 0, 'the copy shows no refusal')
  assert.equal(await panel.getByTestId('substructure-stale').count(), 0, 'the copy is not stale: it has no earlier search')
  // The first document's search is still out, yet the copy's button is its own.
  const copySearch = panel.getByRole('button', { name: 'Search', exact: true })
  await copySearch.waitFor()
  assert.equal(await copySearch.isDisabled(), true, 'no search until another drawing is chosen for the copy')
  await panel.getByLabel('Other drawing').click()
  await page.getByRole('option', { name: 'This drawing', exact: true }).click()
  assert.equal(await copySearch.isDisabled(), false, 'the copy can be searched while the first document search is out')
  await copySearch.click()
  await panel.getByTestId('substructure-result').getByText('12 mapping(s) found.', { exact: true }).waitFor()
  await panel.getByTestId('substructure-asked').getByText('Asked with this drawing as the query; the other drawing is this drawing itself.', { exact: true }).waitFor()
  const lateReply = page.waitForResponse((response) => response.url().endsWith('/api/v1/operations') && response.request().postDataJSON()?.operation === 'chem.substructure')
  releaseCopy()
  await lateReply
  await panel.getByRole('button', { name: 'Search', exact: true }).waitFor()
  await page.unroute('**/api/v1/operations', holdCopy)
  assert.equal(await panel.getByTestId('substructure-error').count(), 0, 'a search that outlived its document does not show for the next one')
  await panel.getByTestId('substructure-result').getByText('12 mapping(s) found.', { exact: true }).waitFor()
  results.substructure_search_panel = true
}

// Stereo: a drawing made through the API shows its notice and its marked bond's name, and an
// edit that clears a configuration shows the note the service sends. The editor has no tool to
// set stereo, so the drawing is made by the stereo commands and only read here.
{
  const stereoPage = await context.newPage()
  try {
    await stereoPage.goto(BASE)
    const stereoWrite = (path, body) =>
      stereoPage.evaluate(async ({ path, body }) => {
        const session = await (await fetch('/api/v1/session')).json()
        const response = await fetch(path, {
          method: 'POST',
          headers: { 'content-type': 'application/json', [session.header]: session.token },
          body: JSON.stringify(body),
        })
        return { status: response.status, body: await response.json() }
      }, { path, body })
    const stereoId = (await stereoWrite('/api/v1/documents', { request_id: 'smoke-stereo-create', title: 'Smoke stereo' })).body.document.id
    const bonded = (x, y, element) => ({
      op: 'add_atom', element, position: { x, y }, bond_to: { atom: '$0', order: 'single' },
    })
    const drawn = await stereoWrite(`/api/v1/documents/${stereoId}/transactions`, {
      request_id: 'smoke-stereo-draw',
      expected_revision: 0,
      commands: [
        { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
        bonded(100, 70, 'F'), bonded(126, 115, 'Cl'), bonded(74, 115, 'Br'),
        { op: 'set_bond_mark', bond: '$2', mark: { kind: 'wedge', narrow: 'begin' } },
        { op: 'set_atom_configuration', atom: '$0', configuration: {
          kind: 'tetrahedral', neighbors: ['$3', '$1', '$2', 'implicit_hydrogen'], turn: 'clockwise' } },
      ],
    })
    assert.equal(drawn.status, 200, JSON.stringify(drawn.body))
    assert.deepEqual(drawn.body.notes, [])
    await stereoPage.reload()
    await stereoPage.locator(`li[data-document-id="${stereoId}"]`).getByRole('button', { name: 'Open' }).click()
    const notice = stereoPage.getByTestId('stereo-notice')
    await notice.waitFor()
    assert.match(await notice.textContent(), /1 stereo mark and 1 stored configuration\./)
    assert.ok((await stereoPage.locator('[aria-label*="wedge"]').count()) >= 1, 'the marked bond is named with its mark')
    assert.equal(await stereoPage.getByTestId('stereo-notes').count(), 0, 'nothing was cleared yet')
    // Erasing the fluorine changes the center's neighbors: the configuration goes, and the note says why.
    const fluorine = drawn.body.document.atoms.find((atom) => atom.element === 'F').position
    await stereoPage.getByRole('button', { name: 'Erase', exact: true }).first().click()
    const at = await stereoPage.evaluate(({ x, y }) => {
      const svg = document.querySelector('svg[data-structure-canvas]')
      const p = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM())
      return { x: p.x, y: p.y }
    }, fluorine)
    await stereoPage.mouse.click(at.x, at.y)
    const notes = stereoPage.getByTestId('stereo-notes')
    await notes.waitFor()
    assert.match(await notes.textContent(), /stereo configuration of atom a1 was cleared because its bonded neighbors changed\. Undoing this edit, while it is still in the history, brings it back\./)
    assert.match(await notice.textContent(), /0 stored configurations/)
    await stereoPage.getByRole('button', { name: 'Undo' }).click()
    await stereoPage.getByTestId('stereo-notes').waitFor({ state: 'detached' })
    assert.match(await notice.textContent(), /1 stored configuration\./)
    results.stereo_notice_and_cleared_note = true
    step = 'after stereo_notice_and_cleared_note'
  } finally {
    await stereoPage.close()
  }
}

// Stereo marks drawn with the bond tool and the inspector: a wedge from a drag, the configuration it
// stores in the same transaction (one undo step), hashed wedge, wavy and crossed, a refusal that
// keeps the mark, what an order change clears, and the keyboard path through the inspector.
{
  const markPage = await context.newPage()
  try {
    await markPage.goto(BASE)
    const write = (path, body) =>
      markPage.evaluate(async ({ path, body }) => {
        const session = await (await fetch('/api/v1/session')).json()
        const response = await fetch(path, {
          method: 'POST',
          headers: { 'content-type': 'application/json', [session.header]: session.token },
          body: JSON.stringify(body),
        })
        return { status: response.status, body: await response.json() }
      }, { path, body })
    const read = (id) => markPage.evaluate(async (id) => (await fetch(`/api/v1/documents/${id}`)).json(), id)
    const place = (point) =>
      markPage.evaluate(({ x, y }) => {
        const svg = document.querySelector('svg[data-structure-canvas]')
        const p = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM())
        return { x: p.x, y: p.y }
      }, point)
    const idle = () => markPage.waitForFunction(() => !document.querySelector('[data-slot="spinner"]'))
    const bonded = (x, y, element, order = 'single') => ({ op: 'add_atom', element, position: { x, y }, bond_to: { atom: '$0', order } })
    const make = async (key, title, commands) => {
      const created = await write('/api/v1/documents', { request_id: `smoke-marks-${key}`, title })
      assert.ok(created.status < 300, JSON.stringify(created.body))
      const id = created.body.document.id
      const drawn = await write(`/api/v1/documents/${id}/transactions`, { request_id: `smoke-marks-draw-${key}`, expected_revision: 0, commands })
      assert.equal(drawn.status, 200, JSON.stringify(drawn.body))
      return id
    }
    const center = (x, y) => ({ op: 'add_atom', element: 'C', position: { x, y } })
    const first = await make('first', 'Smoke marks', [center(100, 100), bonded(100, 70, 'F'), bonded(126, 115, 'Cl')])
    const second = await make('second', 'Smoke marks refused', [
      center(100, 100), bonded(100, 70, 'F'), bonded(126, 115, 'Cl'), bonded(74, 115, 'N'),
      { op: 'set_bond_mark', bond: '$3', mark: { kind: 'wavy' } },
      { op: 'add_atom', element: 'C', position: { x: 300, y: 100 } },
      { op: 'add_atom', element: 'C', position: { x: 330, y: 100 }, bond_to: { atom: '$5', order: 'double' } },
    ])
    const open = async (id) => {
      await markPage.reload()
      await markPage.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
      await markPage.getByTestId('save-state').waitFor()
    }
    const status = markPage.getByTestId('stereo-status')
    const inspector = markPage.getByRole('complementary', { name: 'Selection' })
    const bondName = (id) => markPage.locator(`g[data-bond-kind][aria-label^="Bond ${id}:"]`)
    const middle = async (doc, a, b) => {
      const at = (id) => doc.document.atoms.find((atom) => atom.id === id).position
      return place({ x: (at(a).x + at(b).x) / 2, y: (at(a).y + at(b).y) / 2 })
    }
    // A status line above the canvas moves it, so a bond's place on the screen is read again at each click.
    const clickBetween = async (doc, a, b) => {
      const at = await middle(doc, a, b)
      await markPage.mouse.click(at.x, at.y)
    }
    // The service answers before the page has drawn the reply: edits wait for the new revision, and what
    // the page shows is checked until it shows it.
    const raf = () => markPage.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
    const edit = async (id, act) => {
      const revision = (await read(id)).document.revision
      await act()
      await markPage.waitForFunction(
        async ({ id, revision }) => (await (await fetch(`/api/v1/documents/${id}`)).json()).document.revision !== revision,
        { id, revision },
      )
      await idle()
      await raf()
    }
    const eventually = async (check) => {
      for (let attempt = 0; ; attempt += 1) {
        try {
          return await check()
        } catch (failure) {
          if (attempt >= 50) {
            failure.message += `\nstatus: ${await status.textContent().catch(() => '(none)')}`
            throw failure
          }
          await raf()
        }
      }
    }
    const press = (name) => markPage.getByRole('button', { name, exact: true }).first().click()

    await open(first)
    // The status line is a live region that exists, and is in the accessibility tree, before it has text.
    assert.equal(await markPage.getByRole('status').and(markPage.getByTestId('stereo-status')).count(), 1, 'the empty status line is in the accessibility tree')
    assert.equal(await status.textContent(), '')
    assert.notEqual(await status.evaluate((element) => getComputedStyle(element).display), 'none')
    // Arrow keys move through every bond kind, the orders and the marks, in the order they are shown, from one tab stop.
    const kinds = markPage.getByRole('group', { name: 'Bond kind for new bonds', exact: true })
    const labels = await kinds.getByRole('button').evaluateAll((buttons) => buttons.map((button) => button.getAttribute('aria-label')))
    assert.deepEqual(labels.slice(-3), ['Wedge bond', 'Hashed wedge bond', 'Wavy bond'])
    assert.ok(labels.length >= 8, labels.join(', '))
    assert.equal(await kinds.getByRole('button').evaluateAll((buttons) => buttons.filter((button) => button.tabIndex === 0).length), 1, 'one tab stop')
    await kinds.getByRole('button', { name: labels[0], exact: true }).focus()
    const visited = [labels[0]]
    for (let index = 1; index < labels.length; index += 1) {
      await markPage.keyboard.press('ArrowRight')
      visited.push(await markPage.evaluate(() => document.activeElement?.getAttribute('aria-label')))
    }
    assert.deepEqual(visited, labels, 'the arrow keys visit every bond kind in order')
    // The tools are named and pressed state shows which is chosen.
    for (const name of ['Wedge bond', 'Hashed wedge bond', 'Wavy bond']) {
      assert.equal(await markPage.getByRole('button', { name, exact: true }).getAttribute('aria-pressed'), 'false', name)
    }
    await press('Wedge bond')
    assert.equal(await markPage.getByRole('button', { name: 'Wedge bond', exact: true }).getAttribute('aria-pressed'), 'true')
    let doc = await read(first)
    const start = await place(doc.document.atoms.find((atom) => atom.id === 'a1').position)
    const target = await place({ x: 60, y: 125 })
    await markPage.mouse.move(start.x, start.y)
    await markPage.mouse.down()
    await markPage.mouse.move(start.x - 20, start.y + 10, { steps: 4 })
    // While dragging, the mark shows where its narrow end will be.
    await eventually(async () => assert.equal(await markPage.locator('[data-bond-preview="wedge"] polygon').count(), 1, 'the drag previews a wedge'))
    await markPage.mouse.move(target.x, target.y, { steps: 4 })
    await edit(first, () => markPage.mouse.up())
    doc = await read(first)
    assert.equal(doc.document.revision, 2, 'the mark and its configuration are one revision')
    const wedge = doc.document.bonds.find((bond) => bond.id === 'b3')
    assert.deepEqual(wedge.mark, { kind: 'wedge', narrow: 'begin' })
    assert.equal(doc.document.stereo.atoms.length, 1)
    assert.equal(doc.document.stereo.atoms[0].atom, 'a1')
    const polygon = markPage.locator('g[data-bond-mark="wedge"] polygon[data-stroke="wedge"]')
    await polygon.waitFor()
    const points = (await polygon.getAttribute('points')).split(' ').map((p) => p.split(',').map(Number))
    const a1 = doc.document.atoms.find((atom) => atom.id === 'a1').position
    assert.equal(points.length, 3, 'a point and two wide corners')
    assert.ok(Math.hypot(points[0][0] - a1.x, points[0][1] - a1.y) < 0.6, 'the narrow end is at a1')
    assert.ok(Math.abs(Math.hypot(points[1][0] - points[2][0], points[1][1] - points[2][1]) - 4) < 0.1, 'the wide end is 4 pt wide')
    await eventually(async () => assert.match(await bondName('b3').getAttribute('aria-label'), /wedge, narrow end at a1$/))
    await eventually(async () => assert.match(await status.textContent(), /^Wedge mark on b3, narrow end at a1\. Configuration of a1 stored from the drawing \((clockwise|anticlockwise)\)\.$/))
    // One undo removes the bond, its mark and the configuration together; redo brings them back.
    await edit(first, () => press('Undo'))
    doc = await read(first)
    assert.equal(doc.document.bonds.length, 2)
    assert.deepEqual(doc.document.stereo, { atoms: [], bonds: [], groups: [] })
    await eventually(async () => assert.equal(await polygon.count(), 0))
    await edit(first, () => press('Redo'))
    doc = await read(first)
    assert.equal(doc.document.bonds.length, 3)
    assert.equal(doc.document.stereo.atoms.length, 1)
    await polygon.waitFor()
    await markPage.screenshot({ path: join(outDir, 'editor-marks-wedge.png') })
    // A hovered wedge is marked as any hovered bond is: by the underlay, which is wider than the wedge.
    const hover = await middle(doc, 'a1', 'a4')
    await markPage.mouse.move(hover.x, hover.y)
    const underlay = markPage.locator('[data-underlay="b3"]')
    await eventually(async () => assert.equal(await underlay.count(), 1, 'the hovered wedge has an underlay'))
    assert.ok(Number(await underlay.getAttribute('stroke-width')) >= 8, 'wider than the wedge')
    await markPage.mouse.move(5, 5)
    await eventually(async () => assert.equal(await underlay.count(), 0))

    // A hashed wedge over it keeps the narrow end; the configuration it shows replaces the stored one.
    const shape = doc
    await press('Hashed wedge bond')
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    const hashed = markPage.locator('g[data-bond-mark="hash"] path[data-stroke="hash"]')
    await hashed.waitFor()
    assert.ok(Number(await hashed.getAttribute('data-stripes')) >= 8, 'a stripe every 2.7 pt')
    await eventually(async () => assert.match(await status.textContent(), /^Hashed wedge mark on b3, narrow end at a1\. Configuration of a1 stored from the drawing \(\w+\)\. It replaced a stored configuration \(\w+\); undo restores it\.$/))
    // Again: the narrow end moves to a4, which has one bond, so nothing is stored; a1's record is kept.
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    await eventually(async () => assert.match(await bondName('b3').getAttribute('aria-label'), /hashed wedge, narrow end at a4$/))
    await eventually(async () => assert.match(await status.textContent(), /Nothing stored: a4 has 1 bond; a stereocenter needs 3 or 4\. The stored configuration of a1 is kept; the drawing no longer shows it\.$/))
    assert.equal((await read(first)).document.stereo.atoms.length, 1)
    await markPage.screenshot({ path: join(outDir, 'editor-marks-hashed-wedge.png') })
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    assert.equal((await read(first)).document.bonds.find((bond) => bond.id === 'b3').mark, null)
    await eventually(async () => assert.match(await status.textContent(), /^Hashed wedge mark removed from b3\.$/))

    // Wavy: a wave on the axis, and an unknown configuration for the center.
    await press('Wavy bond')
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    await markPage.locator('g[data-bond-mark="wavy"] path[data-stroke="wave"]').waitFor()
    assert.equal((await read(first)).document.stereo.atoms[0].configuration.kind, 'unknown')
    await markPage.screenshot({ path: join(outDir, 'editor-marks-wavy.png') })
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    await eventually(async () => assert.equal(await markPage.locator('svg[data-structure-canvas] path[data-stroke="wave"]').count(), 0))

    // The bond tool with a kind of bond chosen clears a mark first, and the inspector's change of kind
    // clears it in the same transaction: neither is refused.
    await press('Wedge bond')
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    await press('Single bond')
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    assert.equal((await read(first)).document.bonds.find((bond) => bond.id === 'b3').mark, null)
    assert.equal((await read(first)).document.bonds.find((bond) => bond.id === 'b3').order, 'single', 'the click only cleared the mark')
    await eventually(async () => assert.match(await status.textContent(), /^Wedge mark removed from b3\./))
    await press('Wedge bond')
    await edit(first, () => clickBetween(shape, 'a1', 'a4'))
    await press('Select and move')
    await clickBetween(shape, 'a1', 'a4')
    const revisionBefore = (await read(first)).document.revision
    await edit(first, () => inspector.getByRole('group', { name: 'Kind', exact: true }).getByRole('button', { name: 'Double bond' }).click())
    doc = await read(first)
    assert.equal(doc.document.revision, revisionBefore + 1, 'one transaction')
    assert.equal(doc.document.bonds.find((bond) => bond.id === 'b3').order, 'double')
    assert.equal(doc.document.bonds.find((bond) => bond.id === 'b3').mark, null)
    assert.equal(await markPage.getByText('Edit rejected').count(), 0)
    await eventually(async () => assert.match(await status.textContent(), /^Wedge mark removed from b3: it needs a single bond\./))

    // The keyboard path: the inspector's own controls set the mark and its narrow end.
    await edit(first, () => inspector.getByRole('group', { name: 'Kind', exact: true }).getByRole('button', { name: 'Single bond' }).click())
    const marks = inspector.getByRole('group', { name: 'Drawn mark', exact: true })
    assert.deepEqual(await marks.getByRole('button').evaluateAll((buttons) => buttons.map((b) => b.getAttribute('aria-label'))), ['Plain', 'Wedge', 'Hashed wedge', 'Wavy'])
    await marks.getByRole('button', { name: 'Wedge', exact: true }).focus()
    await edit(first, () => markPage.keyboard.press('Space'))
    assert.deepEqual((await read(first)).document.bonds.find((bond) => bond.id === 'b3').mark, { kind: 'wedge', narrow: 'begin' })
    await inspector.getByRole('group', { name: 'Narrow end', exact: true }).getByRole('button', { name: 'Narrow end at a4' }).focus()
    await edit(first, () => markPage.keyboard.press('Enter'))
    assert.deepEqual((await read(first)).document.bonds.find((bond) => bond.id === 'b3').mark, { kind: 'wedge', narrow: 'end' })
    await eventually(async () => assert.equal(await markPage.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Narrow end at a4', 'focus stays on the control'))

    // A wedge over a double bond is refused without a request.
    await open(second)
    doc = await read(second)
    const before = doc.document.revision
    await press('Wedge bond')
    const pair = doc
    await clickBetween(pair, 'a5', 'a6')
    await status.filter({ hasText: 'A wedge mark needs a single bond' }).waitFor()
    assert.equal((await read(second)).document.revision, before, 'nothing was sent')
    await eventually(async () => assert.equal(await status.textContent(), 'A wedge mark needs a single bond; b4 is a double bond. Change it to single first.'))
    // The same bond takes a crossed mark in the inspector: two lines, the configuration unknown.
    await press('Select and move')
    await clickBetween(pair, 'a5', 'a6')
    await edit(second, () => markPage.getByRole('button', { name: 'Crossed double bond (either)' }).click())
    doc = await read(second)
    assert.deepEqual(doc.document.bonds.find((bond) => bond.id === 'b4').mark, { kind: 'crossed' })
    assert.deepEqual(doc.document.stereo.bonds, [{ bond: 'b4', configuration: { kind: 'unknown' } }])
    await eventually(async () => assert.equal(await markPage.locator('g[data-bond-mark="crossed"] line').count(), 2))
    await eventually(async () => assert.match(await bondName('b4').getAttribute('aria-label'), /double bond between a5 and a6, crossed$/))

    // A wedge the service cannot read a configuration from (a wavy bond to an atom that cannot be a
    // center beside it) is kept, and the reason is shown instead of an error.
    doc = await read(second)
    const revisionRefused = doc.document.revision
    // The wedge is still the chosen kind; pressing it again changes nothing, so the tool is chosen.
    await press('Bond')
    assert.equal(await markPage.getByRole('button', { name: 'Wedge bond', exact: true }).getAttribute('aria-pressed'), 'true')
    const from = await place(doc.document.atoms.find((atom) => atom.id === 'a1').position)
    const down = await place({ x: 100, y: 140 })
    await markPage.mouse.move(from.x, from.y)
    await markPage.mouse.down()
    await markPage.mouse.move(down.x, down.y, { steps: 5 })
    await edit(second, () => markPage.mouse.up())
    doc = await read(second)
    assert.equal(doc.document.revision, revisionRefused + 1)
    assert.equal(doc.document.bonds.filter((bond) => bond.mark?.kind === 'wedge').length, 1, 'the mark was kept')
    assert.deepEqual(doc.document.stereo.atoms, [], 'no configuration was stored')
    await eventually(async () => assert.match(await status.textContent(), /Not stored: .*\(stereo_not_derivable_conflicting_marks\)\.$/))
    assert.equal(await markPage.getByText('Edit rejected').count(), 0, 'a refused derivation is not an error')
    results.stereo_marks_drawn_and_edited = true
    step = 'after stereo_marks_drawn_and_edited'
  } finally {
    await markPage.close()
  }
}

// Stereo agreement indicator: the page asks chem.stereo for a drawing that holds stereo and shows the
// answer only for the drawing on screen, as a panel row, a summary line and a ring on the canvas, and
// resolves a row in one click. The two-center drawing is the public one (F[C@H](Cl)[C@H](Br)I).
{
  const agreePage = await context.newPage()
  try {
    await agreePage.goto(BASE)
    let stereoRequests = 0
    agreePage.on('request', (request) => {
      if (request.method() === 'POST' && request.url().endsWith('/api/v1/operations') && request.postData()?.includes('"chem.stereo"')) stereoRequests += 1
    })
    const write = (path, body) =>
      agreePage.evaluate(async ({ path, body }) => {
        const session = await (await fetch('/api/v1/session')).json()
        const response = await fetch(path, {
          method: 'POST',
          headers: { 'content-type': 'application/json', [session.header]: session.token },
          body: JSON.stringify(body),
        })
        return { status: response.status, body: await response.json() }
      }, { path, body })
    const read = (id) => agreePage.evaluate(async (id) => (await fetch(`/api/v1/documents/${id}`)).json(), id)
    const place = (point) =>
      agreePage.evaluate(({ x, y }) => {
        const svg = document.querySelector('svg[data-structure-canvas]')
        const p = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM())
        return { x: p.x, y: p.y }
      }, point)
    const idle = () => agreePage.waitForFunction(() => !document.querySelector('[data-slot="spinner"]'))
    const raf = () => agreePage.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
    const make = async (key, title, commands) => {
      const created = await write('/api/v1/documents', { request_id: `smoke-agree-${key}`, title })
      assert.ok(created.status < 300, JSON.stringify(created.body))
      const id = created.body.document.id
      const drawn = await write(`/api/v1/documents/${id}/transactions`, { request_id: `smoke-agree-draw-${key}`, expected_revision: 0, commands })
      assert.equal(drawn.status, 200, JSON.stringify(drawn.body))
      return id
    }
    const open = async (id) => {
      await agreePage.reload()
      await agreePage.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
      await agreePage.getByTestId('save-state').waitFor()
    }
    const edit = async (id, act) => {
      const revision = (await read(id)).document.revision
      await act()
      await agreePage.waitForFunction(
        async ({ id, revision }) => (await (await fetch(`/api/v1/documents/${id}`)).json()).document.revision !== revision,
        { id, revision },
      )
      await idle()
      await raf()
    }
    const summary = agreePage.getByTestId('stereo-agreement')
    const status = agreePage.getByTestId('stereo-status')
    const panel = agreePage.getByRole('region', { name: 'Stereo', exact: true })
    const row = (item) => panel.locator(`[data-testid="stereo-row"][data-item="${item}"]`)
    const rings = agreePage.locator('svg[data-structure-canvas] circle[data-agreement]')
    const eventually = async (check) => {
      for (let attempt = 0; ; attempt += 1) {
        try {
          return await check()
        } catch (failure) {
          if (attempt >= 60) {
            failure.message += `\nsummary: ${await summary.textContent().catch(() => '(none)')}; status: ${await status.textContent().catch(() => '(none)')}`
            failure.message += `\nrows: ${await panel.getByTestId('stereo-row').allTextContents().catch(() => '(none)')}`
            throw failure
          }
          await raf()
        }
      }
    }
    const press = (name) => agreePage.getByRole('button', { name, exact: true }).first().click()
    // Waits long enough for a debounce (300 ms) to have fired, to show that nothing was asked.
    const quiet = () => agreePage.waitForTimeout(700)
    const drag = async (from, to) => {
      // The panel beside the canvas is tall, so a click on one of its buttons can scroll the canvas out of the viewport.
      await agreePage.locator('svg[data-structure-canvas]').scrollIntoViewIfNeeded()
      const a = await place(from)
      const b = await place(to)
      await agreePage.mouse.move(a.x, a.y)
      await agreePage.mouse.down()
      await agreePage.mouse.move(b.x, b.y, { steps: 6 })
      await agreePage.mouse.up()
    }
    const hold = () => {
      const state = { used: false, release: () => {}, arrived: () => {}, handler: null }
      const released = new Promise((resolve) => { state.release = resolve })
      const seen = new Promise((resolve) => { state.arrived = resolve })
      state.reached = seen
      state.handler = async (route) => {
        if (state.used || route.request().postDataJSON()?.operation !== 'chem.stereo') return route.continue()
        state.used = true
        const response = await route.fetch()
        state.arrived()
        await released
        await route.fulfill({ response })
      }
      return state
    }
    const centers = (turn2) => [
      { op: 'add_atom', element: 'F', position: { x: 0, y: 40 } },
      { op: 'add_atom', element: 'C', position: { x: 0, y: 0 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Cl', position: { x: -35, y: -18 }, bond_to: { atom: '$1', order: 'single' } },
      { op: 'add_atom', element: 'C', position: { x: 35, y: -18 }, bond_to: { atom: '$1', order: 'single' } },
      { op: 'add_atom', element: 'Br', position: { x: 35, y: -58 }, bond_to: { atom: '$3', order: 'single' } },
      { op: 'add_atom', element: 'I', position: { x: 70, y: 0 }, bond_to: { atom: '$3', order: 'single' } },
      { op: 'set_bond_mark', bond: '$1', mark: { kind: 'hash', narrow: 'end' } },
      { op: 'set_bond_mark', bond: '$4', mark: { kind: 'hash', narrow: 'begin' } },
      { op: 'set_atom_configuration', atom: '$1', configuration: { kind: 'tetrahedral', neighbors: ['$0', '$2', '$3', 'implicit_hydrogen'], turn: turn2 } },
      { op: 'set_atom_configuration', atom: '$3', configuration: { kind: 'tetrahedral', neighbors: ['$1', '$4', '$5', 'implicit_hydrogen'], turn: 'anticlockwise' } },
    ]
    const publicId = await make('public', 'Smoke agreement', centers('clockwise'))

    // A drawing that holds no stereo is never asked about, and says what that means.
    const plain = await make('plain', 'Smoke no stereo', [{ op: 'add_atom', element: 'C', position: { x: 100, y: 100 } }])
    await open(plain)
    await quiet()
    assert.equal(stereoRequests, 0, 'no stereo, no request')
    assert.equal(await panel.getByTestId('stereo-panel-state').textContent(), 'No wedge, hashed wedge or wavy bond is drawn and no configuration is stored. Whether an atom is a stereocenter is not assessed.')
    assert.equal(await panel.getByRole('switch').count(), 0, 'nothing to switch')
    assert.equal(await summary.textContent(), '')
    assert.equal(await agreePage.getByRole('status').and(summary).count(), 1, 'the summary is a live region before it has text')

    // The public drawing: a2 stores clockwise and the hashed wedge draws anticlockwise, a4 agrees.
    await open(publicId)
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 1 disagrees with the drawing, 1 agrees.'))
    assert.equal(stereoRequests, 1, 'one request for the opened drawing')
    assert.equal(await row('a2').getAttribute('data-agreement'), 'disagrees')
    assert.equal(await row('a4').getAttribute('data-agreement'), 'agrees')
    assert.equal(await panel.getAttribute('aria-busy'), 'false')
    assert.match(await row('a2').textContent(), /Stored clockwise; the drawing in s1 shows anticlockwise\./)
    assert.match(await panel.textContent(), /Only stored configurations are compared with the drawing\. CIP labels describe the stored configuration, not the drawing\./)
    assert.doesNotMatch(await agreePage.locator('body').textContent(), /No CIP label is computed/)
    const ring = rings.filter({ has: agreePage.locator('xpath=self::*[@data-agreement="disagrees"]') })
    assert.equal(await rings.count(), 1, 'one ring: the center that disagrees')
    assert.equal(await ring.getAttribute('data-agreement-item'), 'a2')
    assert.equal(await ring.getAttribute('role'), 'img')
    assert.equal(await ring.getAttribute('aria-label'), 'Center a2: the stored configuration disagrees with the drawing in s1')
    assert.equal(await ring.evaluate((element) => getComputedStyle(element).pointerEvents), 'none')
    assert.equal(await ring.evaluate((element) => element.closest('[data-editor-overlay]') !== null), true)
    assert.equal(await ring.getAttribute('stroke-dasharray'), null, 'a disagreement is a solid ring')
    // Hovering and selecting send nothing.
    const before = stereoRequests
    const hoverAt = await place({ x: 15, y: 20 })
    await agreePage.mouse.move(hoverAt.x, hoverAt.y)
    await quiet()
    assert.equal(stereoRequests, before, 'a hover asks nothing')
    await agreePage.screenshot({ path: join(outDir, 'editor-stereo-disagrees.png') })

    // One click: the stored configuration is replaced by what the drawing shows, in one transaction.
    await press('Select and move')
    const revision0 = (await read(publicId)).document.revision
    await edit(publicId, () => panel.getByRole('button', { name: 'Update a2 in s1 from the drawing', exact: true }).click())
    assert.equal((await read(publicId)).document.revision, revision0 + 1, 'one transaction')
    assert.equal(await agreePage.evaluate(() => document.activeElement?.tagName), 'H3', 'focus moved to the panel heading, not lost with the button')
    await eventually(async () => assert.equal(await status.textContent(), 'a2: replaced stored clockwise with anticlockwise from the drawing in s1. Undo restores it.'))
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 2 agree.'))
    assert.equal(await rings.count(), 0)
    assert.equal(await row('a2').getAttribute('data-agreement'), 'agrees')
    assert.equal(await row('a2').getByRole('button').count(), 0, 'nothing to do for a row that agrees')
    await edit(publicId, () => press('Undo'))
    await eventually(async () => assert.equal(await rings.count(), 1))
    assert.equal(await row('a2').getAttribute('data-agreement'), 'disagrees', 'undo restored the stored value, and the page says so')
    await edit(publicId, () => panel.getByRole('button', { name: 'Update a2 in s1 from the drawing', exact: true }).click())
    await eventually(async () => assert.equal(await rings.count(), 0))

    // A move that changes what the drawing shows: no ring is drawn for an answer that is not about the
    // drawing on screen, however late it arrives, and exactly one more request follows the changes made meanwhile.
    const held = hold()
    await agreePage.route('**/api/v1/operations', held.handler)
    const asked = stereoRequests
    await edit(publicId, () => drag({ x: 70, y: 0 }, { x: 75, y: -45 }))
    await held.reached
    await eventually(async () => assert.equal(await summary.textContent(), 'Checking the drawing…'))
    assert.equal(await panel.getAttribute('aria-busy'), 'true')
    assert.equal(await panel.getByTestId('stereo-panel-state').textContent(), 'Checking the drawing…')
    assert.equal(await rings.count(), 0, 'no ring while the drawing is being checked')
    assert.equal(await panel.getByTestId('stereo-row').count(), 0, 'no row from an older answer either')
    await edit(publicId, () => drag({ x: -35, y: -18 }, { x: 35, y: 18 }))
    held.release()
    await agreePage.waitForTimeout(100)
    assert.equal(await rings.count(), 0, 'the answer for the first move is not about the drawing now on screen')
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 2 disagree with the drawing.'))
    assert.equal(stereoRequests - asked, 2, 'the held request, and exactly one more for the second change')
    assert.equal(await rings.count(), 2)
    await agreePage.unroute('**/api/v1/operations', held.handler)
    await agreePage.screenshot({ path: join(outDir, 'editor-stereo-rings.png') })

    // Update all: one transaction for both disagreements, naming each replaced value.
    const revision1 = (await read(publicId)).document.revision
    await edit(publicId, () => panel.getByRole('button', { name: /^Update all from drawing/ }).click())
    assert.equal((await read(publicId)).document.revision, revision1 + 1)
    await eventually(async () => assert.match(await status.textContent(), /^Replaced 2 stored configurations with what the drawing in s1 shows: a2 \(anticlockwise to clockwise\), a4 \(anticlockwise to clockwise\)\. Undo restores them\.$/))
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 2 agree.'))
    await edit(publicId, () => press('Undo'))
    await eventually(async () => assert.equal(await rings.count(), 2))

    // Advisory findings are listed as the service words them; its fixed sentence is the panel's own.
    const details = panel.getByRole('button', { name: /^Details \(\d+ advisory findings?\)$/ })
    await details.click()
    const findings = panel.getByTestId('stereo-findings')
    await findings.waitFor()
    assert.match(await findings.textContent(), /drawing_disagrees \(warning\): Atom a2: the drawing in s1 shows a different configuration from the stored one\./)
    assert.doesNotMatch(await findings.textContent(), /stereo_not_checked/)

    // Checking can be turned off: nothing is asked until "Check now".
    const auto = panel.getByRole('switch', { name: 'Check automatically' })
    assert.equal(await auto.getAttribute('aria-checked'), 'true')
    await auto.click()
    assert.equal(await auto.getAttribute('aria-checked'), 'false')
    const manual = stereoRequests
    await edit(publicId, () => press('Undo'))
    await quiet()
    assert.equal(stereoRequests, manual, 'turned off, a change asks nothing')
    await eventually(async () => assert.equal(await summary.textContent(), 'Not compared with the drawing yet. Press Check now.'))
    assert.equal(await rings.count(), 0, 'no ring for a drawing that was not compared')
    await panel.getByRole('button', { name: 'Check now', exact: true }).click()
    await eventually(async () => assert.match(await summary.textContent(), /^Compared with the drawing: /))
    assert.equal(stereoRequests, manual + 1)
    await auto.click()
    assert.equal(await auto.getAttribute('aria-checked'), 'true')
    await eventually(async () => assert.equal(stereoRequests, manual + 2), 'turning it on asks for the drawing as it is')

    // A failure is shown with its message and "Check again", and draws nothing.
    const refuse = async (route) => {
      if (route.request().postDataJSON()?.operation !== 'chem.stereo') return route.continue()
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ envelope: 'operation_result', contract: 1, outcome: 'error', model_id: 'chemifree-stereo-1', document_revision: 1, loss_report: [], result: null,
          diagnostics: [{ code: 'resource_limit', severity: 'error', message: 'Smoke: too many entries to compare.' }] }),
      })
    }
    await agreePage.route('**/api/v1/operations', refuse)
    await auto.click()
    await panel.getByRole('button', { name: 'Check now', exact: true }).click()
    const failure = panel.getByRole('alert')
    await failure.waitFor()
    assert.equal(await failure.textContent(), 'Smoke: too many entries to compare.')
    assert.equal(await summary.textContent(), 'The comparison with the drawing could not be made.')
    assert.equal(await rings.count(), 0)
    await agreePage.unroute('**/api/v1/operations', refuse)
    await panel.getByRole('button', { name: 'Check again', exact: true }).click()
    await eventually(async () => assert.match(await summary.textContent(), /^Compared with the drawing: /))
    assert.equal(await panel.getByRole('alert').count(), 0)
    await auto.click()

    // A mark with nothing stored: "Store from drawing" stores it; undo takes it away again.
    const drawnOnly = await make('drawn', 'Smoke drawn only', [
      { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
      { op: 'add_atom', element: 'F', position: { x: 100, y: 70 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Cl', position: { x: 126, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Br', position: { x: 74, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'set_bond_mark', bond: '$2', mark: { kind: 'wedge', narrow: 'begin' } },
    ])
    await open(drawnOnly)
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 1 is drawn only.'))
    assert.equal(await row('a1').getAttribute('data-agreement'), 'only_drawn')
    const dashed = agreePage.locator('svg[data-structure-canvas] circle[data-agreement="only_drawn"]')
    assert.equal(await dashed.getAttribute('stroke-dasharray'), '3 2', 'not a disagreement: a dashed ring')
    assert.equal(await dashed.getAttribute('aria-label'), 'Center a1: the marks show a configuration in s1 that is not stored')
    await edit(drawnOnly, () => panel.getByRole('button', { name: 'Store a1 in s1 from the drawing', exact: true }).click())
    await eventually(async () => assert.match(await status.textContent(), /^a1: stored (clockwise|anticlockwise) from the drawing in s1\.$/))
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 1 agrees.'))
    await edit(drawnOnly, () => press('Undo'))
    await eventually(async () => assert.equal(await row('a1').getAttribute('data-agreement'), 'only_drawn'))
    assert.deepEqual((await read(drawnOnly)).document.stereo, { atoms: [], bonds: [], groups: [] })

    // A stored configuration no mark shows: "Remove stored" removes it, and says which value.
    const storedOnly = await make('stored', 'Smoke stored only', [
      { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
      { op: 'add_atom', element: 'F', position: { x: 100, y: 70 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Cl', position: { x: 126, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_atom', element: 'Br', position: { x: 74, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'set_atom_configuration', atom: '$0', configuration: { kind: 'tetrahedral', neighbors: ['$1', '$2', '$3', 'implicit_hydrogen'], turn: 'clockwise' } },
    ])
    await open(storedOnly)
    await eventually(async () => assert.equal(await summary.textContent(), 'Compared with the drawing: 1 is stored only.'))
    assert.match(await row('a1').textContent(), /Stored clockwise; the drawing in s1 shows nothing usable: no wedge, hashed wedge or wavy bond bears on it \(stereo_not_derivable_no_marks\)\./)
    await edit(storedOnly, () => panel.getByRole('button', { name: 'Remove the stored configuration of a1 (row s1)', exact: true }).click())
    await eventually(async () => assert.equal(await status.textContent(), 'a1: removed the stored clockwise. Undo restores it.'))
    assert.deepEqual((await read(storedOnly)).document.stereo, { atoms: [], bonds: [], groups: [] })
    await eventually(async () => assert.equal(await row('a1').count(), 0))
    await eventually(async () => assert.match(await panel.getByTestId('stereo-panel-state').textContent(), /is not assessed\.$/))
    await edit(storedOnly, () => press('Undo'))
    await eventually(async () => assert.equal(await row('a1').getAttribute('data-agreement'), 'only_stored'))

    // CIP labels (P26c): off by default, and then the page sends no chem.cip request at all. A switch asks
    // chem.cip once (scope "stored"), draws "(S)" or "(R)" near each assigned center and "(E)" or "(Z)" near
    // each assigned double bond as an overlay, and puts the label next to the agreement in the panel. Nothing is
    // written to the document.
    {
      const cipBodies = []
      agreePage.on('request', (request) => {
        if (request.method() === 'POST' && request.url().endsWith('/api/v1/operations') && request.postData()?.includes('"chem.cip"')) {
          const body = request.postDataJSON()
          if (!body.request_id.startsWith('smoke-cip-ref')) cipBodies.push(body)
        }
      })
      // `$n` names the n-th command of the transaction: the ten of `centers` come first, so the fluorine is $10.
      const cipId = await make('cip', 'Smoke CIP', [
        ...centers('clockwise'),
        { op: 'add_atom', element: 'F', position: { x: 160, y: 100 } },
        { op: 'add_atom', element: 'C', position: { x: 190, y: 100 }, bond_to: { atom: '$10', order: 'single' } },
        { op: 'add_atom', element: 'C', position: { x: 220, y: 100 }, bond_to: { atom: '$11', order: 'double' } },
        { op: 'add_atom', element: 'Br', position: { x: 250, y: 100 }, bond_to: { atom: '$12', order: 'single' } },
        { op: 'set_bond_configuration', bond: '$12', configuration: { kind: 'relative', begin_neighbor: '$10', end_neighbor: '$13', relation: 'same_side' } },
      ])
      // What the service says the labels are: the page must show exactly these.
      const reference = await write('/api/v1/operations', {
        contract: 1, request_id: 'smoke-cip-ref', profile: 'preserve-semantics', operation: 'chem.cip', parameters: { document_id: cipId },
      })
      assert.equal(reference.body.outcome, 'ok', JSON.stringify(reference.body.diagnostics))
      const expected = new Map([
        ...reference.body.result.atoms.map((item) => [item.atom, item.descriptor]),
        ...reference.body.result.bonds.map((item) => [item.bond, item.descriptor]),
      ])
      assert.deepEqual([...expected.keys()].sort(), ['a2', 'a4', 'b7'].sort())
      assert.ok(['R', 'S'].includes(expected.get('a2')) && ['R', 'S'].includes(expected.get('a4')) && ['E', 'Z'].includes(expected.get('b7')))
      const labels = agreePage.locator('svg[data-structure-canvas] [data-cip-label]')
      const cell = (item) => row(item).getByTestId('cip-cell')
      const cipState = panel.getByTestId('cip-state')
      const atomSwitch = panel.getByRole('switch', { name: 'Atom stereo labels', exact: true })
      const bondSwitch = panel.getByRole('switch', { name: 'Bond stereo labels', exact: true })
      const shown = async () => Object.fromEntries(await labels.evaluateAll((all) => all.map((node) => [node.getAttribute('data-cip-item'), node.textContent])))

      await open(cipId)
      await eventually(async () => assert.equal(await row('b7').count(), 1))
      await quiet()
      assert.equal(cipBodies.length, 0, 'both switches off: no chem.cip request')
      assert.equal(await atomSwitch.getAttribute('aria-checked'), 'false', 'default off')
      assert.equal(await bondSwitch.getAttribute('aria-checked'), 'false', 'default off')
      assert.equal(await labels.count(), 0)
      assert.equal(await panel.getByTestId('cip-cell').count(), 0)
      assert.equal(await cipState.textContent(), 'Switch on atom or bond stereo labels to compute CIP labels for the stored configurations.')
      assert.match(await panel.textContent(), /CIP labels describe the stored configuration, not the drawing\./)
      assert.doesNotMatch(await agreePage.locator('body').textContent(), /No CIP label is computed/)
      await agreePage.waitForFunction(async (id) => {
        const current = await (await fetch(`/api/v1/documents/${id}`)).json()
        return current.session.draft_revision === current.document.revision
      }, cipId)
      const untouched = await read(cipId)

      await atomSwitch.click()
      await eventually(async () => assert.deepEqual(await shown(), { a2: `(${expected.get('a2')})`, a4: `(${expected.get('a4')})` }))
      const label = labels.first()
      assert.equal(await label.getAttribute('role'), 'img')
      assert.equal(await label.evaluate((node) => getComputedStyle(node).pointerEvents), 'none')
      assert.equal(await label.evaluate((node) => node.closest('[data-editor-overlay]') !== null), true)
      assert.match(await label.getAttribute('aria-label'), /^Center a[24]: CIP label [RS], of the stored configuration$/)
      assert.equal(cipBodies.length >= 1, true)
      assert.deepEqual([...new Set(cipBodies.map((body) => JSON.stringify(body.parameters)))], [JSON.stringify({ document_id: cipId, scope: 'stored' })], 'only the stored scope is ever asked')
      // The label sits in the panel next to the agreement: a2 stores clockwise and the hashed wedge draws anticlockwise.
      await eventually(async () => assert.equal(await cell('a2').locator('span').first().textContent(), `CIP: ${expected.get('a2')}`))
      assert.match(await cell('a2').textContent(), /of the stored configuration, which disagrees with the drawing in s1 \(disagrees\)/)
      assert.match(await cell('a4').textContent(), new RegExp(`CIP: ${expected.get('a4')}.*which agrees with the drawing in s1`))
      assert.equal(await cell('b7').count(), 0, 'a bond row shows its label under the bond switch only')
      assert.equal(await shown().then((all) => 'b7' in all), false, 'the bond switch draws the bond label')
      assert.equal(await agreePage.getByTestId('stereo-agreement').textContent(), 'Compared with the drawing: 1 disagrees with the drawing, 1 agrees, 1 is stored only.', 'the comparison and the rings are unchanged')
      assert.equal(await rings.count(), 2, 'rings: a2 disagrees and b7 is stored only (a4 agrees); they are the agreement indicator\'s')
      await agreePage.screenshot({ path: join(outDir, 'editor-cip-atoms.png') })

      const asked = cipBodies.length
      await bondSwitch.click()
      await eventually(async () => assert.deepEqual(await shown(), { a2: `(${expected.get('a2')})`, a4: `(${expected.get('a4')})`, b7: `(${expected.get('b7')})` }))
      assert.equal(cipBodies.length, asked, 'the second switch draws what was already answered')
      await eventually(async () => assert.equal(await cell('b7').locator('span').first().textContent(), `CIP: ${expected.get('b7')}`))
      await atomSwitch.click()
      await eventually(async () => assert.deepEqual(await shown(), { b7: `(${expected.get('b7')})` }))
      assert.equal(await cell('a2').count(), 0, 'an atom row shows its label under the atom switch only')
      await bondSwitch.click()
      await eventually(async () => assert.equal(await labels.count(), 0))
      await eventually(async () => assert.equal(await panel.getByTestId('cip-cell').count(), 0))
      assert.equal(await cipState.textContent(), 'Switch on atom or bond stereo labels to compute CIP labels for the stored configurations.')
      const afterOff = cipBodies.length
      await quiet()
      assert.equal(cipBodies.length, afterOff, 'switched off, nothing is asked')
      assert.deepEqual(await read(cipId), untouched, 'labels never change the document, its stored configurations, its marks or its session')

      // A refused or limited request is shown as such, with no label, and can be tried again.
      const limit = {
        envelope: 'operation_result', contract: 1, outcome: 'error', model_id: 'chemifree-cip-1', document_revision: null, loss_report: [], result: null,
        diagnostics: [{ code: 'resource_limit', severity: 'error', message: '3 items need more than 1000 digraph nodes together for the centers.', path: 'parameters.atoms',
          details: { limit: 1000, request_limit: 2000000, items: 3, item_limit: 20000, stage: 'centers', parameter: 'atoms' } }],
      }
      let refuse = true
      const refusing = async (route) => {
        if (refuse && route.request().postDataJSON()?.operation === 'chem.cip') {
          return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(limit) })
        }
        return route.continue()
      }
      await agreePage.route('**/api/v1/operations', refusing)
      try {
        await atomSwitch.click()
        await eventually(async () => assert.equal(await cipState.textContent(), 'CIP labels unavailable: 3 items need more than 1000 digraph nodes together for the centers.'))
        assert.equal(await labels.count(), 0, 'no label after a refusal')
        assert.equal(await cell('a2').locator('span').first().textContent(), 'CIP: unavailable')
        // Off and on again: the earlier failure is not shown while the new request is awaited.
        await atomSwitch.click()
        await atomSwitch.click()
        await eventually(async () => assert.equal(await cipState.textContent(), 'CIP labels unavailable: 3 items need more than 1000 digraph nodes together for the centers.'))
        refuse = false
        await agreePage.getByRole('button', { name: 'Try CIP labels again', exact: true }).click()
        await eventually(async () => assert.deepEqual(await shown(), { a2: `(${expected.get('a2')})`, a4: `(${expected.get('a4')})` }))
      } finally {
        await agreePage.unroute('**/api/v1/operations', refusing)
      }
      await atomSwitch.click()
      await eventually(async () => assert.equal(await labels.count(), 0))
      assert.deepEqual(await read(cipId), untouched)

      // A switch that is on with nothing stored of its kind says so and asks nothing.
      const bondsOnlyId = await make('cipbond', 'Smoke CIP bonds only', [
        { op: 'add_atom', element: 'F', position: { x: 100, y: 100 } },
        { op: 'add_atom', element: 'C', position: { x: 130, y: 100 }, bond_to: { atom: '$0', order: 'single' } },
        { op: 'add_atom', element: 'C', position: { x: 160, y: 100 }, bond_to: { atom: '$1', order: 'double' } },
        { op: 'add_atom', element: 'Br', position: { x: 190, y: 100 }, bond_to: { atom: '$2', order: 'single' } },
        { op: 'set_bond_configuration', bond: '$2', configuration: { kind: 'relative', begin_neighbor: '$0', end_neighbor: '$3', relation: 'same_side' } },
      ])
      await open(bondsOnlyId)
      await eventually(async () => assert.equal(await row('b2').count(), 1))
      const beforeNothing = cipBodies.length
      await atomSwitch.click()
      await eventually(async () => assert.equal(await cipState.textContent(), 'No stored atom configurations to label.'))
      await quiet()
      assert.equal(cipBodies.length, beforeNothing, 'nothing stored of that kind: nothing is asked')
      assert.equal(await labels.count(), 0)
      results.cip_labels_web = true
      step = 'after cip_labels_web'
    }

    // Enhanced stereo groups (P27c): two centers are stored from the drawing in the panel, then grouped,
    // numbered (a number, 0 and a negative one), refused on a taken number, split, merged, removed and
    // deleted with one click each, undone and redone with the same group ids, and labeled on the canvas
    // while the Group labels switch is on. The page shows what the document holds, read through the API.
    {
      const groupsId = await make('groups', 'Smoke groups', centers('clockwise').filter((command) => command.op !== 'set_atom_configuration'))
      await open(groupsId)
      await eventually(async () => assert.equal(await row('a2').getAttribute('data-agreement'), 'only_drawn'))
      assert.equal(await panel.getByTestId('stereo-groups').count(), 0, 'no stored center, nothing to group')
      await edit(groupsId, () => panel.getByRole('button', { name: 'Store a2 in s1 from the drawing', exact: true }).click())
      await edit(groupsId, () => panel.getByRole('button', { name: 'Store a4 in s1 from the drawing', exact: true }).click())
      assert.deepEqual((await read(groupsId)).document.stereo.atoms.map((record) => record.atom), ['a2', 'a4'])
      const section = panel.getByTestId('stereo-groups')
      await section.waitFor()
      const groupsOf = async () => (await read(groupsId)).document.stereo.groups
      const item = (id) => section.locator(`[data-testid="stereo-group"][data-group="${id}"]`)
      const action = (name) => section.getByRole('button', { name, exact: true })
      const numberField = (id) => item(id).getByRole('textbox', { name: `Number of ${id}`, exact: true })
      const why = (name) => section.locator(`[data-action="${action_ids[name]}"] [data-testid="group-why"]`)
      const action_ids = { 'Make absolute': 'make_absolute', 'Group as AND': 'group_and', 'Group as OR': 'group_or', 'Add to group': 'add_to_group', 'Remove from group': 'remove_from_group', 'Merge groups': 'merge', 'Split selected out': 'split' }
      const newNumber = section.getByLabel('Number for a new or split group (empty for none)')
      const labels = agreePage.locator('svg[data-structure-canvas] [data-group-label]')
      const groupSwitch = section.getByRole('switch', { name: 'Group labels', exact: true })
      const dismiss = async () => { await agreePage.getByRole('button', { name: 'Dismiss', exact: true }).click() }
      const select = async (...points) => {
        await press('Select and move')
        await agreePage.locator('svg[data-structure-canvas]').scrollIntoViewIfNeeded()
        // A click on empty space clears the selection; clicking a selected atom would keep the others.
        const empty = await place({ x: 50, y: 40 })
        await agreePage.mouse.click(empty.x, empty.y)
        for (const [index, point] of points.entries()) {
          const at = await place(point)
          if (index > 0) await agreePage.keyboard.down('Shift')
          await agreePage.mouse.click(at.x, at.y)
          if (index > 0) await agreePage.keyboard.up('Shift')
        }
      }
      const A2 = { x: 0, y: 0 }
      const A4 = { x: 35, y: -18 }
      // The page shows exactly what the document holds: each group's label, count, members and number.
      const listed = async () => section.getByTestId('stereo-group').evaluateAll((all) => all.map((node) => ({
        id: node.getAttribute('data-group'),
        label: node.querySelector('[data-testid="group-label"]').textContent,
        count: node.querySelector('[data-testid="group-count"]').textContent,
        members: [...node.querySelectorAll('[data-testid="group-member"]')].map((chip) => chip.textContent),
        number: node.querySelector('[data-testid="group-number"]').textContent,
      })))
      const same = async () => {
        const held = await groupsOf()
        await eventually(async () => assert.deepEqual(await listed(), held.map((group) => ({
          id: group.id,
          label: group.label,
          count: `${group.members.length} ${group.members.length === 1 ? 'member' : 'members'}`,
          members: group.members,
          number: group.kind === 'absolute' ? 'no number (an absolute group takes none)' : group.stored_number === null ? 'number omitted (reads as 0)' : `number ${group.stored_number}`,
        }))))
        return held
      }

      // Honest text, ungrouped state, and every action disabled with its reason while nothing is selected.
      assert.equal(await section.getByTestId('groups-note').textContent(), 'Groups are stored and edited here. Recorded SMILES writes supported groups; canonical SMILES and MOL (V2000) export require consent to drop them. MOL (V3000) writes groups as COLLECTION entries when you choose V3000 or Auto, and reading a V3000 file with such collections creates groups. Query features, S-groups and R-groups are refused, not read. They are not compared with the drawing, and do not change CIP labels or identity.')
      assert.equal(await section.getByTestId('groups-not-examined').textContent(), 'Not examined or unsupported: double bonds cannot be grouped; centers stored unknown cannot be members; group meaning is ignored by CIP labels, identity and substructure search.')
      assert.equal(await section.getByTestId('groups-empty').count(), 1)
      assert.deepEqual(await groupsOf(), [])
      assert.equal(await row('a2').getByTestId('group-cell').textContent(), 'Group: no group')
      for (const name of Object.keys(action_ids)) {
        assert.equal(await action(name).isDisabled(), true, `${name} needs a selection`)
        assert.equal(await why(name).textContent(), 'Select atoms with a stored tetrahedral configuration. Bonds in the selection are ignored.')
      }
      assert.equal(await groupSwitch.getAttribute('aria-checked'), 'false', 'Group labels start off')

      // Select both centers. Only the creating actions apply; the others say why not.
      await select(A2, A4)
      await eventually(async () => assert.equal(await action('Group as OR').isDisabled(), false))
      assert.equal(await action('Group as AND').isDisabled(), false)
      assert.equal(await action('Make absolute').isDisabled(), false)
      assert.equal(await action('Remove from group').isDisabled(), true)
      assert.equal(await why('Remove from group').textContent(), 'a2 is in no group; select only group members to remove.')
      assert.equal(await why('Split selected out').textContent(), 'a2 is in no group; select only members of one group.')
      // A number typed for an absolute group is refused in text, a bad number in text, and nothing is sent.
      await newNumber.fill('3')
      assert.equal(await why('Make absolute').textContent(), 'An absolute group takes no number. Clear the number field.')
      await newNumber.fill('1.5')
      assert.equal(await action('Group as OR').isDisabled(), true)
      assert.match(await section.getByTestId('group-number-error').textContent(), /whole number from -2147483648 to 2147483647/)
      await newNumber.fill('2147483648')
      assert.equal(await action('Group as OR').isDisabled(), true)
      await newNumber.fill('')

      // Create an OR group with no number: one click, one transaction.
      const revisionBefore = (await read(groupsId)).document.revision
      await edit(groupsId, () => action('Group as OR').click())
      assert.equal((await read(groupsId)).document.revision, revisionBefore + 1)
      const created = await same()
      assert.deepEqual(created, [{ id: 'e1', kind: 'or', stored_number: null, number: 0, label: 'or0', members: ['a2', 'a4'] }])
      await eventually(async () => assert.equal(await status.textContent(), 'Created group e1 (or0) over a2, a4.'))
      assert.equal(await row('a2').getByTestId('group-cell').textContent(), 'Group: e1: or0 (OR)')
      assert.equal(await row('a4').getByTestId('group-cell').textContent(), 'Group: e1: or0 (OR)')
      assert.match(await item('e1').textContent(), /OR: the members invert together or not at all\./)
      assert.doesNotMatch(await section.textContent(), /ratio is \d|1:1/)
      assert.equal(await why('Group as OR').textContent(), 'a2 is already in group e1 (or0); an atom belongs to at most one group. Remove it from that group first, or use Add to group to add the other atoms to it.')
      assert.equal(await section.getByTestId('groups-empty').count(), 0)

      // Numbers: a positive one, 0 and a negative one are accepted and kept as stated.
      const setNumber = async (text) => {
        const input = numberField('e1')
        await input.fill(text)
        await edit(groupsId, () => item('e1').getByRole('button', { name: 'Set the number of e1', exact: true }).click())
      }
      await setNumber('5')
      assert.equal((await groupsOf())[0].stored_number, 5)
      await setNumber('0')
      assert.deepEqual((await same())[0], { id: 'e1', kind: 'or', stored_number: 0, number: 0, label: 'or0', members: ['a2', 'a4'] })
      await setNumber('-2')
      assert.deepEqual((await same())[0], { id: 'e1', kind: 'or', stored_number: -2, number: -2, label: 'or-2', members: ['a2', 'a4'] })
      const numberInput = numberField('e1')
      await numberInput.fill('2147483648')
      assert.equal(await item('e1').getByRole('button', { name: 'Set the number of e1', exact: true }).isDisabled(), true)
      await numberInput.fill('abc')
      assert.equal(await item('e1').getByRole('button', { name: 'Set the number of e1', exact: true }).isDisabled(), true)
      await numberInput.fill('-2')
      assert.equal(await item('e1').getByTestId('group-number-why').textContent(), 'The number is already -2.')

      // Split a4 out into a new group numbered 7: both groups, and the reply's note.
      await select(A4)
      await newNumber.fill('7')
      await eventually(async () => assert.equal(await action('Split selected out').isDisabled(), false, await why('Split selected out').textContent({ timeout: 500 }).catch(() => '(no reason)')))
      await edit(groupsId, () => action('Split selected out').click())
      assert.deepEqual(await same(), [
        { id: 'e1', kind: 'or', stored_number: -2, number: -2, label: 'or-2', members: ['a2'] },
        { id: 'e2', kind: 'or', stored_number: 7, number: 7, label: 'or7', members: ['a4'] },
      ])
      const notes = agreePage.getByTestId('stereo-notes')
      await notes.waitFor()
      // The recovery draft is written about a second after an edit. Its reply states no notes, and it must not clear the ones the user is reading.
      // An earlier draft can leave the indicator at 'saved', so wait for the draft of this edit's own revision, in the service and then on screen.
      const editedRevision = (await read(groupsId)).document.revision
      await agreePage.waitForFunction(
        async ({ id, revision }) => (await (await fetch(`/api/v1/documents/${id}`)).json()).session.draft_revision === revision,
        { id: groupsId, revision: editedRevision },
      )
      await agreePage.waitForFunction(
        (revision) => document.querySelector('[data-testid="draft-state"]')?.textContent?.includes(`(revision ${revision})`),
        editedRevision,
      )
      await raf()
      await notes.waitFor({ timeout: 2000 })
      assert.ok((await notes.locator('li').count()) >= 1, 'the reply states that the members no longer belong together')
      await newNumber.fill('')

      // A taken number is refused with the sentence for it and the merge hint; nothing changes, and the field returns to the stored number.
      const held = await read(groupsId)
      await numberField('e1').fill('7')
      await item('e1').getByRole('button', { name: 'Set the number of e1', exact: true }).click()
      await agreePage.getByText('Another group of this kind already has this number (a group with no number reads as 0). Choose a different number, or merge the two groups: select a member of each and press Merge groups. Nothing was changed.').waitFor()
      assert.deepEqual(await read(groupsId), held, 'a refused edit changes nothing')
      assert.equal(await agreePage.getByTestId('state-reason').count(), 0, 'the plain sentence replaces the raw reason line')
      await eventually(async () => assert.equal(await numberField('e1').inputValue(), '-2'))
      await dismiss()

      // Merge: a member of each group; e1 keeps its kind and number.
      await select(A2, A4)
      await eventually(async () => assert.equal(await action('Merge groups').isDisabled(), false))
      assert.equal(await action('Split selected out').isDisabled(), true)
      assert.equal(await why('Split selected out').textContent(), 'Splitting takes members of exactly one group; the selection holds 2.')
      await edit(groupsId, () => action('Merge groups').click())
      const merged = await same()
      assert.deepEqual(merged, [{ id: 'e1', kind: 'or', stored_number: -2, number: -2, label: 'or-2', members: ['a2', 'a4'] }])
      assert.equal(await status.textContent(), 'Merged e2 into the other group, which keeps its kind and number; e2 is gone.')

      // Remove a4 from the group; removing the last member is refused in text.
      await select(A4)
      await edit(groupsId, () => action('Remove from group').click())
      const removed = await same()
      assert.deepEqual(removed, [{ id: 'e1', kind: 'or', stored_number: -2, number: -2, label: 'or-2', members: ['a2'] }])
      assert.equal(await row('a4').getByTestId('group-cell').textContent(), 'Group: no group')
      await select(A2)
      await eventually(async () => assert.equal(await action('Remove from group').isDisabled(), true))
      assert.equal(await why('Remove from group').textContent(), 'Removing every member of e1 would leave it empty. Delete the group instead.')
      // A member chip selects its atom; with a2 and the loose a4 the Add action applies.
      await item('e1').getByRole('button', { name: 'Select a2', exact: true }).click()
      await select(A2, A4)
      await eventually(async () => assert.equal(await action('Add to group').isDisabled(), false))
      await edit(groupsId, () => action('Add to group').click())
      assert.deepEqual((await same())[0].members, ['a2', 'a4'])

      // Group labels: off by default, drawn beside each member on the canvas while the switch is on,
      // never part of the document, and stacked clear of a CIP label.
      await agreePage.waitForFunction(async (id) => {
        const current = await (await fetch(`/api/v1/documents/${id}`)).json()
        return current.session.draft_revision === current.document.revision
      }, groupsId)
      const beforeLabels = await read(groupsId)
      assert.equal(await labels.count(), 0)
      await groupSwitch.click()
      await eventually(async () => assert.equal(await labels.count(), 2))
      assert.deepEqual(await labels.evaluateAll((all) => all.map((node) => [node.getAttribute('data-group-atom'), node.textContent, node.getAttribute('aria-label')])), [
        ['a2', 'OR -2', 'Center a2: member of group e1, OR -2'], ['a4', 'OR -2', 'Center a4: member of group e1, OR -2'],
      ])
      assert.equal(await labels.first().evaluate((node) => node.closest('[data-editor-overlay]') !== null && getComputedStyle(node).pointerEvents === 'none'), true)
      await agreePage.screenshot({ path: join(outDir, 'editor-stereo-groups.png') })
      assert.deepEqual(await read(groupsId), beforeLabels, 'a label never changes the document')
      await groupSwitch.click()
      await eventually(async () => assert.equal(await labels.count(), 0))

      // Delete the group, then undo and redo: the same group ids and members come back.
      await edit(groupsId, () => item('e1').getByRole('button', { name: 'Delete group e1', exact: true }).click())
      assert.deepEqual(await same(), [])
      assert.equal(await panel.getByTestId('groups-empty').count(), 1)
      const orGroup = (members) => ({ id: 'e1', kind: 'or', stored_number: -2, number: -2, label: 'or-2', members })
      await edit(groupsId, () => press('Undo'))
      assert.deepEqual(await same(), [orGroup(['a2', 'a4'])])
      await edit(groupsId, () => press('Undo'))
      assert.deepEqual(await same(), removed)
      await edit(groupsId, () => press('Undo'))
      assert.deepEqual(await same(), merged)
      await edit(groupsId, () => press('Undo'))
      assert.deepEqual((await same()).map((group) => [group.id, group.members]), [['e1', ['a2']], ['e2', ['a4']]], 'undoing the merge brings back both groups under their ids')
      for (let times = 0; times < 4; times += 1) await edit(groupsId, () => press('Redo'))
      assert.deepEqual(await same(), [], 'redo replays to the state after the delete')
      await edit(groupsId, () => press('Undo'))
      assert.deepEqual(await same(), [orGroup(['a2', 'a4'])], 'the same group id and members as before the delete')

      // Absolute and AND: an absolute group takes no number; one absolute group at most.
      await edit(groupsId, () => item('e1').getByRole('button', { name: 'Delete group e1', exact: true }).click())
      await select(A2)
      await edit(groupsId, () => action('Make absolute').click())
      assert.deepEqual(await same(), [{ id: 'e3', kind: 'absolute', stored_number: null, number: null, label: 'abs', members: ['a2'] }], 'a deleted group id is not reused')
      assert.equal(await numberField('e3').count(), 0, 'an absolute group has no number field')
      await select(A4)
      assert.equal(await action('Make absolute').isDisabled(), true)
      assert.equal(await why('Make absolute').textContent(), 'The document already has an absolute group (e3); there is at most one.')
      await edit(groupsId, () => action('Group as AND').click())
      assert.deepEqual((await same())[1], { id: 'e4', kind: 'and', stored_number: null, number: 0, label: 'and0', members: ['a4'] })
      assert.match(await item('e4').textContent(), /AND: the author means a mixture of the drawn set of members and its all-inverted counterpart\. No ratio is stated or implied\./)
      assert.doesNotMatch(await section.textContent(), /1:1/)
      assert.equal(await panel.getByTestId('cip-state').textContent(), 'Switch on atom or bond stereo labels to compute CIP labels for the stored configurations.', 'groups change no CIP state')
      results.stereo_groups_web = true
      step = 'after stereo_groups_web'
    }

    results.stereo_agreement_indicator = true
    step = 'after stereo_agreement_indicator'
  } finally {
    await agreePage.close()
  }
}

// Viewport controls are local presentation state; editing always uses document coordinates.
{
  step = 'viewport pan zoom keyboard and document-coordinate editing'
  const created = await apiWrite('/api/v1/documents', { request_id: 'viewport-create', title: 'Viewport coordinates' })
  const id = created.body.document.id
  const fixture = await apiWrite(`/api/v1/documents/${id}/transactions`, {
    request_id: 'viewport-fixture', expected_revision: 0, commands: [
      { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
      { op: 'add_atom', element: 'O', position: { x: 130, y: 100 }, bond_to: { atom: '$0', order: 'single' } },
      { op: 'add_graphic', shape: 'arrow', points: [{ x: 200, y: 160 }, { x: 250, y: 160 }] },
    ],
  })
  assert.equal(fixture.status, 200, JSON.stringify(fixture.body))
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  const canvas = page.locator('svg[data-structure-canvas]')
  const controls = page.getByRole('toolbar', { name: 'Canvas view' })
  const viewState = () => canvas.evaluate((svg) => ({
    box: [...svg.getAttribute('viewBox').split(/\s+/).map(Number)], scale: Number(svg.dataset.viewportScale),
  }))
  const initial = (await api(`/api/v1/documents/${id}`)).document
  await controls.getByRole('button', { name: 'Reset zoom (100%)', exact: true }).click()
  assert.equal((await viewState()).scale, 1)
  assert.equal(await controls.getByTestId('zoom-level').textContent(), '100%')
  await controls.getByRole('button', { name: 'Zoom in', exact: true }).click()
  assert.equal((await viewState()).scale, 1.25)
  await controls.getByRole('button', { name: 'Zoom out', exact: true }).click()
  assert.equal((await viewState()).scale, 1)

  // Focused-canvas shortcuts work; an arrow in a text input must not navigate the canvas.
  const unchanged = await viewState()
  await page.getByLabel('Document title').focus()
  await page.keyboard.press('ArrowRight')
  assert.deepEqual(await viewState(), unchanged)
  await canvas.focus()
  await page.keyboard.press('+')
  assert.equal((await viewState()).scale, 1.25)
  const beforeArrow = await viewState()
  await page.keyboard.press('ArrowRight')
  const afterArrow = await viewState()
  assert.ok(Math.abs(afterArrow.box[0] - beforeArrow.box[0] - 32) < 1e-6, '40 CSS pixels at 1.25px/pt pans by 32pt')
  await page.keyboard.press('0')
  assert.equal((await viewState()).scale, 1)
  await page.keyboard.press('f')

  // Ctrl-wheel zoom anchors the exact document point underneath the pointer.
  await canvas.scrollIntoViewIfNeeded()
  const rect = await canvas.boundingBox()
  assert.ok(rect && rect.width > 0 && rect.height > 0)
  // Chromium reports WheelEvent.clientX/Y as integer CSS pixels. Request that same
  // point so the invariant compares the actual event anchor, rather than a nearby
  // fractional Playwright mouse coordinate that the wheel event cannot represent.
  const anchor = { x: Math.round(rect.x + rect.width * 0.65), y: Math.round(rect.y + rect.height * 0.4) }
  const worldAt = () => canvas.evaluate((svg, at) => {
    const world = new DOMPoint(at.x, at.y).matrixTransform(svg.getScreenCTM().inverse())
    return { x: world.x, y: world.y }
  }, anchor)
  const anchored = await worldAt()
  const oldScale = (await viewState()).scale
  await page.mouse.move(anchor.x, anchor.y)
  await page.keyboard.down('Control')
  await page.mouse.wheel(0, -120)
  await page.keyboard.up('Control')
  await page.waitForFunction((scale) => Number(document.querySelector('svg[data-structure-canvas]').dataset.viewportScale) > scale, oldScale)
  const stillAnchored = await worldAt()
  assert.ok(Math.abs(stillAnchored.x - anchored.x) < 0.02 && Math.abs(stillAnchored.y - anchored.y) < 0.02,
    'zoom preserves the world point at an off-center pointer anchor')
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, initial, 'zoom and keyboard pan change no document state or revision')

  // Pan captures a graphic before its selection handler can treat the gesture as an edit.
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  await tool('Pan view')
  const graphicAt = await screen({ x: 225, y: 160 })
  const beforePan = await viewState()
  await page.mouse.move(graphicAt.x, graphicAt.y)
  await page.mouse.down()
  await page.mouse.move(graphicAt.x + 45, graphicAt.y + 20, { steps: 4 })
  await page.mouse.up()
  const afterPan = await viewState()
  assert.ok(Math.abs(afterPan.box[0] - beforePan.box[0] + 45 / beforePan.scale) < 0.05)
  assert.ok(Math.abs(afterPan.box[1] - beforePan.box[1] + 20 / beforePan.scale) < 0.05)
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, initial, 'panning across graphics changes neither selection nor content')

  // At a known non-unit zoom, a new atom still lands at the chosen document coordinates.
  await controls.getByRole('button', { name: 'Reset zoom (100%)', exact: true }).click()
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  await controls.getByRole('button', { name: 'Zoom in', exact: true }).click()
  await page.getByRole('button', { name: 'Element N', exact: true }).click()
  const target = await screen({ x: 200, y: 120 })
  const added = page.waitForResponse((response) => response.url().endsWith(`/documents/${id}/transactions`) && response.request().method() === 'POST')
  await page.mouse.click(target.x, target.y)
  assert.equal((await added).status(), 200)
  await page.getByLabel('Revision 2').waitFor()
  let edited = (await api(`/api/v1/documents/${id}`)).document
  const nitrogen = edited.atoms.find((atom) => atom.id === 'a3')
  assert.equal(nitrogen.element, 'N')
  assert.ok(Math.abs(nitrogen.position.x - 200) <= 0.02 && Math.abs(nitrogen.position.y - 120) <= 0.02,
    `zoomed click stores world coordinates: ${JSON.stringify(nitrogen.position)}`)
  assert.deepEqual(edited.atoms.slice(0, 2), initial.atoms)
  const saving = page.waitForResponse((response) => response.url().endsWith(`/documents/${id}/save`) && response.request().method() === 'POST')
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  assert.equal((await saving).status(), 200)
  await closeShown()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Revision 2').waitFor()
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, edited, 'save/reopen retains document coordinates, independent of transient zoom')
  results.viewport_navigation_and_zoomed_edit = true

  // P31 acceptance: move an existing atom after both zoom and pan, using an expected
  // world delta calculated from fixed CSS-pixel travel and an explicitly chosen scale.
  step = 'viewport zoomed and panned atom move survives save'
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  await controls.getByRole('button', { name: 'Reset zoom (100%)', exact: true }).click()
  await controls.getByRole('button', { name: 'Zoom in', exact: true }).click()
  await controls.getByRole('button', { name: 'Zoom in', exact: true }).click()
  assert.equal((await viewState()).scale, 1.5625, 'two 1.25 zoom steps from 100%')
  await tool('Pan view')
  const panStart = await screen({ x: 225, y: 160 })
  const moveViewBeforePan = await viewState()
  await page.mouse.move(panStart.x, panStart.y)
  await page.mouse.down()
  await page.mouse.move(panStart.x + 50, panStart.y + 30, { steps: 4 })
  await page.mouse.up()
  const moveView = await viewState()
  assert.ok(Math.abs(moveView.box[0] - moveViewBeforePan.box[0] + 32) < 1e-6,
    '50 CSS pixels at 1.5625px/pt changes the view origin by -32pt')
  assert.ok(Math.abs(moveView.box[1] - moveViewBeforePan.box[1] + 19.2) < 1e-6)
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, edited, 'navigation before the move records no edit')
  await tool('Select and move')
  const oxygenStart = await screen({ x: 130, y: 100 })
  await page.mouse.click(oxygenStart.x, oxygenStart.y)
  await page.getByRole('heading', { name: 'Atom a2', exact: true }).waitFor()
  const unchangedAtoms = edited.atoms.filter((atom) => atom.id !== 'a2')
  assert.deepEqual(edited.atoms.find((atom) => atom.id === 'a2').position, { x: 130, y: 100 })
  const movedResponse = page.waitForResponse((response) =>
    response.url().endsWith(`/documents/${id}/transactions`) && response.request().method() === 'POST' &&
    response.request().postDataJSON().commands.some((command) => command.op === 'move_selection'))
  await page.mouse.move(oxygenStart.x, oxygenStart.y)
  await page.mouse.down()
  await page.mouse.move(oxygenStart.x + 40, oxygenStart.y - 20, { steps: 5 })
  await page.mouse.up()
  const committedMove = await movedResponse
  assert.equal(committedMove.status(), 200)
  await page.getByLabel('Revision 3').waitFor()
  const moveRequest = committedMove.request().postDataJSON()
  assert.deepEqual(moveRequest.selection_before.atoms, ['a2'], 'the drag acts on the selected oxygen')
  assert.deepEqual(moveRequest.commands[0].selection, { objects: [], atoms: ['a2'], bonds: [] })
  assert.equal(moveRequest.commands[0].instance, 's1', 'P31 world-coordinate motion retains its explicit graph placement')
  const moved = (await api(`/api/v1/documents/${id}`)).document
  const expectedPosition = { x: 130 + 40 / 1.5625, y: 100 - 20 / 1.5625 }
  const movedOxygen = moved.atoms.find((atom) => atom.id === 'a2')
  assert.ok(Math.abs(movedOxygen.position.x - expectedPosition.x) <= 0.01 &&
    Math.abs(movedOxygen.position.y - expectedPosition.y) <= 0.01,
  `40,-20 CSS pixels must move 25.6,-12.8pt despite pan: ${JSON.stringify(movedOxygen.position)}`)
  assert.deepEqual(moved.atoms.filter((atom) => atom.id !== 'a2'), unchangedAtoms, 'the carbon and nitrogen remain unchanged')
  assert.deepEqual(moved.bonds, edited.bonds, 'the existing bond endpoints and chemistry remain unchanged')
  assert.deepEqual(moved.selection, { atoms: ['a2'], bonds: [], objects: [] }, 'only the moved atom remains selected')
  await page.getByRole('heading', { name: 'Atom a2', exact: true }).waitFor()
  assert.deepEqual(await viewState(), moveView, 'moving the atom does not refit or pan the viewport')
  const savingMove = page.waitForResponse((response) => response.url().endsWith(`/documents/${id}/save`) && response.request().method() === 'POST')
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  assert.equal((await savingMove).status(), 200)
  await closeShown()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Revision 3').waitFor()
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, moved,
    'save/reopen preserves the zoomed move coordinates, selection and unrelated atoms')
  edited = moved
  results.viewport_zoomed_pan_move_save_coordinates = true

  // Cancel an atom drag after its preview moved; release must not send an edit.
  step = 'viewport pointer cancellation'
  await tool('Select and move')
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  const first = await screen({ x: 100, y: 100 })
  let writes = 0
  const countWrite = (request) => {
    if (request.method() === 'POST' && new URL(request.url()).pathname === `/api/v1/documents/${id}/transactions`) writes += 1
  }
  page.on('request', countWrite)
  await canvas.evaluate((svg) => svg.addEventListener('pointerdown', (event) => { svg.dataset.smokePointer = String(event.pointerId) }, { once: true }))
  await page.mouse.move(first.x, first.y)
  await page.mouse.down()
  await page.mouse.move(first.x + 35, first.y + 10, { steps: 4 })
  await canvas.dispatchEvent('pointercancel', { pointerId: Number(await canvas.getAttribute('data-smoke-pointer')), pointerType: 'mouse', isPrimary: true })
  await page.mouse.up()
  await controls.getByRole('button', { name: 'Zoom in', exact: true }).click()
  assert.equal(writes, 0, 'pointer cancellation releases ownership without committing the drag')
  assert.deepEqual((await api(`/api/v1/documents/${id}`)).document, edited)
  page.off('request', countWrite)
  results.viewport_pointer_cancel_no_edit = true

  // A very wide, valid drawing must fit on a narrow display only when Fit is requested.
  step = 'viewport narrow fit'
  const wide = await apiWrite('/api/v1/documents', { request_id: 'viewport-wide-create', title: 'Wide viewport' })
  const wideId = wide.body.document.id
  const wideDrawing = await apiWrite(`/api/v1/documents/${wideId}/transactions`, {
    request_id: 'viewport-wide-draw', expected_revision: 0, commands: [
      { op: 'add_atom', element: 'N', position: { x: -40000, y: 0 } },
      { op: 'add_atom', element: 'O', position: { x: 40000, y: 0 }, bond_to: { atom: '$0', order: 'single' } },
    ],
  })
  assert.equal(wideDrawing.status, 200)
  await page.reload()
  await page.locator(`li[data-document-id="${wideId}"]`).getByRole('button', { name: 'Open' }).click()
  await page.setViewportSize({ width: 320, height: 800 })
  await controls.getByRole('button', { name: 'Fit drawing', exact: true }).click()
  const narrowFits = await canvas.evaluate((svg) => {
    const view = svg.viewBox.baseVal
    const rect = svg.getBoundingClientRect()
    return rect.width > 0 && rect.height > 0 && view.x < -40000 && view.x + view.width > 40000 &&
      view.y < 0 && view.y + view.height > 0 &&
      document.documentElement.scrollWidth <= window.innerWidth
  })
  assert.equal(narrowFits, true, 'explicit Fit includes both distant atoms without horizontal page overflow')
  assert.deepEqual((await api(`/api/v1/documents/${wideId}`)).document, wideDrawing.body.document, 'Fit changes no coordinates or revision')
  await page.setViewportSize({ width: 1280, height: 900 })
  results.viewport_narrow_fit = true
}

// Layout: mobile widths without horizontal overflow, and system dark mode.
for (const width of [390, 320]) {
  await page.setViewportSize({ width, height: 800 })
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  assert.ok(overflow <= 0, `no horizontal overflow at ${width}px (${overflow})`)
}
await page.screenshot({ path: join(outDir, 'editor-mobile.png'), fullPage: true })
await page.setViewportSize({ width: 1280, height: 900 })
await page.emulateMedia({ colorScheme: 'dark' })
await page.waitForTimeout(600)
await page.screenshot({ path: join(outDir, 'editor-dark.png') })
results.mobile_390_and_320_without_horizontal_overflow = true
step = 'after mobile_390_and_320_without_horizontal_overflow'
results.dark_mode_screenshot = true
step = 'after dark_mode_screenshot'

const provoked = ['401', '409', '422', '503', 'ERR_CONNECTION_RESET']
results.uncaught_browser_errors = consoleErrors.filter((text) => !provoked.some((mark) => text.includes(mark)))
results.expected_http_error_console_lines = consoleErrors.length - results.uncaught_browser_errors.length
console.log(JSON.stringify(results, null, 2))
assert.deepEqual(results.uncaught_browser_errors, [])
if (process.env.CHEMIFREE_SMOKE_NO_SKIPS === '1') {
  assert.deepEqual(results.skipped_steps, [], 'CHEMIFREE_SMOKE_NO_SKIPS=1, but steps were skipped (running as root?)')
}
step = 'done'
writeRecords()
await cleanup()
console.log('Browser smoke passed.')
