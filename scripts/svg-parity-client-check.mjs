// Checks that the web lays out the golden documents as the Rust SVG renderer does (P38a1).
// crates/application/tests/fixtures/svg/parity.json holds the Rust layout of each golden document
// (`layout_drawing` in svg.rs). This script lays out the same documents with the web's geometry and
// label modules, in the order structure-canvas.tsx draws them, and compares the two. Both sides round
// stroke coordinates to the hundredth and a tie can round one step apart, so strokes agree within
// 0.011; label and atom numbers within 1e-6; texts, weights, shapes, patterns, counts and ids exactly.
// It only reads the fixture. The web is run without browser label metrics, as the Rust renderer is.
// Each case's paint list (`objects`, P38b1) is compared too: kinds and ids exactly; graphic marks and image
// rectangles within 1e-6 (a graphic's numbers are copied or computed without the rounding bonds' strokes need).
// A caption's entry is compared with the web's run styles (caption-style.ts): its position and anchor, and each
// run's text, family, size, shift, weight, style, colour and outline, numbers within 1e-6. Its reserved box is
// Rust only, since the web measures no advance.
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
const {graphicMarks}=await import('../apps/web/src/editor/graphic-marks.ts')
const {imageCorners}=await import('../apps/web/src/editor/image.ts')
const {captionRunStyle,captionTextAnchor}=await import('../apps/web/src/editor/caption-style.ts')
const fixture=JSON.parse(readFileSync(new URL('../crates/application/tests/fixtures/svg/parity.json',import.meta.url),'utf8'))
const STROKE_TOLERANCE=0.011
const NUMBER_TOLERANCE=1e-6
// Compared exactly whatever their type: texts, font weights, shape and pattern names, counts.
const EXACT_KEYS=new Set(['shape','pattern','text','weight','halfWaves'])
// The golden documents in the order the Rust fixture lists them.
const CASE_NAMES=['chain-bare-vertices','bond-orders','stereo-marks','labels','settings-wide-serif','monospace-small','two-instances','scene-object-order','empty','scene-graphics','scene-image','graphics-only','captions']

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

// The web's paint list, in the shape of the Rust `objects` (ObjectLayout, svg/scene.rs). It mirrors the loop
// over scene.objects at structure-canvas.tsx:1056-1088: a group draws nothing; an image, a graphic and a caption
// draw an entry; an instance draws an entry when it has a placement. The web has no copy of the drawn character
// set, so it lists every caption: a caption outside that set has no Rust entry, and no golden case has one.
// An instance that scene.objects does not list comes first, in placement order, as paint_list in svg.rs has it.
// The web's loop never visits such an instance (structure-canvas.tsx:378-379 only sorts it first), so that rule
// is the export's. No golden case has one, so the fixture does not exercise it.
function webPaintList(document){
 assert.ok(document.scene,'a parity case has a scene: the paint loop draws its scene.objects')
 const listed=new Set(document.scene.objects.map((object)=>object.id))
 const placed=new Set(placements(document).map((placement)=>placement.id))
 const unlisted=drawingOrder(document).filter((placement)=>!listed.has(placement.id)).map((placement)=>({kind:'instance',id:placement.id}))
 const drawn=document.scene.objects.flatMap((object)=>{
  if(object.kind==='instance')return placed.has(object.id)?[{kind:'instance',id:object.id}]:[]
  if(object.kind==='graphic')return [{kind:'graphic',id:object.id,...graphicMarks(object,settingsOf(document).line_width_pt)}]
  if(object.kind==='image'){
   const [origin,end]=imageCorners(object)
   return [{kind:'image',id:object.id,x:origin.x,y:origin.y,width:end.x-origin.x,height:end.y-origin.y}]
  }
  if(object.kind==='caption')return [webCaption(document,object)]
  return []
 })
 return [...unlisted,...drawn]
}

// The web's record of a caption, in the shape captionEntry gives the Rust entry: its anchor and each run's style
// from captionRunStyle (caption-style.ts). A null colour is the theme foreground, which the export draws as #000000.
// The reserved box is not part of the record: the web measures no advance, so only the Rust layout has one.
function webCaption(document,object){
 const settings=settingsOf(document)
 return {
  kind:object.kind,
  id:object.id,
  x:object.anchor.x,
  y:object.anchor.y,
  anchor:captionTextAnchor(object.alignment),
  runs:object.runs.map((run)=>{
   const style=captionRunStyle(run,settings)
   return {text:run.text,family:style.family,size:style.size,shift:style.shift,weight:style.weight,style:style.style,color:style.color??'#000000',outline:style.outline}
  }),
 }
}

// The part of a Rust caption entry that webCaption records: its position and anchor, and each run's text and style.
// The reserved box and the shadow and underline flags are left out.
function captionEntry(entry){
 return {
  kind:entry.kind,
  id:entry.id,
  x:entry.x,
  y:entry.y,
  anchor:entry.anchor,
  runs:entry.runs.map(({text,family,size,shift,weight,style,color,outline})=>({text,family,size,shift,weight,style,color,outline})),
 }
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

// Asserts that the web's paint list agrees with the Rust `objects`: kinds and ids exactly and in order, then
// each entry, a graphic's width and marks and an image's rectangle within the number tolerance. A caption is
// compared on the fields captionEntry keeps, so its reserved box, which only the Rust layout has, is not compared.
function assertObjectsAgree(document,expected){
 const actual=webPaintList(document)
 assert.deepEqual(actual.map(({kind,id})=>({kind,id})),expected.map(({kind,id})=>({kind,id})),'objects: kinds and ids')
 for(const [index,entry] of expected.entries()){
  const wanted=entry.kind==='caption'?captionEntry(entry):entry
  assertAgrees(actual[index],wanted,NUMBER_TOLERANCE,`objects[${index}]`)
 }
}

// P38a1 draws neither radical dots nor attachment points, so a parity case has neither.
function assertPlainAtoms(document){
 for(const atom of document.atoms){
  assert.equal(atom.radical,'none',`${atom.id}: a radical is not drawn in P38a1`)
  assert.deepEqual(atom.attachment_points,[],`${atom.id}: attachment points are not drawn in P38a1`)
 }
}

test('the parity fixture holds the thirteen golden documents in order',()=>{
 assert.deepEqual(fixture.cases.map((testCase)=>testCase.name),CASE_NAMES)
})

for(const testCase of fixture.cases){
 test(`${testCase.name}: the web layout is the Rust layout`,()=>{
  assertPlainAtoms(testCase.document)
  assertLayoutAgrees(webLayout(testCase.document),testCase.layout.instances)
 })
 test(`${testCase.name}: the web paint list is the Rust paint list`,()=>{
  assertObjectsAgree(testCase.document,testCase.layout.objects)
 })
}
