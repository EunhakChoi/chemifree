import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { graphicPointsCommands, parseGraphicCoordinate, selectedGraphic, graphicEndpointCurrent, graphicEndpointPoints } = await import('../apps/web/src/editor/graphic-points.ts')
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')

test('graphic coordinates accept the existing finite decimal point boundary without geometry restrictions', () => {
  for (const [text,expected] of [['-100000',-100000],['+100000',100000],[' 1.5 ',1.5],['-.5',-.5],['1e4',10000],['0',0]]) assert.equal(parseGraphicCoordinate(text),expected)
  for (const text of ['', ' ', 'NaN', 'Infinity', '1pt', '0x10', '0b10', '1e400', '100000.01', '-100000.01', '1,2']) assert.equal(parseGraphicCoordinate(text),null,text)
})

test('graphic edits capture exactly one graphic, preserving point order, shape, group and references', () => {
  const document=sceneCutSource().document
  document.scene.roots.push('s8')
  document.scene.objects.push({kind:'graphic',id:'s8',parent:null,shape:'ellipse',points:[{x:30,y:40},{x:10,y:20}],line_width_pt:null})
  const before=structuredClone(document)
  for (const shape of ['line','arrow','rectangle','ellipse']) {
    const graphic=document.scene.objects.find((object)=>object.kind==='graphic'&&object.shape===shape)
    const points=[{x:100000,y:-100000},{x:-100000,y:100000}]
    const edit={documentId:document.id,revision:document.revision,graphic,points}
    assert.deepEqual(graphicPointsCommands(document,edit),[{op:'update_graphic_points',object:graphic.id,points}])
    const coincident=[{x:1,y:1},{x:1,y:1}]
    assert.deepEqual(graphicPointsCommands(document,{...edit,points:coincident})[0].points,coincident)
    assert.equal(graphicPointsCommands(document,{...edit,points:graphic.points}),null)
    assert.equal(graphicPointsCommands(document,{...edit,points:[{x:Infinity,y:1},{x:1,y:1}]}),null)
    assert.equal(graphicPointsCommands(document,{...edit,points:[{x:100001,y:1},{x:1,y:1}]}),null)
    const built=graphicPointsCommands(document,edit)
    points[0].x=9
    assert.equal(built[0].points[0].x,100000,'queued command owns copied points')
    assert.deepEqual(stillApplicable(built,document),built)
    assert.equal(stillApplicable([{...built[0],object:'removed'}],document),null)
  }
  assert.deepEqual(document,before)
})

test('queued endpoint edits lapse after another revision, document switch, move, regroup or target replacement', () => {
  const document=sceneCutSource().document
  const graphic=document.scene.objects.find((object)=>object.id==='s6')
  const edit={documentId:document.id,revision:document.revision,graphic,points:[{x:10,y:20},{x:40,y:50}]}
  for (const mutate of [
    (d)=>{d.revision+=1},(d)=>{d.id='another'},
    (d)=>{d.scene.objects.find((o)=>o.id==='s6').points[0].x+=10},
    (d)=>{d.scene.objects.find((o)=>o.id==='s6').parent='s3'},
    (d)=>{d.scene.objects=d.scene.objects.filter((o)=>o.id!=='s6')},
    (d)=>{d.scene.objects[d.scene.objects.findIndex((o)=>o.id==='s6')]={kind:'group',id:'s6',parent:'s4',children:[]}},
  ]) {
    const changed=structuredClone(document);mutate(changed)
    assert.equal(graphicPointsCommands(changed,edit),null)
  }
})

test('endpoint handles require one explicit noncoincident graphic, excluding ancestor and mixed selection',()=>{
 const document=sceneCutSource().document
 assert.equal(selectedGraphic(document,new Set(['s6'])).id,'s6')
 for(const ids of [[],['s4'],['a1'],['s6','s5'],['s6','a1'],['removed']])assert.equal(selectedGraphic(document,new Set(ids)),null)
 const coincident=structuredClone(document)
 coincident.scene.objects.find((object)=>object.id==='s6').points=[{x:1,y:1},{x:1,y:1}]
 assert.equal(selectedGraphic(coincident,new Set(['s6'])),null,'coincident points use the numeric form')
})

test('endpoint drag preserves press offset and opposite point and refuses expired target context',()=>{
 const document=sceneCutSource().document
 const graphic=selectedGraphic(document,new Set(['s5']))
 const before=structuredClone(document)
 const gesture={endpoint:1,start:{x:32,y:13},edit:{documentId:document.id,revision:document.revision,graphic,points:graphic.points}}
 const points=graphicEndpointPoints(gesture,{x:42,y:3})
 assert.deepEqual(points,[{x:1,y:12},{x:41,y:2}],'offset inside a handle does not jump its endpoint')
 assert.deepEqual(graphicPointsCommands(document,{...gesture.edit,points}),[{op:'update_graphic_points',object:'s5',points}])
 assert.equal(graphicEndpointCurrent(document,new Set(['s5']),gesture),true)
 assert.equal(graphicEndpointCurrent({...document,revision:document.revision+1},new Set(['s5']),gesture),false)
 assert.equal(graphicEndpointCurrent({...document,id:'another'},new Set(['s5']),gesture),false)
 assert.equal(graphicEndpointCurrent(document,new Set(['s6']),gesture),false)
 assert.equal(graphicEndpointCurrent(document,new Set(['s5','s6']),gesture),false)
 const moved=structuredClone(document);moved.scene.objects.find((object)=>object.id==='s5').points[0].x=2
 assert.equal(graphicEndpointCurrent(moved,new Set(['s5']),gesture),false)
 assert.equal(graphicEndpointPoints(gesture,{x:Infinity,y:1}),null)
 assert.equal(graphicEndpointPoints(gesture,{x:100100,y:1}),null)
 assert.deepEqual(document,before)
})
