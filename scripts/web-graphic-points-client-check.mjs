import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { graphicPointsCommands, parseGraphicCoordinate } = await import('../apps/web/src/editor/graphic-points.ts')
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')

test('graphic coordinates accept the existing finite decimal point boundary without geometry restrictions', () => {
  for (const [text,expected] of [['-100000',-100000],['+100000',100000],[' 1.5 ',1.5],['-.5',-.5],['1e4',10000],['0',0]]) assert.equal(parseGraphicCoordinate(text),expected)
  for (const text of ['', ' ', 'NaN', 'Infinity', '1pt', '0x10', '0b10', '1e400', '100000.01', '-100000.01', '1,2']) assert.equal(parseGraphicCoordinate(text),null,text)
})

test('graphic edits capture exactly one graphic, preserving point order, shape, group and references', () => {
  const document=sceneCutSource().document
  const before=structuredClone(document)
  for (const shape of ['line','arrow','rectangle']) {
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
