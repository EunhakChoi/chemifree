// Browser smoke for the document editor: create → edit → undo/redo → save → reopen, plus
// conflict, save-failure, mobile, and dark-mode checks. It starts the API and Vite on a
// temporary data directory (ports 3001 and 5173 must be free) and drives a real browser.
//
//   npm run test:browser
//
// Uses playwright-core with an installed Chrome/Chromium: set CHEMIFREE_BROWSER to its
// executable, or install Playwright's build with `npx playwright-core install chromium`.
// Screenshots go to CHEMIFREE_SMOKE_OUT (default: the temporary directory).
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
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
const cliPath = resolve(metadata.target_directory, 'debug', `chemifree${process.platform === 'win32' ? '.exe' : ''}`)
const env = { ...process.env, CHEMIFREE_DATA_DIR: dataDir }

const servers = spawn('npm', ['run', 'dev'], { env, stdio: ['ignore', 'pipe', 'pipe'], detached: true })
let log = ''
await new Promise((resolveReady, reject) => {
  const timer = setTimeout(() => reject(new Error(`Servers did not start. ${log}`)), 120_000)
  const read = (data) => {
    log += data.toString()
    if (/listening on http:\/\/127\.0\.0\.1:3001/.test(log) && /Local:\s+http:\/\/127\.0\.0\.1:5173/.test(log)) {
      clearTimeout(timer)
      resolveReady()
    }
  }
  servers.stdout.on('data', read)
  servers.stderr.on('data', read)
  servers.once('exit', (code) => reject(new Error(`Servers exited (${code}). ${log}`)))
})

const browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
let step = 'start'
async function cleanup() {
  await browser.close().catch(() => {})
  try {
    process.kill(-servers.pid, 'SIGTERM')
  } catch {
    // Already stopped.
  }
  if (!process.env.CHEMIFREE_SMOKE_OUT) rmSync(work, { recursive: true, force: true })
}
process.on('uncaughtException', async (error) => {
  console.error(`Browser smoke FAILED at step ${step}:`, error.message)
  await cleanup()
  process.exit(1)
})
const context = await browser.newContext({ viewport: { width: 1280, height: 900 } })
const page = await context.newPage()
page.on('console', (message) => message.type() === 'error' && consoleErrors.push(message.text()))
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
await page.getByRole('button', { name: 'Close document' }).click()
await page.getByRole('alertdialog').waitFor()
await page.getByRole('button', { name: 'Discard and close' }).click()
await page.getByText('No document open').waitFor()
await page.locator(`li[data-document-id="${docId}"]`).getByRole('button', { name: 'Open' }).click()
await page.getByLabel('Document title').waitFor()
assert.deepEqual((await openDoc()).document, saved.document)
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
}, { id, rev: saved.document.revision })
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

results.uncaught_browser_errors = consoleErrors.filter((text) => !text.includes('409') && !text.includes('503'))
results.expected_http_error_console_lines = consoleErrors.length - results.uncaught_browser_errors.length
console.log(JSON.stringify(results, null, 2))
assert.deepEqual(results.uncaught_browser_errors, [])
await cleanup()
console.log('Browser smoke passed.')
