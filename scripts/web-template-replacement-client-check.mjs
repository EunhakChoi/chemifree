import assert from 'node:assert/strict'
import { test } from 'node:test'
import { replacementCase } from './lib/template-replacement-fixture.mjs'
import { templateReply } from './lib/template-attachment-fixture.mjs'
const { DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')
const json = (value, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } })
function service(t, action) {
  const requests = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'replacement-service' })
    assert.equal(path, '/api/v1/operations')
    const body = JSON.parse(init.body)
    requests.push(body)
    return action(body, requests)
  })
  return requests
}
test('single-atom identical replacement is a no-op with the original selection and revision', async (t) => {
  const example = replacementCase()
  const source = example.result.source
  const raw = JSON.parse(example.parameters.payload)
  raw.atoms = [raw.atoms.find((atom) => atom.id === 'a10')]
  raw.atoms[0].kind.symbol = 'F'
  raw.bonds = []
  raw.stereo = []
  const parameters = { ...example.parameters, payload: JSON.stringify(raw) }
  const snapshot = structuredClone(source)
  snapshot.notes = []
  const result = { source, snapshot, mapping: { atoms: [{ source: 'a10', target: 'a3' }], bonds: [] }, boundary: 'b2' }
  service(t, (body) => {
    if (body.operation === 'document.template_replace_plan') return json(templateReply(body, result))
    const { source: _source, ...applied } = result
    return json(templateReply(body, applied))
  })
  const client = new DocumentClient()
  const offer = await client.previewTemplateReplacement(source.document, parameters)
  assert.deepEqual(offer.preview.document, source.document)
  assert.deepEqual((await client.createTemplateReplacement(offer).send()).snapshot.document, source.document)
})
for (const kind of ['center', 'double']) {
  test(`replacement retains the boundary and old stereo while canonicalizing copied ${kind}`, async (t) => {
    const example = replacementCase(kind)
    service(t, (body) => json(templateReply(body, example.result)))
    const client = new DocumentClient()
    const offer = await client.previewTemplateReplacement(example.result.source.document, example.parameters)
    assert.deepEqual(offer.preview.document.stereo, example.result.snapshot.document.stereo)
    assert.equal(offer.boundary, 'b2')
    assert.equal(client.isCurrentSnapshot(offer.source), true)
    assert.equal(client.isCurrentSnapshot(offer.preview), false)
  })
}
for (const [name, change] of [
  ['outside mark removed', (result) => { result.snapshot.document.bonds[1].mark = null }],
  ['outside reference removed', (result) => { result.snapshot.document.scene.references.pop() }],
  ['neighbor statement changed', (result) => { result.snapshot.document.stereo.atoms[0].configuration.turn = 'anticlockwise' }],
  ['copied statement changed', (result) => { result.snapshot.document.stereo.atoms[1].configuration.turn = 'anticlockwise' }],
  ['bound state omitted', (result) => { result.snapshot.document.atoms[2].element = 'F' }],
  ['old target position moved', (result) => {
    result.snapshot.document.atoms[2].position.x = 1
    result.snapshot.document.scene.instances[0].positions[2].x = 1
  }],
  ['different boundary named', (result) => { result.boundary = 'b3' }],
  ['fresh state altered', (result) => { result.snapshot.document.atoms[4].isotope = 13 }],
]) {
  test(`replacement preview refuses ${name}`, async (t) => {
    const example = replacementCase()
    change(example.result)
    service(t, (body) => json(templateReply(body, example.result)))
    await assert.rejects(new DocumentClient().previewTemplateReplacement(example.result.source.document, example.parameters), { code: 'incompatible_response' })
  })
}
test('replacement reply loss freezes target, file, mapping and body across repeated outcome checks', async (t) => {
  const example = replacementCase()
  const requests = service(t, (body, all) => {
    if (body.operation === 'document.template_replace_plan') return json(templateReply(body, example.result))
    assert.equal(body.operation, 'document.template_replace')
    if (all.length <= 3) throw new TypeError('Committed replacement reply lost.')
    const { source: _source, ...result } = example.result
    return json(templateReply(body, result))
  })
  const client = new DocumentClient()
  const offer = await client.previewTemplateReplacement(example.result.source.document, example.parameters)
  const attempt = client.createTemplateReplacement(offer)
  offer.parameters.atom = 'a4'
  offer.preview.document.title = 'Later changes cannot redirect the replacement'
  await assert.rejects(attempt.send(), { code: 'clipboard_paste_unconfirmed' })
  const result = await attempt.send()
  assert.deepEqual(result.snapshot.document, example.result.snapshot.document)
  assert.deepEqual(requests.slice(1), [requests[1], requests[1], requests[1]])
  assert.equal(requests[1].parameters.atom, 'a3')
  assert.equal(await attempt.send(), result)
  assert.equal(requests.length, 4)
})
for (const [code, status, expected] of [['unauthorized', 401, 'clipboard_paste_restarted'], ['revision_conflict', 409, 'clipboard_paste_reconcile']]) {
  test(`replacement never resends after ${code}`, async (t) => {
    const example = replacementCase('double')
    const requests = service(t, (body, all) => {
      if (body.operation === 'document.template_replace_plan') return json(templateReply(body, example.result))
      if (all.length <= 3) throw new TypeError('Lost reply.')
      return json({ api_version: 'v1', code, message: 'Reconcile explicitly.' }, status)
    })
    const client = new DocumentClient()
    const attempt = client.createTemplateReplacement(await client.previewTemplateReplacement(example.result.source.document, example.parameters))
    await assert.rejects(attempt.send(), { code: 'clipboard_paste_unconfirmed' })
    await assert.rejects(attempt.send(), { code: expected })
    await assert.rejects(attempt.send(), { code: expected })
    assert.equal(requests.length, 4)
    assert.deepEqual(requests.slice(1), [requests[1], requests[1], requests[1]])
  })
}
