import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs',import.meta.url)
const {graphicCreationCommands,graphicCreationCurrent}=await import('../apps/web/src/editor/graphic-creation.ts')
const {graphicPointValid}=await import('../apps/web/src/editor/graphic-points.ts')

test('graphic creation freezes exact ordered points and adds only a page-root graphic',()=>{
 const document=sceneCutSource().document
 const original=structuredClone(document)
 for(const shape of ['line','arrow','rectangle','ellipse']){
  const creation={documentId:document.id,revision:document.revision,serviceGeneration:2,shape,points:[{x:100000,y:-100000},{x:-100000,y:100000}]}
  const commands=graphicCreationCommands(document,creation)
  assert.deepEqual(commands,[{op:'add_graphic',shape,points:creation.points}])
  assert.equal('parent' in commands[0],false,'selection cannot implicitly parent a new graphic')
  assert.equal('index' in commands[0],false)
  creation.points[0].x=5
  assert.equal(commands[0].points[0].x,100000,'queued payload owns copied points')
 }
 assert.deepEqual(document,original)
})

test('graphic creation refuses stale document/revision, wrong shape/service and unsupported coordinates',()=>{
 const document=sceneCutSource().document
 const creation={documentId:document.id,revision:document.revision,serviceGeneration:2,shape:'arrow',points:[{x:1,y:2},{x:3,y:4}]}
 assert.equal(graphicCreationCurrent(document,'arrow',2,creation),true)
 for(const [current,shape,generation] of [[{...document,id:'other'},'arrow',2],[{...document,revision:4},'arrow',2],[document,'line',2],[document,null,2],[document,'arrow',3]])assert.equal(graphicCreationCurrent(current,shape,generation,creation),false)
 assert.equal(graphicCreationCommands({...document,id:'other'},creation),null)
 assert.equal(graphicCreationCommands({...document,revision:4},creation),null)
 for(const point of [{x:Infinity,y:0},{x:NaN,y:0},{x:0,y:100001},{x:-100001,y:0}]){
  assert.equal(graphicPointValid(point),false)
  assert.equal(graphicCreationCommands(document,{...creation,points:[creation.points[0],point]}),null)
 }
 assert.equal(graphicPointValid({x:100000,y:-100000}),true)
 assert.deepEqual(graphicCreationCommands(document,{...creation,points:[{x:1,y:1},{x:1,y:1}]}),[{op:'add_graphic',shape:'arrow',points:[{x:1,y:1},{x:1,y:1}]}],'the command builder adds no geometry restriction beyond existing point bounds')
})
