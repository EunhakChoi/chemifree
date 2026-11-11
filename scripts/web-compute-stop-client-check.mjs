// The web's reading of a stop reply (a time limit or a cancel): the reader, each client method that
// reads a reply, and the CIP view that follows a stop. Every reply is scripted, so no test waits on a clock.
import assert from 'node:assert/strict'
import { test } from 'node:test'

import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

// Imported after the TypeScript alias loader is installed by web-client-check.mjs.
const { readComputeStop, stopTitle } = await import('../apps/web/src/lib/compute-stop.ts')
const { ComputeStopped, DocumentClient, SubstructureRefusal, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { cipFailureSentence, cipMarks, cipView } = await import('../apps/web/src/editor/cip-labels.ts')

const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
const expectOf = (models, revision = 'count') => ({ models, revision })
const PROPERTIES = expectOf(['chemifree-properties-1'])
const CONVERT = expectOf(['chemifree-aromaticity-2'], 'count_or_null')
const LIMIT_MESSAGE = 'The computation did not finish within the time limit of 5000 ms; nothing was changed.'
const CANCEL_MESSAGE = 'The computation was cancelled before it finished; nothing was changed.'
const CIP_LIMIT_MESSAGE = '9 items need more than 1000 digraph nodes together for the centers.'
const SELECTION = { objects: [], atoms: [], bonds: [] }
const SESSION = { saved_revision: 1, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }

function limitDiagnostic(overrides = {}) {
  return { code: 'time_limit', severity: 'error', message: LIMIT_MESSAGE, path: null, details: { limit_ms: 5000 }, ...overrides }
}

function cancelDiagnostic() {
  return { code: 'cancelled', severity: 'error', message: CANCEL_MESSAGE, path: null, details: {} }
}

function cipLimitDiagnostic() {
  return {
    code: 'resource_limit', severity: 'error', message: CIP_LIMIT_MESSAGE, path: 'parameters.atoms',
    details: { limit: 1000, request_limit: 2000000, items: 9, item_limit: 20000, stage: 'centers', parameter: 'atoms' },
  }
}

/** An operation reply that stopped at its time limit: no result, no loss, and the model and revision it names. */
function stopReply(overrides = {}) {
  return {
    envelope: 'operation_result', contract: 1, request_id: 'r', operation: 'chem.properties', profile: 'preserve-semantics',
    outcome: 'error', document_revision: 3, model_id: 'chemifree-properties-1', loss_report: [], result: null,
    diagnostics: [limitDiagnostic()], ...overrides,
  }
}

function without(reply, key) {
  const copy = { ...reply }
  delete copy[key]
  return copy
}

async function rejectionOf(promise) {
  return promise.then(() => assert.fail('the call should have been refused'), (error) => error)
}

/** Answers the session and the operations endpoint; every operation body is recorded in order. */
function operations(t, reply) {
  const bodies = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'compute-stop' })
    assert.equal(path, '/api/v1/operations')
    const body = JSON.parse(init.body)
    bodies.push(body)
    return reply(body, bodies.length)
  })
  return bodies
}

// ---------------------------------------------------------------------------------------------
// readComputeStop: what counts as a stop, and what carries a stop code but not the stop contract.
// ---------------------------------------------------------------------------------------------

test('a time-limit reply reads as a time-limit stop with the limit it reached', () => {
  assert.deepEqual(readComputeStop(stopReply(), PROPERTIES), { kind: 'time_limit', limitMs: 5000 })
})

test('a limit of zero, which a request may ask for, reads as a stop at 0 ms', () => {
  const reply = stopReply({ diagnostics: [limitDiagnostic({ details: { limit_ms: 0 } })] })
  assert.deepEqual(readComputeStop(reply, PROPERTIES), { kind: 'time_limit', limitMs: 0 })
  assert.equal(stopTitle({ kind: 'time_limit', limitMs: 0 }), 'Stopped at the time limit (0 ms)')
})

test('a cancelled reply with empty details reads as a cancel', () => {
  assert.deepEqual(readComputeStop(stopReply({ diagnostics: [cancelDiagnostic()] }), PROPERTIES), { kind: 'cancelled' })
})

test('a convert stop names its revision as null, which only the count-or-null expectation accepts', () => {
  const reply = stopReply({ operation: 'chem.aromaticity_convert', model_id: 'chemifree-aromaticity-2', document_revision: null })
  assert.deepEqual(readComputeStop(reply, CONVERT), { kind: 'time_limit', limitMs: 5000 })
  assert.equal(readComputeStop(reply, expectOf(['chemifree-aromaticity-2'])), 'malformed')
})

test('a convert stop may also name the inspected revision as an integer', () => {
  const reply = stopReply({ operation: 'chem.aromaticity_convert', model_id: 'chemifree-aromaticity-2', document_revision: 3 })
  assert.deepEqual(readComputeStop(reply, CONVERT), { kind: 'time_limit', limitMs: 5000 })
})

const MALFORMED = [
  ['limit_ms is missing', () => stopReply({ diagnostics: [limitDiagnostic({ details: {} })] })],
  ['limit_ms is a string', () => stopReply({ diagnostics: [limitDiagnostic({ details: { limit_ms: '5000' } })] })],
  ['limit_ms is negative', () => stopReply({ diagnostics: [limitDiagnostic({ details: { limit_ms: -1 } })] })],
  ['limit_ms is fractional', () => stopReply({ diagnostics: [limitDiagnostic({ details: { limit_ms: 1.5 } })] })],
  ['details is not a record', () => stopReply({ diagnostics: [limitDiagnostic({ details: [5000] })] })],
  ['result is not null', () => stopReply({ result: {} })],
  ['model_id is another model', () => stopReply({ model_id: 'chemifree-cip-1' })],
  ['model_id is missing', () => without(stopReply(), 'model_id')],
  ['there are two diagnostics', () => stopReply({ diagnostics: [limitDiagnostic(), limitDiagnostic()] })],
  ['loss_report is not empty', () => stopReply({ loss_report: [{ feature: 'stereo', source_path: 'stereo', disposition: 'dropped', reason: 'x', semantic_impact: 'stereo' }] })],
  ['path is not null', () => stopReply({ diagnostics: [limitDiagnostic({ path: 'parameters.atoms' })] })],
  ['severity is not error', () => stopReply({ diagnostics: [limitDiagnostic({ severity: 'warning' })] })],
  ['message is not a string', () => stopReply({ diagnostics: [limitDiagnostic({ message: 5000 })] })],
  ['document_revision is negative', () => stopReply({ document_revision: -1 })],
  ['document_revision is a string', () => stopReply({ document_revision: '3' })],
]

for (const [name, build] of MALFORMED) {
  test(`malformed, not a stop: time-limit reply, ${name}`, () => {
    assert.equal(readComputeStop(build(), PROPERTIES), 'malformed')
  })
}

const NOT_STOPS = [
  ['an ok outcome', () => stopReply({ outcome: 'ok', result: {}, diagnostics: [] }), PROPERTIES],
  ['an unsupported outcome', () => stopReply({
    outcome: 'unsupported', operation: 'chem.substructure', model_id: 'chemifree-substructure-1', document_revision: null,
    diagnostics: [{ code: 'match_unsupported', severity: 'error', message: 'The matcher cannot read this bond.', path: null, details: {} }],
  }), expectOf(['chemifree-substructure-1'])],
  ['a resource_limit of chem.cip', () => stopReply({ operation: 'chem.cip', model_id: 'chemifree-cip-1', document_revision: 1, diagnostics: [cipLimitDiagnostic()] }), expectOf(['chemifree-cip-1'])],
  ['an invalid_request of chem.properties', () => stopReply({
    model_id: 'chemifree-properties-1', document_revision: null,
    diagnostics: [{ code: 'invalid_request', severity: 'error', message: 'Formula-only requests do not select mass data.', path: null, details: {} }],
  }), PROPERTIES],
  ['a revision_conflict of a conversion', () => stopReply({
    operation: 'chem.aromaticity_convert', model_id: 'chemifree-aromaticity-2', document_revision: 4,
    diagnostics: [{ code: 'revision_conflict', severity: 'error', message: 'The drawing changed after it was inspected.', path: null, details: { current_revision: 4 } }],
  }), CONVERT],
  ['a match_limit of chem.substructure', () => stopReply({
    operation: 'chem.substructure', model_id: 'chemifree-substructure-1',
    diagnostics: [{ code: 'match_limit', severity: 'error', message: 'The search reached a limit before it could give a complete answer.', path: null, details: { limit_kind: 'search_steps', limit: 2000000 } }],
  }), expectOf(['chemifree-substructure-1'])],
  ['a cip_labeler_invalid of chem.cip', () => stopReply({
    operation: 'chem.cip', model_id: 'chemifree-cip-1', document_revision: 1,
    diagnostics: [{ code: 'cip_labeler_invalid', severity: 'error', message: 'The labeler could not rank the ligands of a2.', path: null, details: { unit: 'a2', problem: 'nodes' } }],
  }), expectOf(['chemifree-cip-1'])],
  ['no diagnostic', () => stopReply({ diagnostics: [] }), PROPERTIES],
  ['a value that is not an object', () => null, PROPERTIES],
]

for (const [name, build, expect] of NOT_STOPS) {
  test(`not a stop: ${name}`, () => {
    assert.equal(readComputeStop(build(), expect), null)
  })
}

test('a stop has a title that names its limit, or says that it was cancelled', () => {
  assert.equal(stopTitle({ kind: 'time_limit', limitMs: 5000 }), 'Stopped at the time limit (5000 ms)')
  assert.equal(stopTitle({ kind: 'cancelled' }), 'Cancelled')
})

// ---------------------------------------------------------------------------------------------
// DocumentClient: each operation the web stops reads the stop before its own checks.
// ---------------------------------------------------------------------------------------------

const CALLS = [
  { method: 'inspectProperties', operation: 'chem.properties', model: 'chemifree-properties-1', revision: 3, call: (client) => client.inspectProperties('d1', 'molecule', 'formula', null, null) },
  { method: 'inspectAromaticity', operation: 'chem.aromaticity', model: 'chemifree-aromaticity-2', revision: 3, call: (client) => client.inspectAromaticity('d1') },
  { method: 'inspectIdentity', operation: 'chem.identity', model: 'chemifree-chemical-graph-2', revision: 3, call: (client) => client.inspectIdentity('d1', 'chemical', null) },
  { method: 'searchSubstructure', operation: 'chem.substructure', model: 'chemifree-substructure-1', revision: 3, call: (client) => client.searchSubstructure('q1', 'd1') },
  { method: 'exportSmiles', operation: 'document.export', model: 'chemifree-smiles-1', revision: 3, call: (client) => client.exportSmiles('d1', 'recorded') },
  { method: 'inspectCip', operation: 'chem.cip', model: 'chemifree-cip-1', revision: 3, call: (client) => client.inspectCip('d1', { scope: 'stored' }) },
  { method: 'convertAromaticity', operation: 'chem.aromaticity_convert', model: 'chemifree-aromaticity-2', revision: null, call: (client) => client.convertAromaticity('d1', 3, 'aromatic', SELECTION) },
]

for (const { method, operation, model, revision, call } of CALLS) {
  test(`${method} turns a time-limit stop into a ComputeStopped error that names the limit`, async (t) => {
    const bodies = operations(t, () => json(stopReply({ operation, model_id: model, document_revision: revision })))
    const error = await rejectionOf(call(new DocumentClient()))
    assert.ok(error instanceof ComputeStopped)
    assert.equal(error.code, 'time_limit')
    assert.deepEqual(error.stop, { kind: 'time_limit', limitMs: 5000 })
    assert.equal(error.message, LIMIT_MESSAGE)
    assert.equal(bodies.length, 1)
    assert.equal(JSON.stringify(bodies[0]).includes('time_limit_ms'), false, 'the web sends no time limit of its own')
  })

  test(`${method} refuses a time-limit stop without its limit as an incompatible reply`, async (t) => {
    operations(t, () => json(stopReply({ operation, model_id: model, document_revision: revision, diagnostics: [limitDiagnostic({ details: {} })] })))
    const error = await rejectionOf(call(new DocumentClient()))
    assert.equal(error instanceof ComputeStopped, false)
    assert.equal(error.code, 'incompatible_response')
    assert.equal(error.message, 'The local service returned an incompatible stop reply.')
  })
}

test('a cancelled operation is a ComputeStopped error of kind cancelled', async (t) => {
  operations(t, () => json(stopReply({
    operation: 'chem.aromaticity', model_id: 'chemifree-aromaticity-2', document_revision: 3, diagnostics: [cancelDiagnostic()],
  })))
  const error = await rejectionOf(new DocumentClient().inspectAromaticity('d1'))
  assert.ok(error instanceof ComputeStopped)
  assert.deepEqual([error.code, error.stop, error.message], ['cancelled', { kind: 'cancelled' }, CANCEL_MESSAGE])
})

test('a canonical SMILES stop is a ComputeStopped error too', async (t) => {
  operations(t, () => json(stopReply({ operation: 'document.export', model_id: 'chemifree-smiles-canonical-1', document_revision: 3 })))
  const error = await rejectionOf(new DocumentClient().exportSmiles('d1', 'canonical'))
  assert.ok(error instanceof ComputeStopped)
  assert.deepEqual([error.code, error.stop], ['time_limit', { kind: 'time_limit', limitMs: 5000 }])
})

test('a SMILES stop of the other style is not the stop of the style asked for', async (t) => {
  operations(t, () => json(stopReply({ operation: 'document.export', model_id: 'chemifree-smiles-1', document_revision: 3 })))
  const error = await rejectionOf(new DocumentClient().exportSmiles('d1', 'canonical'))
  assert.equal(error instanceof ComputeStopped, false)
  assert.equal(error.code, 'incompatible_response')
})

test('a request that gets no reply is unreachable and is never read as a stop', async (t) => {
  operations(t, () => { throw new TypeError('fetch failed') })
  const error = await rejectionOf(new DocumentClient().inspectAromaticity('d1'))
  assert.equal(error instanceof ComputeStopped, false)
  assert.deepEqual([error.name, error.code, error.status], ['ApiError', 'unreachable', 0])
})

test('a properties refusal that is not a stop stays properties_unavailable with its message', async (t) => {
  operations(t, () => json(stopReply({
    operation: 'chem.properties', model_id: null, document_revision: null,
    diagnostics: [{ code: 'invalid_request', severity: 'error', message: 'Formula-only requests do not select mass data.', path: null, details: {} }],
  })))
  const error = await rejectionOf(new DocumentClient().inspectProperties('d1', 'molecule', 'formula', null, 'synthetic-test'))
  assert.equal(error instanceof ComputeStopped, false)
  assert.deepEqual([error.name, error.code, error.message], ['ApiError', 'properties_unavailable', 'Formula-only requests do not select mass data.'])
})

test('a chem.cip resource limit that is not a stop stays cip_unavailable, with no stop', async (t) => {
  operations(t, () => json(stopReply({ operation: 'chem.cip', model_id: 'chemifree-cip-1', document_revision: 1, diagnostics: [cipLimitDiagnostic()] })))
  const error = await rejectionOf(new DocumentClient().inspectCip('d1', { scope: 'stored' }))
  assert.equal(error instanceof ComputeStopped, false)
  assert.deepEqual([error.name, error.code, error.message], ['ApiError', 'cip_unavailable', CIP_LIMIT_MESSAGE])
})

/** A snapshot the service sends with an operation reply: the sample drawing, under another ID. */
function snapshotOf(id) {
  const response = sceneCutSource()
  response.document.id = id
  return response
}

test('a substructure refusal that is not a stop stays a SubstructureRefusal with both drawings', async (t) => {
  operations(t, () => json(stopReply({
    operation: 'chem.substructure', model_id: 'chemifree-substructure-1', document_revision: 3,
    diagnostics: [{
      code: 'match_limit', severity: 'error', message: 'The search reached a limit before it could give a complete answer.', path: null,
      details: { query_snapshot: snapshotOf('q1'), target_snapshot: snapshotOf('d1'), limit_kind: 'search_steps', limit: 2000000 },
    }],
  })))
  const failure = await rejectionOf(new DocumentClient().searchSubstructure('q1', 'd1'))
  assert.ok(failure instanceof SubstructureRefusal)
  assert.equal(failure instanceof ComputeStopped, false)
  assert.deepEqual(
    [failure.code, failure.message, failure.query.id, failure.target.id],
    ['match_limit', 'The search reached a limit before it could give a complete answer. Limit: search steps (2000000).', 'q1', 'd1'],
  )
})

// ---------------------------------------------------------------------------------------------
// CIP view: a stop replaces what was shown, and names the stop in the view and its sentence.
// ---------------------------------------------------------------------------------------------

/** F-C(a2)=C(a3)-Br with the Z configuration stored on the double bond b2. */
function storedBondDrawing() {
  const atom = (id, element, x) => ({
    id, element, charge: 0, isotope: null, position: { x, y: 0 }, kind: 'element', query: null,
    hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [],
  })
  const bond = (id, begin, end, order = 'single') => ({ id, begin, end, order, query: null, mark: null })
  const atoms = [atom('a1', 'F', 0), atom('a2', 'C', 20), atom('a3', 'C', 40), atom('a4', 'Br', 60)]
  return {
    id: 'stored-bond', title: 'Stored bond', revision: 1, atoms,
    bonds: [bond('b1', 'a1', 'a2'), bond('b2', 'a2', 'a3', 'double'), bond('b3', 'a3', 'a4')],
    fragment_displays: [],
    molecule: { id: 'm1' },
    scene: {
      pages: [{ id: 'p1' }],
      instances: [{ id: 's1', page: 'p1', molecule: 'm1', positions: atoms.map(({ id, position }) => ({ atom: id, x: position.x, y: position.y })) }],
    },
    stereo: { atoms: [], bonds: [{ bond: 'b2', configuration: { kind: 'relative', begin_neighbor: 'a1', end_neighbor: 'a4', relation: 'same_side' } }] },
  }
}

/** The service's ok reply for `chem.cip` of that drawing: the bond b2 is assigned Z. */
function storedBondReply(drawing) {
  return {
    envelope: 'operation_result', contract: 1, request_id: 'r', operation: 'chem.cip', profile: 'preserve-semantics',
    outcome: 'ok', document_revision: drawing.revision, model_id: 'chemifree-cip-1', loss_report: [],
    diagnostics: [
      { code: 'cip_scope', severity: 'info', message: 'Listed: every stored configuration.', path: null, details: { scope: 'stored' } },
      { code: 'cip_not_checked', severity: 'info', message: 'Rules 1a, 1b, 2 and 3 only.', path: null, details: {} },
    ],
    result: {
      snapshot: { document: drawing, session: SESSION, recovery: null, notes: [] },
      model: 'chemifree-cip-1', rules: ['1a', '1b', '2', '3'], scope: 'stored', atoms: [],
      bonds: [{
        bond: 'b2', state: 'assigned', descriptor: 'Z', reason: null,
        stored: { kind: 'relative', begin_neighbor: 'a1', end_neighbor: 'a4', relation: 'same_side' },
        ends: [{ atom: 'a2', higher: 'a1', decided_by: '1a' }, { atom: 'a3', higher: 'a4', decided_by: '1a' }],
      }],
    },
  }
}

/** The shown ok outcome for the drawing, read through the client as the page reads it. */
async function shownLabels(t, drawing) {
  operations(t, () => json(storedBondReply(drawing)))
  const inspection = await new DocumentClient().inspectCip(drawing.id, { scope: 'stored' })
  return { kind: 'ok', generation: 0, inspection }
}

test('the stored-bond drawing is a drawing the service accepts', () => {
  assert.equal(isDocumentResponse({ api_version: 'v1', document: storedBondDrawing(), session: SESSION, recovery: null }), true)
})

test('a time-limit stop of CIP labels is a failed view that keeps the stop, with its sentence', () => {
  const drawing = storedBondDrawing()
  const view = cipView(drawing, { kind: 'failed', generation: 0, snapshot: drawing, message: LIMIT_MESSAGE, stop: { kind: 'time_limit', limitMs: 5000 } }, 0, false, false, true)
  assert.deepEqual(view, { kind: 'failed', message: LIMIT_MESSAGE, stop: { kind: 'time_limit', limitMs: 5000 } })
  assert.equal(cipFailureSentence(view), 'CIP labels stopped at the time limit (5000 ms). No labels are shown.')
})

test('a cancelled CIP run says so, and a refusal keeps the unavailable sentence', () => {
  assert.equal(cipFailureSentence({ message: CANCEL_MESSAGE, stop: { kind: 'cancelled' } }), 'CIP labels cancelled. No labels are shown.')
  assert.equal(cipFailureSentence({ message: 'too big', stop: null }), 'CIP labels unavailable: too big')
})

test('a time-limit stop after a shown reply for the same drawing replaces it with a failed view', async (t) => {
  const drawing = storedBondDrawing()
  const ok = await shownLabels(t, drawing)
  assert.equal(cipView(drawing, ok, 0, false, false, true).kind, 'ready')
  const stopped = { kind: 'failed', generation: 0, snapshot: structuredClone(drawing), message: LIMIT_MESSAGE, stop: { kind: 'time_limit', limitMs: 5000 } }
  const view = cipView(drawing, stopped, 0, false, false, true)
  assert.equal(view.kind, 'failed', 'a stop replaces the labels that were shown')
  assert.deepEqual(view, { kind: 'failed', message: LIMIT_MESSAGE, stop: { kind: 'time_limit', limitMs: 5000 } })
})

test('a time-limit stop after a shown reply for the same drawing draws no stale CIP label', async (t) => {
  const drawing = storedBondDrawing()
  const ok = await shownLabels(t, drawing)
  const ready = cipView(drawing, ok, 0, false, false, true)
  assert.deepEqual(cipMarks(ready, false, true).map((mark) => [mark.item.id, mark.text]), [['b2', '(Z)']])
  const stopped = { kind: 'failed', generation: 0, snapshot: structuredClone(drawing), message: LIMIT_MESSAGE, stop: { kind: 'time_limit', limitMs: 5000 } }
  assert.deepEqual(cipMarks(cipView(drawing, stopped, 0, false, false, true), false, true), [])
})
