import assert from 'node:assert/strict'
import { test } from 'node:test'
const { validDisplayLabel } = await import('../apps/web/src/lib/document-contract.ts')
const { compactLabel } = await import('../apps/web/src/editor/label.ts')

test('display text accepts bounded Unicode and refuses the caption control/whitespace profile', () => {
  for (const value of ['Ph','Me','한 e\u0301 🧪','"'.repeat(128),'한'.repeat(42)+'ab']) assert.equal(validDisplayLabel(value),true)
  for (const value of ['', '  ', '\t', 'a\nb', 'a\rb', 'a\u0085b', 'a\u2028b', 'a\u2029b', '한'.repeat(43), 'a'.repeat(129), '\ud800', null, 12]) assert.equal(validDisplayLabel(value),false)
})
test('custom plain Unicode uses measured chunk ink even with normal document styling', () => {
  const style={font:'sans-serif',size:12,carbons:'default',weight:'normal',style:'normal',underline:false,outline:false,shadow:false}
  const provisional=compactLabel('Ph 한 🧪',style)
  const key=provisional.chunks[0].metricKey
  const actual={x:-45,y:-12,width:90,height:24}
  const measured=compactLabel('Ph 한 🧪',{...style,metrics:{runs:new Map(),chunks:new Map([[key,{box:actual,underlines:[]}]])}})
  assert.equal(measured.chunks[0].measured,true)
  assert.deepEqual(measured.box,{x:-46,y:-13,width:92,height:26})
  assert.equal(measured.chunks[0].runs[0].text,'Ph 한 🧪')
})

test('display label control boundaries and lone surrogates match the single-line Unicode profile', () => {
  for (let point = 0; point <= 0x9f; point += 1) {
    const value = `a${String.fromCodePoint(point)}b`
    const expected = point >= 0x20 && point < 0x7f
    assert.equal(validDisplayLabel(value), expected, `U+${point.toString(16)}`)
  }
  for (const point of [0x2028, 0x2029, 0xd800, 0xdbff, 0xdc00, 0xdfff]) {
    assert.equal(validDisplayLabel(`a${String.fromCodePoint(point)}b`), false)
  }
  for (const point of [0xa0, 0x2027, 0x202a, 0xd7ff, 0xe000, 0x1f9ea, 0x10ffff]) {
    assert.equal(validDisplayLabel(`a${String.fromCodePoint(point)}b`), true)
  }
  assert.equal(validDisplayLabel(String.fromCodePoint(0xfeff)), true, 'Rust White_Space excludes BOM')
})
