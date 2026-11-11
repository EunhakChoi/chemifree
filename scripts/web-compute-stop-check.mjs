// Real panels, editor and client with the HTTP boundary scripted. Each panel that can stop gets a
// time-limit or cancel reply for one request and an ok reply for its retry, and the check reads what
// stays on screen: the notice, its retry, and the result or stop that remains. It checks the web's
// display of a stop, not the services' computations or their limits.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const root = fileURLToPath(new URL('../apps/web', import.meta.url))
const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { PropertiesPanel } from '/src/components/properties-panel.tsx'
import { AromaticityPanel } from '/src/components/aromaticity-panel.tsx'
import { IdentityPanel } from '/src/components/identity-panel.tsx'
import { SubstructurePanel } from '/src/components/substructure-panel.tsx'
import { SmilesPanel } from '/src/components/smiles-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  const shown = editor.current?.document
  return <>
    <output aria-label="Current document">{shown?.id ?? 'loading'}</output>
    {shown && <main>
      <PropertiesPanel document={shown} editor={editor} />
      <AromaticityPanel document={shown} editor={editor} />
      <IdentityPanel document={shown} editor={editor} />
      <SubstructurePanel document={shown} editor={editor} />
      <SmilesPanel document={shown} editor={editor} />
    </main>}
  </>
}
createRoot(document.getElementById('root')).render(<Harness />)
`

// ---------------------------------------------------------------------------------------------
// The scripted service: one drawing, and the outcome of each request to each operation in turn.
// ---------------------------------------------------------------------------------------------

const LIMIT_TITLE = 'Stopped at the time limit (5000 ms)'
const CANCEL_TITLE = 'Cancelled'
const STOP_DETAIL = 'The computation was stopped before it finished. No result is shown, and the drawing was not changed.'
const STOPS = {
  time_limit: {
    code: 'time_limit', severity: 'error', path: null, details: { limit_ms: 5000 },
    message: 'The computation did not finish within the time limit of 5000 ms; nothing was changed.',
  },
  cancelled: {
    code: 'cancelled', severity: 'error', path: null, details: {},
    message: 'The computation was cancelled before it finished; nothing was changed.',
  },
}
/** The outcome of each request to each operation, in the order the panels send them. */
const SCRIPT = {
  'chem.properties': ['time_limit', 'ok'],
  'chem.aromaticity': ['time_limit', 'ok'],
  'chem.identity': ['cancelled', 'ok'],
  'chem.substructure': ['ok', 'time_limit', 'ok'],
  'document.export': ['time_limit', 'time_limit', 'ok'],
}

const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
const atom = (id, element, x, y = 0) => ({ id, element, kind: 'element', query: null, charge: 0, isotope: null,
  hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } })
// Ethanol's heavy atoms. No stored stereo, so no operation reports a stereo note for it.
const STATE = {
  api_version: 'v1',
  document: {
    id: 'drawing', title: 'Stop display drawing', revision: 3,
    atoms: [atom('a1', 'C', 0), atom('a2', 'C', 26, 15), atom('a3', 'O', 52, 0)],
    bonds: [
      { id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null },
      { id: 'b2', begin: 'a2', end: 'a3', order: 'single', query: null, mark: null },
    ],
    stereo: { atoms: [], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings,
    scene: {
      pages: [{ id: 'p1' }],
      instances: [{ id: 's1', page: 'p1', molecule: 'm1', positions: [{ atom: 'a1', x: 0, y: 0 }, { atom: 'a2', x: 26, y: 15 }, { atom: 'a3', x: 52, y: 0 }] }],
      roots: ['s1'], objects: [{ kind: 'instance', id: 's1', parent: null, molecule: 'm1' }], references: [],
    },
    selection: { objects: [], atoms: [], bonds: [] },
  },
  session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null },
  recovery: null,
  notes: [],
}

/** The model an operation's reply names; a stop of the same request names it too. */
function modelOf({ operation, parameters }) {
  if (operation === 'chem.properties') return 'chemifree-properties-1'
  if (operation === 'chem.aromaticity') return 'chemifree-aromaticity-2'
  if (operation === 'chem.identity') return parameters.mode === 'recorded' ? 'chemifree-recorded-graph-1' : 'chemifree-chemical-graph-2'
  if (operation === 'chem.substructure') return 'chemifree-substructure-1'
  if (operation === 'document.export') return parameters.style === 'canonical' ? 'chemifree-smiles-canonical-1' : 'chemifree-smiles-1'
  throw new Error(`The check names no model for ${operation}`)
}

/** The result an ok reply carries for each operation, shaped as the client reads it. */
function okResult({ operation, parameters }) {
  if (operation === 'chem.properties') {
    return {
      snapshot: structuredClone(STATE), scope: 'molecule', property: 'formula', selection: null, display_decimals: 0,
      valence_model: 'chemifree-valence-3', arithmetic: 'Formula counts from the recorded atoms.', data: null, valence_warnings: [],
      components: [{ atoms: ['a1', 'a2', 'a3'], formula: 'C2H6O', formal_charge: 0, mass: null, isotope_distribution: null, omitted_abundances: [], abundance_sums: [] }],
    }
  }
  if (operation === 'chem.aromaticity') return { snapshot: structuredClone(STATE), systems: [] }
  if (operation === 'chem.identity') {
    return {
      snapshot: structuredClone(STATE), comparison: null,
      identity: { code: `${parameters.mode}:C2H6O`, numbering: ['a1', 'a2', 'a3'], symmetry: [['a1'], ['a2'], ['a3']], folded_hydrogens: [] },
    }
  }
  if (operation === 'chem.substructure') {
    // The drawing searched in itself: every atom and bond maps to itself.
    return {
      query: structuredClone(STATE), target: structuredClone(STATE), aromaticity_model: 'chemifree-aromaticity-2', steps: 3,
      limits: { query_atoms: 3, search_steps: 2000000, scanned_candidates: 3, mappings: 1000 },
      mappings: [{
        atoms: [{ query: 'a1', target: 'a1' }, { query: 'a2', target: 'a2' }, { query: 'a3', target: 'a3' }],
        bonds: [{ query: 'b1', target: 'b1' }, { query: 'b2', target: 'b2' }],
      }],
    }
  }
  if (operation === 'document.export') {
    return { content: parameters.style === 'canonical' ? 'OCC' : 'CCO', style: parameters.style, snapshot: structuredClone(STATE) }
  }
  throw new Error(`The check has no ok result for ${operation}`)
}

function envelope(request, outcome, result, diagnostics) {
  return {
    contract: 1, envelope: 'operation_result', request_id: request.request_id, operation: request.operation,
    profile: 'preserve-semantics', outcome, diagnostics, loss_report: [], document_revision: STATE.document.revision,
    model_id: modelOf(request), result,
  }
}

/** The operation reply for one request, given the outcome the script gives it. */
function replyFor(request, outcome) {
  if (outcome === 'ok') return envelope(request, 'ok', okResult(request), [])
  return envelope(request, 'error', null, [STOPS[outcome]])
}

const respond = (route, value, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(value) })

const server = await createServer({
  root,
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{
    name: 'compute-stop-check-fixture',
    configureServer(vite) {
      vite.middlewares.use('/compute-stop-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/compute-stop-check', '<!doctype html><html><head><title>Compute stop check</title></head><body><div id="root"></div><script type="module" src="/compute-stop-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/compute-stop-harness.tsx') return id },
    load(id) { if (id === '/compute-stop-harness.tsx') return fixture },
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
  const requests = []
  const unexpected = []
  const outcomesLeft = new Map(Object.entries(SCRIPT).map(([operation, outcomes]) => [operation, [...outcomes]]))
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return respond(route, { header: 'x-chemifree-session', token: 'compute-stop' })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return respond(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return respond(route, STATE)
    const request = path === '/api/v1/operations' ? route.request().postDataJSON() : null
    const outcome = outcomesLeft.get(request?.operation)?.shift()
    if (outcome === undefined) {
      unexpected.push(request?.operation ?? path)
      return respond(route, { api_version: 'v1', code: 'unexpected_request', message: 'The check did not script this request.' }, 500)
    }
    requests.push(request)
    return respond(route, replyFor(request, outcome))
  })
  await page.goto(`http://127.0.0.1:${address.port}/compute-stop-check`)
  await page.getByLabel('Current document').filter({ hasText: 'drawing' }).waitFor()
  assert.equal(requests.length, 0, 'opening the drawing sends no operation')

  const region = (name) => page.getByRole('region', { name, exact: true })
  const properties = region('Molecular properties')
  const aromaticity = region('Rings and aromaticity')
  const identity = region('Graph identity')
  const substructure = region('Substructure search')
  const smiles = page.getByRole('complementary', { name: 'SMILES', exact: true })
  const sent = (operation) => requests.filter((request) => request.operation === operation)

  /** Waits until a stop notice in `scope` shows its title and its statement, and returns the notice. */
  async function showsStop(scope, testId, title) {
    const notice = scope.getByTestId(testId)
    await notice.getByText(title, { exact: true }).waitFor()
    await notice.getByText(STOP_DETAIL, { exact: true }).waitFor()
    return notice
  }

  // Properties: the time limit shows its notice with no result; the retry replaces the notice with the result.
  await properties.getByRole('button', { name: 'Calculate', exact: true }).click()
  const propertiesStop = await showsStop(properties, 'compute-stop', LIMIT_TITLE)
  assert.equal(await properties.getByTestId('properties-result').count(), 0)
  assert.equal(await properties.getByText('Could not calculate', { exact: true }).count(), 0)
  await propertiesStop.getByRole('button', { name: 'Try again', exact: true }).click()
  await propertiesStop.waitFor({ state: 'detached' })
  const propertiesResult = properties.getByTestId('properties-result')
  await propertiesResult.getByText('Formal charge: 0', { exact: true }).waitFor()
  assert.match(await propertiesResult.innerText(), /Formula: C2H6O/)
  assert.equal(sent('chem.properties').length, 2)

  // Rings and aromaticity: the same pattern with the time limit.
  await aromaticity.getByRole('button', { name: 'Inspect rings', exact: true }).click()
  const aromaticityStop = await showsStop(aromaticity, 'compute-stop', LIMIT_TITLE)
  assert.equal(await aromaticity.getByTestId('aromaticity-result').count(), 0)
  assert.equal(await aromaticity.locator('p[role="alert"]').count(), 0)
  await aromaticityStop.getByRole('button', { name: 'Try again', exact: true }).click()
  await aromaticityStop.waitFor({ state: 'detached' })
  await aromaticity.getByTestId('aromaticity-result').getByText('No cyclic systems.', { exact: true }).waitFor()
  assert.equal(sent('chem.aromaticity').length, 2)

  // Graph identity: a cancelled run shows the cancel title, not the time limit.
  await identity.getByRole('button', { name: 'Inspect identity', exact: true }).click()
  const identityStop = await showsStop(identity, 'compute-stop', CANCEL_TITLE)
  assert.equal(await identity.getByTestId('identity-result').count(), 0)
  assert.equal(await identity.getByText('Could not inspect identity', { exact: true }).count(), 0)
  await identityStop.getByRole('button', { name: 'Try again', exact: true }).click()
  await identityStop.waitFor({ state: 'detached' })
  await identity.getByTestId('identity-result').getByText('Canonical numbers: 1→a1, 2→a2, 3→a3', { exact: true }).waitFor()
  assert.equal(sent('chem.identity').length, 2)

  // Substructure search: the second search stops, so the earlier result must leave the screen first.
  await substructure.getByLabel('Other drawing', { exact: true }).click()
  await page.getByRole('option', { name: 'This drawing', exact: true }).click()
  await substructure.getByRole('button', { name: 'Search', exact: true }).click()
  await substructure.getByTestId('substructure-result').waitFor()
  assert.equal(await substructure.getByTestId('substructure-mapping').count(), 1)
  await substructure.getByRole('button', { name: 'Search', exact: true }).click()
  await substructure.getByTestId('substructure-result').waitFor({ state: 'detached' })
  const substructureStop = await showsStop(substructure, 'substructure-stop', LIMIT_TITLE)
  assert.equal(await substructure.getByTestId('substructure-error').count(), 0)
  await substructureStop.getByRole('button', { name: 'Try again', exact: true }).click()
  await substructureStop.waitFor({ state: 'detached' })
  await substructure.getByTestId('substructure-result').waitFor()
  assert.equal(await substructure.getByTestId('substructure-mapping').count(), 1)
  assert.deepEqual(sent('chem.substructure').map((request) => request.parameters), [
    { query_document_id: 'drawing', target_document_id: 'drawing' },
    { query_document_id: 'drawing', target_document_id: 'drawing' },
    { query_document_id: 'drawing', target_document_id: 'drawing' },
  ])

  // SMILES: a stop shows its notice; changing the form clears it without a request; the notice's
  // own retry answers. Recorded and canonical writes send no accepted_losses.
  const writeButton = smiles.getByRole('button', { name: 'Write SMILES', exact: true })
  const written = smiles.getByRole('textbox', { name: 'Written SMILES', exact: true })
  await writeButton.click()
  const recordedStop = await showsStop(smiles, 'smiles-stop', LIMIT_TITLE)
  assert.equal(await written.count(), 0)
  assert.deepEqual(sent('document.export').at(-1).parameters, { format: 'smiles', document_id: 'drawing', style: 'recorded' })
  const beforeFormChange = requests.length
  await smiles.getByRole('combobox', { name: 'Write the drawing as', exact: true }).selectOption('canonical')
  await recordedStop.waitFor({ state: 'detached' })
  assert.equal(requests.length, beforeFormChange)
  await writeButton.click()
  const canonicalStop = await showsStop(smiles, 'smiles-stop', LIMIT_TITLE)
  assert.deepEqual(sent('document.export').at(-1).parameters, { format: 'smiles', document_id: 'drawing', style: 'canonical' })
  await canonicalStop.getByRole('button', { name: 'Write SMILES again', exact: true }).click()
  await canonicalStop.waitFor({ state: 'detached' })
  await written.waitFor()
  assert.equal(await written.inputValue(), 'OCC')
  assert.equal(sent('document.export').length, 3)
  assert.equal(await smiles.locator('p[role="alert"]').count(), 0)

  assert.deepEqual(unexpected, [])
  assert.deepEqual([...outcomesLeft].filter(([, left]) => left.length > 0).map(([operation]) => operation), [])
  assert.equal(JSON.stringify(requests).includes('time_limit_ms'), false, 'the web sends no time limit of its own')
  assert.deepEqual(errors, [])
  console.log('Compute stop display checks passed for the five panels: time-limit and cancel notices, retries, and the SMILES form change.')
} finally {
  await browser?.close()
  await server.close()
}
