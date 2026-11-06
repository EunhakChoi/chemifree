// Real graphics panel, editor hook and client in Chromium. Only HTTP and clipboard
// boundaries are scripted; this does not prove OS permission grants or backend behavior.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const fixture = `
import React, { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { SceneClipboardPanel } from '/src/components/scene-clipboard-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  const [outcome, setOutcome] = useState('')
  useEffect(() => { void editor.open('alpha') }, [])
  window.sceneHarness = {
    editor,
    select: (ids) => editor.setSelection(new Set(ids)),
    queuePaste: (payload) => {
      setOutcome('waiting')
      const offset = { x: 11, y: -13 }
      editor.setSelection(new Set(['a1', 'b1', 's6']))
      void editor.apply(() => [{ op: 'rename', title: 'Before queued paste' }])
      void editor.pasteSceneGraphics('alpha', payload, offset).then(() => setOutcome('pasted'), (error) => setOutcome(error.code))
      offset.x = 900
      editor.setSelection(new Set(['a2']))
    },
    queueCopy: (missing) => {
      setOutcome('waiting')
      editor.setSelection(new Set([missing ? 's6' : 's3']))
      void editor.apply(() => missing ? [{ op: 'delete_objects', objects: ['s6'], mode: 'subtree' }] : [{ op: 'rename', title: 'Queued scene' }])
      void editor.copySceneGraphics('alpha').then(() => setOutcome('copied'), (error) => setOutcome(error.code))
      editor.setSelection(new Set(['a1']))
    },
  }
  return <>
    <output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    <output aria-label="Hook outcome">{outcome}</output>
    {editor.current && <SceneClipboardPanel key={editor.current.document.id} document={editor.current.document} editor={editor} />}
  </>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{
    name: 'scene-clipboard-check-fixture',
    configureServer(vite) {
      vite.middlewares.use('/scene-clipboard-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/scene-clipboard-check', '<!doctype html><html><head><title>Scene clipboard check</title></head><body><div id="root"></div><script type="module" src="/scene-clipboard-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/scene-clipboard-harness.tsx') return id },
    load(id) { if (id === '/scene-clipboard-harness.tsx') return fixture },
  }],
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
  const pageErrors = []
  page.on('pageerror', (error) => pageErrors.push(error.message))
  const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
  function snapshot(id) {
    const atoms = [0, 30].map((x, index) => ({ id: `a${index + 1}`, element: 'C', kind: 'element', query: null, charge: 0, isotope: null,
      hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y: 0 } }))
    return { api_version: 'v1', document: { id, title: id, revision: 3, atoms,
      bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null }],
      stereo: { atoms: [], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings,
      scene: { pages: [{ id: 'p1' }], instances: ['s1', 's2'].map((instance, index) => ({ id: instance, page: 'p1', molecule: 'm1',
        positions: atoms.map((atom) => ({ atom: atom.id, x: atom.position.x + index * 100, y: 0 })) })),
        roots: ['s1', 's2', 's3', 's6'], objects: [
          { kind: 'instance', id: 's1', parent: null, molecule: 'm1' },
          { kind: 'instance', id: 's2', parent: null, molecule: 'm1' },
          { kind: 'group', id: 's3', parent: null, children: ['s5', 's4'] },
          { kind: 'graphic', id: 's5', parent: 's3', shape: 'arrow', points: [{ x: 10, y: 30 }, { x: 40, y: 30 }] },
          { kind: 'graphic', id: 's4', parent: 's3', shape: 'line', points: [{ x: 10, y: 20 }, { x: 40, y: 20 }] },
          { kind: 'graphic', id: 's6', parent: null, shape: 'rectangle', points: [{ x: 50, y: 10 }, { x: 70, y: 40 }] },
        ], references: [
          { source: 's4', target: { kind: 'object', id: 's5' } },
          { source: 's5', target: { kind: 'object', id: 's4' } },
          { source: 's5', target: { kind: 'object', id: 's5' } },
        ] }, selection: { objects: [], atoms: [], bonds: [] } },
      session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null, notes: [] }
  }
  const documents = { alpha: snapshot('alpha'), beta: snapshot('beta') }
  const operations = []
  const mutations = []
  const replies = new Map()
  let history
  let failures = 0
  let serviceToken = 'scene-service'
  let rejectInspection = false
  let holdPaste = false
  let releasePaste
  let releaseMutation
  let mutationGate = gate()
  const pasteGate = gate()
  function payloadFor(source, selected) {
    const byId = new Map(source.document.scene.objects.map((object) => [object.id, object]))
    const roots = []
    const objects = []
    const visit = (id, parentIncluded) => {
      const object = byId.get(id)
      const included = parentIncluded || selected.includes(id)
      if (included && !parentIncluded) roots.push(id)
      if (included) {
        const { parent: _parent, ...entry } = object
        objects.push(entry)
      }
      if (object.kind === 'group') object.children.forEach((child) => visit(child, included))
    }
    source.document.scene.roots.forEach((root) => visit(root, false))
    const included = new Set(objects.map((object) => object.id))
    return { format: 'chemifree.clipboard', version: 2, kind: 'graphics', settings, roots, objects,
      references: source.document.scene.references.filter((reference) => included.has(reference.source)) }
  }
  function envelope(request, result) {
    return { envelope: 'operation_result', contract: 1, request_id: request.request_id, operation: request.operation,
      profile: 'preserve-semantics', document_revision: result.snapshot.document.revision, model_id: null,
      outcome: 'ok', diagnostics: [], loss_report: [], result }
  }
  const reply = (route, value) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(value) })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return reply(route, { header: 'x-chemifree-session', token: serviceToken })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return reply(route, { api_version: 'v1', documents: [], unreadable: [] })
    const documentMatch = path.match(/^\/api\/v1\/documents\/(alpha|beta)$/)
    if (documentMatch) return reply(route, documents[documentMatch[1]])
    if (path.endsWith('/undo') || path.endsWith('/redo')) {
      assert.ok(history)
      const undo = path.endsWith('/undo')
      const next = structuredClone(undo ? history.before : history.after)
      next.document.revision = documents.alpha.document.revision + 1
      next.session.saved_revision = next.document.revision
      next.session.undo_depth = undo ? 0 : 1
      next.session.redo_depth = undo ? 1 : 0
      documents.alpha = next
      return reply(route, next)
    }
    if (path.endsWith('/transactions')) {
      const body = route.request().postDataJSON()
      mutations.push(body)
      await new Promise((resolve) => { releaseMutation = resolve; mutationGate.release() })
      const next = structuredClone(documents.alpha)
      next.document.revision += 1
      next.session.saved_revision = next.document.revision
      if (body.commands[0].op === 'rename') next.document.title = body.commands[0].title
      else {
        assert.deepEqual(body.commands, [{ op: 'delete_objects', objects: ['s6'], mode: 'subtree' }])
        next.document.scene.roots = next.document.scene.roots.filter((id) => id !== 's6')
        next.document.scene.objects = next.document.scene.objects.filter((object) => object.id !== 's6')
      }
      documents.alpha = next
      return reply(route, next)
    }
    assert.equal(path, '/api/v1/operations', `unexpected request ${path}`)
    const request = route.request().postDataJSON()
    operations.push(request)
    if (request.operation === 'chem.identity' && rejectInspection) return route.fulfill({ status: 401, contentType: 'application/json',
      body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'The service restarted.' }) })
    if (request.operation === 'document.clipboard_scene_copy') {
      const source = structuredClone(documents[request.parameters.document_id])
      const payload = payloadFor(source, request.parameters.selection.objects)
      return reply(route, envelope(request, { snapshot: source, copied: { objects: payload.objects.map((object) => object.id), atoms: [], bonds: [] }, roots: payload.roots, payload: JSON.stringify(payload) }))
    }
    assert.equal(request.operation, 'document.clipboard_scene_paste')
    let confirmed = replies.get(request.request_id)
    if (!confirmed) {
      const before = structuredClone(documents[request.parameters.document_id])
      assert.equal(request.parameters.expected_revision, before.document.revision)
      before.document.selection = request.parameters.selection_before
      const next = structuredClone(before)
      const payload = JSON.parse(request.parameters.payload)
      const start = Math.max(...next.document.scene.objects.map((object) => Number(object.id.slice(1)))) + 1
      const mapping = payload.objects.map((object, index) => ({ source: object.id, target: `s${start + index}` }))
      const targets = new Map(mapping.map((entry) => [entry.source, entry.target]))
      const parents = new Map(payload.objects.filter((object) => object.kind === 'group').flatMap((group) => group.children.map((child) => [child, group.id])))
      const roots = payload.roots.map((id) => targets.get(id))
      next.document.scene.roots.push(...roots)
      next.document.scene.objects.push(...payload.objects.map((object) => ({ ...object, id: targets.get(object.id), parent: targets.get(parents.get(object.id)) ?? null,
        ...(object.kind === 'group' ? { children: object.children.map((child) => targets.get(child)) }
          : { points: object.points.map((point) => ({ x: point.x + request.parameters.offset.x, y: point.y + request.parameters.offset.y })) }) })))
      next.document.scene.references.push(...payload.references.map((reference) => ({ source: targets.get(reference.source), target: { kind: 'object', id: targets.get(reference.target.id) } })))
      next.document.selection = { objects: roots, atoms: [], bonds: [] }
      next.document.revision += 1
      next.session.saved_revision = next.document.revision
      next.session.undo_depth = 1
      next.session.redo_depth = 0
      documents[request.parameters.document_id] = next
      history = { before, after: structuredClone(next) }
      confirmed = envelope(request, { snapshot: next, mapping: { objects: mapping } })
      replies.set(request.request_id, structuredClone(confirmed))
    }
    if (failures > 0) { failures -= 1; return route.abort('failed') }
    if (holdPaste) { releasePaste = () => reply(route, confirmed); pasteGate.release(); return }
    return reply(route, confirmed)
  })
  await page.addInitScript(() => {
    window.sceneClipboard = { reads: [], writes: [], denyRead: false, denyWrite: false, deferredRead: false }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText() {
        const state = window.sceneClipboard
        state.reads.push(navigator.userActivation.isActive)
        if (state.denyRead) return Promise.reject(new DOMException('Denied', 'NotAllowedError'))
        if (state.deferredRead) return new Promise((resolve) => { state.finishRead = resolve })
        return Promise.resolve('')
      },
      writeText(text) {
        const state = window.sceneClipboard
        state.writes.push({ activated: navigator.userActivation.isActive, text })
        return state.denyWrite ? Promise.reject(new DOMException('Denied', 'NotAllowedError')) : Promise.resolve()
      },
    } })
  })
  await page.goto(`http://127.0.0.1:${address.port}/scene-clipboard-check`)
  await page.getByLabel('Current document').filter({ hasText: 'alpha' }).waitFor()
  // The hook must send the entire mixed selection. This boundary deliberately offers
  // an invalid successful forest reply; the real client rejects it instead of filtering.
  assert.equal(await page.evaluate(async () => {
    window.sceneHarness.select(['s3', 'a1'])
    return window.sceneHarness.editor.copySceneGraphics('alpha').then(() => 'unexpected', (error) => error.code)
  }), 'incompatible_response')
  assert.deepEqual(operations.at(-1).parameters.selection, { objects: ['s3'], atoms: ['a1'], bonds: [] })
  await page.evaluate(() => window.sceneHarness.select(['s3', 's4']))
  await page.getByRole('button', { name: 'Copy graphics…', exact: true }).click()
  const text = page.getByRole('textbox', { name: 'Native graphics text', exact: true })
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  const payload = await text.inputValue()
  assert.deepEqual(JSON.parse(payload).objects.map((object) => object.id), ['s3', 's5', 's4'])
  assert.match(await page.getByRole('dialog').innerText(), /1 roots, 3 objects, revision 3/)
  assert.deepEqual(await page.evaluate(() => window.sceneClipboard), { reads: [], writes: [], denyRead: false, denyWrite: false, deferredRead: false })
  await page.getByRole('button', { name: 'Select native graphics text', exact: true }).click()
  assert.deepEqual(await text.evaluate((element) => [element.selectionStart, element.selectionEnd]), [0, payload.length])
  await page.evaluate(() => { window.sceneClipboard.denyWrite = true })
  await page.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Select and copy the graphics text manually.' }).waitFor()
  assert.equal(await text.inputValue(), payload)
  await page.evaluate(() => { window.sceneClipboard.denyWrite = false })
  await page.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await page.getByRole('status').filter({ hasText: 'Native graphics text copied to clipboard.' }).waitFor()
  assert.deepEqual(await page.evaluate(() => window.sceneClipboard.writes), [{ activated: true, text: payload }, { activated: true, text: payload }])
  await page.getByRole('button', { name: 'Close', exact: true }).click()

  // No active-placement prop exists; the two-placement document permits graphics paste.
  await page.getByRole('button', { name: 'Paste graphics…', exact: true }).click()
  assert.equal(await text.inputValue(), '')
  assert.deepEqual(await page.evaluate(() => window.sceneClipboard.reads), [])
  assert.equal(await page.getByRole('textbox', { name: 'Offset x (pt)' }).inputValue(), '30')
  await page.evaluate(() => { window.sceneClipboard.denyRead = true })
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Paste native graphics text manually.' }).waitFor()
  assert.deepEqual(await page.evaluate(() => window.sceneClipboard.reads), [true])
  await text.fill('é'.repeat(32769))
  await page.getByText('Native graphics text exceeds 64 KiB.', { exact: true }).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Paste graphics', exact: true }).isDisabled(), true)
  await text.fill(payload)
  await page.getByRole('textbox', { name: 'Offset x (pt)' }).fill('5')
  await page.getByRole('textbox', { name: 'Offset y (pt)' }).fill('')
  assert.equal(await page.getByRole('button', { name: 'Paste graphics', exact: true }).isDisabled(), true)
  await page.setViewportSize({ width: 360, height: 480 })
  const bounds = await page.getByRole('dialog').evaluate((element) => {
    const rect = element.getBoundingClientRect()
    return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
  })
  assert.ok(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= 360 && bounds.bottom <= 480, JSON.stringify(bounds))
  assert.ok((await text.boundingBox()).height <= 192)
  await page.setViewportSize({ width: 1280, height: 720 })
  await page.getByRole('textbox', { name: 'Offset y (pt)' }).fill('-7')
  const chemicalBefore = structuredClone({ atoms: documents.alpha.document.atoms, bonds: documents.alpha.document.bonds, instances: documents.alpha.document.scene.instances })
  await page.evaluate(() => window.sceneHarness.select(['a1', 'b1', 's6']))
  failures = 2
  await page.getByRole('button', { name: 'Paste graphics', exact: true }).click()
  await page.getByRole('button', { name: 'Check graphics paste outcome', exact: true }).waitFor()
  const attempts = operations.filter((request) => request.operation === 'document.clipboard_scene_paste')
  assert.equal(attempts.length, 2)
  assert.deepEqual(attempts[0], attempts[1])
  assert.equal(attempts[0].parameters.payload, payload)
  assert.deepEqual(attempts[0].parameters.offset, { x: 5, y: -7 })
  assert.deepEqual(attempts[0].parameters.selection_before, { objects: ['s6'], atoms: ['a1'], bonds: ['b1'] })
  await page.evaluate(async () => { window.sceneHarness.select(['s1']); await window.sceneHarness.editor.apply([{ op: 'rename', title: 'Blocked' }]) })
  assert.equal(mutations.length, 0, 'unconfirmed graphics paste must block programmatic queued edits')
  const transportBeforeCheck = await page.evaluate(() => ({ reads: window.sceneClipboard.reads.length, writes: window.sceneClipboard.writes.length }))
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  const recovery = page.getByRole('region', { name: 'Scene graphics clipboard', exact: true })
  await recovery.getByRole('button', { name: 'Check graphics paste outcome', exact: true }).click()
  await page.waitForFunction(() => !window.sceneHarness.editor.clipboardPending)
  assert.deepEqual(await page.evaluate(() => ({ reads: window.sceneClipboard.reads.length, writes: window.sceneClipboard.writes.length })), transportBeforeCheck)
  assert.deepEqual(operations.filter((request) => request.operation === 'document.clipboard_scene_paste')[2], attempts[0])
  assert.equal(documents.alpha.document.scene.objects.length, 9, 'replay must not duplicate the forest')
  assert.deepEqual(documents.alpha.document.scene.roots, ['s1', 's2', 's3', 's6', 's7'])
  assert.deepEqual(documents.alpha.document.scene.objects.find((object) => object.id === 's8').points, [{ x: 15, y: 23 }, { x: 45, y: 23 }])
  assert.deepEqual({ atoms: documents.alpha.document.atoms, bonds: documents.alpha.document.bonds, instances: documents.alpha.document.scene.instances }, chemicalBefore)
  assert.deepEqual(await page.evaluate(() => [...window.sceneHarness.editor.selection]), ['s7'])
  await page.evaluate(() => window.sceneHarness.editor.undo())
  assert.deepEqual(await page.evaluate(() => [...window.sceneHarness.editor.selection].sort()), ['a1', 'b1', 's6'])
  await page.evaluate(() => window.sceneHarness.editor.redo())
  assert.deepEqual(await page.evaluate(() => [...window.sceneHarness.editor.selection]), ['s7'])
  assert.equal(await page.evaluate(() => window.sceneClipboard.writes.length), 2)

  // Queued paste freezes the caller's mutable offset and mixed selection while using
  // the revision produced by the earlier edit. Later live selection does not retarget it.
  const queuedRevision = documents.alpha.document.revision + 1
  await page.evaluate((value) => window.sceneHarness.queuePaste(value), payload)
  await mutationGate.ready
  releaseMutation()
  releaseMutation = undefined
  mutationGate = gate()
  await page.getByLabel('Hook outcome').filter({ hasText: 'pasted' }).waitFor()
  const queuedPaste = operations.filter((request) => request.operation === 'document.clipboard_scene_paste').at(-1)
  assert.equal(queuedPaste.parameters.expected_revision, queuedRevision)
  assert.equal(queuedPaste.parameters.payload, payload)
  assert.deepEqual(queuedPaste.parameters.offset, { x: 11, y: -13 })
  assert.deepEqual(queuedPaste.parameters.selection_before, { objects: ['s6'], atoms: ['a1'], bonds: ['b1'] })

  // A held real transaction makes press-time target capture and latest revision deterministic.
  await page.evaluate(() => window.sceneHarness.queueCopy(false))
  await page.waitForFunction(() => window.sceneHarness.editor.busy)
  await mutationGate.ready
  releaseMutation()
  releaseMutation = undefined
  mutationGate = gate()
  await page.getByLabel('Hook outcome').filter({ hasText: 'copied' }).waitFor()
  const copied = operations.filter((request) => request.operation === 'document.clipboard_scene_copy').at(-1)
  assert.deepEqual(copied.parameters.selection, { objects: ['s3'], atoms: [], bonds: [] })
  assert.equal(documents.alpha.document.title, 'Queued scene')
  const copyCount = operations.filter((request) => request.operation === 'document.clipboard_scene_copy').length
  await page.evaluate(() => window.sceneHarness.queueCopy(true))
  await mutationGate.ready
  releaseMutation()
  releaseMutation = undefined
  await page.getByLabel('Hook outcome').filter({ hasText: 'clipboard_scene_selection_unknown' }).waitFor()
  assert.equal(operations.filter((request) => request.operation === 'document.clipboard_scene_copy').length, copyCount, 'a missing captured target must lapse the whole copy')

  // A committed old-process reply held across restart cannot settle the new process.
  await page.getByRole('button', { name: 'Paste graphics…', exact: true }).click()
  await text.fill(payload)
  const restartedDrawing = structuredClone(documents.alpha)
  restartedDrawing.document.title = 'Restarted scene'
  holdPaste = true
  await page.getByRole('button', { name: 'Paste graphics', exact: true }).click()
  await pasteGate.ready
  documents.alpha = restartedDrawing
  serviceToken = 'new-scene-service'
  rejectInspection = true
  assert.equal(await page.evaluate(() => window.sceneHarness.editor.inspectIdentity('alpha', 'recorded', null).then(() => 'unexpected', (error) => error.code)), 'service_restarted')
  await releasePaste()
  holdPaste = false
  await page.getByRole('button', { name: 'Reload document to reconcile', exact: true }).waitFor()
  assert.equal(await page.evaluate(() => window.sceneHarness.editor.clipboardPending), true)
  assert.equal(await page.evaluate(() => window.sceneHarness.editor.current.document.title), 'Queued scene')
  const pasteCount = operations.filter((request) => request.operation === 'document.clipboard_scene_paste').length
  await page.getByRole('button', { name: 'Reload document to reconcile', exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  assert.equal(await page.evaluate(() => window.sceneHarness.editor.current.document.title), 'Restarted scene')
  assert.equal(operations.filter((request) => request.operation === 'document.clipboard_scene_paste').length, pasteCount)

  // A delayed clipboard result cannot populate the dialog for another document.
  await page.evaluate(() => { window.sceneClipboard.denyRead = false; window.sceneClipboard.deferredRead = true })
  await page.getByRole('button', { name: 'Paste graphics…', exact: true }).click()
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.waitForFunction(() => typeof window.sceneClipboard.finishRead === 'function')
  await page.evaluate(() => window.sceneHarness.editor.open('beta'))
  await page.getByLabel('Current document').filter({ hasText: 'beta' }).waitFor()
  await page.getByRole('button', { name: 'Paste graphics…', exact: true }).click()
  await page.evaluate(() => window.sceneClipboard.finishRead('late text for old document'))
  assert.equal(await text.inputValue(), '')
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  assert.deepEqual(pageErrors, [])
  console.log('Scene clipboard explicit permissions, bounded dialogs, queue capture, replay, history and restart checks passed.')
} finally {
  await browser?.close()
  await server.close()
}

// Route-owned promises make queue ordering explicit without timing sleeps.
function gate() {
  let release
  const ready = new Promise((resolve) => { release = resolve })
  return { ready, release }
}
