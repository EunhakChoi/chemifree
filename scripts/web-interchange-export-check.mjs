// Focused operation-boundary checks for the drawing export client (CDXML/CDX/SVG).
import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { ApiError, ConsentRequired, DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')

const digest = 'a'.repeat(64)
const stereoLoss = {
  feature: 'stereo_configuration', source_path: 'document.atoms[a1]', disposition: 'dropped',
  reason: 'The stored configuration differs from the written drawing.', semantic_impact: 'stereo',
}
const snapshot = () => ({
  document: { id: 'drawing', title: 'Stereo', revision: 3, atoms: [], bonds: [], stereo: { atoms: [], bonds: [], groups: [] } },
  session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null },
  recovery: null,
})
const envelope = (request, overrides = {}) => ({
  envelope: 'operation_result', contract: 1, request_id: request.request_id,
  operation: 'document.export', profile: 'preserve-semantics', document_revision: 3,
  outcome: 'ok', diagnostics: [], loss_report: [], result: { content: '<CDXML/>', snapshot: snapshot() },
  ...overrides,
})

const radicalLoss = {
  feature: 'radical_mark', source_path: 'document.atoms[a1]', disposition: 'dropped',
  reason: 'The radical mark is left out of the picture.', semantic_impact: 'chemistry',
}
const chemicalGraphLoss = {
  feature: 'chemical_graph', source_path: 'document', disposition: 'dropped',
  reason: 'The picture carries no machine-readable chemical graph.', semantic_impact: 'chemistry',
}
const svgContent = '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"/>'
/** A valid SVG reply result. Each call returns fresh objects, so a test may change its own copy. */
const svgResult = (overrides = {}) => ({
  content: svgContent, media_type: 'image/svg+xml', view_box: { x: 0, y: 0, width: 120, height: 80 },
  font_table: 'chemifree-label-advance-1', snapshot: snapshot(), ...overrides,
})
/** The export that a valid svgResult() reply from model chemifree-svg-3 must yield, with the given losses. */
const svgExpected = (losses = []) => ({
  content: svgContent, media_type: 'image/svg+xml', view_box: { x: 0, y: 0, width: 120, height: 80 },
  font_table: 'chemifree-label-advance-1', model: 'chemifree-svg-3', snapshot: snapshot().document, losses,
})

/** HTTP alone is replaced; the real client validates every response and request. */
function scripted(t, reply) {
  const requests = []
  t.mock.method(globalThis, 'fetch', async (url, init = {}) => {
    const json = (body, status = 200) => new Response(JSON.stringify(body), {
      status, headers: { 'content-type': 'application/json' },
    })
    if (url === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'test-session' })
    assert.equal(url, '/api/v1/operations')
    assert.equal(init.method, 'POST')
    assert.equal(init.headers['x-chemifree-session'], 'test-session')
    const request = JSON.parse(init.body)
    requests.push(request)
    return json(reply(request))
  })
  return requests
}

for (const format of ['cdxml', 'cdx']) {
  test(`drawing ${format} export sends only the explicit drawing selector and accepted digest`, async (t) => {
    const content = format === 'cdxml' ? '<CDXML/>' : 'VmpDRDAxMDA='
    const requests = scripted(t, (request) => envelope(request, { result: { content, snapshot: snapshot() } }))
    const written = await new DocumentClient().exportInterchange('drawing', format, digest)
    assert.deepEqual(requests[0].parameters, { document_id: 'drawing', format, accepted_losses: digest })
    assert.equal(requests[0].operation, 'document.export')
    assert.deepEqual(written, { content, format, snapshot: snapshot().document, losses: [] })
  })
}

test('an initial drawing export omits loss acceptance until the caller has reviewed a refusal', async (t) => {
  const requests = scripted(t, (request) => envelope(request, {
    document_revision: null, outcome: 'error', result: null,
    diagnostics: [{ code: 'loss_consent_required', message: 'Review these losses.',
      details: { features: ['stereo_configuration'], losses: [stereoLoss], loss_digest: digest } }],
  }))
  const client = new DocumentClient()
  await assert.rejects(client.exportInterchange('drawing', 'cdxml'), (error) => {
    assert.ok(error instanceof ConsentRequired)
    assert.equal(error.digest, digest)
    assert.deepEqual(error.features, ['stereo_configuration'])
    assert.deepEqual(error.losses, [stereoLoss])
    return true
  })
  assert.deepEqual(requests[0].parameters, { document_id: 'drawing', format: 'cdxml' })
})

for (const [name, change] of [
  ['different operation', (reply) => { reply.operation = 'document.import' }],
  ['different request', (reply) => { reply.request_id = 'another-request' }],
  ['different contract', (reply) => { reply.contract = 2 }],
  ['different profile', (reply) => { reply.profile = 'guessed-semantics' }],
  ['different envelope', (reply) => { reply.envelope = 'document_result' }],
  ['different drawing', (reply) => { reply.result.snapshot.document.id = 'other' }],
  ['revision mismatch', (reply) => { reply.document_revision = 2 }],
  ['invalid snapshot', (reply) => { reply.result.snapshot.document.atoms = [{ id: 'a1' }] }],
  ['a successful result carrying an error', (reply) => { reply.diagnostics = [{ code: 'bad', message: 'Bad' }] }],
  ['an unknown loss disposition', (reply) => { reply.outcome = 'partial'; reply.loss_report = [{ ...stereoLoss, disposition: 'retained_maybe' }] }],
]) {
  test(`drawing export rejects ${name}`, async (t) => {
    scripted(t, (request) => { const reply = envelope(request); change(reply); return reply })
    await assert.rejects(new DocumentClient().exportInterchange('drawing', 'cdxml'), { code: 'incompatible_response' })
  })
}

for (const content of ['', 'not base64', 'VmpDRDAxMDA', 'VmpDRDAxMDA=\n', 'VmpDRDAxMDB=']) {
  test(`CDX export rejects noncanonical or empty binary text ${JSON.stringify(content)}`, async (t) => {
    scripted(t, (request) => envelope(request, { result: { content, snapshot: snapshot() } }))
    await assert.rejects(new DocumentClient().exportInterchange('drawing', 'cdx'), { code: 'incompatible_response' })
  })
}

for (const [name, details] of [
  ['missing digest', { features: ['stereo_configuration'], losses: [stereoLoss] }],
  ['uppercase digest', { features: ['stereo_configuration'], losses: [stereoLoss], loss_digest: 'A'.repeat(64) }],
  ['feature mismatch', { features: ['stereo_group'], losses: [stereoLoss], loss_digest: digest }],
  ['unknown semantic impact', { features: ['stereo_configuration'], losses: [{ ...stereoLoss, semantic_impact: 'perhaps' }], loss_digest: digest }],
]) {
  test(`drawing export rejects a consent offer with ${name}`, async (t) => {
    scripted(t, (request) => envelope(request, {
      document_revision: null, outcome: 'error', result: null,
      diagnostics: [{ code: 'loss_consent_required', message: 'Accept.', details }],
    }))
    await assert.rejects(new DocumentClient().exportInterchange('drawing', 'cdxml'), { code: 'incompatible_response' })
  })
}

test('a service restart never silently retries a drawing export with old consent', async (t) => {
  let sends = 0
  t.mock.method(globalThis, 'fetch', async (url) => {
    if (url === '/api/v1/session') return new Response(JSON.stringify({ header: 'x-chemifree-session', token: `token-${sends}` }), { status: 200 })
    sends += 1
    return new Response(JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'Old service.' }), { status: 401 })
  })
  const client = new DocumentClient()
  let restarts = 0
  client.onRestart(() => { restarts += 1 })
  await assert.rejects(client.exportInterchange('drawing', 'cdxml', digest), { code: 'service_restarted' })
  assert.equal(sends, 1, 'export cannot resend stale acceptance to another process')
  assert.equal(restarts, 1)
})

test('svg export sends only the drawing selector and format before any consent', async (t) => {
  const requests = scripted(t, (request) => envelope(request, { model_id: 'chemifree-svg-3', result: svgResult() }))
  await new DocumentClient().exportSvg('drawing')
  assert.equal(requests[0].operation, 'document.export')
  assert.deepEqual(requests[0].parameters, { format: 'svg', document_id: 'drawing' })
})

test('svg export adds accepted_losses only when a consent digest is given', async (t) => {
  const requests = scripted(t, (request) => envelope(request, { model_id: 'chemifree-svg-3', result: svgResult() }))
  await new DocumentClient().exportSvg('drawing', digest)
  assert.deepEqual(requests[0].parameters, { format: 'svg', document_id: 'drawing', accepted_losses: digest })
})

test('svg export returns the picture, view box, font table, model, snapshot and losses of a partial reply', async (t) => {
  scripted(t, (request) => envelope(request, {
    outcome: 'partial', model_id: 'chemifree-svg-3', loss_report: [chemicalGraphLoss], result: svgResult(),
  }))
  assert.deepEqual(await new DocumentClient().exportSvg('drawing'), svgExpected([chemicalGraphLoss]))
})

test('svg export ignores an unknown top-level reply field', async (t) => {
  scripted(t, (request) => envelope(request, {
    model_id: 'chemifree-svg-3', result: svgResult(), unknown_reply_field: 'ignored',
  }))
  assert.deepEqual(await new DocumentClient().exportSvg('drawing'), svgExpected())
})

test('svg export ignores an unknown result field and does not copy it into the export', async (t) => {
  scripted(t, (request) => envelope(request, {
    model_id: 'chemifree-svg-3', result: svgResult({ unknown_result_field: 'ignored' }),
  }))
  assert.deepEqual(await new DocumentClient().exportSvg('drawing'), svgExpected())
})

test('svg export ignores an unknown view box field and does not copy it into the export', async (t) => {
  scripted(t, (request) => envelope(request, {
    model_id: 'chemifree-svg-3',
    result: svgResult({ view_box: { x: 0, y: 0, width: 120, height: 80, unknown_view_box_field: 'ignored' } }),
  }))
  assert.deepEqual(await new DocumentClient().exportSvg('drawing'), svgExpected())
})

test('svg export accepts the empty drawing view box 0 0 0 0', async (t) => {
  scripted(t, (request) => envelope(request, {
    model_id: 'chemifree-svg-3', result: svgResult({ view_box: { x: 0, y: 0, width: 0, height: 0 } }),
  }))
  assert.deepEqual(await new DocumentClient().exportSvg('drawing'), {
    ...svgExpected(), view_box: { x: 0, y: 0, width: 0, height: 0 },
  })
})

test('svg export surfaces a consent refusal as ConsentRequired with its digest, features and losses', async (t) => {
  scripted(t, (request) => envelope(request, {
    document_revision: null, outcome: 'error', result: null,
    diagnostics: [{ code: 'loss_consent_required', message: 'Review these losses.',
      details: { features: ['radical_mark'], losses: [radicalLoss], loss_digest: digest } }],
  }))
  await assert.rejects(new DocumentClient().exportSvg('drawing'), (error) => {
    assert.ok(error instanceof ConsentRequired)
    assert.equal(error.digest, digest)
    assert.deepEqual(error.features, ['radical_mark'])
    assert.deepEqual(error.losses, [radicalLoss])
    return true
  })
})

test('svg export reports changed loss consent as ApiError, never as ConsentRequired', async (t) => {
  // As CONTRACT describes for the SVG export, details hold the current losses and digest, so only the code tells the two apart.
  scripted(t, (request) => envelope(request, {
    document_revision: null, outcome: 'error', result: null,
    diagnostics: [{ code: 'loss_consent_changed', message: 'The losses changed since you accepted them.',
      details: { features: ['radical_mark'], losses: [radicalLoss], loss_digest: digest } }],
  }))
  await assert.rejects(new DocumentClient().exportSvg('drawing', digest), (error) => {
    assert.ok(error instanceof ApiError)
    assert.ok(!(error instanceof ConsentRequired), 'a changed consent must not be offered as a new consent request')
    assert.equal(error.code, 'loss_consent_changed')
    return true
  })
})

for (const [name, change] of [
  ['a media type other than image/svg+xml', (reply) => { reply.result.media_type = 'image/png' }],
  ['empty content', (reply) => { reply.result.content = '' }],
  ['a view box width of -1', (reply) => { reply.result.view_box.width = -1 }],
  // JSON cannot carry NaN, so the client receives null, which is not a finite number.
  ['a view box x that is NaN', (reply) => { reply.result.view_box.x = Number.NaN }],
  ['a view box without y', (reply) => { delete reply.result.view_box.y }],
  ['a font table that is not a string', (reply) => { reply.result.font_table = 7 }],
  ['no model_id', (reply) => { delete reply.model_id }],
  ['a snapshot of another drawing', (reply) => { reply.result.snapshot.document.id = 'other' }],
  ['a document_revision that differs from the snapshot', (reply) => { reply.document_revision = 2 }],
]) {
  test(`svg export rejects ${name} as incompatible_response`, async (t) => {
    scripted(t, (request) => {
      const reply = envelope(request, { model_id: 'chemifree-svg-3', result: svgResult() })
      change(reply)
      return reply
    })
    await assert.rejects(new DocumentClient().exportSvg('drawing'), { code: 'incompatible_response' })
  })
}

for (const [name, malformed] of [
  ['an uppercase digest', 'A'.repeat(64)],
  ['a short digest', 'a'.repeat(63)],
  ['a digest with a non-hex character', `${'a'.repeat(63)}g`],
]) {
  test(`svg export refuses ${name} before sending a request`, async (t) => {
    let fetches = 0
    t.mock.method(globalThis, 'fetch', async () => {
      fetches += 1
      return new Response('{}', { status: 500 })
    })
    await assert.rejects(new DocumentClient().exportSvg('drawing', malformed), { code: 'invalid_request' })
    assert.equal(fetches, 0)
  })
}

const { lossHeading } = await import('../apps/web/src/lib/ct-conversion.ts')
for (const [feature, sentence] of [
  ['stereo_group', 'One or more enhanced stereo groups are omitted; the reported reason identifies the affected groups.'],
  ['stereo_group_identity', 'Native group identifiers (e1, e2, ...) are not embedded; group content is accounted for separately.'],
  ['stereo_ungrouped_centers', 'Stored tetrahedral centers without an enhanced stereo group: the drawing may carry their configuration, but another reader’s enhanced stereo interpretation is uncertain.'],
  ['double_bond_configuration', 'A stored double-bond configuration that this interchange output does not retain; a drawn mark can still be kept.'],
]) {
  test(`the ${feature} heading distinguishes the lost statement from retained drawing content`, () => {
    assert.equal(lossHeading(feature), sentence)
  })
}
