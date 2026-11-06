// Real panel + editor hook + client in Chromium. Only HTTP and browser clipboard boundaries
// are scripted. No chemical engine, browser permission grant, or OS clipboard is simulated.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const root = fileURLToPath(new URL('../apps/web', import.meta.url))

/** Keep clipping strict and include enough CSS/geometry evidence to diagnose CI failures. */
async function assertDialogFits(page, message) {
  const geometry = await page.getByRole('dialog').evaluate((element) => {
    const rect = element.getBoundingClientRect()
    const style = getComputedStyle(element)
    return {
      left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom,
      width: rect.width, height: rect.height, viewportWidth: innerWidth, viewportHeight: innerHeight,
      maxHeight: style.maxHeight, transitionProperty: style.transitionProperty,
      transitionDuration: style.transitionDuration, animationName: style.animationName,
      animationDuration: style.animationDuration,
    }
  })
  assert.ok(geometry.left >= 0 && geometry.top >= 0 && geometry.right <= geometry.viewportWidth && geometry.bottom <= geometry.viewportHeight,
    `${message}: ${JSON.stringify(geometry)}`)
}

const fixture = `
import React, { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { NativeClipboardPanel } from '/src/components/native-clipboard-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  const [placement, setPlacement] = useState('s1')
  const [outcome, setOutcome] = useState('')
  useEffect(() => { void editor.open('alpha') }, [])
  window.clipboardHarness = {
    editor,
    select: (ids) => editor.setSelection(new Set(ids)),
    setPlacement,
    queueCapture: () => {
      void editor.apply(() => [{op:'rename',title:'Queued rename'}])
      void editor.copyFragment('alpha', 's2').then(() => setOutcome('copied'), (error) => setOutcome(error.code))
      editor.setSelection(new Set(['a2']))
      setPlacement('s1')
    },
    queueStale: () => {
      void editor.open('beta')
      void editor.copyFragment('alpha', 's1').then(() => setOutcome('wrong-copy'), (error) => setOutcome(error.code))
    },
  }
  return <>
    <output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    <output aria-label="Hook outcome">{outcome}</output>
    {editor.current && <NativeClipboardPanel key={editor.current.document.id} document={editor.current.document} editor={editor} activePlacement={placement} />}
  </>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root,
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{
    name: 'native-clipboard-check-fixture',
    configureServer(vite) {
      vite.middlewares.use('/native-clipboard-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/native-clipboard-check', '<!doctype html><html><head><title>Native clipboard check</title></head><body><div id="root"></div><script type="module" src="/native-clipboard-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/native-clipboard-harness.tsx') return id },
    load(id) { if (id === '/native-clipboard-harness.tsx') return fixture },
  }],
})
let browser
try {
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  const base = `http://127.0.0.1:${address.port}`
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const context = await browser.newContext()
  await keepRequestInterception(context)
  const page = await context.newPage()
  const pageErrors = []
  page.on('pageerror', (error) => pageErrors.push(error.message))
  const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
  const atom = (id, x) => ({ id, element: 'C', kind: 'element', query: null, charge: 0, isotope: null,
    hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y: 0 } })
  function snapshot(id, multiple) {
    const positions = [{ atom: 'a1', x: 0, y: 0 }, { atom: 'a2', x: 30, y: 0 }]
    const instances = [{ id: 's1', page: 'p1', molecule: 'm1', positions }]
    if (multiple) instances.push({ id: 's2', page: 'p1', molecule: 'm1', positions: [{ atom: 'a1', x: 100, y: 20 }, { atom: 'a2', x: 130, y: 20 }] })
    return { api_version: 'v1', document: { id, title: id, revision: 3,
      atoms: [atom('a1', 0), atom('a2', 30)], bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null }],
      stereo: { atoms: [], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings,
      scene: { pages: [{ id: 'p1' }], instances, roots: instances.map((item) => item.id), objects: instances.map((item) => ({ kind: 'instance', id: item.id, parent: null, molecule: 'm1' })), references: [] },
      selection: { objects: [], atoms: [], bonds: [] } },
      session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null, notes: [] }
  }
  const documents = { alpha: snapshot('alpha', true), beta: snapshot('beta', false), gamma: snapshot('gamma', false) }
  const operations = []
  const cutRequests = []
  const cutReplies = new Map()
  let cutHistory
  let cutFails = 0
  let heldCopy = null
  let delayCopy = false
  let copyReceived
  const copyReady = new Promise((resolve) => { copyReceived = resolve })
  let renameRelease
  let renameReceived
  const renameReady = new Promise((resolve) => { renameReceived = resolve })
  let pasteFails = 0
  let pasteConflict = false
  let transactionsReceived = 0
  let readsReceived = 0
  let serviceToken = 'clipboard-service'
  let rejectInspection = false
  let holdPaste = false
  let heldPaste
  let pasteReceived
  const pasteReady = new Promise((resolve) => { pasteReceived = resolve })
  const pastedReplies = new Map()
  function nativePayload(source, selected, instance) {
    return JSON.stringify({ format: 'chemifree.clipboard', version: 1, settings,
      atoms: source.document.atoms.filter((item) => selected.includes(item.id)).map((item) => ({
        id: item.id, kind: { type: 'element', symbol: item.element }, charge: item.charge, isotope: item.isotope,
        hydrogens: item.hydrogens, radical: item.radical, attachment_points: item.attachment_points,
        position: source.document.scene.instances.find((item) => item.id === instance).positions.find((entry) => entry.atom === item.id),
      })).map((item) => ({ ...item, position: { x: item.position.x, y: item.position.y } })),
      bonds: source.document.bonds.filter((bond) => selected.includes(bond.begin) && selected.includes(bond.end)), stereo: [] })
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
    const documentMatch = path.match(/^\/api\/v1\/documents\/(alpha|beta|gamma)$/)
    if (documentMatch) { readsReceived += 1; return reply(route, documents[documentMatch[1]]) }
    if (path.endsWith('/undo') || path.endsWith('/redo')) {
      assert.ok(cutHistory, 'history requires an actual committed cut')
      const undo = path.endsWith('/undo')
      const next = structuredClone(undo ? cutHistory.before : cutHistory.after)
      next.document.revision = documents.gamma.document.revision + 1
      next.session.saved_revision = next.document.revision
      next.session.undo_depth = undo ? 0 : 1
      next.session.redo_depth = undo ? 1 : 0
      documents.gamma = next
      return reply(route, next)
    }
    if (path.endsWith('/transactions')) {
      const body = route.request().postDataJSON()
      if (body.commands[0]?.op === 'delete_atoms') {
        cutRequests.push(body)
        let confirmed = cutReplies.get(body.request_id)
        if (!confirmed) {
          assert.equal(body.expected_revision, documents.gamma.document.revision)
          assert.deepEqual(body.commands, [{ op: 'delete_atoms', atoms: ['a1', 'a2'] }])
          const before = structuredClone(documents.gamma)
          before.document.selection = body.selection_before
          const next = structuredClone(documents.gamma)
          next.document.revision += 1
          next.document.atoms = []
          next.document.bonds = []
          next.document.scene.instances[0].positions = []
          next.document.selection = body.selection_after
          next.session.saved_revision = next.document.revision
          next.session.undo_depth = 1
          next.session.redo_depth = 0
          documents.gamma = next
          cutHistory = { before, after: structuredClone(next) }
          confirmed = structuredClone(next)
          cutReplies.set(body.request_id, confirmed)
        }
        if (cutFails > 0) { cutFails -= 1; return route.abort('failed') }
        return reply(route, confirmed)
      }
      transactionsReceived += 1
      renameReceived()
      await new Promise((resolve) => { renameRelease = resolve })
      documents.alpha.document.title = 'Queued rename'
      documents.alpha.document.revision += 1
      documents.alpha.session.saved_revision += 1
      return reply(route, documents.alpha)
    }
    assert.equal(path, '/api/v1/operations', `unexpected request ${path}`)
    const request = route.request().postDataJSON()
    operations.push(request)
    if (request.operation === 'chem.identity' && rejectInspection) return route.fulfill({
      status: 401, contentType: 'application/json', body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'The service restarted.' }),
    })
    if (request.operation === 'document.clipboard_copy') {
      const source = structuredClone(documents[request.parameters.document_id])
      const selected = [...new Set([...request.parameters.selection.atoms,
        ...source.document.bonds.filter((bond) => request.parameters.selection.bonds.includes(bond.id)).flatMap((bond) => [bond.begin, bond.end])])]
      const copiedBonds = source.document.bonds.filter((bond) => selected.includes(bond.begin) && selected.includes(bond.end)).map((bond) => bond.id)
      const boundaryBonds = source.document.bonds.filter((bond) => selected.includes(bond.begin) !== selected.includes(bond.end)).map((bond) => bond.id)
      const result = envelope(request, { snapshot: source, instance: request.parameters.instance,
        copied: { objects: [], atoms: selected, bonds: copiedBonds }, boundary_bonds: boundaryBonds,
        payload: nativePayload(source, selected, request.parameters.instance) })
      if (delayCopy) { heldCopy = () => reply(route, result); copyReceived(); return }
      return reply(route, result)
    }
    assert.equal(request.operation, 'document.clipboard_paste')
    if (pasteConflict && pasteFails > 0) { pasteFails -= 1; return route.abort('failed') }
    if (pasteConflict) return route.fulfill({ status: 409, contentType: 'application/json', body: JSON.stringify({
      api_version: 'v1', code: 'revision_conflict', message: 'The replay is no longer available; drawing changed.', current_revision: 4,
    }) })
    let confirmed = pastedReplies.get(request.request_id)
    if (!confirmed) {
      const next = structuredClone(documents.beta)
      next.document.revision = request.parameters.expected_revision + 1
      const source = JSON.parse(request.parameters.payload).atoms[0]
      const target = `a${next.document.atoms.length + 1}`
      const position = { x: source.position.x + request.parameters.offset.x, y: source.position.y + request.parameters.offset.y }
      next.document.atoms.push({ ...atom(target, position.x), position })
      next.document.scene.instances[0].positions.push({ atom: target, ...position })
      next.document.selection = { objects: [], atoms: [target], bonds: [] }
      next.session.saved_revision = next.document.revision
      next.session.undo_depth = 1
      documents.beta = next
      confirmed = envelope(request, { snapshot: next, mapping: { atoms: [{ source: source.id, target }], bonds: [] } })
      pastedReplies.set(request.request_id, confirmed)
    }
    if (pasteFails > 0) { pasteFails -= 1; return route.abort('failed') }
    if (holdPaste) { heldPaste = () => reply(route, confirmed); pasteReceived(); return }
    return reply(route, confirmed)
  })
  await page.addInitScript(() => {
    window.clipboardAdapter = { reads: [], writes: [], denyRead: false, denyWrite: false, nextText: '', deferred: false, deferredWrite: false }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText() {
        const state = window.clipboardAdapter
        state.reads.push({ activated: navigator.userActivation.isActive })
        if (state.denyRead) return Promise.reject(new DOMException('Denied', 'NotAllowedError'))
        if (state.deferred) return new Promise((resolve) => { state.finishRead = resolve })
        return Promise.resolve(state.nextText)
      },
      writeText(text) {
        const state = window.clipboardAdapter
        state.writes.push({ activated: navigator.userActivation.isActive, text })
        if (state.denyWrite) return Promise.reject(new DOMException('Denied', 'NotAllowedError'))
        if (state.deferredWrite) return new Promise((resolve) => { state.finishWrite = resolve })
        return Promise.resolve()
      },
    } })
  })
  await page.goto(`${base}/native-clipboard-check`)
  await page.getByLabel('Current document').filter({ hasText: 'alpha' }).waitFor()
  await page.evaluate(() => { window.clipboardHarness.select(['a1']); window.clipboardHarness.setPlacement('s2') })
  await page.getByRole('button', { name: 'Copy fragment…', exact: true }).click()
  const copiedText = page.getByRole('textbox', { name: 'Native fragment text' })
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  const payload = await copiedText.inputValue()
  assert.match(await page.getByRole('dialog').innerText(), /1 atoms, 0 bonds from s2, revision 3/)
  assert.match(await page.getByRole('dialog').innerText(), /1 crossing bonds omitted/)
  assert.equal(await copiedText.getAttribute('readonly'), '')
  assert.deepEqual(await page.evaluate(() => window.clipboardAdapter.writes), [])
  await page.evaluate(() => { window.clipboardAdapter.denyWrite = true })
  await page.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Select and copy the native text manually.' }).waitFor()
  assert.equal(await copiedText.inputValue(), payload)
  assert.deepEqual(await page.evaluate(() => window.clipboardAdapter.writes), [{ activated: true, text: payload }])
  await page.evaluate(() => { window.clipboardAdapter.denyWrite = false })
  await page.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await page.getByRole('status').filter({ hasText: 'Native fragment text copied to clipboard.' }).waitFor()
  assert.deepEqual(await page.evaluate(() => window.clipboardAdapter.writes), [{ activated: true, text: payload }, { activated: true, text: payload }])
  await page.getByRole('button', { name: 'Close', exact: true }).click()

  // Opening Paste never reads clipboard; denied reads still permit manual input. Multiple
  // placements block commit with an explanation, without choosing one on the user's behalf.
  await page.getByRole('button', { name: 'Paste fragment…', exact: true }).click()
  assert.equal(await page.getByRole('textbox', { name: 'Native fragment text' }).inputValue(), '')
  assert.equal(await page.getByRole('textbox', { name: 'Offset x (pt)' }).inputValue(), '30')
  assert.deepEqual(await page.evaluate(() => window.clipboardAdapter.reads), [])
  await page.evaluate(() => { window.clipboardAdapter.denyRead = true })
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Paste native fragment text manually.' }).waitFor()
  await page.getByRole('textbox', { name: 'Native fragment text' }).fill(payload)
  assert.equal(await page.getByRole('button', { name: 'Paste fragment', exact: true }).isDisabled(), true)
  assert.match(await page.getByRole('dialog').innerText(), /exactly one placement/)
  // A narrow, short viewport must contain the dialog even with long pasted text and
  // validation messages. The body scrolls while its header Close stays reachable.
  await page.setViewportSize({ width: 360, height: 480 })
  await assertDialogFits(page, 'the paste dialog must fit immediately after narrowing the viewport')
  await page.getByRole('textbox', { name: 'Native fragment text' }).fill('native fragment text\n'.repeat(2000))
  await page.getByRole('textbox', { name: 'Offset y (pt)' }).fill('')
  await page.getByRole('textbox', { name: 'Offset y (pt)' }).press('Tab')
  assert.equal(await page.getByRole('button', { name: 'Read clipboard', exact: true }).evaluate((element) => element === document.activeElement), true)
  await assertDialogFits(page, 'the paste dialog must fit inside the narrow viewport')
  const narrowText = await page.getByRole('textbox', { name: 'Native fragment text' }).boundingBox()
  assert.ok(narrowText && narrowText.height <= 192, 'long clipboard text must scroll within a bounded textarea')
  const focusedRead = await page.getByRole('button', { name: 'Read clipboard', exact: true }).boundingBox()
  assert.ok(focusedRead && focusedRead.y >= 0 && focusedRead.y + focusedRead.height <= 480, 'keyboard focus must bring footer actions into view')
  const narrowClose = await page.getByRole('button', { name: 'Close', exact: true }).boundingBox()
  assert.ok(narrowClose && narrowClose.y >= 0 && narrowClose.y + narrowClose.height <= 480, 'Close must remain in the fixed dialog header after body scrolling')
  await page.getByRole('button', { name: 'Close', exact: true }).click()

  await page.getByRole('button', { name: 'Copy fragment…', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  await page.getByRole('button', { name: 'Select native text', exact: true }).focus()
  await page.getByRole('button', { name: 'Select native text', exact: true }).press('Enter')
  assert.equal(await page.getByRole('textbox', { name: 'Native fragment text' }).evaluate((element) => element === document.activeElement), true)
  assert.deepEqual(await page.getByRole('textbox', { name: 'Native fragment text' }).evaluate((element) => [element.selectionStart, element.selectionEnd]), [0, payload.length])
  assert.equal(await page.evaluate(() => window.clipboardAdapter.writes.length), 2, 'manual selection must not write the clipboard')
  await assertDialogFits(page, 'the copy dialog must fit the same narrow viewport')
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.setViewportSize({ width: 1280, height: 720 })

  // A late permitted clipboard read cannot populate another opening of the dialog.
  await page.evaluate(() => { window.clipboardAdapter.denyRead = false; window.clipboardAdapter.deferred = true })
  await page.getByRole('button', { name: 'Paste fragment…', exact: true }).click()
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.waitForFunction(() => typeof window.clipboardAdapter.finishRead === 'function')
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.getByRole('button', { name: 'Paste fragment…', exact: true }).click()
  await page.evaluate(() => window.clipboardAdapter.finishRead('late fragment text'))
  assert.equal(await page.getByRole('textbox', { name: 'Native fragment text' }).inputValue(), '')
  await page.getByRole('button', { name: 'Close', exact: true }).click()

  // A late source response is discarded when the copy dialog was closed.
  delayCopy = true
  await page.getByRole('button', { name: 'Copy fragment…', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('[role="dialog"]')?.textContent.includes('Preparing fragment'))
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  // Base UI keeps a closed popup mounted for its exit animation. First prove that the
  // original popup has left; then release the response and await the hook's queue settlement.
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  await copyReady
  await heldCopy()
  delayCopy = false
  heldCopy = null
  await page.waitForFunction(() => !window.clipboardHarness.editor.busy)
  assert.equal(await page.getByRole('dialog').count(), 0)
  assert.equal(await page.getByRole('textbox', { name: 'Native fragment text' }).count(), 0)

  // Real hook queue: the copy captures a1/s2 before live changes, but reads the queued
  // renamed snapshot. Holding the real HTTP transaction makes this ordering deterministic.
  await page.evaluate(() => { window.clipboardHarness.select(['a1']); window.clipboardHarness.queueCapture() })
  await renameReady
  renameRelease()
  await page.getByLabel('Hook outcome').filter({ hasText: 'copied' }).waitFor()
  const captured = operations.filter((item) => item.operation === 'document.clipboard_copy').at(-1)
  assert.deepEqual(captured.parameters.selection, { objects: [], atoms: ['a1'], bonds: [] })
  assert.equal(captured.parameters.instance, 's2')
  const copiesBefore = operations.filter((item) => item.operation === 'document.clipboard_copy').length
  await page.evaluate(() => window.clipboardHarness.queueStale())
  await page.getByLabel('Hook outcome').filter({ hasText: 'clipboard_instance_invalid' }).waitFor()
  assert.equal(operations.filter((item) => item.operation === 'document.clipboard_copy').length, copiesBefore)
  await page.getByLabel('Current document').filter({ hasText: 'beta' }).waitFor()

  // Real hook/client unknown outcome: two lost replies retain a frozen attempt. Check sends
  // exactly that request ID/body; editing or closing/reopening cannot turn it into a new paste.
  await page.getByRole('button', { name: 'Paste fragment…', exact: true }).click()
  await page.evaluate((value) => { window.clipboardAdapter.deferred = false; window.clipboardAdapter.nextText = value }, payload)
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.waitForFunction((value) => document.querySelector('textarea')?.value === value, payload)
  assert.deepEqual(await page.evaluate(() => window.clipboardAdapter.reads.at(-1)), { activated: true })
  assert.equal(operations.filter((item) => item.operation === 'document.clipboard_paste').length, 0, 'reading alone must never submit clipboard text')
  await page.getByRole('textbox', { name: 'Offset x (pt)' }).fill('')
  assert.equal(await page.getByRole('button', { name: 'Paste fragment', exact: true }).isDisabled(), true)
  await page.getByRole('textbox', { name: 'Offset x (pt)' }).fill('5')
  await page.getByRole('textbox', { name: 'Offset y (pt)' }).fill('-7')
  pasteFails = 2
  await page.getByRole('button', { name: 'Paste fragment', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Check paste outcome', exact: true }).waitFor()
  const firstAttempts = operations.filter((item) => item.operation === 'document.clipboard_paste')
  assert.equal(firstAttempts.length, 2)
  assert.equal(documents.beta.document.atoms.length, 3, 'the first paste committed before its reply was lost')
  assert.deepEqual(firstAttempts[1], firstAttempts[0])
  assert.equal(firstAttempts[0].parameters.payload, payload)
  assert.deepEqual(firstAttempts[0].parameters.offset, { x: 5, y: -7 })
  assert.equal(await page.getByRole('button', { name: 'Paste fragment', exact: true }).isDisabled(), true)
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  // Closing moves recovery to the panel. The exiting popup must not keep a duplicate
  // recovery action mounted, and its overlay must leave before the panel is interactive.
  assert.equal(await page.getByRole('button', { name: 'Check paste outcome', exact: true, includeHidden: true }).count(), 1)
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  const checking = page.getByRole('region', { name: 'Native fragment clipboard', exact: true })
    .getByRole('button', { name: 'Check paste outcome', exact: true })
  assert.equal(await checking.isEnabled(), true, 'the unresolved gate must not disable checking its original request')
  await checking.click()
  await page.waitForFunction(() => !document.body.textContent.includes('Check paste outcome'))
  const allAttempts = operations.filter((item) => item.operation === 'document.clipboard_paste')
  assert.equal(allAttempts.length, 3)
  assert.deepEqual(allAttempts[2], firstAttempts[0])
  assert.equal(documents.beta.document.atoms.length, 3)
  assert.equal(documents.beta.document.atoms[2].position.x, 105)
  assert.equal(documents.beta.document.atoms[2].position.y, 13)

  // After replay expires, a terminal conflict proves no attempt with that ID remains in
  // progress, but does not prove the original failed. Explicit reconciliation must reread
  // the authoritative snapshot, without sending another paste or fabricating a mapping.
  await page.getByRole('button', { name: 'Paste fragment…', exact: true }).click()
  await page.getByRole('textbox', { name: 'Native fragment text' }).fill(payload)
  pasteFails = 2
  pasteConflict = true
  await page.getByRole('button', { name: 'Paste fragment', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Check paste outcome', exact: true }).waitFor()
  await page.evaluate(() => window.clipboardHarness.editor.apply([{ op: 'rename', title: 'Must remain blocked' }]))
  assert.equal(transactionsReceived, 1, 'an unresolved attempt must block programmatic queued edits too')
  await page.getByRole('dialog').getByRole('button', { name: 'Check paste outcome', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Reload document to reconcile', exact: true }).waitFor()
  assert.match(await page.getByRole('dialog').innerText(), /may already be present/)
  const beforeReconcile = operations.filter((item) => item.operation === 'document.clipboard_paste')
  assert.equal(beforeReconcile.length, 6)
  assert.deepEqual(beforeReconcile[3], beforeReconcile[4])
  assert.deepEqual(beforeReconcile[4], beforeReconcile[5])
  const beforeReads = readsReceived
  await page.getByRole('dialog').getByRole('button', { name: 'Reload document to reconcile', exact: true }).click()
  await page.waitForFunction(() => !document.body.textContent.includes('Reload document to reconcile'))
  assert.equal(readsReceived, beforeReads + 1)
  assert.equal(operations.filter((item) => item.operation === 'document.clipboard_paste').length, 6)
  assert.equal(documents.beta.document.atoms.length, 3)
  assert.equal(await page.getByRole('button', { name: 'Paste fragment…', exact: true }).isEnabled(), true)

  // Hold an otherwise valid success from the original process while another real-client
  // request detects a restart. The old reply must neither replace the shown drawing nor
  // clear the unresolved gate; only explicit new-process reconciliation may release it.
  const restartedDrawing = structuredClone(documents.beta)
  restartedDrawing.document.title = 'Restarted drawing'
  pasteConflict = false
  holdPaste = true
  await page.getByRole('button', { name: 'Paste fragment…', exact: true }).click()
  await page.getByRole('textbox', { name: 'Native fragment text' }).fill(payload)
  await page.getByRole('button', { name: 'Paste fragment', exact: true }).click()
  await pasteReady
  assert.equal(documents.beta.document.atoms.length, 4, 'the held old-process success is a committed, valid paste')
  documents.beta = restartedDrawing
  serviceToken = 'restarted-clipboard-service'
  rejectInspection = true
  assert.equal(await page.evaluate(() => window.clipboardHarness.editor.inspectIdentity('beta', 'recorded', null)
    .then(() => 'unexpected success', (error) => error.code)), 'service_restarted')
  await page.waitForFunction(() => window.clipboardHarness.editor.serviceGeneration === 1)
  await heldPaste()
  await page.getByRole('dialog').getByRole('button', { name: 'Reload document to reconcile', exact: true }).waitFor()
  assert.deepEqual(await page.evaluate(() => ({
    atoms: window.clipboardHarness.editor.current.document.atoms.map((item) => item.id),
    revision: window.clipboardHarness.editor.current.document.revision,
    selection: [...window.clipboardHarness.editor.selection],
    pending: window.clipboardHarness.editor.clipboardPending,
    needsReload: window.clipboardHarness.editor.clipboardNeedsReload,
  })), { atoms: ['a1', 'a2', 'a3'], revision: 4, selection: ['a3'], pending: true, needsReload: true })
  const pasteCountBeforeRestartReconcile = operations.filter((item) => item.operation === 'document.clipboard_paste').length
  assert.equal(pasteCountBeforeRestartReconcile, 7)
  await page.getByRole('dialog').getByRole('button', { name: 'Reload document to reconcile', exact: true }).click()
  await page.waitForFunction(() => !window.clipboardHarness.editor.clipboardPending)
  assert.equal(await page.evaluate(() => window.clipboardHarness.editor.current.document.title), 'Restarted drawing')
  assert.equal(operations.filter((item) => item.operation === 'document.clipboard_paste').length, pasteCountBeforeRestartReconcile)
  // Cut eligibility is checked before touching clipboard or sending any delete. Whole
  // source components are required, with exactly one placement and no incoming references.
  const writesBeforeEligibility = await page.evaluate(() => window.clipboardAdapter.writes.length)
  await page.evaluate(async () => { await window.clipboardHarness.editor.open('alpha'); window.clipboardHarness.select(['a1', 'a2']); window.clipboardHarness.setPlacement('s1') })
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'exactly one source placement' }).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Cut to clipboard', exact: true }).isDisabled(), true)
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.evaluate(async () => { await window.clipboardHarness.editor.open('gamma'); window.clipboardHarness.select(['a1']) })
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'no crossing bonds' }).waitFor()
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  documents.gamma.document.scene.objects.push({ kind: 'graphic', id: 's2', parent: null, shape: 'line', points: [{ x: 50, y: 0 }, { x: 60, y: 0 }] })
  documents.gamma.document.scene.roots.push('s2')
  documents.gamma.document.scene.references.push({ source: 's2', target: { kind: 'atom', id: 'a1' } })
  await page.evaluate(async () => { await window.clipboardHarness.editor.reload(); window.clipboardHarness.select(['a1', 'a2']) })
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Scene references target this fragment' }).waitFor()
  assert.equal(await page.evaluate(() => window.clipboardAdapter.writes.length), writesBeforeEligibility)
  assert.equal(cutRequests.length, 0)
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  documents.gamma.document.scene.references = []
  documents.gamma.document.scene.objects.pop()
  documents.gamma.document.scene.roots.pop()
  await page.evaluate(async () => { await window.clipboardHarness.editor.reload(); window.clipboardHarness.select(['a1', 'a2']); window.clipboardAdapter.denyWrite = true })

  // An eligible Cut offers selectable text first. Selecting text or a denied explicit
  // write never authorizes deletion. The direct write still retains user activation.
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  assert.match(await page.getByRole('dialog').innerText(), /2 atoms, 1 bonds from s1/)
  const cutPayload = await page.getByRole('textbox', { name: 'Native fragment text' }).inputValue()
  await page.getByRole('button', { name: 'Select native text', exact: true }).click()
  assert.equal(await page.evaluate(() => window.clipboardAdapter.writes.length), writesBeforeEligibility)
  await page.getByRole('button', { name: 'Cut to clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'The source was not cut' }).waitFor()
  assert.deepEqual(await page.evaluate(() => window.clipboardAdapter.writes.at(-1)), { activated: true, text: cutPayload })
  assert.equal(cutRequests.length, 0)
  assert.equal(documents.gamma.document.atoms.length, 2)
  assert.equal(await page.getByRole('textbox', { name: 'Native fragment text' }).inputValue(), cutPayload)
  await page.getByRole('button', { name: 'Close', exact: true }).click()

  // The clipboard write can wait while another real-client inspection discovers a new
  // service/source. Even a successful write must then retain the source and send no delete.
  await page.evaluate(() => { window.clipboardAdapter.denyWrite = false; window.clipboardAdapter.deferredWrite = true })
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  await page.getByRole('button', { name: 'Cut to clipboard', exact: true }).click()
  await page.waitForFunction(() => typeof window.clipboardAdapter.finishWrite === 'function')
  serviceToken = 'cut-restarted-service'
  documents.gamma.document.title = 'Changed source after restart'
  assert.equal(await page.evaluate(() => window.clipboardHarness.editor.inspectIdentity('gamma', 'recorded', null)
    .then(() => 'unexpected success', (error) => error.code)), 'service_restarted')
  await page.evaluate(() => window.clipboardAdapter.finishWrite())
  await page.waitForFunction(() => !window.clipboardHarness.editor.busy)
  assert.equal(cutRequests.length, 0)
  assert.equal(documents.gamma.document.atoms.length, 2)
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.evaluate(async () => { await window.clipboardHarness.editor.reload(); window.clipboardHarness.select(['a1', 'a2']); window.clipboardAdapter.deferredWrite = false })

  // A successful explicit Cut is one delete command with the frozen history selection.
  // Undo restores that selection and graph; redo removes them again without rewriting clipboard.
  // Selecting only the bond must still cut its copied endpoints, while Undo restores
  // the original bond-only selection rather than an expanded atom selection.
  await page.evaluate(() => window.clipboardHarness.select(['b1']))
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  await page.getByRole('button', { name: 'Cut to clipboard', exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  assert.equal(cutRequests.length, 1)
  assert.deepEqual(cutRequests[0].commands, [{ op: 'delete_atoms', atoms: ['a1', 'a2'] }])
  assert.deepEqual(cutRequests[0].selection_before, { objects: [], atoms: [], bonds: ['b1'] })
  assert.deepEqual(cutRequests[0].selection_after, { objects: [], atoms: [], bonds: [] })
  assert.equal(documents.gamma.document.atoms.length, 0)
  const writesAfterCut = await page.evaluate(() => window.clipboardAdapter.writes.length)
  await page.evaluate(() => window.clipboardHarness.editor.undo())
  assert.deepEqual(await page.evaluate(() => ({
    atoms: window.clipboardHarness.editor.current.document.atoms.map((item) => item.id),
    bonds: window.clipboardHarness.editor.current.document.bonds.map((item) => item.id),
    selection: [...window.clipboardHarness.editor.selection],
  })), { atoms: ['a1', 'a2'], bonds: ['b1'], selection: ['b1'] })
  await page.evaluate(() => window.clipboardHarness.editor.redo())
  assert.equal(await page.evaluate(() => window.clipboardHarness.editor.current.document.atoms.length), 0)
  assert.equal(await page.evaluate(() => window.clipboardAdapter.writes.length), writesAfterCut)
  await page.evaluate(() => window.clipboardHarness.editor.undo())

  // Commit-and-lose both initial replies. Check cut outcome repeats the immutable delete
  // request; it must not write clipboard again or create a second cut transaction.
  cutFails = 2
  await page.getByRole('button', { name: 'Cut fragment…', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
  await page.getByRole('button', { name: 'Cut to clipboard', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Check cut outcome', exact: true }).waitFor()
  assert.equal(documents.gamma.document.atoms.length, 0)
  assert.equal(cutRequests.length, 3)
  assert.deepEqual(cutRequests[1], cutRequests[2])
  const writesBeforeCutCheck = await page.evaluate(() => window.clipboardAdapter.writes.length)
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  const checkCut = page.getByRole('button', { name: 'Check cut outcome', exact: true })
  assert.equal(await checkCut.isEnabled(), true)
  await checkCut.click()
  await page.waitForFunction(() => !window.clipboardHarness.editor.clipboardPending)
  assert.equal(cutRequests.length, 4)
  assert.deepEqual(cutRequests[3], cutRequests[1])
  assert.equal(cutReplies.size, 2, 'one original successful cut and one recovered cut, never a duplicate commit')
  assert.equal(await page.evaluate(() => window.clipboardAdapter.writes.length), writesBeforeCutCheck)
  assert.equal(await page.evaluate(() => window.clipboardHarness.editor.current.document.atoms.length), 0)
  assert.deepEqual(pageErrors, [])
  console.log('Native clipboard panel, explicit permissions, Cut eligibility and real-hook queue/replay checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
