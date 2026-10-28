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
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
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
  apiServer?.child.kill('SIGTERM')
  if (web) {
    try {
      process.kill(-web.child.pid, 'SIGTERM')
    } catch {
      // Already stopped.
    }
  }
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
  const path = new URL(request.url()).pathname
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
    const svg = document.querySelector('svg[role="img"]')
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

await page.goto(BASE)
await page.getByText('No document open').waitFor()

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
const canvas = page.locator('svg[role="img"]')
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
await page.locator('text[data-atom="a4"]').waitFor()

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
assert.equal((await page.locator('text[data-atom="a4"]').textContent()).replace(/\s/g, ''), '18O−')
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
await page.getByText('Save failed', { exact: true }).waitFor()
assert.equal(await saveState(), 'Unsaved changes')
rmSync(file, { recursive: true })
writeFileSync(file, committed)
await page.getByRole('button', { name: 'Save', exact: true }).click()
await waitIdle()
assert.equal(await saveState(), 'Saved')
assert.equal(await page.getByText('Save failed', { exact: true }).count(), 0)
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
for (const title of ['Save failed', 'Save not confirmed']) {
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
const drawPair = async () => {
  await tool('Bond')
  const area = await page.locator('svg[role="img"]').boundingBox()
  const response = page.waitForResponse((r) => r.request().method() === 'POST' && r.url().endsWith('/transactions'))
  await page.mouse.click(area.x + area.width * 0.3, area.y + area.height * 0.7)
  await response
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
const outdatedDraftAtoms = JSON.parse(readFileSync(draftFile(), 'utf8')).document.atoms.length
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

// A failed autosave is shown and retried; the saved file is untouched meanwhile.
if (runningAsRoot) {
  results.skipped_steps.push('autosave_retry_is_not_postponed_by_edits', 'autosave_failure_is_shown_and_retried')
} else {
  const draftsDir = join(dataDir, 'drafts')
  // The retry keeps its schedule (2 s after the first failure) while edits keep coming.
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await waitDraft('failed')
  chmodSync(draftsDir, 0o700)
  let retried = false
  const onRetry = (r) => {
    if (r.url().endsWith(`/${docId}/autosave`) && r.status() === 200) retried = true
  }
  page.on('response', onRetry)
  const drawingUntil = performance.now() + 8_000
  while (!retried && performance.now() < drawingUntil) {
    await drawPair()
    await page.waitForTimeout(300)
  }
  page.off('response', onRetry)
  assert.ok(retried, 'the retry ran while edits kept coming')
  await waitDraft('saved')
  results.autosave_retry_is_not_postponed_by_edits = true

  chmodSync(draftsDir, 0o500)
  await drawPair()
  await waitDraft('failed')
  assert.equal(await saveState(), 'Unsaved changes')
  chmodSync(draftsDir, 0o700)
  await clickFor(page.getByRole('button', { name: 'Retry now' }), '/autosave')
  await waitDraft('saved')
  assert.ok(existsSync(draftFile()))
  results.autosave_failure_is_shown_and_retried = true
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
