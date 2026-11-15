// Checks the web's `layout.structure` client (preview and apply) against a scripted service, and the
// pure helpers that the layout panel and canvas use: ghost segments, refusal sentences and request
// equality. Replies are built from the CONTRACT's worked example, so a reader that accepts a wrong
// reply, or a client that sends a wrong body, fails here.
import assert from 'node:assert/strict'
import { test } from 'node:test'

// Imported after the TypeScript alias loader is installed. web-client-check.mjs installs it first.
const { ApiError, ComputeStopped, DocumentClient, LAYOUT_MODEL, LayoutRefusal, layoutMovedCount, sameLayoutRequest } =
  await import('../apps/web/src/lib/document-contract.ts')
const { layoutGhostSegments } = await import('../apps/web/src/editor/layout-ghost.ts')
const { layoutRefusalSentence } = await import('../apps/web/src/lib/layout-refusals.ts')

const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

/** Answers the session and operation requests. `reply(body, count)` answers the count-th operation. */
function service(t, reply) {
  const bodies = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'layout-service' })
    assert.equal(path, '/api/v1/operations')
    const body = JSON.parse(init.body)
    bodies.push(body)
    return reply(body, bodies.length)
  })
  return bodies
}

/** A validator for `assert.rejects`: the rejection is a `type` whose listed fields are as expected. */
function rejectedWith(type, expected) {
  return (error) => {
    assert.ok(error instanceof type, `expected a ${type.name}, got ${error}`)
    assert.deepEqual(Object.fromEntries(Object.keys(expected).map((key) => [key, error[key]])), expected)
    return true
  }
}

const NOT_IMPLEMENTED = 'Fused, bridged and spiro ring layout, rings of more than 12 atoms, templates, scoring and search, and collision resolution are not implemented.'
const PREVIEW_ONLY = 'Nothing was changed; mode "apply" applies this layout as one undoable edit.'
const UNCHANGED = 'Every layout atom is already at its proposed position; nothing was changed.'
const SERVICE_MESSAGE = 'The service refused the selected layout.'
const TIME_LIMIT_MESSAGE = 'The computation was stopped at its time limit; nothing was changed.'

const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
const ATOM_IDS = ['a1', 'a2', 'a3', 'a4']
/** Butane as drawn: x = 0, 30, 60, 90 on the baseline. */
const DRAWN = { a1: { x: 0, y: 0 }, a2: { x: 30, y: 0 }, a3: { x: 60, y: 0 }, a4: { x: 90, y: 0 } }
/** The CONTRACT worked-example proposal for the same chain. */
const PROPOSED = { a1: { x: 6.03, y: 7.5 }, a2: { x: 32.01, y: -7.5 }, a3: { x: 57.99, y: 7.5 }, a4: { x: 83.97, y: -7.5 } }

const requestOf = (atoms, bonds = []) => ({ documentId: 'drawing', instance: 's1', scope: 'components', atoms, bonds })
const selectionOf = () => ({ objects: [], atoms: ['a2'], bonds: [] })

const atomOf = (id, { x, y }) => ({ id, element: 'C', kind: 'element', query: null, charge: 0, isotope: null,
  hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } })
const bondOf = (id, begin, end) => ({ id, begin, end, order: 'single', query: null, mark: null })

function documentAt(revision, placed) {
  return {
    id: 'drawing', title: 'Butane', revision,
    atoms: ATOM_IDS.map((id) => atomOf(id, placed[id])),
    bonds: [bondOf('b1', 'a1', 'a2'), bondOf('b2', 'a2', 'a3'), bondOf('b3', 'a3', 'a4')],
    stereo: { atoms: [], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings: { ...settings },
    scene: {
      pages: [{ id: 'p1' }],
      instances: [{ id: 's1', page: 'p1', molecule: 'm1', positions: ATOM_IDS.map((atom) => ({ atom, x: placed[atom].x, y: placed[atom].y })) }],
      roots: ['s1'], objects: [{ kind: 'instance', id: 's1', parent: null, molecule: 'm1' }], references: [],
    },
    selection: { objects: [], atoms: [], bonds: [] },
  }
}

/** A DocumentResponse without `api_version`, the shape of `result` and `result.snapshot`. */
function snapshotAt(revision, placed = DRAWN) {
  return {
    document: documentAt(revision, placed),
    session: { saved_revision: revision, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null },
    recovery: null,
    notes: [],
  }
}

const diagnostic = (code, severity, message, details = {}) => ({ code, severity, path: null, message, details })
const notImplemented = () => diagnostic('layout_not_implemented', 'info', NOT_IMPLEMENTED)
const unchangedDiagnostic = () => diagnostic('layout_unchanged', 'info', UNCHANGED)
const previewOnlyDiagnostic = () => diagnostic('layout_preview_only', 'info', PREVIEW_ONLY)
const overlapDiagnostic = (pairs, count) => diagnostic('layout_overlap', 'warning', 'Some atoms lie closer than half a bond length.', { pairs, count })
const timeLimitDiagnostic = () => diagnostic('time_limit', 'error', TIME_LIMIT_MESSAGE, { limit_ms: 5000 })

/** A successful preview of butane's four atoms, as the service answers the request for a2. */
function previewReply() {
  return {
    envelope: 'operation_result', contract: 1, outcome: 'ok', model_id: LAYOUT_MODEL, document_revision: 3, loss_report: [],
    diagnostics: [previewOnlyDiagnostic(), notImplemented()],
    result: {
      snapshot: snapshotAt(3),
      mode: 'preview', instance: 's1', scope: 'components', bond_length_pt: 30,
      layout: { atoms: [...ATOM_IDS], bonds: ['b1', 'b2', 'b3'] },
      fixed_atom_count: 0, fixed_ring_systems: 0, boundary: [],
      components: [{ atoms: [...ATOM_IDS], bonds: ['b1', 'b2', 'b3'], anchor: null, root: 'a1', chains: [{ atoms: [...ATOM_IDS], branch_from: null }], rings: [] }],
      positions: ATOM_IDS.map((atom) => ({ atom, before: { ...DRAWN[atom] }, after: { ...PROPOSED[atom] } })),
    },
  }
}

/** A successful apply whose returned document is at `revision` and carries `placed`. */
function applyReply(revision, placed, ...extra) {
  return {
    envelope: 'operation_result', contract: 1, outcome: 'ok', model_id: LAYOUT_MODEL, document_revision: revision, loss_report: [],
    diagnostics: [...extra, notImplemented()],
    result: snapshotAt(revision, placed),
  }
}

/** A time limit stop, which changed nothing. `revision` is the reply's `document_revision`. */
function stopReply(revision) {
  return { envelope: 'operation_result', contract: 1, outcome: 'error', model_id: LAYOUT_MODEL, document_revision: revision, loss_report: [], result: null, diagnostics: [timeLimitDiagnostic()] }
}

/** An unsupported preview: the service names a reason and the atoms or limit it gives. */
function refusalReply(reason, details) {
  return {
    envelope: 'operation_result', contract: 1, outcome: 'unsupported', model_id: LAYOUT_MODEL, document_revision: 3, loss_report: [], result: null,
    diagnostics: [diagnostic('layout_unsupported', 'error', SERVICE_MESSAGE, { reason, ...details })],
  }
}

const MALFORMED_PREVIEWS = [
  ['a model id other than the layout model', (reply) => { reply.model_id = 'other' }],
  ['an instance other than the request', (reply) => { reply.result.instance = 's2' }],
  ['a scope other than the request', (reply) => { reply.result.scope = 'selection' }],
  ['positions out of atom order', (reply) => { const { positions } = reply.result; positions.splice(1, 2, positions[2], positions[1]) }],
  ['a layout atom that the snapshot does not contain', (reply) => { reply.result.layout.atoms[3] = 'a9'; reply.result.positions[3].atom = 'a9' }],
  ['a coordinate that is a string', (reply) => { reply.result.positions[0].after.x = '6.03' }],
  ['no layout_not_implemented diagnostic', (reply) => { reply.diagnostics = reply.diagnostics.filter((item) => item.code !== 'layout_not_implemented') }],
  ['an overlap pair naming an unknown atom', (reply) => { reply.diagnostics.push(overlapDiagnostic([['a1', 'a9']], 1)) }],
  ['an overlap count below its pair count', (reply) => { reply.diagnostics.push(overlapDiagnostic([['a1', 'a3']], 0)) }],
  ['a snapshot revision other than document_revision', (reply) => { reply.result.snapshot = snapshotAt(2) }],
]

const REJECTED_APPLIES = [
  ['layout_unchanged at revision 4', 4, DRAWN, [unchangedDiagnostic()]],
  ['a changed document at revision 5', 5, PROPOSED, []],
  ['a changed document at the expected revision', 3, PROPOSED, []],
  ['layout_preview_only', 4, PROPOSED, [previewOnlyDiagnostic()]],
]

test('a preview sends exactly the request it was given, copied at call time', async (t) => {
  const bodies = service(t, () => json(previewReply()))
  const request = requestOf(['a2'])
  const pending = new DocumentClient().previewLayout(request)
  request.atoms.push('a3')
  request.bonds.push('b1')
  const preview = await pending
  assert.deepEqual({ ...bodies[0], request_id: 'fixed' }, {
    contract: 1, request_id: 'fixed', profile: 'preserve-semantics', operation: 'layout.structure',
    parameters: { document_id: 'drawing', mode: 'preview', selection: { objects: [], atoms: ['a2'], bonds: [] }, scope: 'components', instance: 's1' },
  })
  assert.deepEqual(preview.request, requestOf(['a2']))
  assert.deepEqual({
    model: preview.model, atoms: preview.atoms, bondCount: preview.bondCount, componentCount: preview.componentCount,
    ringCount: preview.ringCount, bondLengthPt: preview.bondLengthPt, moved: layoutMovedCount(preview),
    noBonds: preview.noBonds, overlap: preview.overlap, notImplemented: preview.notImplemented,
  }, {
    model: LAYOUT_MODEL, atoms: ATOM_IDS, bondCount: 3, componentCount: 1, ringCount: 0, bondLengthPt: 30, moved: 4,
    noBonds: false, overlap: null, notImplemented: NOT_IMPLEMENTED,
  })
  assert.deepEqual(preview.positions[0], { atom: 'a1', before: { x: 0, y: 0 }, after: { x: 6.03, y: 7.5 } })
})

test('a layout_no_bonds diagnostic marks the preview as having no bonds', async (t) => {
  const reply = previewReply()
  reply.diagnostics.push(diagnostic('layout_no_bonds', 'info', 'No bonds lie between the laid-out atoms.'))
  service(t, () => json(reply))
  const preview = await new DocumentClient().previewLayout(requestOf(['a2']))
  assert.equal(preview.noBonds, true)
})

test('layoutMovedCount ignores differences within the 1e-9 pt tolerance', async (t) => {
  const reply = previewReply()
  reply.result.positions[0].after = { x: 0, y: 0 }
  reply.result.positions[1].after = { x: 30 + 1e-12, y: 0 }
  reply.result.positions[2].after = { x: 60.5, y: 0 }
  reply.result.positions[3].after = { x: 90, y: 2e-9 }
  service(t, () => json(reply))
  const preview = await new DocumentClient().previewLayout(requestOf(['a2']))
  assert.equal(layoutMovedCount(preview), 2)
})

for (const [name, change] of MALFORMED_PREVIEWS) {
  test(`a preview reply with ${name} is an incompatible response`, async (t) => {
    const reply = previewReply()
    change(reply)
    service(t, () => json(reply))
    await assert.rejects(new DocumentClient().previewLayout(requestOf(['a2'])), rejectedWith(ApiError, { code: 'incompatible_response' }))
  })
}

for (const [name, reason, details, atoms, limit] of [
  ['a ring only partly in the layout set', 'ring_partly_in_layout_set', { atoms: ['a1'] }, ['a1'], null],
  ['a ring above the size bound', 'large_ring_in_layout_set', { atoms: ['a5'], limit: 12 }, ['a5'], 12],
  ['a refusal that names no atoms or limit', 'large_ring_in_layout_set', {}, [], null],
]) {
  test(`an unsupported preview with ${name} rejects with a LayoutRefusal`, async (t) => {
    service(t, () => json(refusalReply(reason, details)))
    await assert.rejects(new DocumentClient().previewLayout(requestOf(['a2'])),
      rejectedWith(LayoutRefusal, { name: 'LayoutRefusal', reason, atoms, limit, message: SERVICE_MESSAGE }))
  })
}

test('an error reply other than a stop rejects with an ApiError and not a refusal', async (t) => {
  service(t, () => json({
    envelope: 'operation_result', contract: 1, outcome: 'error', model_id: LAYOUT_MODEL, document_revision: 3, loss_report: [], result: null,
    diagnostics: [diagnostic('invalid_request', 'error', 'The selection names no atom.')],
  }))
  await assert.rejects(new DocumentClient().previewLayout(requestOf(['a2'])),
    rejectedWith(ApiError, { code: 'invalid_request', message: 'The selection names no atom.' }))
})

test('a preview time limit stop with the snapshot revision is a ComputeStopped', async (t) => {
  service(t, () => json(stopReply(3)))
  await assert.rejects(new DocumentClient().previewLayout(requestOf(['a2'])),
    rejectedWith(ComputeStopped, { code: 'time_limit', stop: { kind: 'time_limit', limitMs: 5000 } }))
})

test('a preview time limit stop with a null revision is an incompatible response', async (t) => {
  service(t, () => json(stopReply(null)))
  await assert.rejects(new DocumentClient().previewLayout(requestOf(['a2'])), rejectedWith(ApiError, { code: 'incompatible_response' }))
})

test('an apply sends the previewed request under its expected revision and the history selection', async (t) => {
  const bodies = service(t, () => json(applyReply(4, PROPOSED)))
  await new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf())
  assert.deepEqual({ ...bodies[0], request_id: 'fixed' }, {
    contract: 1, request_id: 'fixed', profile: 'preserve-semantics', operation: 'layout.structure',
    parameters: {
      document_id: 'drawing', mode: 'apply', selection: { objects: [], atoms: ['a2'], bonds: [] }, scope: 'components', instance: 's1',
      expected_revision: 3, selection_before: selectionOf(), selection_after: selectionOf(),
    },
  })
})

test('an apply that moves atoms is one new revision and is not unchanged', async (t) => {
  service(t, () => json(applyReply(4, PROPOSED)))
  const applied = await new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf())
  assert.deepEqual({ unchanged: applied.unchanged, overlap: applied.overlap, revision: applied.response.document.revision },
    { unchanged: false, overlap: null, revision: 4 })
})

test('an apply that moves nothing keeps the revision and is unchanged', async (t) => {
  service(t, () => json(applyReply(3, DRAWN, unchangedDiagnostic())))
  const applied = await new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf())
  assert.deepEqual({ unchanged: applied.unchanged, revision: applied.response.document.revision }, { unchanged: true, revision: 3 })
})

test('an apply parses the layout_overlap warning of the reply', async (t) => {
  service(t, () => json(applyReply(4, PROPOSED, overlapDiagnostic([['a1', 'a3']], 1))))
  const applied = await new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf())
  assert.deepEqual(applied.overlap, { pairs: [['a1', 'a3']], count: 1 })
})

for (const [name, revision, placed, extra] of REJECTED_APPLIES) {
  test(`an apply reply with ${name} is an incompatible response`, async (t) => {
    service(t, () => json(applyReply(revision, placed, ...extra)))
    await assert.rejects(new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf()), rejectedWith(ApiError, { code: 'incompatible_response' }))
  })
}

test('an apply time limit stop with a null revision is a ComputeStopped', async (t) => {
  service(t, () => json(stopReply(null)))
  await assert.rejects(new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf()),
    rejectedWith(ComputeStopped, { code: 'time_limit', stop: { kind: 'time_limit', limitMs: 5000 } }))
})

test('an apply refused as a revision conflict reports the current revision', async (t) => {
  service(t, () => json({ api_version: 'v1', code: 'revision_conflict', message: 'The drawing changed since the preview.', current_revision: 5 }, 409))
  await assert.rejects(new DocumentClient().applyLayout(requestOf(['a2']), 3, selectionOf()),
    rejectedWith(ApiError, { code: 'revision_conflict', currentRevision: 5 }))
})

test('a ghost line runs from each end to its ghost or drawn position, in document order', () => {
  const bonds = [bondOf('b1', 'a1', 'a2'), bondOf('b2', 'a2', 'a3'), bondOf('b3', 'a3', 'a4'), bondOf('b4', 'a5', 'a6'), bondOf('b5', 'a3', 'a7')]
  const positions = new Map([['a2', { x: 1, y: 1 }], ['a3', { x: 2, y: 2 }]])
  const drawnAt = new Map([['a1', { x: 0, y: 0 }], ['a4', { x: 90, y: 0 }]])
  assert.deepEqual(layoutGhostSegments(bonds, positions, (atom) => drawnAt.get(atom) ?? null), [
    { bond: 'b1', from: { x: 0, y: 0 }, to: { x: 1, y: 1 } },
    { bond: 'b2', from: { x: 1, y: 1 }, to: { x: 2, y: 2 } },
    { bond: 'b3', from: { x: 2, y: 2 }, to: { x: 90, y: 0 } },
  ])
})

test('a bond with no moved end, or with a fixed end that cannot be placed, is left out of the ghost', () => {
  const bonds = [bondOf('b4', 'a5', 'a6'), bondOf('b5', 'a3', 'a7')]
  const positions = new Map([['a3', { x: 2, y: 2 }]])
  assert.deepEqual(layoutGhostSegments(bonds, positions, () => null), [])
})

test('each of the ten layout_unsupported reasons has its own non-empty sentence', () => {
  const reasons = ['object_selection', 'ring_partly_in_layout_set', 'fused_ring_in_layout_set', 'spiro_ring_in_layout_set',
    'large_ring_in_layout_set', 'stereo_in_layout', 'display_in_layout_set', 'multiple_attachments',
    'coincident_fixed_atoms', 'coordinate_out_of_range']
  const sentences = reasons.map((reason) => layoutRefusalSentence(reason, null))
  assert.ok(sentences.every((sentence) => typeof sentence === 'string' && sentence.length > 0))
  assert.equal(new Set(sentences).size, reasons.length)
})

test('the large-ring sentence names the bound the service gave, and 12 when it gave none', () => {
  assert.equal(layoutRefusalSentence('large_ring_in_layout_set', 8), 'Rings of more than 8 atoms are not laid out yet.')
  assert.equal(layoutRefusalSentence('large_ring_in_layout_set', null), 'Rings of more than 12 atoms are not laid out yet.')
})

for (const reason of ['unknown_reason', 'constructor', '__proto__']) {
  test(`the reason "${reason}" has no layout sentence`, () => {
    assert.equal(layoutRefusalSentence(reason, null), null)
  })
}

const baseRequest = () => requestOf(['a1', 'a2'], ['b1'])

test('a structured copy of a layout request is the same request', () => {
  assert.equal(sameLayoutRequest(baseRequest(), structuredClone(baseRequest())), true)
})

for (const [name, change] of [
  ['document', (request) => { request.documentId = 'other' }],
  ['instance', (request) => { request.instance = 's2' }],
  ['scope', (request) => { request.scope = 'selection' }],
  ['atom order', (request) => { request.atoms = ['a2', 'a1'] }],
  ['atom count', (request) => { request.atoms = ['a1'] }],
  ['bond', (request) => { request.bonds = ['b2'] }],
  ['bond count', (request) => { request.bonds = ['b1', 'b2'] }],
]) {
  test(`a layout request whose ${name} differs is a different request`, () => {
    const other = baseRequest()
    change(other)
    assert.equal(sameLayoutRequest(baseRequest(), other), false)
  })
}
