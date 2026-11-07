import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource, sceneCutCopy } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { sceneClipboardGraphicStyles, sceneClipboardCopyMatches, sceneClipboardPasteMapping } = await import('../apps/web/src/lib/scene-clipboard-contract.ts')

function styledCopy(width = .1) {
  const source = sceneCutSource()
  source.document.scene.objects.find((object) => object.id === 's6').line_width_pt = width
  const copy = sceneCutCopy(source)
  const payload = JSON.parse(copy.payload)
  payload.version = 3
  for (const object of payload.objects) if (object.kind === 'graphic') object.line_width_pt ??= null
  return { source, copy, payload }
}
const requested = { objects: ['s4'], atoms: [], bonds: [] }
function matches(value) {
  return sceneClipboardCopyMatches(JSON.stringify(value.payload), value.copy.copied, value.copy.roots, requested, value.source.document, value.source.document.settings)
}

test('v3 displays and copies exact overrides including document-equal values and both range endpoints', () => {
  for (const width of [.1, 10, 1.2]) {
    const value = styledCopy(width)
    assert.equal(matches(value), true)
    assert.deepEqual(sceneClipboardGraphicStyles(JSON.stringify(value.payload)), [
      { id: 's6', shape: 'line', lineWidth: width }, { id: 's5', shape: 'arrow', lineWidth: null },
    ])
    value.payload.objects.find((object) => object.id === 's6').line_width_pt = null
    assert.equal(matches(value), false, 'a successful service reply cannot erase an explicit override')
  }
})

for (const [name, change] of [
  ['missing width', (p) => { delete p.objects[1].line_width_pt }],
  ['string width', (p) => { p.objects[1].line_width_pt = '1' }],
  ['boolean width', (p) => { p.objects[1].line_width_pt = false }],
  ['below range', (p) => { p.objects[1].line_width_pt = .099 }],
  ['above range', (p) => { p.objects[1].line_width_pt = 10.01 }],
  ['group width', (p) => { p.objects[0].line_width_pt = null }],
  ['unknown graphic key', (p) => { p.objects[1].stroke = 'solid' }],
  ['future version', (p) => { p.version = 4 }],
]) test(`v3 rejects ${name}`, () => {
  const value = styledCopy(); change(value.payload)
  assert.equal(sceneClipboardGraphicStyles(JSON.stringify(value.payload)), null)
  assert.equal(matches(value), false)
})

test('v2 remains strict and all-inherited copying remains canonical v2', () => {
  const source = sceneCutSource(); const copy = sceneCutCopy(source)
  const payload = JSON.parse(copy.payload)
  assert.equal(sceneClipboardCopyMatches(copy.payload, copy.copied, copy.roots, requested, source.document, source.document.settings), true)
  assert.deepEqual(sceneClipboardGraphicStyles(copy.payload), [{ id: 's6', shape: 'line', lineWidth: null }, { id: 's5', shape: 'arrow', lineWidth: null }])
  payload.objects[1].line_width_pt = null
  assert.equal(sceneClipboardGraphicStyles(JSON.stringify(payload)), null, 'v2 forbids even a null field')
  payload.version = 3
  payload.objects[2].line_width_pt = null
  assert.deepEqual(sceneClipboardGraphicStyles(JSON.stringify(payload)), [{ id: 's6', shape: 'line', lineWidth: null }, { id: 's5', shape: 'arrow', lineWidth: null }], 'all-null v3 is valid input')
  assert.equal(sceneClipboardCopyMatches(JSON.stringify(payload), copy.copied, copy.roots, requested, source.document, source.document.settings), false, 'writer must choose v2 for all inheritance')
  assert.equal(sceneClipboardGraphicStyles('x'.repeat(65537)), null)
  assert.equal(sceneClipboardGraphicStyles(JSON.stringify(payload).replace('"line_width_pt":null', '"line_width_pt":1e999')), null)
})

test('v3 paste mapping preserves nullable widths, outside objects, references and the atom graph', () => {
  const value = styledCopy(10)
  const before = value.source.document
  const after = structuredClone(before)
  const mapping = { objects: [{ source: 's4', target: 's8' }, { source: 's6', target: 's9' }, { source: 's5', target: 's10' }] }
  after.revision += 1
  after.selection = { objects: ['s8'], atoms: [], bonds: [] }
  after.scene.roots.push('s8')
  after.scene.objects.push(
    { kind: 'group', id: 's8', parent: null, children: ['s9', 's10'] },
    { kind: 'graphic', id: 's9', parent: 's8', shape: 'line', points: [{ x: 11, y: 22 }, { x: 41, y: 22 }], line_width_pt: 10 },
    { kind: 'graphic', id: 's10', parent: 's8', shape: 'arrow', points: [{ x: 11, y: 32 }, { x: 41, y: 32 }], line_width_pt: null },
  )
  after.scene.references.push(
    { source: 's10', target: { kind: 'object', id: 's9' } },
    { source: 's9', target: { kind: 'object', id: 's10' } },
    { source: 's9', target: { kind: 'object', id: 's9' } },
  )
  const paste = (document) => sceneClipboardPasteMapping(JSON.stringify(value.payload), mapping, { x: 10, y: 20 }, before, document, before.settings)
  assert.deepEqual(paste(after), mapping.objects)
  for (const mutate of [
    (d) => { d.scene.objects.find((o) => o.id === 's9').line_width_pt = null },
    (d) => { d.scene.objects.find((o) => o.id === 's10').line_width_pt = 1.2 },
    (d) => { d.scene.objects.find((o) => o.id === 's6').line_width_pt = null },
    (d) => { d.atoms[0].charge = 1 },
    (d) => { d.scene.references.pop() },
  ]) {
    const changed = structuredClone(after); mutate(changed)
    assert.equal(paste(changed), null)
  }
  value.payload.objects[1].line_width_pt = null
  after.scene.objects.find((o) => o.id === 's9').line_width_pt = null
  assert.deepEqual(paste(after), mapping.objects, 'v3 all-null input pastes as inheritance')
})
