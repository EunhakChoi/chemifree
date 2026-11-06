import assert from 'node:assert/strict'
import { test } from 'node:test'
import { attachAfter, attachSource } from './lib/attach-ring-fixture.mjs'
import { ringSelection } from './lib/ring-fixture.mjs'
const { DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')
const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
function service(t, reply) {
  const bodies = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'attach-service' })
    assert.equal(path, '/api/v1/documents/drawing/transactions')
    const body = JSON.parse(init.body)
    bodies.push(body)
    return reply(body, bodies.length)
  })
  return bodies
}
for (const [size, pattern] of [[5, 'single'], [6, 'single'], [6, 'kekule']]) {
  test(`attached ${size}-ring ${pattern} freezes target/direction/size/pattern/selection and accepts exact ordinary stereo clearing`, async (t) => {
    const bodies = service(t, (_body, count) => {
      if (count <= 2) throw new TypeError('Committed reply lost.')
      return json(attachAfter(size, pattern))
    })
    const direction = { dx: 50, dy: 0 }
    const selection = ringSelection()
    const source = attachSource().document
    const attempt = new DocumentClient().createRingAttachment(source, 's1', 'a1', direction, selection, size, pattern)
    direction.dy = 500
    selection.atoms.length = 0
    source.atoms[0].position.x = 900
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
    const result = await attempt.send()
    assert.equal(result.document.atoms.length, 4 + size)
    assert.equal(result.document.bonds.length, 4 + size)
    assert.deepEqual(result.document.stereo, { atoms: [], bonds: [], groups: [] })
    assert.equal(result.notes.length, 4)
    assert.deepEqual(bodies[0].commands, [{ op: 'attach_ring', instance: 's1', atom: 'a1', direction: { dx: 50, dy: 0 }, ...(size === 6 ? {} : { size }), ...(pattern === 'single' ? {} : { pattern }) }])
    assert.equal(result.document.bonds.at(-1).order, 'single')
    assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
    assert.equal(await attempt.send(), result)
    assert.equal(bodies.length, 3)
  })
}
for (const [name, change] of [
  ['missing connection', (reply) => { reply.document.bonds.pop() }],
  ['wrong connection target', (reply) => { reply.document.bonds.at(-1).begin = 'a2' }],
  ['missing cycle edge', (reply) => { reply.document.bonds[8].end = 'a6' }],
  ['wrong direction', (reply) => { reply.document.atoms[4].position.x += 1; reply.document.scene.instances[0].positions[4].x += 1 }],
  ['altered old mark', (reply) => { reply.document.bonds[0].mark = null }],
  ['missing notes', (reply) => { reply.notes = [] }],
  ['missing group removal note', (reply) => { reply.notes.pop() }],
  ['duplicate note', (reply) => { reply.notes[1] = reply.notes[0] }],
  ['wrong note target', (reply) => { reply.notes[0].source_path = 'atoms[a2]' }],
  ['unexpected note', (reply) => { reply.notes.push({ ...reply.notes[0], source_path: 'atoms[a2]' }) }],
]) {
  test(`attachment refuses a claimed success with ${name}`, async (t) => {
    const reply = attachAfter(6)
    change(reply)
    service(t, () => json(reply))
    const attempt = new DocumentClient().createRingAttachment(attachSource().document, 's1', 'a1', { dx: 1, dy: 0 }, ringSelection())
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  })
}
test('attachment rejects missing target, invalid size and zero/nonfinite direction without sending', (t) => {
  const bodies = service(t, () => { throw new Error('Must not send.') })
  const source = attachSource().document
  const client = new DocumentClient()
  assert.throws(() => client.createRingAttachment(source, 's1', 'a99', { dx: 1, dy: 0 }, ringSelection()), { code: 'ring_destination_invalid' })
  assert.throws(() => client.createRingAttachment(source, 's1', 'a1', { dx: 1, dy: 0 }, ringSelection(), null), { code: 'ring_size_invalid' })
  for (const direction of [{ dx: 0, dy: 0 }, { dx: NaN, dy: 1 }, { dx: Infinity, dy: 1 }]) {
    assert.throws(() => client.createRingAttachment(source, 's1', 'a1', direction, ringSelection()), { code: 'ring_direction_invalid' })
  }
  assert.deepEqual(bodies, [])
})
for (const [code, status, expected, size, pattern] of [
  ['unauthorized', 401, 'clipboard_ring_restarted', 5, 'single'],
  ['revision_conflict', 409, 'clipboard_ring_reconcile', 5, 'single'],
  ['unauthorized', 401, 'clipboard_ring_restarted', 6, 'kekule'],
  ['revision_conflict', 409, 'clipboard_ring_reconcile', 6, 'kekule'],
]) {
  test(`attachment ${pattern} recovery stops after ${code}`, async (t) => {
    const bodies = service(t, (_body, count) => {
      if (count <= 2) throw new TypeError('Reply lost.')
      return json({ api_version: 'v1', code, message: 'Inspect the authoritative document.' }, status)
    })
    const attempt = new DocumentClient().createRingAttachment(attachSource().document, 's1', 'a1', { dx: 1, dy: 0 }, ringSelection(), size, pattern)
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
    await assert.rejects(attempt.send(), { code: expected })
    await assert.rejects(attempt.send(), { code: expected })
    assert.equal(bodies.length, 3)
    assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
    assert.equal(bodies[0].commands[0].pattern, pattern === 'single' ? undefined : pattern)
  })
}

function attachedAtOtherEnd() {
  const reply = attachAfter()
  const original = attachSource()
  // The other double-bond end loses that bond record, but a1's neighbor set is unchanged.
  reply.document.stereo = { ...structuredClone(original.document.stereo), bonds: [] }
  for (const atom of reply.document.atoms.slice(4)) atom.position.x += 30
  for (const position of reply.document.scene.instances[0].positions.slice(4)) position.x += 30
  reply.document.bonds.at(-1).begin = 'a2'
  reply.notes = reply.notes.filter((note) => note.source_path === 'bonds[b1]')
  return reply
}

test('attachment at the other double-bond end retains the neighboring center and its group', async (t) => {
  service(t, () => json(attachedAtOtherEnd()))
  const attempt = new DocumentClient().createRingAttachment(attachSource().document, 's1', 'a2', { dx: 1, dy: 0 }, ringSelection())
  const result = await attempt.send()
  assert.deepEqual(result.document.stereo.atoms, attachSource().document.stereo.atoms)
  assert.deepEqual(result.document.stereo.groups, attachSource().document.stereo.groups)
  assert.deepEqual(result.document.stereo.bonds, [])
  assert.equal(result.notes.length, 1)
})

test('attachment rejects deletion of an unrelated center and group', async (t) => {
  const reply = attachedAtOtherEnd()
  reply.document.stereo.atoms = []
  reply.document.stereo.groups = []
  service(t, () => json(reply))
  const attempt = new DocumentClient().createRingAttachment(attachSource().document, 's1', 'a2', { dx: 1, dy: 0 }, ringSelection())
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
})

for (const [name, change] of [
  ['single first cycle edge', (reply) => { reply.document.bonds[3].order = 'single' }],
  ['wrong alternating phase', (reply) => {
    const orders = ['single', 'double', 'single', 'double', 'single', 'double']
    reply.document.bonds.slice(3, 9).forEach((bond, index) => { bond.order = orders[index] })
  }],
  ['double connector', (reply) => { reply.document.bonds.at(-1).order = 'double' }],
  ['missing stereo notes', (reply) => { reply.notes = [] }],
  ['retained target configuration', (reply) => { reply.document.stereo = attachSource().document.stereo }],
]) {
  test(`Kekulé attachment rejects ${name}`, async (t) => {
    const reply = attachAfter(6, 'kekule')
    change(reply)
    service(t, () => json(reply))
    const attempt = new DocumentClient().createRingAttachment(attachSource().document, 's1', 'a1', { dx: 1, dy: 0 }, ringSelection(), 6, 'kekule')
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  })
}

test('Kekulé attachment runtime-invalid patterns and size5 refuse before HTTP', (t) => {
  const bodies = service(t, () => { throw new Error('Must not send.') })
  const source = attachSource().document
  const client = new DocumentClient()
  for (const pattern of [null, 'aromatic', 'Kekule', '', 6, true, {}, []]) {
    assert.throws(() => client.createRingAttachment(source, 's1', 'a1', { dx: 1, dy: 0 }, ringSelection(), 6, pattern), { code: 'ring_pattern_invalid' })
  }
  assert.throws(() => client.createRingAttachment(source, 's1', 'a1', { dx: 1, dy: 0 }, ringSelection(), 5, 'kekule'), { code: 'ring_pattern_invalid' })
  assert.deepEqual(bodies, [])
})
