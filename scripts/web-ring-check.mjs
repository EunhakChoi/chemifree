// Real canvas, editor queue and client; HTTP is scripted. Not chemistry validation.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'
import { ringAfter, ringSelection, ringSource } from './lib/ring-fixture.mjs'

const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { StructureCanvas } from '/src/editor/structure-canvas.tsx'
import { DocumentWorkspace } from '/src/components/document-workspace.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.ringHarness = editor
  return <main>
    <output aria-label="Current atom count">{editor.current?.document.atoms.length ?? 'loading'}</output>
    <output aria-label="Current document">{editor.current?.document.id ?? 'none'}</output>
    <output aria-label="Pending ring">{String(editor.clipboardPending)}</output>
    <output aria-label="Editor error">{editor.error?.message}</output>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo</button>
    {editor.clipboardPending && <button onClick={() => void editor.checkClipboardPaste()}>Check ring insertion outcome</button>}
    {editor.current && <div style={{ width: 760, height: 600 }}><StructureCanvas document={editor.current.document}
      tool="ring" element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection}
      activePlacement="s1" onActivePlacementChange={() => {}} onCommands={() => { throw new Error('Ring must use its dedicated attempt') }}
      onMark={() => {}} onRing={(instance, center) => void editor.insertRing(editor.current.document.id, instance, center)} disabled={editor.busy}/></div>}
  </main>
}
createRoot(document.getElementById('root')).render(location.search.includes('workspace') ? <DocumentWorkspace /> : <Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{ name: 'ring-check-fixture', configureServer(vite) {
    vite.middlewares.use('/ring-check', async (_request, response) => {
      const html = await vite.transformIndexHtml('/ring-check', '<!doctype html><html><head><title>Ring check</title></head><body><div id="root"></div><script type="module" src="/ring-harness.tsx"></script></body></html>')
      response.setHeader('content-type', 'text/html')
      response.end(html)
    })
  }, resolveId(id) { if (id === '/ring-harness.tsx') return id },
  load(id) { if (id === '/ring-harness.tsx') return fixture } }],
})
let browser
try {
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const context = await browser.newContext()
  await keepRequestInterception(context)
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  let state = ringSource()
  let lost = 2
  let droppedBeforeDispatch = 0
  let holdBeforeDispatch = false
  let releaseHeld
  let heldReady
  let expectedSelection = ringSelection()
  let expectedSize = 6
  let expectedPattern = 'single'
  const bodies = []
  const replies = new Map()
  const navigation = []
  const reply = (route, body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return reply(route, { header: 'x-chemifree-session', token: 'ring-service' })
    if (path === '/api/v1/preferences') {
      const computation = { status: 'not_implemented', owner: 'preferences', followup: 'P19', note: 'Not yet.' }
      return reply(route, { api_version: 'v1', revision: 0, ui: { theme: 'system' }, new_documents: {},
        new_document_settings: state.document.settings, computation: { nmr: computation, numeric_precision: computation } })
    }
    if (path === '/api/v1/documents') {
      if (route.request().method() !== 'GET') navigation.push(path)
      return reply(route, { api_version: 'v1', documents: [{ id: 'drawing', title: state.document.title,
        revision: state.document.revision, atom_count: 2, bond_count: 1, open: true,
        unsaved_changes: state.session.unsaved_changes, recovery: null, saved_at_ms: 1 }], unreadable: [] })
    }
    if (path === '/api/v1/trash') return reply(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return reply(route, state)
    if (path === '/api/v1/documents/other') {
      navigation.push(path)
      return reply(route, { ...state, document: { ...state.document, id: 'other' } })
    }
    if (path.endsWith('/close')) {
      navigation.push(path)
      return reply(route, { api_version: 'v1', discarded_changes: route.request().postDataJSON().discard_unsaved })
    }
    if (path.endsWith('/autosave')) return reply(route, state)
    if (path.endsWith('/undo') || path.endsWith('/redo')) {
      const revision = state.document.revision + 1
      const undo = path.endsWith('/undo')
      state = undo ? ringSource() : ringAfter(expectedSize, expectedPattern)
      state.document.revision = revision
      state.document.selection = ringSelection()
      state.session.undo_depth = undo ? 0 : 1
      state.session.redo_depth = undo ? 1 : 0
      return reply(route, state)
    }
    assert.ok(path === '/api/v1/documents/drawing/transactions' || path === '/api/v1/operations', path)
    const body = route.request().postDataJSON()
    bodies.push(body)
    if (holdBeforeDispatch) {
      holdBeforeDispatch = false
      await new Promise((resolve) => { releaseHeld = resolve; heldReady() })
    }
    if (droppedBeforeDispatch > 0) { droppedBeforeDispatch -= 1; return route.abort('failed') }
    if (path === '/api/v1/operations') {
      assert.ok(['document.clipboard_paste', 'document.clipboard_scene_paste'].includes(body.operation))
      const conflict = bodies.length > 3
      if (conflict) {
        state.document.title = 'Changed elsewhere'
        state.document.revision = 4
        state.session.saved_revision = 4
      }
      return reply(route, { contract: 1, envelope: 'operation_result', request_id: body.request_id,
        operation: body.operation, profile: 'preserve-semantics', document_revision: null,
        outcome: 'error', result: null, diagnostics: [{ code: conflict ? 'revision_conflict' : 'invalid_command',
          message: conflict ? 'Revision changed before replay could confirm the attempt.' : 'Unrelated refusal after reply loss.',
          severity: 'error', path: null, details: conflict ? { current_revision: 4 } : {} }], loss_report: [] })
    }
    assert.deepEqual(body.commands, [{ op: 'insert_ring', instance: 's1', center: { x: 100, y: 100 }, ...(expectedSize === 6 ? {} : { size: expectedSize }), ...(expectedPattern === 'single' ? {} : { pattern: expectedPattern }) }])
    let confirmed = replies.get(body.request_id)
    if (!confirmed) {
      assert.equal(body.expected_revision, 3)
      assert.deepEqual(body.selection_before, expectedSelection)
      state = ringAfter(expectedSize, expectedPattern)
      state.document.selection = structuredClone(expectedSelection)
      confirmed = structuredClone(state)
      replies.set(body.request_id, confirmed)
    }
    if (lost > 0) { lost -= 1; return route.abort('failed') }
    return reply(route, confirmed)
  })
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
    readText() { throw new Error('Ring insertion must not read the OS clipboard.') },
    writeText() { throw new Error('Ring insertion must not write the OS clipboard.') },
  } }))
  await page.goto(`http://127.0.0.1:${address.port}/ring-check`)
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '2' }).waitFor()
  await page.evaluate(() => window.ringHarness.setSelection(new Set(['s7', 'a1', 'b1'])))
  const canvas = page.locator('[data-structure-canvas]')
  await canvas.focus()
  for (let index = 0; index < 5; index += 1) await canvas.press('-')
  await canvas.press('ArrowRight')
  const point = await canvas.evaluate((svg) => {
    const location = new DOMPoint(100, 100).matrixTransform(svg.getScreenCTM())
    return { x: location.x, y: location.y }
  })
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await page.mouse.move(point.x + 20, point.y + 20)
  await page.mouse.up()
  assert.deepEqual(bodies, [], 'dragging the ring tool does not insert')
  await page.mouse.click(point.x, point.y)
  await page.getByLabel('Pending ring', { exact: true }).filter({ hasText: 'true' }).waitFor()
  assert.equal(state.document.atoms.length, 8, 'backend committed one ring despite losing both replies')
  assert.equal(await page.getByRole('button', { name: 'Undo', exact: true }).isDisabled(), true)
  await page.getByRole('button', { name: 'Check ring insertion outcome', exact: true }).click()
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '8' }).waitFor()
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.equal(replies.size, 1)
  assert.deepEqual(await page.evaluate(() => [...window.ringHarness.selection]), ['s7', 'a1', 'b1'])
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '2' }).waitFor()
  await page.getByRole('button', { name: 'Redo', exact: true }).click()
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '8' }).waitFor()
  assert.deepEqual(state.document.atoms, ringAfter().document.atoms)
  assert.deepEqual(state.document.bonds, ringAfter().document.bonds)

  const url = `http://127.0.0.1:${address.port}/ring-check`
  const emptySelection = { objects: [], atoms: [], bonds: [] }
  function resetRequests() {
    state = ringSource()
    lost = 0
    droppedBeforeDispatch = 2
    expectedSelection = emptySelection
    expectedSize = 6
    expectedPattern = 'single'
    bodies.length = 0
    navigation.length = 0
    replies.clear()
  }

  // Production size control + CloseButton + frozen five-ring recovery; sends fail before dispatch.
  resetRequests()
  expectedSize = 5
  await page.goto(`${url}?workspace`)
  await page.locator('[data-document-id="drawing"]').getByRole('button', { name: 'Open', exact: true }).click()
  await page.getByLabel('Document title', { exact: true }).waitFor()
  const close = page.getByRole('button', { name: 'Close document', exact: true })
  assert.equal(await close.isEnabled(), true, 'a clean idle document can close')
  await page.getByRole('button', { name: 'Insert carbon ring', exact: true }).click()
  const five = page.getByRole('button', { name: 'Five-member carbon ring', exact: true })
  const six = page.getByRole('button', { name: 'Six-member carbon ring', exact: true })
  assert.equal(await six.getAttribute('aria-pressed'), 'true', 'initial size is six')
  await five.click()
  assert.equal(await five.getAttribute('aria-pressed'), 'true')
  const productionCanvas = page.locator('[data-structure-canvas]')
  await productionCanvas.scrollIntoViewIfNeeded()
  await productionCanvas.focus()
  for (let index = 0; index < 5; index += 1) await productionCanvas.press('-')
  const center = await productionCanvas.evaluate((svg) => {
    const location = new DOMPoint(100, 100).matrixTransform(svg.getScreenCTM())
    return { x: location.x, y: location.y }
  })
  await page.mouse.click(center.x, center.y)
  const checkRing = page.getByRole('button', { name: 'Check ring insertion outcome', exact: true })
  await checkRing.waitFor()
  assert.equal(await five.isDisabled(), true, 'size cannot retarget an unresolved attempt')
  assert.equal(await six.isDisabled(), true)
  assert.equal(state.session.unsaved_changes, false)
  assert.equal(replies.size, 0, 'both sends failed before dispatch')
  assert.equal(await close.isDisabled(), true, 'Close cannot remove the production recovery controls')
  assert.equal(await page.getByLabel('Document title', { exact: true }).inputValue(), 'Independent ring')
  assert.deepEqual(navigation, [])
  await checkRing.click()
  await checkRing.waitFor({ state: 'hidden' })
  assert.equal(state.document.atoms.length, 7, 'production size selection inserts five atoms')
  assert.equal(await five.isEnabled(), true)
  assert.equal(bodies[0].commands[0].size, 5)
  assert.deepEqual(state.document.atoms, ringAfter(5).document.atoms)
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  await close.click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Discard and close', exact: true }).click()
  await page.getByText('No document open', { exact: true }).waitFor()
  assert.deepEqual(navigation, ['/api/v1/documents/drawing/close'])
  assert.equal(await page.getByRole('button', { name: 'New document', exact: true }).isEnabled(), true)

  // Production explicit Kekulé choice is frozen through response loss, then remains undoable.
  resetRequests()
  expectedPattern = 'kekule'
  await page.goto(`${url}?workspace`)
  await page.locator('[data-document-id="drawing"]').getByRole('button', { name: 'Open', exact: true }).click()
  await page.getByLabel('Document title', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Insert carbon ring', exact: true }).click()
  const singlePattern = page.getByRole('button', { name: 'All single bonds', exact: true })
  const kekulePattern = page.getByRole('button', { name: 'Kekulé alternating bonds', exact: true })
  assert.equal(await singlePattern.getAttribute('aria-pressed'), 'true')
  await kekulePattern.click()
  assert.equal(await kekulePattern.getAttribute('aria-pressed'), 'true')
  // Switching to five or attachment explicitly drops the unsupported recipe.
  await page.getByRole('button', { name: 'Five-member carbon ring', exact: true }).click()
  await kekulePattern.waitFor({ state: 'hidden' })
  await page.getByRole('button', { name: 'Six-member carbon ring', exact: true }).click()
  assert.equal(await singlePattern.getAttribute('aria-pressed'), 'true')
  await kekulePattern.click()
  await page.getByRole('button', { name: 'Attach to atom', exact: true }).click()
  assert.equal(await singlePattern.getAttribute('aria-pressed'), 'true')
  await page.getByRole('button', { name: 'Independent', exact: true }).click()
  assert.equal(await singlePattern.getAttribute('aria-pressed'), 'true')
  await kekulePattern.click()
  const kekuleCanvas = page.locator('[data-structure-canvas]')
  await kekuleCanvas.scrollIntoViewIfNeeded()
  await kekuleCanvas.focus()
  for (let index = 0; index < 5; index += 1) await kekuleCanvas.press('-')
  const kekuleCenter = await kekuleCanvas.evaluate((svg) => {
    const location = new DOMPoint(100, 100).matrixTransform(svg.getScreenCTM())
    return { x: location.x, y: location.y }
  })
  await page.mouse.click(kekuleCenter.x, kekuleCenter.y)
  await checkRing.waitFor()
  assert.equal(await kekulePattern.isDisabled(), true)
  assert.equal(await singlePattern.isDisabled(), true)
  assert.equal(await page.getByRole('button', { name: 'Close document', exact: true }).isDisabled(), true)
  await checkRing.click()
  await checkRing.waitFor({ state: 'hidden' })
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.equal(bodies[0].commands[0].pattern, 'kekule')
  assert.deepEqual(state.document.bonds.slice(1).map((bond) => bond.order), ['double', 'single', 'double', 'single', 'double', 'single'])
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await page.locator('[data-structure-canvas][aria-label="Independent ring: 2 atoms and 1 bonds"]').waitFor()
  assert.equal(state.document.atoms.length, 2)
  await page.getByRole('button', { name: 'Redo', exact: true }).click()
  await page.locator('[data-structure-canvas][aria-label="Independent ring: 8 atoms and 7 bonds"]').waitFor()
  assert.deepEqual(state.document.bonds, ringAfter(6, 'kekule').document.bonds)

  // Hook callers can queue navigation before a sending ring becomes "unconfirmed".
  resetRequests()
  holdBeforeDispatch = true
  const firstRequestHeld = new Promise((resolve) => { heldReady = resolve })
  await page.goto(url)
  await page.getByLabel('Current document', { exact: true }).filter({ hasText: 'drawing' }).waitFor()
  await page.evaluate(() => {
    const editor = window.ringHarness
    void editor.insertRing('drawing', 's1', { x: 100, y: 100 })
    window.queuedNavigation = Promise.all([editor.close(false), editor.close(true), editor.open('other'), editor.create('Blocked')])
  })
  await firstRequestHeld
  releaseHeld()
  await page.evaluate(() => window.queuedNavigation)
  await page.getByLabel('Pending ring', { exact: true }).filter({ hasText: 'true' }).waitFor()
  assert.deepEqual(navigation, [], 'run checks the unresolved ref after earlier queued work fails')
  assert.equal(await page.getByLabel('Current document', { exact: true }).textContent(), 'drawing')
  assert.equal(state.document.atoms.length, 2)
  await page.evaluate(async () => { await window.ringHarness.close(false); await window.ringHarness.open('other') })
  assert.deepEqual(navigation, [], 'direct close/open calls cannot bypass the hook gate')
  await page.getByRole('button', { name: 'Check ring insertion outcome', exact: true }).click()
  await page.getByLabel('Pending ring', { exact: true }).filter({ hasText: 'false' }).waitFor()
  await page.evaluate(() => window.ringHarness.close(true))
  assert.deepEqual(navigation, ['/api/v1/documents/drawing/close'])
  assert.equal(await page.getByLabel('Current document', { exact: true }).textContent(), 'none')

  // Existing graph and scene attempts share the same navigation gate and remain checkable.
  const graphPayload = JSON.stringify({ format: 'chemifree.clipboard', version: 1, settings: ringSource().document.settings,
    atoms: [{ id: 'a1', kind: { type: 'element', symbol: 'C' }, charge: 0, isotope: null,
      hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x: 0, y: 0 } }], bonds: [], stereo: [] })
  const scenePayload = JSON.stringify({ format: 'chemifree.clipboard', version: 2, kind: 'graphics', settings: ringSource().document.settings,
    roots: ['s9'], objects: [{ id: 's9', kind: 'graphic', shape: 'line', points: [{ x: 0, y: 0 }, { x: 10, y: 0 }] }], references: [] })
  for (const [kind, payload] of [['paste', graphPayload], ['scene_paste', scenePayload]]) {
    resetRequests()
    await page.goto(url)
    await page.getByLabel('Current document', { exact: true }).filter({ hasText: 'drawing' }).waitFor()
    await page.evaluate(({ kind, payload }) => {
      const editor = window.ringHarness
      const pending = kind === 'paste' ? editor.pasteFragment('drawing', payload, { x: 0, y: 0 })
        : editor.pasteSceneGraphics('drawing', payload, { x: 0, y: 0 })
      void pending.catch((failure) => { window.expectedMutationFailure = failure.code })
    }, { kind, payload })
    await page.getByLabel('Pending ring', { exact: true }).filter({ hasText: 'true' }).waitFor()
    await page.evaluate(async () => { await window.ringHarness.close(false); await window.ringHarness.close(true); await window.ringHarness.open('other') })
    assert.deepEqual(navigation, [], `${kind}: direct navigation stays blocked`)
    assert.equal(await page.getByLabel('Current document', { exact: true }).textContent(), 'drawing')
    const refusal = await page.evaluate(async () => {
      try { await window.ringHarness.checkClipboardPaste(); return 'unexpected success' }
      catch (failure) { return failure.code }
    })
    assert.equal(refusal, `clipboard_${kind}_unconfirmed`, 'an unrelated refusal cannot prove what a lost attempt did')
    await page.evaluate(() => window.ringHarness.close(false))
    assert.deepEqual(navigation, [], `${kind}: an unrelated refusal must not release navigation`)
    const conflict = await page.evaluate(async () => {
      try { await window.ringHarness.checkClipboardPaste(); return 'unexpected success' }
      catch (failure) { return failure.code }
    })
    assert.equal(conflict, `clipboard_${kind}_reconcile`, 'a terminal revision conflict requires an explicit authoritative reread')
    await page.evaluate(() => window.ringHarness.reconcileClipboardPaste())
    await page.getByLabel('Pending ring', { exact: true }).filter({ hasText: 'false' }).waitFor()
    assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0], bodies[0]])
    assert.equal(await page.evaluate(() => window.ringHarness.current.document.revision), 4)
    await page.evaluate(() => window.ringHarness.close(false))
    assert.deepEqual(navigation, ['/api/v1/documents/drawing/close'])
    assert.equal(await page.getByLabel('Current document', { exact: true }).textContent(), 'none')
  }

  // A Close already queued behind a successful Open must not close the new document.
  resetRequests()
  droppedBeforeDispatch = 0
  await page.goto(url)
  await page.getByLabel('Current document', { exact: true }).filter({ hasText: 'drawing' }).waitFor()
  await page.evaluate(() => Promise.all([window.ringHarness.open('other'), window.ringHarness.close(false)]))
  assert.deepEqual(navigation, ['/api/v1/documents/other'])
  assert.equal(await page.getByLabel('Current document', { exact: true }).textContent(), 'other')
  assert.deepEqual(errors, [])
  console.log('Ring canvas, navigation guards, outcome recovery and history checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
