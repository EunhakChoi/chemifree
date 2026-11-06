import assert from 'node:assert/strict'
import { test } from 'node:test'

import { ringAfter, ringSelection, ringSource } from './lib/ring-fixture.mjs'

// Imported after the normal TypeScript alias loader is installed.
const { DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')
const { selectionSnapshot } = await import('../apps/web/src/editor/scene-selection.ts')
const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

function service(t, reply) {
  const bodies = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'ring-service' })
    assert.equal(path, '/api/v1/documents/drawing/transactions')
    const body = JSON.parse(init.body)
    bodies.push(body)
    return reply(body, bodies.length)
  })
  return bodies
}

test('ring insertion freezes center and selection and validates the independent six-cycle', async (t) => {
  const bodies = service(t, () => json(ringAfter()))
  const source = ringSource().document
  const point = { x: 100, y: 100 }
  const selected = ringSelection()
  const attempt = new DocumentClient().createRingInsertion(source, 's1', point, selected)
  point.x = 999
  selected.atoms.length = 0
  source.title = 'Changed after capture'
  const result = await attempt.send()
  assert.equal(result.document.atoms.length, 8)
  assert.equal(result.document.bonds.length, 7)
  assert.deepEqual(result.document.selection, ringSelection())
  assert.deepEqual({ ...bodies[0], request_id: 'fixed' }, { request_id: 'fixed', expected_revision: 3,
    commands: [{ op: 'insert_ring', instance: 's1', center: { x: 100, y: 100 } }], selection_before: ringSelection() })
  assert.equal(await attempt.send(), result)
  assert.equal(bodies.length, 1)
})

test('ring outcome checking accepts reordered scene selection while preserving its original request', async (t) => {
  const source = ringSource().document
  const after = ringAfter()
  // Reorder the two page roots: scene DFS/paint order is now s3 ... s1.
  source.scene.roots.reverse()
  source.scene.objects.push(source.scene.objects.shift())
  after.document.scene.roots.reverse()
  after.document.scene.objects.push(after.document.scene.objects.shift())
  const captured = selectionSnapshot(new Set(['s1', 's3', 'a1', 'b1']), source)
  assert.deepEqual(captured, { objects: ['s3', 's1'], atoms: ['a1'], bonds: ['b1'] })
  after.document.selection = { objects: ['s1', 's3'], atoms: ['a1'], bonds: ['b1'] }
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Committed reply lost.')
    return json(after)
  })
  const attempt = new DocumentClient().createRingInsertion(source, 's1', { x: 100, y: 100 }, captured)
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  const result = await attempt.send()
  assert.deepEqual(result.document.selection, { objects: ['s1', 's3'], atoms: ['a1'], bonds: ['b1'] })
  assert.deepEqual(bodies[0].selection_before, captured)
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.equal(await attempt.send(), result)
  assert.equal(bodies.length, 3)
})

for (const [name, change] of [
  ['missing closing edge', (after) => { after.document.bonds[6].end = 'a4' }],
  ['connected to existing atom', (after) => { after.document.bonds[1].begin = 'a1' }],
  ['wrong new atom', (after) => { after.document.atoms[2].element = 'N' }],
  ['changed original atom', (after) => { after.document.atoms[0].charge = 1 }],
  ['changed original reference', (after) => { after.document.scene.references.pop() }],
  ['wrong ring geometry', (after) => { after.document.atoms[2].position.x += 2; after.document.scene.instances[0].positions[2].x += 2 }],
  ['wrong revision', (after) => { after.document.revision += 1 }],
  ['lost captured selection', (after) => { after.document.selection.atoms = [] }],
  ['lost scene selection', (after) => { after.document.selection.objects = [] }],
  ['lost bond selection', (after) => { after.document.selection.bonds = [] }],
  ['added scene selection', (after) => { after.document.selection.objects.push('s4') }],
  ['replaced atom selection', (after) => { after.document.selection.atoms = ['a2'] }],
  ['duplicate scene selection', (after) => { after.document.selection.objects.push('s7') }],
  ['duplicate atom selection', (after) => { after.document.selection.atoms.push('a1') }],
  ['duplicate bond selection', (after) => { after.document.selection.bonds.push('b1') }],
]) {
  test(`ring insertion treats ${name} in a successful reply as uncertain`, async (t) => {
    const after = ringAfter()
    change(after)
    service(t, () => json(after))
    const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection())
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  })
}

test('lost ring replies are checked using the exact same body without a second insertion', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Committed reply lost.')
    return json(ringAfter())
  })
  const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection())
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  assert.equal((await attempt.send()).document.atoms.length, 8)
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
})

test('an unconfirmed ring attempt never resends to a restarted service', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Reply lost.')
    return json({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }, 401)
  })
  const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection())
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_restarted' })
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_restarted' })
  assert.equal(bodies.length, 3)
})

test('unsupported ring destinations are rejected before any request', (t) => {
  const bodies = service(t, () => { throw new Error('Must not send.') })
  const source = ringSource().document
  const client = new DocumentClient()
  assert.throws(() => client.createRingInsertion(source, 's99', { x: 0, y: 0 }, ringSelection()), { code: 'ring_destination_invalid' })
  assert.throws(() => client.createRingInsertion(source, 's1', { x: NaN, y: 0 }, ringSelection()), { code: 'ring_destination_invalid' })
  source.scene.instances.push({ ...source.scene.instances[0], id: 's99' })
  assert.throws(() => client.createRingInsertion(source, 's1', { x: 0, y: 0 }, ringSelection()), { code: 'ring_destination_invalid' })
  assert.deepEqual(bodies, [])
})

test('a lost ring result followed by revision conflict requires explicit reconciliation', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Committed reply lost.')
    return json({ api_version: 'v1', code: 'revision_conflict', message: 'Replay expired.', current_revision: 6 }, 409)
  })
  const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection())
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_reconcile' })
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_reconcile' })
  assert.equal(bodies.length, 3)
  assert.deepEqual(bodies[2], bodies[0])
})

test('five-ring capture survives changed inputs and lost responses with the same size and body', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Committed five-ring reply lost.')
    return json(ringAfter(5))
  })
  const point = { x: 100, y: 100 }
  const selection = ringSelection()
  let size = 5
  const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', point, selection, size)
  size = 6
  point.x = 900
  selection.atoms.length = 0
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  const result = await attempt.send()
  assert.equal(size, 6)
  assert.equal(result.document.atoms.length, 7)
  assert.equal(result.document.bonds.length, 6)
  assert.deepEqual(bodies[0].commands, [{ op: 'insert_ring', instance: 's1', center: { x: 100, y: 100 }, size: 5 }])
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.deepEqual(result.document.selection, ringSelection())
})

for (const [name, reply] of [
  ['six-ring returned for five', () => ringAfter(6)],
  ['five-ring returned for six', () => ringAfter(5)],
]) {
  test(`size-specific ring reply validation rejects ${name}`, async (t) => {
    service(t, () => json(reply()))
    const size = name.startsWith('six') ? 5 : 6
    const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection(), size)
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  })
}

for (const [name, change] of [
  ['open cycle', (after) => { after.document.bonds[5].end = 'a4' }],
  ['altered geometry', (after) => { after.document.atoms[2].position.x += 2; after.document.scene.instances[0].positions[2].x += 2 }],
]) {
  test(`five-ring validation rejects ${name}`, async (t) => {
    const after = ringAfter(5)
    change(after)
    service(t, () => json(after))
    const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection(), 5)
    await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  })
}

test('runtime-invalid ring sizes are refused without sending', (t) => {
  const bodies = service(t, () => { throw new Error('Must not send.') })
  for (const size of [null, 4, 7, 5.5, '5', true]) {
    assert.throws(() => new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection(), size), { code: 'ring_size_invalid' })
  }
  assert.deepEqual(bodies, [])
})

test('five-ring recovery never changes its size or resends after restart', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Reply lost.')
    return json({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }, 401)
  })
  const attempt = new DocumentClient().createRingInsertion(ringSource().document, 's1', { x: 100, y: 100 }, ringSelection(), 5)
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_unconfirmed' })
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_restarted' })
  await assert.rejects(attempt.send(), { code: 'clipboard_ring_restarted' })
  assert.equal(bodies.length, 3)
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.equal(bodies[2].commands[0].size, 5)
})
