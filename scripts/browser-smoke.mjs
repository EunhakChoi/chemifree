// Browser smoke for the document editor: create → edit → undo/redo → save → reopen, plus
// conflict, save-failure, autosave and crash recovery, save as copy, mobile, and dark-mode
// checks. It starts the API and Vite on a temporary data directory (ports 3001 and 5173 must
// be free) and drives a real browser. The API runs as its own process so it can be killed.
//
//   npm run test:browser
//
// Uses playwright-core with an installed Chrome/Chromium: set CHEMIFREE_BROWSER to its
// executable, or install Playwright's build with `npx playwright-core install chromium`.
// Screenshots go to CHEMIFREE_SMOKE_OUT (default: the temporary directory).
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { chromium } from 'playwright-core'

const BASE = 'http://127.0.0.1:5173'
const results = {}
const consoleErrors = []
const work = mkdtempSync(join(tmpdir(), 'chemifree-browser-'))
const dataDir = join(work, 'data')
const outDir = process.env.CHEMIFREE_SMOKE_OUT ?? work
mkdirSync(outDir, { recursive: true })

execFileSync('cargo', ['build', '--workspace', '--locked'], { stdio: 'inherit' })
const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { encoding: 'utf8' }),
)
const suffix = process.platform === 'win32' ? '.exe' : ''
const cliPath = resolve(metadata.target_directory, 'debug', `chemifree${suffix}`)
const apiPath = resolve(metadata.target_directory, 'debug', `chemifree-api${suffix}`)
const env = { ...process.env, CHEMIFREE_DATA_DIR: dataDir }

/** Starts a process and resolves once its output matches `ready`. */
function start(command, args, ready, options = {}) {
  const child = spawn(command, args, { env, stdio: ['ignore', 'pipe', 'pipe'], ...options })
  let log = ''
  const started = new Promise((resolveReady, reject) => {
    const timer = setTimeout(() => reject(new Error(`${command} did not start. ${log}`)), 120_000)
    const read = (data) => {
      log += data.toString()
      if (ready.test(log)) {
        clearTimeout(timer)
        resolveReady(child)
      }
    }
    child.stdout.on('data', read)
    child.stderr.on('data', read)
    child.once('exit', (code) => reject(new Error(`${command} exited (${code}). ${log}`)))
  })
  return { child, started }
}
const startApi = () => start(apiPath, ['--port', '3001'], /listening on http:\/\/127\.0\.0\.1:3001/)
let apiServer = startApi()
const web = start('npm', ['run', 'dev:web'], /Local:\s+http:\/\/127\.0\.0\.1:5173/, { detached: true })
// Registered before anything can fail so a failed start or launch never leaves servers running.
let browser = null
let step = 'start'
async function cleanup() {
  await browser?.close().catch(() => {})
  apiServer.child.kill('SIGTERM')
  try {
    process.kill(-web.child.pid, 'SIGTERM')
  } catch {
    // Already stopped.
  }
  if (!process.env.CHEMIFREE_SMOKE_OUT) rmSync(work, { recursive: true, force: true })
}
let page = null
const recentRequests = []
process.on('uncaughtException', async (error) => {
  console.error(`Browser smoke FAILED at step ${step}:`, error.message)
  await page?.screenshot({ path: join(outDir, 'failure.png') }).catch(() => {})
  console.error(recentRequests.join('\n'))
  await cleanup()
  process.exit(1)
})
await Promise.all([apiServer.started, web.started])
/** Kills the API the way a crash would and starts a new process on the same port. */
async function crashApi() {
  const exited = new Promise((resolveExit) => apiServer.child.once('exit', resolveExit))
  apiServer.child.kill('SIGKILL')
  await exited
  apiServer = startApi()
  await apiServer.started
}

browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } })
page = await context.newPage()
page.on('console', (message) => message.type() === 'error' && consoleErrors.push(message.text()))
const started = new Map()
page.on('request', (request) => started.set(request, Date.now()))
const note = (request, outcome) => {
  recentRequests.push(`${new Date().toISOString()} ${request.method()} ${new URL(request.url()).pathname} ${outcome} ${Date.now() - (started.get(request) ?? Date.now())}ms`)
  if (recentRequests.length > 40) recentRequests.shift()
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
await page.getByRole('button', { name: 'Increase charge' }).click()
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
await clickFor(page.getByRole('alertdialog').getByRole('button', { name: 'Discard draft' }), '/discard-draft')
await page.getByTestId('recovery-notice').waitFor({ state: 'detached' })
assert.ok(!existsSync(draftFile()))
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

// A failed autosave is shown and retried; the saved file is untouched meanwhile.
if (process.getuid?.() !== 0) {
  const draftsDir = join(dataDir, 'drafts')
  chmodSync(draftsDir, 0o500)
  await drawPair()
  await waitDraft('failed')
  assert.equal(await saveState(), 'Unsaved changes')
  chmodSync(draftsDir, 0o700)
  await clickFor(page.getByRole('button', { name: 'Retry now' }), '/autosave')
  await waitDraft('saved')
  assert.ok(existsSync(draftFile()))
}
results.autosave_failure_is_shown_and_retried = true
step = 'after autosave_failure_is_shown_and_retried'

// Save as copy: the unsaved drawing becomes a new document; the original keeps its save.
const originalFile = join(dataDir, 'documents', `${docId}.json`)
const originalBytes = readFileSync(originalFile)
const originalId = docId
const copyResponse = page.waitForResponse((r) => r.url().endsWith('/save-as'))
await page.getByRole('button', { name: 'Save as copy' }).click()
await page.getByRole('dialog').getByLabel('Title of the copy').fill('Branch')
await page.getByRole('dialog').getByRole('button', { name: 'Save copy' }).click()
const copyBody = await (await copyResponse).json()
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
await drawPair()
await clickFor(page.getByRole('button', { name: 'Save', exact: true }), '/save')
assert.deepEqual(readFileSync(originalFile), originalBytes, 'editing the copy leaves the original alone')
await page.getByRole('button', { name: 'Open original' }).click()
await page.getByTestId('copy-notice').waitFor({ state: 'detached' })
assert.equal((await api(`/api/v1/documents/${originalId}`)).session.unsaved_changes, false)
docId = originalId
results.save_as_copy_is_independent = true
step = 'after save_as_copy_is_independent'

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
await cleanup()
console.log('Browser smoke passed.')
