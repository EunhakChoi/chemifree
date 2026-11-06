// Real panel/hook/client. HTTP and clipboard are boundary fakes, not chemistry proof.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'
import { sceneCutAfter, sceneCutCopy, sceneCutSource } from './lib/scene-cut-fixture.mjs'

const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { SceneClipboardPanel } from '/src/components/scene-clipboard-panel.tsx'
import { NativeClipboardPanel } from '/src/components/native-clipboard-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.graphicsCutHarness = { editor }
  return <main className="p-4"><output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    {editor.current && <><SceneClipboardPanel key={editor.current.document.id} document={editor.current.document} editor={editor}/>
    <NativeClipboardPanel key={'graph-' + editor.current.document.id} document={editor.current.document} editor={editor} activePlacement="s1"/></>}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{ name: 'graphics-cut-check-fixture', configureServer(vite) {
    vite.middlewares.use('/graphics-cut-check', async (_request, response) => {
      const html = await vite.transformIndexHtml('/graphics-cut-check', '<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><title>Graphics Cut check</title></head><body><div id="root"></div><script type="module" src="/graphics-cut-harness.tsx"></script></body></html>')
      response.setHeader('content-type', 'text/html')
      response.end(html)
    })
  }, resolveId(id) { if (id === '/graphics-cut-harness.tsx') return id },
  load(id) { if (id === '/graphics-cut-harness.tsx') return fixture } }],
})
let browser
try {
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const context = await browser.newContext({ viewport: { width: 390, height: 600 } })
  await keepRequestInterception(context)
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  let state = sceneCutSource()
  let history
  const bodies = []
  const replies = new Map()
  let lost = 0
  let hold = false
  let finishCut
  let cutHeld
  const heldCutReady = new Promise((resolve) => { cutHeld = resolve })
  let token = 'graphics-cut-service'
  let rejectInspection = false
  const reply = (route, body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return reply(route, { header: 'x-chemifree-session', token })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return reply(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return reply(route, state)
    if (path === '/api/v1/documents/other') return reply(route, { ...state, document: { ...state.document, id: 'other' } })
    if (path.endsWith('/undo') || path.endsWith('/redo')) {
      const undo = path.endsWith('/undo')
      const next = structuredClone(undo ? history.before : history.after)
      next.document.revision = state.document.revision + 1
      next.session.undo_depth = undo ? 0 : 1
      next.session.redo_depth = undo ? 1 : 0
      next.session.unsaved_changes = true
      state = next
      return reply(route, state)
    }
    if (path.endsWith('/transactions')) {
      const body = route.request().postDataJSON()
      bodies.push(body)
      let confirmed = replies.get(body.request_id)
      if (!confirmed) {
        assert.equal(body.expected_revision, 3)
        assert.deepEqual(body.commands, [{ op: 'delete_objects', objects: ['s4'], mode: 'subtree' }])
        const before = structuredClone(state)
        before.document.selection = structuredClone(body.selection_before)
        state = sceneCutAfter(state)
        history = { before, after: structuredClone(state) }
        confirmed = structuredClone(state)
        replies.set(body.request_id, confirmed)
      }
      if (lost > 0) { lost -= 1; return route.abort('failed') }
      if (hold) { finishCut = () => reply(route, confirmed); cutHeld(); return }
      return reply(route, confirmed)
    }
    assert.equal(path, '/api/v1/operations')
    const request = route.request().postDataJSON()
    if (request.operation === 'chem.identity' && rejectInspection) return route.fulfill({ status: 401,
      contentType: 'application/json', body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }) })
    assert.equal(request.operation, 'document.clipboard_scene_copy')
    return reply(route, { contract: 1, envelope: 'operation_result', request_id: request.request_id,
      operation: request.operation, profile: 'preserve-semantics', document_revision: state.document.revision,
      outcome: 'ok', diagnostics: [], loss_report: [], result: sceneCutCopy(state) })
  })
  await page.addInitScript(() => {
    window.graphicsCutClipboard = { writes: [], mode: 'ok', finish: null }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      writeText(text) {
        const clipboard = window.graphicsCutClipboard
        clipboard.writes.push({ text, active: navigator.userActivation.isActive })
        if (clipboard.mode === 'deny') return Promise.reject(new Error('Denied.'))
        if (clipboard.mode === 'hold') return new Promise((resolve) => { clipboard.finish = resolve })
        return Promise.resolve()
      },
      readText() { throw new Error('Cut must never read the clipboard.') },
    } })
  })
  const url = `http://127.0.0.1:${address.port}/graphics-cut-check`
  async function reset() {
    state = sceneCutSource()
    await page.goto(url)
    await page.getByLabel('Current document', { exact: true }).filter({ hasText: 'drawing' }).waitFor()
    await page.evaluate(() => window.graphicsCutHarness.editor.setSelection(new Set(['s4', 's5'])))
  }
  async function prepare() {
    await page.getByRole('button', { name: 'Cut graphics…', exact: true }).click()
    await page.getByRole('dialog').getByText('1 roots, 3 objects, revision 3.', { exact: true }).waitFor()
  }
  const dialog = page.getByRole('dialog')
  const scenePanel = page.getByRole('region', { name: 'Scene graphics clipboard', exact: true })
  const cut = dialog.getByRole('button', { name: 'Cut to clipboard', exact: true })
  await reset()
  await prepare()
  const payload = await dialog.getByLabel('Native graphics text', { exact: true }).inputValue()
  assert.equal(bodies.length, 0)
  assert.deepEqual(await page.evaluate(() => window.graphicsCutClipboard.writes), [])
  await dialog.getByRole('button', { name: 'Select native graphics text', exact: true }).click()
  assert.equal(bodies.length, 0, 'manual text selection never authorizes deletion')
  await page.evaluate(() => { window.graphicsCutClipboard.mode = 'deny' })
  await cut.click()
  await dialog.getByRole('alert').filter({ hasText: 'The graphics were retained' }).waitFor()
  assert.equal(bodies.length, 0)
  await page.evaluate(() => { window.graphicsCutClipboard.mode = 'hold' })
  await cut.click()
  await page.waitForFunction(() => typeof window.graphicsCutClipboard.finish === 'function')
  assert.equal(bodies.length, 0, 'write must resolve before deleting')
  await page.evaluate(() => window.graphicsCutHarness.editor.setSelection(new Set(['a1', 's7'])))
  await page.evaluate(() => window.graphicsCutClipboard.finish())
  await dialog.waitFor({ state: 'hidden' })
  assert.equal(bodies.length, 1)
  assert.deepEqual(bodies[0].selection_before, { objects: ['s4', 's5'], atoms: [], bonds: [] })
  assert.deepEqual(await page.evaluate(() => [...window.graphicsCutHarness.editor.selection]), [])
  assert.deepEqual(await page.evaluate(() => window.graphicsCutClipboard.writes.at(-1)), { text: payload, active: true })
  assert.deepEqual(state.document.scene.objects.map((object) => object.id), ['s1', 's2', 's3', 's7'])
  await page.evaluate(() => window.graphicsCutHarness.editor.undo())
  await page.waitForFunction(() => window.graphicsCutHarness.editor.current.document.revision === 5)
  assert.deepEqual(await page.evaluate(() => [...window.graphicsCutHarness.editor.selection].sort()), ['s4', 's5'])
  assert.deepEqual(state.document.scene, sceneCutSource().document.scene)
  await page.evaluate(() => window.graphicsCutHarness.editor.redo())
  await page.waitForFunction(() => window.graphicsCutHarness.editor.current.document.revision === 6)
  assert.deepEqual(state.document.scene, sceneCutAfter().document.scene)
  assert.equal((await page.evaluate(() => window.graphicsCutClipboard.writes)).length, 2, 'Undo/Redo does not rewrite the clipboard')

  await reset()
  await prepare()
  await page.evaluate(() => { Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined }) })
  await cut.click()
  await dialog.getByRole('alert').filter({ hasText: 'Browser clipboard writing is unavailable' }).waitFor()
  assert.equal(bodies.length, 1, 'API absence cannot authorize deletion')

  await reset()
  state.document.scene.references.push({ source: 's7', target: { kind: 'object', id: 's5' } })
  await page.evaluate(() => window.graphicsCutHarness.editor.open('drawing'))
  await page.getByRole('button', { name: 'Cut graphics…', exact: true }).click()
  await dialog.getByRole('alert').filter({ hasText: 'Objects outside these graphics reference them' }).waitFor()
  assert.equal(bodies.length, 1, 'incoming reference refusal is before clipboard access and deletion')
  assert.deepEqual(await page.evaluate(() => window.graphicsCutClipboard.writes), [])
  await dialog.getByRole('button', { name: 'Close', exact: true }).click()
  await page.getByRole('button', { name: 'Copy graphics…', exact: true }).click()
  await dialog.getByText('1 roots, 3 objects, revision 3.', { exact: true }).waitFor()
  assert.equal(await dialog.getByLabel('Native graphics text', { exact: true }).inputValue(), sceneCutCopy(state).payload, 'Copy keeps its broader incoming-reference eligibility')
  await dialog.getByRole('button', { name: 'Close', exact: true }).click()

  await reset()
  await page.evaluate(() => window.graphicsCutHarness.editor.setSelection(new Set(['s4', 'a1'])))
  await page.getByRole('button', { name: 'Cut graphics…', exact: true }).click()
  await dialog.getByRole('alert').waitFor()
  assert.deepEqual(await page.evaluate(() => window.graphicsCutClipboard.writes), [])
  assert.equal(bodies.length, 1, 'mixed graph selection is refused rather than filtered')

  await reset()
  await prepare()
  state.document.title = 'Changed during preparation'
  state.document.revision = 4
  await page.evaluate(() => window.graphicsCutHarness.editor.open('drawing'))
  await cut.click()
  await dialog.getByRole('alert').filter({ hasText: 'No Cut deletion was sent' }).waitFor()
  assert.equal(bodies.length, 1)

  await reset()
  await prepare()
  lost = 2
  await cut.click()
  await dialog.getByRole('button', { name: 'Check graphics Cut outcome', exact: true }).waitFor()
  const uncertain = structuredClone(bodies.at(-1))
  assert.equal(bodies.length, 3)
  await page.evaluate(() => window.graphicsCutHarness.editor.apply([{ op: 'rename', title: 'Must remain blocked' }]))
  assert.equal(bodies.length, 3, 'an unresolved graphics deletion blocks later queued mutations')
  assert.equal(state.document.title, 'Graphic Cut')
  assert.equal(await page.getByRole('button', { name: 'Check paste outcome', exact: true }).count(), 0, 'graph panel must not duplicate graphics recovery')
  await dialog.getByRole('button', { name: 'Close', exact: true }).click()
  // The portal remains mounted during its exit animation. Recover from the panel
  // only after dismissal, rather than matching its temporarily retained button too.
  await dialog.waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => window.graphicsCutHarness.editor.clipboardPending), true)
  const writes = (await page.evaluate(() => window.graphicsCutClipboard.writes)).length
  await scenePanel.getByRole('button', { name: 'Check graphics Cut outcome', exact: true }).click()
  await page.waitForFunction(() => !window.graphicsCutHarness.editor.clipboardPending)
  assert.deepEqual(bodies.slice(-3), [uncertain, uncertain, uncertain])
  assert.equal((await page.evaluate(() => window.graphicsCutClipboard.writes)).length, writes, 'recovery never writes again')

  await reset()
  await prepare()
  hold = true
  await cut.click()
  await page.waitForFunction(() => window.graphicsCutHarness.editor.clipboardAction === 'scene_cut')
  // The HTTP route deliberately holds an old-process successful deletion reply.
  await heldCutReady
  assert.equal(typeof finishCut, 'function')
  token = 'new-graphics-service'
  rejectInspection = true
  await page.evaluate(async () => {
    try { await window.graphicsCutHarness.editor.inspectIdentity('drawing', 'chemical', null) } catch { /* Expected restart. */ }
  })
  await page.waitForFunction(() => window.graphicsCutHarness.editor.serviceGeneration > 0)
  await finishCut()
  await page.waitForFunction(() => window.graphicsCutHarness.editor.clipboardNeedsReload)
  assert.equal(await page.evaluate(() => window.graphicsCutHarness.editor.current.document.revision), 3, 'held old-process success cannot replace the current snapshot')
  await dialog.getByRole('button', { name: 'Reload document to reconcile', exact: true }).click()
  await page.waitForFunction(() => !window.graphicsCutHarness.editor.clipboardPending)
  assert.equal(await page.evaluate(() => window.graphicsCutHarness.editor.current.document.revision), 4)
  assert.deepEqual(errors, [])
  console.log('Graphic forest Cut activation, frozen scope, incoming references, history, lost reply and restart checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
