// Checks that the web lays out the golden documents as the Rust SVG renderer does (P38a1).
// crates/application/tests/fixtures/svg/parity.json holds the Rust layout of each golden document
// (`layout_drawing` in svg.rs). This script lays out the same documents with the web's geometry and
// label modules, in the order structure-canvas.tsx draws them, and compares the two. Both sides round
// stroke coordinates to the hundredth and a tie can round one step apart, so strokes agree within
// 0.011; label and atom numbers within 1e-6; texts, weights, shapes, patterns, counts and ids exactly.
// It only reads the fixture. The web is run without browser label metrics, as the Rust renderer is.
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {register} from 'node:module'
import {test} from 'node:test'
register('./lib/web-alias-hooks.mjs',import.meta.url)
const {placements,settingsOf}=await import('../apps/web/src/lib/document-contract.ts')
const {placedView}=await import('../apps/web/src/editor/placement.ts')
const {compactDisplaysIn}=await import('../apps/web/src/editor/abbreviation.ts')
const {atomLabel,labelStyleOf}=await import('../apps/web/src/editor/label.ts')
const {atomIndex,bondRings,bondSides,bondSpacing,degrees,drawingStrokes,leaningRight}=await import('../apps/web/src/editor/geometry.ts')
const fixture=JSON.parse(readFileSync(new URL('../crates/application/tests/fixtures/svg/parity.json',import.meta.url),'utf8'))
const STROKE_TOLERANCE=0.011
const NUMBER_TOLERANCE=1e-6
// Compared exactly whatever their type: texts, font weights, shape and pattern names, counts.
const EXACT_KEYS=new Set(['shape','pattern','text','weight','halfWaves'])
// The golden documents in the order the Rust fixture lists them.
const CASE_NAMES=['chain-bare-vertices','bond-orders','stereo-marks','labels','settings-wide-serif','monospace-small','two-instances','scene-object-order','empty']

// The order structure-canvas.tsx draws the placements in: sorted by where their objects stand in
// scene.objects. An instance without an object comes first, as indexOf gives it -1.
function drawingOrder(document){
 const all=placements(document)
 const order=document.scene?.objects?.filter((object)=>object.kind==='instance').map((object)=>object.id)
 if(order)all.sort((a,b)=>order.indexOf(a.id??'')-order.indexOf(b.id??''))
 return all
}

// The label fields the Rust renderer records (`AtomLabel` in svg/label.rs): the offset of the label's
// chunk, its runs, the chunk's box, and the box of the whole label, which clips bonds. The web lays a
// label out as one chunk that holds every run in reading order.
function labelRecord(label){
 assert.equal(label.chunks.length,1,'a web label is one chunk of runs')
 const [chunk]=label.chunks
 return {x:chunk.x,runs:chunk.runs.map(({shift,size,text,weight})=>({shift,size,text,weight})),chunkBox:chunk.box,box:label.box}
}

// The web's layout of a document, instance by instance in drawing order, in the shape of the Rust
// layout_drawing output. It follows `drawings` in structure-canvas.tsx, without the compact view,
// which P38a1 does not draw.
function webLayout(document){
 const settings=settingsOf(document)
 const style=labelStyleOf(settings)
 const counts=degrees(document)
 const lineWidth=settings.line_width_pt
 const spacing=bondSpacing(lineWidth)
 const rings=bondRings(document)
 return drawingOrder(document).map((placement)=>{
  assert.deepEqual(compactDisplaysIn(document,placement.id),[],'abbreviation and fragment displays are not drawn in P38a1')
  const view=placedView(document,placement)
  const atoms=atomIndex(view)
  const left=leaningRight(view)
  const labels=new Map([...atoms.values()].map((atom)=>[atom.id,atomLabel(atom,counts.get(atom.id)??0,left.has(atom.id),style)]))
  const sides=bondSides(view,atoms,rings)
  return {
   instance:placement.id,
   atoms:Object.fromEntries([...atoms].map(([id,atom])=>[id,{x:atom.position.x,y:atom.position.y}])),
   bonds:Object.fromEntries(drawingStrokes(view,labels,atoms,sides,spacing,lineWidth)),
   labels:Object.fromEntries([...labels].filter(([,label])=>label!==null).map(([id,label])=>[id,labelRecord(label)])),
  }
 })
}

// Asserts that actual agrees with expected: the same keys and array lengths, numbers within the
// tolerance (EXACT_KEYS aside), and every other value equal.
function assertAgrees(actual,expected,tolerance,path){
 if(Array.isArray(expected)){
  assert.ok(Array.isArray(actual),`${path}: expected an array`)
  assert.equal(actual.length,expected.length,`${path}: length`)
  for(const [index,item] of expected.entries())assertAgrees(actual[index],item,tolerance,`${path}[${index}]`)
 }else if(expected!==null&&typeof expected==='object'){
  assert.ok(actual!==null&&typeof actual==='object',`${path}: expected an object`)
  assert.deepEqual(Object.keys(actual).sort(),Object.keys(expected).sort(),`${path}: keys`)
  for(const [key,item] of Object.entries(expected))assertAgrees(actual[key],item,EXACT_KEYS.has(key)?0:tolerance,`${path}.${key}`)
 }else if(typeof expected==='number'&&tolerance>0){
  assert.ok(typeof actual==='number'&&Math.abs(actual-expected)<=tolerance,`${path}: web ${actual} differs from Rust ${expected} by more than ${tolerance}`)
 }else{
  assert.equal(actual,expected,`${path}: web ${JSON.stringify(actual)} differs from Rust ${JSON.stringify(expected)}`)
 }
}

// Each instance of the Rust layout against the web's, in drawing order: ids exactly, atoms and labels
// within the number tolerance, bond strokes within the stroke tolerance.
function assertLayoutAgrees(actual,expected){
 assert.equal(actual.length,expected.length,'instance count')
 for(const [index,instance] of expected.entries()){
  assert.equal(actual[index].instance,instance.instance,`instances[${index}] id`)
  assertAgrees(actual[index].atoms,instance.atoms,NUMBER_TOLERANCE,`instances[${index}].atoms`)
  assertAgrees(actual[index].labels,instance.labels,NUMBER_TOLERANCE,`instances[${index}].labels`)
  assertAgrees(actual[index].bonds,instance.bonds,STROKE_TOLERANCE,`instances[${index}].bonds`)
 }
}

// P38a1 draws neither radical dots nor attachment points, so a parity case has neither.
function assertPlainAtoms(document){
 for(const atom of document.atoms){
  assert.equal(atom.radical,'none',`${atom.id}: a radical is not drawn in P38a1`)
  assert.deepEqual(atom.attachment_points,[],`${atom.id}: attachment points are not drawn in P38a1`)
 }
}

test('the parity fixture holds the nine golden documents in order',()=>{
 assert.deepEqual(fixture.cases.map((testCase)=>testCase.name),CASE_NAMES)
})

for(const testCase of fixture.cases){
 test(`${testCase.name}: the web layout is the Rust layout`,()=>{
  assertPlainAtoms(testCase.document)
  assertLayoutAgrees(webLayout(testCase.document),testCase.layout.instances)
 })
}
