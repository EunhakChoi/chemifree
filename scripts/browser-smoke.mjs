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

// Atom tool: O element, click a4 to relabel it.
await page.getByRole('button', { name: 'Element O' }).click()
let a4s = await atomScreen(doc, 'a4')
await page.mouse.click(a4s.x, a4s.y)
await waitIdle()
doc = await openDoc()
assert.equal(doc.document.atoms.find((atom) => atom.id === 'a4').element, 'O')
await page.locator('[data-atom-label="a4"]').waitFor()

// Select a4, set charge and isotope in the inspector.
await tool('Select and move')
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
  const edited = page.waitForResponse(
    (r) => r.request().method() === 'POST' && r.url().endsWith(`/${docId}/transactions`) && r.status() === 200,
  )
  await drawPair()
  await edited
  await notice.waitFor({ state: 'detached' })
  const restartedCopy = await openDoc()
  assert.ok(restartedCopy.document.atoms.length > after.document.atoms.length, 'the edit was retried on the restarted service')
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
await operations.getByTestId('operation-summary').filter({ hasText: '11 of 21 operations run' }).waitFor()
await operations.getByRole('button', { name: 'Operations' }).click()
const operationRow = (id) => operations.locator(`[data-operation="${id}"]`)
await operationRow('document.transaction').getByText('Runs', { exact: true }).waitFor()
for (const [id, followup] of [['chem.properties', 'P19']]) {
  const row = operationRow(id)
  assert.equal(await row.locator('[data-execution]').getAttribute('data-execution'), 'not_implemented', `${id} is not shown as running`)
  assert.equal(await row.getByRole('cell').nth(3).textContent(), followup, `${id} names its follow-up`)
}
assert.equal(await operationRow('chem.validate').locator('[data-execution]').getAttribute('data-execution'), 'executable')
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
  const drawingProblems = () =>
    page.evaluate(async () => {
      await document.fonts.ready
      const svg = document.querySelector('svg[data-structure-canvas]')
      const view = svg.viewBox.baseVal
      const problems = []
      const boxOf = (element) => {
        const [x, y, width, height] = element.getAttribute('data-box').split(' ').map(Number)
        return { x, y, width, height }
      }
      // Within, beyond rounding.
      const within = (inner, outer) =>
        inner.x >= outer.x - 0.05 &&
        inner.y >= outer.y - 0.05 &&
        inner.x + inner.width <= outer.x + outer.width + 0.05 &&
        inner.y + inner.height <= outer.y + outer.height + 0.05
      const inView = (box) => within(box, view)
      if (svg.querySelector('[textLength], [lengthAdjust]')) problems.push('text is drawn at a forced length')
      for (const part of svg.querySelectorAll('[data-frame], [data-frame-id]')) {
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
          if (!inView(box)) problems.push(`${where} ${id}: label outside the view box`)
          for (const part of parts) {
            if (!within(part.box, part.reserved) || !within(part.reserved, box)) {
              problems.push(`${where} ${id}: ${part.name} drawn outside the box reserved for it`)
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
    })
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
  assert.deepEqual(await drawingProblems(), [])
  await page.screenshot({ path: join(outDir, 'editor-states.png') })
  const recorded = await statesDoc()

  await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
  await page.reload()
  await alphaItem.getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  assert.deepEqual(await drawnStates(), expected, 'after a page reload')
  assert.deepEqual(await drawingProblems(), [])
  await closeShown()
  await alphaItem.getByRole('button', { name: 'Open' }).click()
  await page.getByLabel('Document title').waitFor()
  assert.deepEqual(await drawnStates(), expected, 'reopened from storage')
  assert.deepEqual(await drawingProblems(), [])
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
  await page.waitForFunction(() => document.documentElement.classList.contains('dark'))
  const stored = await api('/api/v1/preferences')
  assert.deepEqual([stored.ui.theme, stored.new_documents], ['dark', { label_font: 'monospace' }])
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
