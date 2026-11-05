// Focused operation-boundary checks for the CDXML/CDX drawing export client.
import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { ConsentRequired, DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')

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
