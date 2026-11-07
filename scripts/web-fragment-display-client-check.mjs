import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { ringSource } from './lib/ring-fixture.mjs'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { isDocumentResponse, validDisplayLabel, validFragmentDisplayRuns, fragmentDisplayRuns } = await import('../apps/web/src/lib/document-contract.ts')
const { abbreviationTargets, compactView, ethylLabel } = await import('../apps/web/src/editor/abbreviation.ts')
const { marqueeSelection, selectionMove } = await import('../apps/web/src/editor/selection-geometry.ts')
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')
const { fragmentLabel } = await import('../apps/web/src/editor/label.ts')
const { atomIndex, hitTest, drawingStrokes } = await import('../apps/web/src/editor/geometry.ts')

function source() {
  const response = ringSource()
  response.document.atoms = ['C', 'C', 'O', 'N'].map((element, index) => ({ id: `a${index + 1}`, kind: 'element', element, query: null,
    charge: 0, isotope: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [],
    position: [{ x: 0, y: 0 }, { x: 80, y: 0 }, { x: 100, y: 30 }, { x: 100, y: -30 }][index] }))
  response.document.bonds = [['a1', 'a2'], ['a2', 'a3'], ['a2', 'a4']].map(([begin, end], index) => ({id: `b${index + 1}`, begin, end, order: 'single', query: null, mark: null}))
  response.document.scene.instances[0].positions = response.document.atoms.map((atom) => ({ atom: atom.id, ...atom.position }))
  response.document.fragment_displays = [{ instance: 's1', anchor: 'a2', members: ['a2', 'a3', 'a4'], text: 'Ph 한 e\u0301 🧪' }]
  const item = response.document.fragment_displays[0]
  item.runs = [{text:item.text,script:'baseline'}]
  return response
}

test('fragment metadata validates the retained branched heteroatom tree and historical omission', () => {
  assert.equal(isDocumentResponse(source()), true)
  const older = source(); delete older.document.fragment_displays
  assert.equal(isDocumentResponse(older), true)
  const single = source(); single.document.fragment_displays = [{instance:'s1',anchor:'a3',members:['a3'],text:'literal'}]
  assert.equal(isDocumentResponse(single), true)
  for (const mutate of [
    (d) => { d.fragment_displays = null },
    (d) => { d.fragment_displays[0].anchor = 'a1' },
    (d) => { d.fragment_displays[0].members.push('a3') },
    (d) => { d.fragment_displays[0].members.push('missing') },
    (d) => { d.fragment_displays[0].text = '\n' },
    (d) => { d.fragment_displays.push(d.fragment_displays[0]) },
    (d) => { d.atoms[2].charge = 1 },
    (d) => { d.bonds[1].order = 'double' },
    (d) => { d.bonds[1].begin = 'a1' },
    (d) => { d.bonds.push({ ...d.bonds[0], id: 'b4', begin: 'a3', end: 'a4' }) },
    (d) => { d.bonds[0].mark = { kind: 'wavy' } },
    (d) => { d.fragment_displays[0].instance = 'expired' },
  ]) {
    const reply = source(); mutate(reply.document)
    assert.equal(isDocumentResponse(reply), false)
  }
})

test('fragment projection hides only members, with whole-fragment selection and rigid movement', () => {
  const document = source().document
  const original = structuredClone(document)
  const view = compactView(document, 's1')
  assert.deepEqual(view.atoms.map((atom) => atom.id), ['a1', 'a2'])
  assert.deepEqual(view.bonds.map((bond) => bond.id), ['b1'])
  const targets = ['a2', 'a3', 'a4', 'b2', 'b3']
  assert.deepEqual(abbreviationTargets(document, 's1', 'a2'), targets)
  assert.deepEqual([...marqueeSelection(document, 's1', { x: 79, y: -1 }, { x: 81, y: 1 }, new Set(), false, true)], targets)
  assert.deepEqual([...marqueeSelection(document, 's1', { x: 99, y: 29 }, { x: 101, y: 31 }, new Set(), false, true)], [])
  assert.deepEqual(selectionMove(document, new Set(targets), 's1', {x: 7, y: -2}), [{ op: 'move_selection', instance: 's1', delta: {x: 7, y: -2}, selection: {objects: [], atoms: ['a2', 'a3', 'a4'], bonds: ['b2', 'b3']} }])
  const style = {font: 'sans-serif', size: 12, carbons: 'default', weight: 'normal', style: 'normal', underline: false, outline: false, shadow: false}
  const label = ethylLabel(style, '한', true)
  const labels = new Map([['a2', label]])
  assert.equal(hitTest(view, {x: 100, y: 30}, new Set(), atomIndex(view), labels), null)
  assert.equal(drawingStrokes(view, labels, atomIndex(view), new Map(), 2, 1.2).has('b2'), false)
  const key = label.chunks[0].metricKey
  const actual = {x: -21, y: -8, width: 42, height: 16}
  const measured = ethylLabel({...style, metrics: {runs: new Map(), chunks: new Map([[key, {box: actual, underlines: []}]])}}, '한', true)
  assert.equal(measured.chunks[0].measured, true)
  assert.ok(measured.box.width >= actual.width)
  const shared = new Map([['a2', measured]])
  const boundary = drawingStrokes(view, shared, atomIndex(view), new Map(), 2, 1.2).get('b1')[0]
  assert.equal(boundary.shape, 'line')
  assert.ok(boundary.x2 <= 80 + measured.box.x, 'boundary stops outside the measured ink box')
  assert.deepEqual(hitTest(view, {x: 100, y: 0}, new Set(), atomIndex(view), shared), {kind:'atom', id:'a2'})
  const etKey = ethylLabel(style, 'Et', true).chunks[0].metricKey
  const literalEt = ethylLabel({...style, metrics: {runs: new Map(), chunks: new Map([[etKey, {box: actual, underlines: []}]])}}, 'Et', true)
  assert.equal(literalEt.chunks[0].measured, true, 'even literal Et fragment text uses actual measured ink')
  assert.deepEqual(document, original)
})

test('fragment commands lapse with missing members; text accepts Unicode only inside the shared byte bound', () => {
  const document = source().document
  const command = {op:'contract_fragment', instance:'s1', anchor:'a2', members:['a2','a3','a4'], text:'literal label'}
  assert.deepEqual(stillApplicable([command], document), [command])
  document.atoms.pop()
  assert.equal(stillApplicable([command], document), null)
  assert.equal(stillApplicable([{op:'expand_fragment', instance:'expired', anchor:'a2'}], document), null)
  assert.equal(validDisplayLabel('한'.repeat(42) + 'ab'), true)
  assert.equal(validDisplayLabel('한'.repeat(43)), false)
  assert.equal(validDisplayLabel('e\u0301 🧪'), true)
})

test('fragment run replies require exact text agreement, preserving boundaries and historical fallback', () => {
  const reply = source()
  const item = reply.document.fragment_displays[0]
  const original = item.text
  delete item.runs
  assert.equal(isDocumentResponse(reply), true)
  assert.deepEqual(fragmentDisplayRuns(item), [{text:original,script:'baseline'}])
  item.runs = [{text:'한 ',script:'baseline'},{text:'2',script:'subscript'},{text:'+',script:'superscript'},{text:' e\u0301 🧪',script:'baseline'}]
  item.text = item.runs.map((run)=>run.text).join('')
  assert.equal(isDocumentResponse(reply),true)
  const retained = structuredClone(reply.document)
  assert.deepEqual(compactView(reply.document,'s1').atoms.map((atom)=>atom.id),['a1','a2'])
  assert.deepEqual(reply.document,retained)
  for (const runs of [null,[],[{text:'',script:'baseline'}],[{text:'x',script:'unknown'}],[{text:'x',script:null}],[{text:'different',script:'baseline'}]]) {
    const changed = structuredClone(reply)
    changed.document.fragment_displays[0].runs=runs
    assert.equal(isDocumentResponse(changed),false)
  }
})

test('fragment runs have one combined byte boundary, nonempty scalar runs and no separate count cap', () => {
  assert.equal(validFragmentDisplayRuns(Array.from({length:128},()=>({text:'x',script:'baseline'}))),true)
  assert.equal(validFragmentDisplayRuns(Array.from({length:129},()=>({text:'x',script:'baseline'}))),false)
  assert.equal(validFragmentDisplayRuns([{text:'한'.repeat(42),script:'baseline'},{text:'ab',script:'subscript'}]),true)
  assert.equal(validFragmentDisplayRuns([{text:'한'.repeat(42),script:'baseline'},{text:'abc',script:'superscript'}]),false)
  assert.equal(validFragmentDisplayRuns([{text:' ',script:'subscript'},{text:'x',script:'baseline'}]),true)
  for (const text of ['', '\n', '\u0085', '\u2028', '\u2029', '\ud800', '\udfff']) {
    assert.equal(validFragmentDisplayRuns([{text:'x',script:'baseline'},{text,script:'subscript'}]),false)
  }
  assert.equal(validFragmentDisplayRuns([{text:' ',script:'baseline'},{text:'\u00a0',script:'subscript'}]),false)
  assert.equal(validFragmentDisplayRuns([{text:'\ud800',script:'baseline'},{text:'\udc00',script:'baseline'}]),false,'separate invalid scalars cannot join into a valid surrogate pair')
  assert.equal(validFragmentDisplayRuns([{text:'e\u0301 👩‍🔬',script:'superscript'}]),true)
})

test('fragment scripts reuse label size/shift and measured shared ink without chemical parsing', () => {
  const style={font:'sans-serif',size:24,carbons:'default',weight:'bold',style:'italic',underline:true,outline:true,shadow:true}
  const runs=[{text:'N',script:'baseline'},{text:'2',script:'subscript'},{text:'+',script:'superscript'},{text:'₂ 한',script:'baseline'}]
  const provisional=fragmentLabel(runs,style)
  assert.deepEqual(provisional.chunks[0].runs.map(({text,size,shift,part})=>({text,size,shift,part})),[
    {text:'N',size:24,shift:0,part:'fragment'},{text:'2',size:16,shift:8,part:'fragment'},
    {text:'+',size:16,shift:-10,part:'fragment'},{text:'₂ 한',size:24,shift:0,part:'fragment'},
  ])
  assert.notEqual(provisional.chunks[0].metricKey,fragmentLabel(runs.map((run)=>({...run,script:'baseline'})),style).chunks[0].metricKey)
  const actual={x:-42,y:-24,width:84,height:47}
  const measured=fragmentLabel(runs,{...style,metrics:{runs:new Map(),chunks:new Map([[provisional.chunks[0].metricKey,{box:actual,underlines:[{x:-42,y:22,width:84,height:2}]}]])}})
  assert.equal(measured.chunks[0].measured,true)
  assert.ok(measured.box.x<=actual.x && measured.box.y<=actual.y)
  assert.ok(measured.box.x+measured.box.width>=actual.x+actual.width)
  assert.ok(measured.box.y+measured.box.height>=24)
  const document=source().document
  const command={op:'set_fragment_display_runs',instance:'s1',anchor:'a2',runs}
  assert.deepEqual(stillApplicable([command],document),[command])
  assert.equal(stillApplicable([{...command,anchor:'expired'}],document),null)
})
