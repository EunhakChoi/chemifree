import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { parseRotationDegrees, selectionRotation } = await import('../apps/web/src/editor/selection-rotation.ts')

for (const [text, expected] of [['30',30],[' -37.5 ',-37.5],['.5',.5],['3e1',30],['+450',450],['1e100',1e100],['1.7976931348623157e308',Number.MAX_VALUE]]) {
  test(`rotation accepts finite decimal degrees ${text} without a magnitude cap`, () => assert.equal(parseRotationDegrees(text),expected))
}
for (const text of ['', ' ', 'NaN', 'Infinity', '-Infinity', '1e309', '0x10', '1_000', '30 deg', '30,5']) {
  test(`rotation refuses invalid angle ${JSON.stringify(text)}`, () => assert.equal(parseRotationDegrees(text),null))
}

function edit(document, degrees = 37.5) {
  return { documentId: document.id, selection: new Set(['s4','a1','b1']), instance: 's2', degrees }
}

test('arbitrary rotations retain the raw angle and the captured mixed placement with a bounds-center pivot', () => {
  const document = sceneCutSource().document
  const original = structuredClone(document)
  assert.deepEqual(selectionRotation(document,edit(document)), [{op:'transform_selection',selection:{objects:['s4'],atoms:['a1'],bonds:['b1']},instance:'s2',pivot:{x:65.5,y:6},transform:{type:'rotate',degrees:37.5}}])
  assert.equal(selectionRotation(document,edit(document,-330))[0].transform.degrees,-330,'normalization remains the domain transform policy')
  assert.deepEqual(document,original)
})

test('queued rotation recomputes moved bounds but lapses with a missing target, placement or different document', () => {
  const document = sceneCutSource().document
  const captured = edit(document)
  const moved = structuredClone(document);moved.revision += 1
  for(const object of moved.scene.objects)if(object.id==='s5'||object.id==='s6')for(const point of object.points)point.x += 10
  assert.deepEqual(selectionRotation(moved,captured),[{op:'transform_selection',selection:{objects:['s4'],atoms:['a1'],bonds:['b1']},instance:'s2',pivot:{x:70.5,y:6},transform:{type:'rotate',degrees:37.5}}])
  assert.equal(selectionRotation({...document,id:'other'},captured),null)
  const deleted = structuredClone(document);deleted.scene.objects=deleted.scene.objects.filter((object)=>object.id!=='s4')
  assert.equal(selectionRotation(deleted,captured),null)
  const placement = structuredClone(document);placement.scene.instances=placement.scene.instances.filter((instance)=>instance.id!=='s2')
  assert.equal(selectionRotation(placement,captured),null)
})

test('full-turn identities and invalid numbers send no rotation command', () => {
  const document = sceneCutSource().document
  for(const degrees of [0,-0,360,-720,-Number.MIN_VALUE,NaN,Infinity,-Infinity])assert.equal(selectionRotation(document,edit(document,degrees)),null)
  assert.equal(selectionRotation(document,edit(document,Number.MIN_VALUE))[0].transform.degrees,Number.MIN_VALUE,'a tiny positive finite angle is not rounded into an invented UI identity')
  assert.equal(selectionRotation(document,edit(document,1e100))[0].transform.degrees,1e100)
})

test('caption descendants remain unsupported; rectangle rotations are left to the atomic domain refusal', () => {
  const document = sceneCutSource().document
  document.scene.objects.push({kind:'caption',id:'s8',parent:'s4',anchor:{x:0,y:0},alignment:'left',runs:[{text:'caption',script:'baseline',weight:'normal',style:'normal'}]})
  document.scene.objects.find((object)=>object.id==='s4').children.push('s8')
  assert.equal(selectionRotation(document,edit(document)),null)
  const rectangle = sceneCutSource().document
  const commands=selectionRotation(rectangle,{...edit(rectangle,45),selection:new Set(['s7'])})
  assert.equal(commands[0].transform.degrees,45,'the existing API diagnoses an axis-aligned rectangle atomically')
  assert.equal('instance' in commands[0],false,'scene-only geometry ignores the movement placement')
})
