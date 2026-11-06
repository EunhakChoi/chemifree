import assert from 'node:assert/strict'
import { test } from 'node:test'

import { sceneCutAfter, sceneCutCopy, sceneCutSource } from './lib/scene-cut-fixture.mjs'

// Imported by web-client-check after its TypeScript alias loader is registered.
const { DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')
const selection = () => ({ objects: ['s4', 's5'], atoms: [], bonds: [] })
const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

function service(t, reply) {
  const bodies = []
  t.mock.method(globalThis, 'fetch', async (path, init = {}) => {
    if (path === '/api/v1/session') return json({ header: 'x-chemifree-session', token: 'scene-cut-service' })
    assert.equal(path, '/api/v1/documents/drawing/transactions')
    const body = JSON.parse(init.body)
    bodies.push(body)
    return reply(body, bodies.length)
  })
  return bodies
}

test('graphics Cut freezes outermost roots, raw history selection and source response validation', async (t) => {
  const bodies = service(t, () => json(sceneCutAfter()))
  const copy = sceneCutCopy()
  const before = selection()
  const attempt = new DocumentClient().createSceneClipboardCut(copy, before)
  copy.roots.push('s7')
  copy.snapshot.document.atoms[0].charge = 9
  before.objects.length = 0
  const first = attempt.send()
  assert.equal(attempt.send(), first, 'concurrent outcome checks share one send')
  const result = await first
  assert.deepEqual(result.document.scene.objects.map((object) => object.id), ['s1', 's2', 's3', 's7'])
  assert.deepEqual(result.document.scene.objects[2].children, ['s7'])
  assert.deepEqual(result.document.scene.instances, sceneCutSource().document.scene.instances)
  assert.equal(await attempt.send(), result)
  assert.equal(bodies.length, 1)
  assert.deepEqual({ ...bodies[0], request_id: 'fixed' }, { request_id: 'fixed', expected_revision: 3,
    commands: [{ op: 'delete_objects', objects: ['s4'], mode: 'subtree' }], selection_before: selection(),
    selection_after: { objects: [], atoms: [], bonds: [] } })
})

test('graphics Cut refuses outside-owned incoming references before any mutation', (t) => {
  const bodies = service(t, () => { throw new Error('Cut must not send.') })
  const copy = sceneCutCopy()
  copy.snapshot.document.scene.references.push({ source: 's7', target: { kind: 'object', id: 's5' } })
  assert.throws(() => new DocumentClient().createSceneClipboardCut(copy, selection()), { code: 'clipboard_scene_cut_invalid' })
  assert.deepEqual(bodies, [])
})

test('graphics Cut retains an empty outside parent when all of its children are selected roots', async (t) => {
  const source = sceneCutSource()
  source.document.scene.references.pop()
  const copy = sceneCutCopy(source)
  const payload = JSON.parse(copy.payload)
  const { parent, ...rectangle } = source.document.scene.objects[6]
  payload.roots = ['s4', 's7']
  payload.objects.push(rectangle)
  copy.roots = ['s4', 's7']
  copy.copied.objects.push('s7')
  copy.payload = JSON.stringify(payload)
  const after = sceneCutAfter(source)
  after.document.scene.objects = [after.document.scene.objects[0], after.document.scene.objects[1],
    { kind: 'group', id: 's3', parent: null, children: [] }]
  after.document.scene.references = []
  const bodies = service(t, () => json(after))
  const result = await new DocumentClient().createSceneClipboardCut(copy, { objects: ['s4', 's7'], atoms: [], bonds: [] }).send()
  assert.deepEqual(result.document.scene.objects[2], { kind: 'group', id: 's3', parent: null, children: [] })
  assert.deepEqual(bodies[0].commands, [{ op: 'delete_objects', objects: ['s4', 's7'], mode: 'subtree' }])
})

for (const [name, change] of [
  ['retained copied forest', (after) => { after.document = { ...sceneCutSource().document, revision: 4 } }],
  ['changed outside geometry', (after) => { after.document.scene.objects[3].points[0].x = 999 }],
  ['changed atom', (after) => { after.document.atoms[0].charge = 1 }],
  ['changed placement', (after) => { after.document.scene.instances[1].positions[0].x = 888 }],
  ['dropped outside reference', (after) => { after.document.scene.references = [] }],
  ['removed outside parent', (after) => { after.document.scene.roots = ['s1', 's2', 's7']; after.document.scene.objects.splice(2, 1); after.document.scene.objects[2].parent = null }],
  ['nonempty history selection', (after) => { after.document.selection.objects = ['s7'] }],
  ['wrong revision', (after) => { after.document.revision = 5 }],
]) {
  test(`graphics Cut treats ${name} in a successful reply as an uncertain outcome`, async (t) => {
    const after = sceneCutAfter()
    change(after)
    const bodies = service(t, () => json(after))
    const attempt = new DocumentClient().createSceneClipboardCut(sceneCutCopy(), selection())
    await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_unconfirmed' })
    assert.equal(bodies.length, 1)
  })
}

test('graphics Cut validates unchanged coordinate bits including a raw JSON negative zero', async (t) => {
  // JSON.stringify would erase this difference, so supply the actual valid wire spelling.
  const text = JSON.stringify(sceneCutAfter()).replace('"atom":"a1","x":0', '"atom":"a1","x":-0')
  assert.match(text, /"atom":"a1","x":-0/)
  service(t, () => new Response(text, { status: 200, headers: { 'content-type': 'application/json' } }))
  const attempt = new DocumentClient().createSceneClipboardCut(sceneCutCopy(), selection())
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_unconfirmed' })
})

test('graphics Cut checks lost committed replies with the exact same body and never deletes twice', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Committed reply lost.')
    return json(sceneCutAfter())
  })
  const attempt = new DocumentClient().createSceneClipboardCut(sceneCutCopy(), selection())
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_unconfirmed' })
  const confirmed = await attempt.send()
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.equal(await attempt.send(), confirmed)
  assert.equal(bodies.length, 3)
})

test('graphics Cut retains a definitive refusal without sending another deletion', async (t) => {
  const bodies = service(t, () => json({ api_version: 'v1', code: 'revision_conflict', message: 'Source changed.', current_revision: 4 }, 409))
  const attempt = new DocumentClient().createSceneClipboardCut(sceneCutCopy(), selection())
  await assert.rejects(attempt.send(), { code: 'revision_conflict' })
  await assert.rejects(attempt.send(), { code: 'revision_conflict' })
  assert.equal(bodies.length, 1)
})

test('graphics Cut never resends into another process after an unauthorized outcome check', async (t) => {
  const bodies = service(t, () => json({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }, 401))
  const attempt = new DocumentClient().createSceneClipboardCut(sceneCutCopy(), selection())
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_restarted' })
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_restarted' })
  assert.equal(bodies.length, 1)
})

test('graphics Cut terminal conflict after uncertainty requires reconciliation rather than a fresh deletion', async (t) => {
  const bodies = service(t, (_body, count) => {
    if (count <= 2) throw new TypeError('Reply lost.')
    return json({ api_version: 'v1', code: 'revision_conflict', message: 'Replay expired.', current_revision: 4 }, 409)
  })
  const attempt = new DocumentClient().createSceneClipboardCut(sceneCutCopy(), selection())
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_unconfirmed' })
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_reconcile' })
  await assert.rejects(attempt.send(), { code: 'clipboard_scene_cut_reconcile' })
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
})
