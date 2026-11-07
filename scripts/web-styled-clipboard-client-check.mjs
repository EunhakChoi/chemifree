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
function copyForVersion(version) {
  if (version === 3) return styledCopy()
  const source = sceneCutSource(); const copy = sceneCutCopy(source)
  return { source, copy, payload: JSON.parse(copy.payload) }
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
  ['future version', (p) => { p.version = 7 }],
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

for (const [name, duplicate] of [
  ['conflicting width', (text) => text.replace('"line_width_pt":0.1', '"line_width_pt":0.1,"line_width_pt":10')],
  ['identical width', (text) => text.replace('"line_width_pt":0.1', '"line_width_pt":0.1,"line_width_pt":0.1')],
  ['escaped width key', (text) => text.replace('"line_width_pt":0.1', String.raw`"line_width_pt":0.1,"line_\u0077idth_pt":10`)],
  ['nested coordinate', (text) => text.replace('"x":1', '"x":2,"x":1')],
  ['document setting', (text) => text.replace('"line_width_pt":1.2', '"line_width_pt":3,"line_width_pt":1.2')],
]) test(`clipboard summaries and successful copy replies reject duplicate ${name}`, () => {
  const value = styledCopy()
  const text = duplicate(JSON.stringify(value.payload))
  assert.doesNotThrow(() => JSON.parse(text), 'this is valid JSON syntax, so ordinary JSON.parse cannot enforce the boundary')
  assert.equal(sceneClipboardGraphicStyles(text), null, 'ambiguous values are not previewed')
  assert.equal(sceneClipboardCopyMatches(text, value.copy.copied, value.copy.roots, requested, value.source.document, value.source.document.settings), false)
})

const { parseStrictJson } = await import('../apps/web/src/lib/strict-json.ts')

test('strict JSON scan distinguishes escaped strings, punctuation and nested object scopes', () => {
  const text = String.raw`{"a":[{"key":"quoted \"key\": 1, { [ , }","slash":"\\"},{"key":"second scope","unicode":"\u0077"}],"key":"outer"}`
  assert.deepEqual(parseStrictJson(text), JSON.parse(text))
  assert.deepEqual(parseStrictJson('[{"a":1},{"a":2}]'), [{a:1},{a:2}])
  assert.deepEqual(parseStrictJson('{"a":{"a":[{"a":1}]}}'), {a:{a:[{a:1}]}})
  assert.deepEqual(parseStrictJson('{"a" \t\n : [{"a"\r : 1}]}'), {a:[{a:1}]})
  const scalar = 'key: { , [ "quoted" \\ end'
  assert.equal(parseStrictJson(JSON.stringify(scalar)), scalar)
  const deep = '['.repeat(2000)+'0'+']'.repeat(2000)
  assert.doesNotThrow(() => parseStrictJson(deep), 'the duplicate scan uses an iterative stack within the caller byte bound')
})

for (const text of ['{"a":1,"a":2}', '{"a":{"b":1,"b":2}}', '[{"a":1,"\\u0061":2}]', String.raw`{"a\"b":1,"a\u0022b":2}`, String.raw`{"a\/b":1,"a/b":2}`, '{"a":1,}', '{"a":"unterminated}', '{']) {
  test(`strict JSON refuses duplicate keys or malformed grammar: ${text}`, () => {
    assert.throws(() => parseStrictJson(text), SyntaxError)
  })
}

for (const version of [2, 3]) {
  for (const [name, change] of [
    ['missing base fields', () => ({ bogus: true })],
    ['missing bond length', (s) => { delete s.bond_length_pt; return s }],
    ['missing line width', (s) => { delete s.line_width_pt; return s }],
    ['missing label font', (s) => { delete s.label_font; return s }],
    ['missing label size', (s) => { delete s.label_size_pt; return s }],
    ['missing carbon labels', (s) => { delete s.carbon_labels; return s }],
    ['missing display unit', (s) => { delete s.display_unit; return s }],
    ['unknown setting', (s) => ({ ...s, bogus: true })],
    ['string numeric setting', (s) => ({ ...s, bond_length_pt: '30' })],
    ['invalid font', (s) => ({ ...s, label_font: 'comic-sans' })],
    ['invalid weight', (s) => ({ ...s, label_weight: 'heavy' })],
    ['invalid style', (s) => ({ ...s, label_style: 'oblique' })],
    ['invalid carbon labels', (s) => ({ ...s, carbon_labels: 'none' })],
    ['invalid unit', (s) => ({ ...s, display_unit: 'px' })],
    ['null weight', (s) => ({ ...s, label_weight: null })],
    ['null style', (s) => ({ ...s, label_style: null })],
    ['nonboolean underline', (s) => ({ ...s, label_underline: 0 })],
    ['nonboolean outline', (s) => ({ ...s, label_outline: 'false' })],
    ['null shadow', (s) => ({ ...s, label_shadow: null })],
    ['bond length below minimum', (s) => ({ ...s, bond_length_pt: 4.99 })],
    ['bond length above maximum', (s) => ({ ...s, bond_length_pt: 200.01 })],
    ['line width below minimum', (s) => ({ ...s, line_width_pt: .099 })],
    ['line width above maximum', (s) => ({ ...s, line_width_pt: 10.01 })],
    ['label size below minimum', (s) => ({ ...s, label_size_pt: 3.99 })],
    ['label size above maximum', (s) => ({ ...s, label_size_pt: 72.01 })],
  ]) test(`v${version} refuses settings with ${name} in previews and successful copy replies`, () => {
    const value = copyForVersion(version)
    value.payload.settings = change(value.payload.settings)
    const text = JSON.stringify(value.payload)
    assert.equal(sceneClipboardGraphicStyles(text), null)
    // Even an equally malformed expected settings record cannot authorize a bad reply.
    const expected = { label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, ...value.payload.settings }
    assert.equal(sceneClipboardCopyMatches(text, value.copy.copied, value.copy.roots, requested, value.source.document, expected), false)
  })

  for (const [name, settings] of [
    ['minimum numeric values', { bond_length_pt: 5, line_width_pt: .1, label_font: 'serif', label_weight: 'bold', label_style: 'italic', label_underline: true, label_outline: true, label_shadow: true, label_size_pt: 4, carbon_labels: 'all', display_unit: 'mm' }],
    ['maximum numeric values', { bond_length_pt: 200, line_width_pt: 10, label_font: 'monospace', label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, label_size_pt: 72, carbon_labels: 'default', display_unit: 'in' }],
    ['legacy omitted label fields', { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_size_pt: 12, carbon_labels: 'default', display_unit: 'cm' }],
  ]) test(`v${version} accepts ${name} in previews and successful copy replies`, () => {
    const { source, copy, payload } = copyForVersion(version)
    source.document.settings = { label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, ...settings }
    payload.settings = settings
    assert.deepEqual(sceneClipboardGraphicStyles(JSON.stringify(payload)), [
      { id: 's6', shape: 'line', lineWidth: version === 3 ? .1 : null }, { id: 's5', shape: 'arrow', lineWidth: null },
    ])
    assert.equal(sceneClipboardCopyMatches(JSON.stringify(payload), copy.copied, copy.roots, requested, source.document, source.document.settings), true)
  })
}
