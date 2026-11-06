// Real SMILES panel/editor/client with HTTP and clipboard boundaries scripted. This checks
// transport, consent and recovery, not the independent SMILES parser or chemical correctness.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const root = fileURLToPath(new URL('../apps/web', import.meta.url))
const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { SmilesPanel } from '/src/components/smiles-panel.tsx'
import { NativeClipboardPanel } from '/src/components/native-clipboard-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.smilesHarness = { editor, select: (ids) => editor.setSelection(new Set(ids)) }
  return <><output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    {editor.current && <><SmilesPanel key={editor.current.document.id} document={editor.current.document} editor={editor} />
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
    name: 'smiles-clipboard-check-fixture',
    configureServer(vite) {
      vite.middlewares.use('/smiles-clipboard-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/smiles-clipboard-check', '<!doctype html><html><head><title>SMILES clipboard check</title></head><body><div id="root"></div><script type="module" src="/smiles-clipboard-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/smiles-clipboard-harness.tsx') return id },
    load(id) { if (id === '/smiles-clipboard-harness.tsx') return fixture },
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
  const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
  const atom = (id, element, x, y = 0) => ({ id, element, kind: 'element', query: null, charge: 0, isotope: null,
    hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } })
  let state = { api_version: 'v1', document: { id: 'drawing', title: 'SMILES clipboard drawing', revision: 3,
    atoms: [atom('a1', 'C', 0), atom('a2', 'F', 0, -30), atom('a3', 'Cl', 26, 15), atom('a4', 'Br', -26, 15)],
    bonds: [
      { id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null },
      { id: 'b2', begin: 'a1', end: 'a3', order: 'single', query: null, mark: null },
      { id: 'b3', begin: 'a1', end: 'a4', order: 'single', query: null, mark: null },
    ],
    stereo: { atoms: [{ atom: 'a1', configuration: { kind: 'tetrahedral', neighbors: ['a2', 'a3', 'a4', 'implicit_hydrogen'], turn: 'clockwise' } }], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings,
    scene: { pages: [{ id: 'p1' }], instances: [{ id: 's1', page: 'p1', molecule: 'm1', positions: [{ atom: 'a1', x: 0, y: 0 }, { atom: 'a2', x: 0, y: -30 }, { atom: 'a3', x: 26, y: 15 }, { atom: 'a4', x: -26, y: 15 }] }],
      roots: ['s1'], objects: [{ kind: 'instance', id: 's1', parent: null, molecule: 'm1' }], references: [] }, selection: { objects: [], atoms: [], bonds: [] } },
    session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null, notes: [] }
  const initialAtoms = structuredClone(state.document.atoms)
  const initialBonds = structuredClone(state.document.bonds)
  const initialStereo = structuredClone(state.document.stereo)
  // A supported ethanol prefix with an ignored second line and trailing whitespace.
  // The HTTP fixture supplies documented wire fields without implementing a parser.
  const tailedSource = 'CCO\nCC  \n'
  const layout = { feature: 'generated_layout', source_path: 'source', disposition: 'normalized',
    reason: 'Atoms are placed on a grid in input order below the existing drawing.', semantic_impact: 'geometry' }
  const ignored = { feature: 'ignored_text', source_path: 'source[4..=5]', disposition: 'dropped',
    reason: 'Text after the first whitespace is not read.', semantic_impact: 'document' }
  const stereoLoss = { feature: 'stereo_configuration', source_path: 'document.stereo', disposition: 'dropped',
    reason: 'Canonical output does not retain the stored configuration.', semantic_impact: 'stereo' }
  const requests = []
  const commits = new Map()
  let importFails = 0
  let holdImport = false
  let finishImport
  let importReceived
  const heldImportReady = new Promise((resolve) => { importReceived = resolve })
  let token = 'smiles-service'
  let rejectInspection = false
  const respond = (route, value) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(value) })
  function envelope(request, result, losses = [], model = 'chemifree-smiles-1') {
    return { contract: 1, envelope: 'operation_result', request_id: request.request_id, operation: request.operation,
      profile: 'preserve-semantics', outcome: losses.length ? 'partial' : 'ok', diagnostics: [], loss_report: losses,
      document_revision: state.document.revision, model_id: model, result }
  }
  function consent(request, features, losses, digest) {
    return { ...envelope(request, null), outcome: 'error', diagnostics: [{ code: 'loss_consent_required', message: 'Review the reported losses.',
      details: { features, losses, ...(digest ? { loss_digest: digest } : {}) } }] }
  }
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return respond(route, { header: 'x-chemifree-session', token })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return respond(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return respond(route, state)
    assert.equal(path, '/api/v1/operations')
    const request = route.request().postDataJSON()
    requests.push(request)
    if (request.operation === 'chem.identity' && rejectInspection) return route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }) })
    const parameters = request.parameters
    assert.equal(parameters.format, 'smiles')
    if (request.operation === 'document.export') {
      assert.equal(parameters.document_id, 'drawing')
      if (parameters.style === 'canonical' && parameters.accepted_losses !== 'd'.repeat(64)) return respond(route, consent(request, ['stereo_configuration'], [stereoLoss], 'd'.repeat(64)))
      return respond(route, envelope(request, {
        content: parameters.style === 'recorded' ? '[C@H](F)(Cl)Br' : 'FC(Cl)Br', style: parameters.style, snapshot: structuredClone(state),
      }, parameters.style === 'recorded' ? [] : [stereoLoss], parameters.style === 'recorded' ? 'chemifree-smiles-1' : 'chemifree-smiles-canonical-1'))
    }
    assert.equal(request.operation, 'document.import')
    assert.ok(parameters.source === tailedSource || parameters.source === 'N', 'only named supported fixtures reach the mock service')
    const tailed = parameters.source === tailedSource
    const losses = tailed ? [layout, ignored] : [layout]
    if (parameters.preview) {
      const elements = tailed ? ['C', 'C', 'O'] : ['N']
      return respond(route, envelope(request, {
        preview_digest: (tailed ? 'a' : 'b').repeat(64), snapshot: structuredClone(state), document: structuredClone(state.document),
        atoms: elements.map((element, index) => ({ index: index + 1, element, charge: 0, isotope: null,
          hydrogens: { mode: 'inferred' }, radical: 'none', span: { offset: index, length: 1, character_offset: index, character_length: 1 } })),
        bonds: tailed ? [{ begin: 1, end: 2, order: 'single' }, { begin: 2, end: 3, order: 'single' }] : [],
        ignored_text: tailed ? { offset: 4, length: 2, character_offset: 4, character_length: 2 } : null,
        valence_model: 'chemifree-valence-3', aromaticity_model: 'chemifree-aromaticity-2',
      }, losses))
    }
    if (tailed && !parameters.accept_loss) return respond(route, consent(request, ['ignored_text'], [ignored]))
    let result = commits.get(request.request_id)
    if (!result) {
      assert.equal(parameters.expected_revision, state.document.revision)
      assert.equal(parameters.preview_digest, (tailed ? 'a' : 'b').repeat(64))
      const next = structuredClone(state)
      const first = next.document.atoms.length + 1
      const elements = tailed ? ['C', 'C', 'O'] : ['N']
      // Expected service geometry for these named fixtures; this is not a second layout engine.
      const positions = tailed
        ? (first === 5 ? [{ x: -26, y: 75 }, { x: 4, y: 75 }, { x: 34, y: 75 }] : [{ x: -26, y: 195 }, { x: 4, y: 195 }, { x: 34, y: 195 }])
        : [{ x: -26, y: 135 }]
      next.document.atoms.push(...elements.map((element, index) => atom(`a${first + index}`, element, positions[index].x, positions[index].y)))
      next.document.scene.instances[0].positions.push(...positions.map((position, index) => ({ atom: `a${first + index}`, ...position })))
      if (tailed) {
        const bond = next.document.bonds.length + 1
        next.document.bonds.push({ id: `b${bond}`, begin: `a${first}`, end: `a${first + 1}`, order: 'single', query: null, mark: null },
          { id: `b${bond + 1}`, begin: `a${first + 1}`, end: `a${first + 2}`, order: 'single', query: null, mark: null })
      }
      next.document.revision += 1
      next.session.saved_revision = next.document.revision
      next.session.undo_depth += 1
      state = next
      result = envelope(request, structuredClone(next), losses)
      commits.set(request.request_id, result)
    }
    if (importFails > 0) { importFails -= 1; return route.abort('failed') }
    if (holdImport) { finishImport = () => respond(route, result); importReceived(); return }
    return respond(route, result)
  })
  await page.addInitScript(() => {
    window.smilesClipboard = { reads: [], writes: [], denyRead: false, denyWrite: false, deferredRead: false, text: '' }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText() {
        const state = window.smilesClipboard
        state.reads.push(navigator.userActivation.isActive)
        if (state.denyRead) return Promise.reject(new DOMException('Denied', 'NotAllowedError'))
        if (state.deferredRead) return new Promise((resolve) => { state.finishRead = resolve })
        return Promise.resolve(state.text)
      },
      writeText(text) {
        window.smilesClipboard.writes.push({ text, activated: navigator.userActivation.isActive })
        return window.smilesClipboard.denyWrite ? Promise.reject(new DOMException('Denied', 'NotAllowedError')) : Promise.resolve()
      },
    } })
  })
  await page.goto(`http://127.0.0.1:${address.port}/smiles-clipboard-check`)
  await page.getByLabel('Current document').filter({ hasText: 'drawing' }).waitFor()
  const panel = page.getByRole('complementary', { name: 'SMILES', exact: true })
  const source = panel.getByRole('textbox', { name: 'SMILES text', exact: true })
  assert.deepEqual(await page.evaluate(() => window.smilesClipboard.reads), [])
  assert.equal(requests.length, 0)
  await page.evaluate(() => window.smilesHarness.select(['a2', 's1']))

  // The whole graph writer ignores atom/scene selection; clipboard write needs a distinct
  // activated click and no backend round trip. Manual selection works after a denied write.
  await panel.getByRole('button', { name: 'Write SMILES', exact: true }).click()
  await panel.getByRole('textbox', { name: 'Written SMILES', exact: true }).waitFor()
  assert.deepEqual(requests[0].parameters, { format: 'smiles', document_id: 'drawing', style: 'recorded' })
  assert.equal(await panel.getByRole('textbox', { name: 'Written SMILES', exact: true }).inputValue(), '[C@H](F)(Cl)Br')
  assert.match(await panel.innerText(), /entire chemical graph.*regardless of selected atoms, bonds or scene objects/)
  await panel.getByRole('button', { name: 'Select written SMILES', exact: true }).click()
  assert.deepEqual(await panel.getByRole('textbox', { name: 'Written SMILES', exact: true }).evaluate((element) => [element.selectionStart, element.selectionEnd]), [0, '[C@H](F)(Cl)Br'.length])
  assert.deepEqual(await page.evaluate(() => window.smilesClipboard.writes), [])
  const beforeWrite = requests.length
  await page.evaluate(() => { window.smilesClipboard.denyWrite = true })
  await panel.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await panel.getByRole('alert').filter({ hasText: 'Select and copy the written SMILES manually' }).waitFor()
  assert.equal(requests.length, beforeWrite)
  assert.deepEqual(await page.evaluate(() => window.smilesClipboard.writes), [{ text: '[C@H](F)(Cl)Br', activated: true }])
  await panel.getByRole('combobox', { name: 'Write the drawing as' }).selectOption('canonical')
  assert.equal(await panel.getByRole('button', { name: 'Copy to clipboard', exact: true }).count(), 0)
  await panel.getByRole('button', { name: 'Write SMILES', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and write SMILES', exact: true }).waitFor()
  assert.equal(await panel.getByRole('textbox', { name: 'Written SMILES', exact: true }).count(), 0)
  await panel.getByRole('button', { name: 'Accept these losses and write SMILES', exact: true }).click()
  await panel.getByRole('textbox', { name: 'Written SMILES', exact: true }).waitFor()
  assert.equal(requests.at(-1).parameters.accepted_losses, 'd'.repeat(64))
  await page.evaluate(() => { window.smilesClipboard.denyWrite = false })
  await panel.getByRole('button', { name: 'Copy to clipboard', exact: true }).click()
  await panel.getByRole('status').filter({ hasText: 'Written SMILES copied to clipboard.' }).waitFor()
  assert.deepEqual(await page.evaluate(() => window.smilesClipboard.writes.at(-1)), { text: 'FC(Cl)Br', activated: true })

  // Reading alone sends no source. Permission failure retains the manual field, over-limit
  // UTF-8 disables preview, and a late read cannot replace a newer typed source.
  const beforeReads = requests.length
  await page.evaluate(() => { window.smilesClipboard.denyRead = true })
  await panel.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await panel.getByRole('alert').filter({ hasText: 'Paste SMILES text manually' }).waitFor()
  await source.fill('é'.repeat(8193))
  assert.equal(await panel.getByRole('button', { name: 'Preview SMILES import', exact: true }).isDisabled(), true)
  await panel.getByRole('alert').filter({ hasText: 'exceeds 16 KiB of UTF-8' }).waitFor()
  assert.equal(requests.length, beforeReads)
  await source.fill('N')
  await page.evaluate(() => { window.smilesClipboard.denyRead = false; window.smilesClipboard.deferredRead = true })
  await panel.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.waitForFunction(() => typeof window.smilesClipboard.finishRead === 'function')
  await source.fill('N newer edit')
  await page.evaluate((value) => window.smilesClipboard.finishRead(value), tailedSource)
  await panel.getByRole('status').filter({ hasText: 'clipboard result was not used' }).waitFor()
  assert.equal(await source.inputValue(), 'N newer edit')
  await page.evaluate((value) => { window.smilesClipboard.deferredRead = false; window.smilesClipboard.text = value }, tailedSource)
  await panel.getByRole('button', { name: 'Read clipboard', exact: true }).click()
  await page.waitForFunction((value) => document.getElementById('smiles-source')?.value === value, tailedSource)
  assert.equal(requests.length, beforeReads)
  assert.deepEqual(await page.evaluate(() => window.smilesClipboard.reads), [true, true, true])

  // Exact source, false initial consent, explicit true consent. Both lost accepted replies
  // are checked with one immutable ID/body, even after manual edits remove the old preview.
  await panel.getByRole('button', { name: 'Preview SMILES import', exact: true }).click()
  await panel.getByRole('button', { name: 'Import SMILES', exact: true }).waitFor()
  assert.equal(requests.at(-1).parameters.source, tailedSource)
  assert.match(await panel.innerText(), /3 atom\(s\), 2 bond\(s\)/)
  await panel.getByRole('button', { name: 'Import SMILES', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and import SMILES', exact: true }).waitFor()
  const declined = requests.at(-1)
  assert.equal(declined.parameters.accept_loss, false)
  assert.equal(declined.parameters.source, tailedSource)
  assert.equal(state.document.atoms.length, 4)
  importFails = 2
  await panel.getByRole('button', { name: 'Accept these losses and import SMILES', exact: true }).click()
  await panel.getByRole('button', { name: 'Check SMILES import outcome', exact: true }).waitFor()
  const accepted = requests.filter((request) => request.operation === 'document.import' && !request.parameters.preview && request.parameters.accept_loss)
  assert.equal(accepted.length, 2)
  assert.deepEqual(accepted[0], accepted[1])
  assert.notEqual(accepted[0].request_id, declined.request_id, 'explicit new consent is a new, distinct attempt')
  assert.equal(accepted[0].parameters.source, tailedSource)
  assert.equal(accepted[0].parameters.preview_digest, 'a'.repeat(64))
  assert.equal(state.document.atoms.length, 7, 'only one accepted import committed')
  await source.fill('N')
  assert.equal(await panel.getByRole('button', { name: 'Import SMILES', exact: true }).count(), 0)
  assert.equal(await panel.getByRole('button', { name: 'Preview SMILES import', exact: true }).isDisabled(), true)
  assert.equal(await page.getByRole('button', { name: 'Check SMILES import outcome', exact: true, includeHidden: true }).count(), 1)
  await page.evaluate(() => window.smilesHarness.select(['a3']))
  const checking = panel.getByRole('button', { name: 'Check SMILES import outcome', exact: true })
  assert.equal(await checking.isEnabled(), true)
  await checking.click()
  await page.waitForFunction(() => !window.smilesHarness.editor.clipboardPending)
  const checked = requests.filter((request) => request.operation === 'document.import' && !request.parameters.preview && request.parameters.accept_loss)
  assert.equal(checked.length, 3)
  assert.deepEqual(checked[2], accepted[0])
  assert.equal(commits.size, 1)
  assert.deepEqual(state.document.atoms.slice(0, 4), initialAtoms)
  assert.deepEqual(state.document.bonds.slice(0, 3), initialBonds)
  assert.deepEqual(state.document.stereo, initialStereo)
  assert.deepEqual(await page.evaluate(() => ({
    selection: [...window.smilesHarness.editor.selection],
    positions: window.smilesHarness.editor.current.document.atoms.slice(4).map((atom) => atom.position),
    source: document.getElementById('smiles-source').value,
  })), { selection: ['a3'], positions: [{ x: -26, y: 75 }, { x: 4, y: 75 }, { x: 34, y: 75 }], source: 'N' })

  // Generated layout alone is informational: N imports with accept_loss:false and never
  // forces the person to approve a fictitious chemistry loss.
  await panel.getByRole('button', { name: 'Preview SMILES import', exact: true }).click()
  await panel.getByRole('button', { name: 'Import SMILES', exact: true }).click()
  await panel.getByRole('status').filter({ hasText: 'Imported. The drawing now holds the new atoms' }).waitFor()
  assert.equal(requests.at(-1).parameters.source, 'N')
  assert.equal(requests.at(-1).parameters.accept_loss, false)
  assert.equal(state.document.atoms.length, 8)
  assert.equal(commits.size, 2)
  assert.equal(await panel.getByRole('button', { name: 'Accept these losses and import SMILES', exact: true }).count(), 0)

  // A committed old-process success held across a detected restart cannot replace the
  // shown drawing or release the gate. Reconciliation is local to this SMILES panel.
  await source.fill(tailedSource)
  await panel.getByRole('button', { name: 'Preview SMILES import', exact: true }).click()
  await panel.getByRole('button', { name: 'Import SMILES', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and import SMILES', exact: true }).waitFor()
  const restarted = structuredClone(state)
  restarted.document.title = 'Restarted SMILES drawing'
  holdImport = true
  await panel.getByRole('button', { name: 'Accept these losses and import SMILES', exact: true }).click()
  await heldImportReady
  assert.equal(state.document.atoms.length, 11)
  state = restarted
  token = 'restarted-smiles-service'
  rejectInspection = true
  assert.equal(await page.evaluate(() => window.smilesHarness.editor.inspectIdentity('drawing', 'recorded', null)
    .then(() => 'unexpected', (error) => error.code)), 'service_restarted')
  await finishImport()
  await panel.getByRole('button', { name: 'Reload document to reconcile', exact: true }).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Reload document to reconcile', exact: true, includeHidden: true }).count(), 1)
  assert.equal(await page.evaluate(() => window.smilesHarness.editor.current.document.atoms.length), 8)
  const beforeReload = requests.length
  await panel.getByRole('button', { name: 'Reload document to reconcile', exact: true }).click()
  await page.waitForFunction(() => !window.smilesHarness.editor.clipboardPending)
  assert.equal(requests.length, beforeReload)
  assert.equal(await page.evaluate(() => window.smilesHarness.editor.current.document.title), 'Restarted SMILES drawing')
  assert.equal(await page.evaluate(() => window.smilesHarness.editor.current.document.atoms.length), 8)
  assert.deepEqual(errors, [])
  console.log('SMILES clipboard exact transport, whole-graph styles, explicit consent and real-hook recovery checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
