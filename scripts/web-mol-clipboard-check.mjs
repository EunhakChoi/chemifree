// Real CT panel, clipboard recovery panel, editor hook and client. HTTP and browser
// clipboard transport are scripted; the fixture makes no claim to test a MOL codec.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const root = fileURLToPath(new URL('../apps/web', import.meta.url))
const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { CtfilePanel } from '/src/components/ctfile-panel.tsx'
import { NativeClipboardPanel } from '/src/components/native-clipboard-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.molHarness = { editor, select: (ids) => editor.setSelection(new Set(ids)) }
  return <><output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    {editor.current && <><CtfilePanel key={editor.current.document.id} document={editor.current.document} editor={editor} />
      <NativeClipboardPanel key={'native-' + editor.current.document.id} document={editor.current.document} editor={editor} activePlacement="s1" /></>}
  </>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root,
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{
    name: 'mol-clipboard-check-fixture',
    configureServer(vite) {
      vite.middlewares.use('/mol-clipboard-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/mol-clipboard-check', '<!doctype html><html><head><title>MOL clipboard check</title></head><body><div id="root"></div><script type="module" src="/mol-clipboard-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/mol-clipboard-harness.tsx') return id },
    load(id) { if (id === '/mol-clipboard-harness.tsx') return fixture },
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
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_style: 'normal', label_underline: false, label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
  const atom = (id, element, x, y = 0) => ({ id, element, kind: 'element', query: null, charge: 0, isotope: null,
    hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } })
  let state = { api_version: 'v1', document: { id: 'drawing', title: 'MOL clipboard drawing', revision: 3,
    atoms: [atom('a1', 'C', 0), atom('a2', 'C', 30)], bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null }],
    stereo: { atoms: [], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings,
    scene: { pages: [{ id: 'p1' }], instances: [{ id: 's1', page: 'p1', molecule: 'm1', positions: [{ atom: 'a1', x: 0, y: 0 }, { atom: 'a2', x: 30, y: 0 }] }],
      roots: ['s1'], objects: [{ kind: 'instance', id: 's1', parent: null, molecule: 'm1' }], references: [] }, selection: { objects: [], atoms: [], bonds: [] } },
    session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null, notes: [] }
  const wholeMol = 'Whole drawing\n  Chemifree test\n\n  2  1  0  0  0  0            999 V2000\n    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n    1.5000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n  1  2  1  0  0  0  0\nM  END\n'
  const wholeMol3000 = 'Whole drawing\n  Chemifree test\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS 2 1 0 0 0\nM  V30 BEGIN ATOM\nM  V30 1 C 0 0 0 0\nM  V30 2 C 1.5 0 0 0\nM  V30 END ATOM\nM  V30 BEGIN BOND\nM  V30 1 1 1 2\nM  V30 END BOND\nM  V30 END CTAB\nM  END\n'
  const inputMol = 'Clipboard oxygen\n  Chemifree test\n\n  1  0  0  0  0  0            999 V2000\n    1.0000    2.0000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n'
  const emptyMol = 'Empty\n  Chemifree test\n\n  0  0  0  0  0  0            999 V2000\nM  END\n'
  const importLoss = { feature: 'title', source_path: 'records[0].title', disposition: 'dropped', reason: 'Source title is not stored by append.', semantic_impact: 'document' }
  const exportLoss = { feature: 'drawing_settings', source_path: 'document.settings', disposition: 'dropped', reason: 'MOL does not carry document style.', semantic_impact: 'rendering' }
  const requests = []
  const committed = new Map()
  let importFails = 0
  let holdImport = false
  let finishImport
  let importReceived
  const heldImportReady = new Promise((resolve) => { importReceived = resolve })
  let token = 'mol-service'
  let rejectInspection = false
  const response = (route, value) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(value) })
  function envelope(request, result, losses = []) {
    return { contract: 1, envelope: 'operation_result', request_id: request.request_id, operation: request.operation,
      profile: 'preserve-semantics', outcome: losses.length ? 'partial' : 'ok', diagnostics: [], loss_report: losses,
      document_revision: state.document.revision, model_id: null, result }
  }
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return response(route, { header: 'x-chemifree-session', token })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return response(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return response(route, state)
    assert.equal(path, '/api/v1/operations')
    const request = route.request().postDataJSON()
    requests.push(request)
    if (request.operation === 'chem.identity' && rejectInspection) return route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }) })
    if (request.operation === 'document.export') {
      const digest = (request.parameters.ctab === 'v2000' ? 'a' : 'b').repeat(64)
      if (request.parameters.accepted_losses !== digest) return response(route, {
        ...envelope(request, null), outcome: 'error', diagnostics: [{ code: 'loss_consent_required', message: 'Review style omission.',
          details: { features: ['drawing_settings'], losses: [exportLoss], loss_digest: digest } }],
      })
      const ctab = request.parameters.ctab === 'v2000' ? 'v2000' : 'v3000'
      return response(route, envelope(request, { content: ctab === 'v2000' ? wholeMol : wholeMol3000, ctab, records: [{ snapshot: structuredClone(state) }] }, [exportLoss]))
    }
    assert.equal(request.operation, 'document.import')
    const parameters = request.parameters
    assert.equal(parameters.format, 'mol')
    assert.equal(parameters.record, 1)
    const empty = parameters.source === emptyMol
    if (parameters.preview) return response(route, envelope(request, {
      preview_digest: 'c'.repeat(64), selected_record: 1, snapshot: structuredClone(state), document: structuredClone(state.document),
      records: [{ title: empty ? 'Empty' : 'Clipboard oxygen', atoms: empty ? [] : [{}], bonds: [], fields: [] }],
    }, empty ? [] : [importLoss]))
    let result = committed.get(request.request_id)
    if (!result) {
      assert.equal(parameters.expected_revision, state.document.revision)
      assert.equal(parameters.preview_digest, 'c'.repeat(64))
      assert.equal(parameters.accept_loss, true)
      if (!empty) {
        state = structuredClone(state)
        state.document.revision += 1
        const id = `a${state.document.atoms.length + 1}`
        state.document.atoms.push(atom(id, 'O', 20, -40))
        state.document.scene.instances[0].positions.push({ atom: id, x: 20, y: -40 })
        state.session.saved_revision = state.document.revision
        state.session.undo_depth += 1
      }
      result = envelope(request, structuredClone(state), empty ? [] : [importLoss])
      committed.set(request.request_id, result)
    }
    if (importFails > 0) { importFails -= 1; return route.abort('failed') }
    if (holdImport) { finishImport = () => response(route, result); importReceived(); return }
    return response(route, result)
  })
  await page.addInitScript(() => {
    window.molClipboard = { reads: [], writes: [], denyRead: false, denyWrite: false, deferredRead: false, text: '' }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText() {
        const state = window.molClipboard
        state.reads.push(navigator.userActivation.isActive)
        if (state.denyRead) return Promise.reject(new DOMException('Denied', 'NotAllowedError'))
        if (state.deferredRead) return new Promise((resolve) => { state.finishRead = resolve })
        return Promise.resolve(state.text)
      },
      writeText(text) {
        window.molClipboard.writes.push({ text, activated: navigator.userActivation.isActive })
        return window.molClipboard.denyWrite ? Promise.reject(new DOMException('Denied', 'NotAllowedError')) : Promise.resolve()
      },
    } })
  })
  await page.goto(`http://127.0.0.1:${address.port}/mol-clipboard-check`)
  await page.getByLabel('Current document').filter({ hasText: 'drawing' }).waitFor()
  await page.evaluate(() => window.molHarness.select(['a1']))
  const ct = page.getByRole('complementary', { name: 'MOL and SDF', exact: true })

  // This is whole drawing export: neither live selected atom IDs nor native fragment scope
  // may leak into the export request. Loss acceptance is required before clipboard text exists.
  await ct.getByRole('button', { name: 'Preview MOL export', exact: true }).click()
  await ct.getByRole('button', { name: 'Accept these losses and preview MOL', exact: true }).waitFor()
  assert.equal(await page.getByRole('button', { name: 'MOL clipboard text…', exact: true }).count(), 0)
  assert.deepEqual(requests[0].parameters, { format: 'mol', records: [{ document_id: 'drawing' }], ctab: 'v2000' })
  assert.deepEqual(await page.evaluate(() => window.molClipboard.writes), [])
  await ct.getByRole('button', { name: 'Accept these losses and preview MOL', exact: true }).click()
  await ct.getByRole('button', { name: 'MOL clipboard text…', exact: true }).click()
  assert.equal(await page.getByRole('textbox', { name: 'Whole drawing MOL text' }).inputValue(), wholeMol)
  assert.match(await page.getByRole('dialog').innerText(), /entire drawing export.*regardless of selected atoms or bonds/)
  await page.getByRole('button', { name: 'Select MOL text', exact: true }).click()
  assert.deepEqual(await page.getByRole('textbox', { name: 'Whole drawing MOL text' }).evaluate((element) => [element.selectionStart, element.selectionEnd]), [0, wholeMol.length])
  const beforeWriteRequests = requests.length
  await page.evaluate(() => { window.molClipboard.denyWrite = true })
  await page.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Select and copy the MOL text manually' }).waitFor()
  assert.equal(requests.length, beforeWriteRequests, 'clipboard write must use prepared text without a backend await')
  assert.deepEqual(await page.evaluate(() => window.molClipboard.writes), [{ text: wholeMol, activated: true }])
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await ct.getByRole('combobox', { name: 'MOL version to write' }).selectOption('v3000')
  assert.equal(await ct.getByRole('button', { name: 'MOL clipboard text…', exact: true }).count(), 0)
  await ct.getByRole('button', { name: 'Preview MOL export', exact: true }).click()
  await ct.getByRole('button', { name: 'Accept these losses and preview MOL', exact: true }).waitFor()
  assert.deepEqual(requests.at(-1).parameters, { format: 'mol', records: [{ document_id: 'drawing' }], ctab: 'v3000' })
  await ct.getByRole('button', { name: 'Accept these losses and preview MOL', exact: true }).click()
  assert.equal(requests.at(-1).parameters.accepted_losses, 'b'.repeat(64))

  // Opening never reads clipboard or sends text. Denied permission leaves manual input;
  // changing the source text invalidates its prior digest, and the UTF-8 byte cap is local.
  const beforeOpen = requests.length
  await ct.getByRole('button', { name: 'Import MOL text…', exact: true }).click()
  assert.deepEqual(await page.evaluate(() => window.molClipboard.reads), [])
  assert.equal(requests.length, beforeOpen)
  assert.equal(await page.getByRole('textbox', { name: 'MOL text to import' }).inputValue(), '')
  await page.evaluate(() => { window.molClipboard.denyRead = true })
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Paste MOL text manually' }).waitFor()
  await page.getByRole('textbox', { name: 'MOL text to import' }).fill(inputMol)
  assert.equal(requests.length, beforeOpen)
  await page.getByRole('button', { name: 'Preview MOL text', exact: true }).click()
  await page.getByRole('button', { name: 'Import MOL text', exact: true }).waitFor()
  assert.match(await page.getByRole('dialog').innerText(), /1 atoms, 0 bonds/)
  assert.match(await page.getByRole('dialog').innerText(), /Importing accepts every loss listed above/)
  assert.match(await page.getByRole('dialog').innerText(), /Source title is not stored by append/)
  assert.deepEqual(requests.at(-1).parameters, { document_id: 'drawing', expected_revision: 3, format: 'mol', source: inputMol, record: 1, preview: true })
  await page.getByRole('textbox', { name: 'MOL text to import' }).fill(`${inputMol}\n`)
  assert.equal(await page.getByRole('button', { name: 'Import MOL text', exact: true }).count(), 0)
  await page.getByRole('textbox', { name: 'MOL text to import' }).fill('é'.repeat(102401))
  assert.equal(await page.getByRole('button', { name: 'Preview MOL text', exact: true }).isDisabled(), true)
  await page.getByRole('alert').filter({ hasText: 'up to 200 KiB of UTF-8' }).waitFor()
  await page.getByRole('button', { name: 'Close', exact: true }).click()

  // A late permitted read cannot repopulate a new dialog. No backend preview is sent by reading.
  await page.evaluate(() => { window.molClipboard.denyRead = false; window.molClipboard.deferredRead = true })
  await ct.getByRole('button', { name: 'Import MOL text…', exact: true }).click()
  await page.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.waitForFunction(() => typeof window.molClipboard.finishRead === 'function')
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await ct.getByRole('button', { name: 'Import MOL text…', exact: true }).click()
  await page.evaluate(() => window.molClipboard.finishRead('late source'))
  assert.equal(await page.getByRole('textbox', { name: 'MOL text to import' }).inputValue(), '')

  // One committed import with two lost replies. The single visible recovery action belongs
  // to the MOL panel; retrying it freezes request ID/body and preserves the live selection.
  await page.getByRole('textbox', { name: 'MOL text to import' }).fill(inputMol)
  await page.getByRole('button', { name: 'Preview MOL text', exact: true }).click()
  await page.getByRole('button', { name: 'Import MOL text', exact: true }).waitFor()
  importFails = 2
  await page.getByRole('button', { name: 'Import MOL text', exact: true }).click()
  await page.getByRole('button', { name: 'Check MOL import outcome', exact: true }).waitFor()
  const firstSends = requests.filter((request) => request.operation === 'document.import' && !request.parameters.preview)
  assert.equal(firstSends.length, 2)
  assert.deepEqual(firstSends[0], firstSends[1])
  assert.equal(firstSends[0].parameters.preview_digest, 'c'.repeat(64))
  assert.equal(firstSends[0].parameters.accept_loss, true)
  assert.equal('selection_before' in firstSends[0].parameters, false)
  assert.equal('selection_after' in firstSends[0].parameters, false)
  assert.equal(state.document.atoms.length, 3)
  assert.equal(await page.getByRole('button', { name: 'Check MOL import outcome', exact: true, includeHidden: true }).count(), 1)
  await page.evaluate(() => window.molHarness.select(['a2']))
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  const checkImport = ct.getByRole('button', { name: 'Check MOL import outcome', exact: true })
  assert.equal(await checkImport.isEnabled(), true)
  await checkImport.click()
  await page.waitForFunction(() => !window.molHarness.editor.clipboardPending)
  const allSends = requests.filter((request) => request.operation === 'document.import' && !request.parameters.preview)
  assert.equal(allSends.length, 3)
  assert.deepEqual(allSends[2], firstSends[0])
  assert.equal(committed.size, 1)
  assert.deepEqual(await page.evaluate(() => ({
    atoms: window.molHarness.editor.current.document.atoms.map((item) => item.id),
    position: window.molHarness.editor.current.document.atoms[2].position,
    selection: [...window.molHarness.editor.selection],
  })), { atoms: ['a1', 'a2', 'a3'], position: { x: 20, y: -40 }, selection: ['a2'] })

  // A valid empty first record is a no-op import: revision and live selection are retained.
  const noOpRevision = state.document.revision
  await ct.getByRole('button', { name: 'Import MOL text…', exact: true }).click()
  await page.getByRole('textbox', { name: 'MOL text to import' }).fill(emptyMol)
  await page.getByRole('button', { name: 'Preview MOL text', exact: true }).click()
  await page.getByRole('button', { name: 'Import MOL text', exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'detached' })
  assert.equal(state.document.revision, noOpRevision)
  assert.deepEqual(await page.evaluate(() => [...window.molHarness.editor.selection]), ['a2'])

  // Hold a valid committed old-process success, detect a restart through another real
  // client request, then release it. Only explicit reconciliation can show new-process state.
  const restarted = structuredClone(state)
  restarted.document.title = 'Restarted MOL drawing'
  holdImport = true
  await ct.getByRole('button', { name: 'Import MOL text…', exact: true }).click()
  await page.getByRole('textbox', { name: 'MOL text to import' }).fill(inputMol)
  await page.getByRole('button', { name: 'Preview MOL text', exact: true }).click()
  await page.getByRole('button', { name: 'Import MOL text', exact: true }).click()
  await heldImportReady
  assert.equal(state.document.atoms.length, 4)
  state = restarted
  token = 'restarted-mol-service'
  rejectInspection = true
  assert.equal(await page.evaluate(() => window.molHarness.editor.inspectIdentity('drawing', 'recorded', null)
    .then(() => 'unexpected', (error) => error.code)), 'service_restarted')
  await finishImport()
  await page.getByRole('button', { name: 'Reload document to reconcile', exact: true }).waitFor()
  assert.equal(await page.evaluate(() => window.molHarness.editor.current.document.atoms.length), 3)
  const sendsBeforeReload = requests.length
  await page.getByRole('button', { name: 'Reload document to reconcile', exact: true }).click()
  await page.waitForFunction(() => !window.molHarness.editor.clipboardPending)
  assert.equal(requests.length, sendsBeforeReload)
  assert.equal(await page.evaluate(() => window.molHarness.editor.current.document.title), 'Restarted MOL drawing')
  assert.equal(await page.evaluate(() => window.molHarness.editor.current.document.atoms.length), 3)
  assert.deepEqual(errors, [])
  console.log('MOL clipboard explicit transport, consent/version guards and real-hook recovery checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
