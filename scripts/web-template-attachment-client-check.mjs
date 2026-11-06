import assert from 'node:assert/strict'
import { test } from 'node:test'
import { attachSource } from './lib/attach-ring-fixture.mjs'
import { templateParameters, templatePayload, templateResult, templateReply } from './lib/template-attachment-fixture.mjs'
const { DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')
const json = (value, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } })
function service(t, action) {
  const requests = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'template-service' })
    assert.equal(path, '/api/v1/operations')
    const body = JSON.parse(init.body)
    requests.push(body)
    return action(body, requests)
  })
  return requests
}
test('template inspection rejects a fabricated source ID list', async (t) => {
  service(t, (body) => json(templateReply(body, { payload: templatePayload(), atoms: [{ id: 'a99', element: 'Cl', x: 100, y: 100, supported: true }] })))
  await assert.rejects(new DocumentClient().inspectTemplate(templatePayload()), { code: 'incompatible_response' })
})
test('read-only candidate is not a live snapshot; immutable apply preserves body, notes and mapping after reply loss', async (t) => {
  const requests = service(t, (body, all) => {
    if (body.operation === 'document.template_plan') return json(templateReply(body, templateResult(true)))
    assert.equal(body.operation, 'document.template_attach')
    if (all.length <= 3) throw new TypeError('Committed reply lost.')
    return json(templateReply(body, templateResult()))
  })
  const client = new DocumentClient()
  const parameters = templateParameters()
  const offer = await client.previewTemplateAttachment(attachSource().document, parameters)
  assert.equal(client.isCurrentSnapshot(offer.source), true)
  assert.equal(client.isCurrentSnapshot(offer.preview), false)
  const attempt = client.createTemplateAttachment(offer)
  parameters.direction.dy = 9
  offer.parameters.atom = 'a2'
  offer.preview.document.title = 'Changed after capture'
  await assert.rejects(attempt.send(), { code: 'clipboard_paste_unconfirmed' })
  const result = await attempt.send()
  assert.deepEqual(result.snapshot.document, templateResult().snapshot.document)
  assert.deepEqual(requests.slice(1), [requests[1], requests[1], requests[1]])
  assert.equal(requests[1].parameters.atom, 'a1')
  assert.deepEqual(requests[1].parameters.direction, { dx: 1, dy: 0 })
  assert.equal(await attempt.send(), result)
  assert.equal(requests.length, 4)
})
for (const [name, change] of [
  ['changed destination', (result) => { result.source.document.title = 'Other drawing' }],
  ['altered original mark', (result) => { result.snapshot.document.bonds[0].mark = null }],
  ['wrong connector', (result) => { result.snapshot.document.bonds.at(-1).begin = 'a2' }],
  ['missing notes', (result) => { result.snapshot.notes = [] }],
  ['wrong fresh state', (result) => { result.snapshot.document.atoms.at(-1).charge = 1 }],
  ['wrong translation', (result) => { result.snapshot.document.atoms.at(-1).position.x = 40; result.snapshot.document.scene.instances[0].positions.at(-1).x = 40 }],
]) {
  test(`template preview refuses ${name}`, async (t) => {
    const result = templateResult(true)
    change(result)
    service(t, (body) => json(templateReply(body, result)))
    await assert.rejects(new DocumentClient().previewTemplateAttachment(attachSource().document, templateParameters()), { code: 'incompatible_response' })
  })
}
for (const [code, status, expected] of [['unauthorized', 401, 'clipboard_paste_restarted'], ['revision_conflict', 409, 'clipboard_paste_reconcile']]) {
  test(`template recovery never changes source or retries after ${code}`, async (t) => {
    const requests = service(t, (body, all) => {
      if (body.operation === 'document.template_plan') return json(templateReply(body, templateResult(true)))
      if (all.length <= 3) throw new TypeError('Lost response.')
      return json({ api_version: 'v1', code, message: 'Explicit reconciliation required.' }, status)
    })
    const client = new DocumentClient()
    const offer = await client.previewTemplateAttachment(attachSource().document, templateParameters())
    const attempt = client.createTemplateAttachment(offer)
    await assert.rejects(attempt.send(), { code: 'clipboard_paste_unconfirmed' })
    await assert.rejects(attempt.send(), { code: expected })
    await assert.rejects(attempt.send(), { code: expected })
    assert.equal(requests.length, 4)
    assert.deepEqual(requests.slice(1), [requests[1], requests[1], requests[1]])
  })
}
test('a success differing from the frozen preview remains unconfirmed', async (t) => {
  service(t, (body) => {
    const result = templateResult(body.operation === 'document.template_plan')
    if (body.operation === 'document.template_attach') result.snapshot.document.title = 'Unexpected change'
    return json(templateReply(body, result))
  })
  const client = new DocumentClient()
  const attempt = client.createTemplateAttachment(await client.previewTemplateAttachment(attachSource().document, templateParameters()))
  await assert.rejects(attempt.send(), { code: 'clipboard_paste_unconfirmed' })
})

const { templateStereoCase } = await import('./lib/template-attachment-fixture.mjs')
for (const kind of ['center', 'double']) {
  test(`template copied ${kind} accepts the hand-stated canonical remapping after ID permutation`, async (t) => {
    const example = templateStereoCase(kind)
    service(t, (body) => json(templateReply(body, example.result)))
    const offer = await new DocumentClient().previewTemplateAttachment(attachSource().document, example.parameters)
    assert.deepEqual(offer.preview.document.stereo, example.result.snapshot.document.stereo)
  })
  test(`template copied ${kind} refuses a valid record whose stereo statement changed`, async (t) => {
    const example = templateStereoCase(kind)
    if (kind === 'center') example.result.snapshot.document.stereo.atoms[0].configuration.turn = 'clockwise'
    else example.result.snapshot.document.stereo.bonds[0].configuration.relation = 'opposite_sides'
    service(t, (body) => json(templateReply(body, example.result)))
    await assert.rejects(new DocumentClient().previewTemplateAttachment(attachSource().document, example.parameters), { code: 'incompatible_response' })
  })
  test(`template copied ${kind} refuses known-to-unknown normalization`, async (t) => {
    const example = templateStereoCase(kind)
    if (kind === 'center') example.result.snapshot.document.stereo.atoms[0].configuration = { kind: 'unknown', neighbors: ['a6', 'a7', 'a8', 'a9'] }
    else example.result.snapshot.document.stereo.bonds[0].configuration = { kind: 'unknown' }
    service(t, (body) => json(templateReply(body, example.result)))
    await assert.rejects(new DocumentClient().previewTemplateAttachment(attachSource().document, example.parameters), { code: 'incompatible_response' })
  })
}

test('template preview rejects unsupported destination and direction before network access', async (t) => {
  const requests = service(t, () => { throw new Error('Must not send.') })
  const client = new DocumentClient()
  const before = attachSource().document
  await assert.rejects(client.previewTemplateAttachment(before, { ...templateParameters(), atom: 'a99' }), { code: 'template_destination_invalid' })
  await assert.rejects(client.previewTemplateAttachment(before, { ...templateParameters(), direction: { dx: 0, dy: 0 } }), { code: 'template_direction_invalid' })
  await assert.rejects(client.previewTemplateAttachment(before, { ...templateParameters(), direction: { dx: Infinity, dy: 1 } }), { code: 'template_direction_invalid' })
  before.scene.instances.push({ ...before.scene.instances[0], id: 's99' })
  await assert.rejects(client.previewTemplateAttachment(before, templateParameters()), { code: 'template_destination_invalid' })
  assert.deepEqual(requests, [])
})
