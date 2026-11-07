import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs',import.meta.url)
const {captureSceneOrder,sceneOrderCommands}=await import('../apps/web/src/editor/scene-order.ts')
const capture=(document,id)=>captureSceneOrder(document,new Set([id]))

test('order captures exactly one scene object and its parent without targeting graph IDs',()=>{
 const document=sceneCutSource().document
 assert.equal(captureSceneOrder(document,new Set()),null)
 assert.equal(captureSceneOrder(document,new Set(['a1','b1'])),null)
 assert.equal(captureSceneOrder(document,new Set(['s5','s6'])),null)
 assert.deepEqual(captureSceneOrder(document,new Set(['s6','a1','b1'])),{documentId:document.id,object:'s6',parent:'s4'})
})

test('root and nested group boundaries use the existing reorder command and skip identities',()=>{
 const document=sceneCutSource().document;const before=structuredClone(document)
 assert.deepEqual(sceneOrderCommands(document,capture(document,'s2'),'first'),[{op:'reorder',object:'s2',index:0}])
 assert.deepEqual(sceneOrderCommands(document,capture(document,'s2'),'last'),[{op:'reorder',object:'s2',index:2}])
 assert.equal(sceneOrderCommands(document,capture(document,'s1'),'first'),null)
 assert.equal(sceneOrderCommands(document,capture(document,'s3'),'last'),null)
 assert.deepEqual(sceneOrderCommands(document,capture(document,'s3'),'first'),[{op:'reorder',object:'s3',index:0}],'a group is one subtree target')
 assert.equal(sceneOrderCommands(document,capture(document,'s4'),'first'),null)
 assert.deepEqual(sceneOrderCommands(document,capture(document,'s4'),'last'),[{op:'reorder',object:'s4',index:1}])
 assert.deepEqual(sceneOrderCommands(document,capture(document,'s5'),'first'),[{op:'reorder',object:'s5',index:0}])
 assert.deepEqual(sceneOrderCommands(document,capture(document,'s6'),'last'),[{op:'reorder',object:'s6',index:1}])
 assert.deepEqual(document,before)
})

test('queued order recomputes the latest sibling count and boundary after additions or prior reorders',()=>{
 const document=sceneCutSource().document
 const target=capture(document,'s2')
 const grown=structuredClone(document);grown.revision+=1;grown.scene.roots.push('s8');grown.scene.objects.push({kind:'graphic',id:'s8',parent:null,shape:'line',points:[{x:0,y:0},{x:1,y:1}]})
 assert.deepEqual(sceneOrderCommands(grown,target,'last'),[{op:'reorder',object:'s2',index:3}])
 const nested=capture(document,'s6')
 const child=structuredClone(document);child.scene.objects.find((object)=>object.id==='s4').children.push('s8');child.scene.objects.push({...grown.scene.objects.at(-1),parent:'s4'})
 assert.deepEqual(sceneOrderCommands(child,nested,'last'),[{op:'reorder',object:'s6',index:2}])
 const boundary=structuredClone(document);boundary.scene.roots=['s1','s3','s2']
 assert.equal(sceneOrderCommands(boundary,target,'last'),null,'already at the queued boundary: no extra transaction')
 const shrunk=structuredClone(document);shrunk.scene.objects.find((object)=>object.id==='s4').children=['s6'];shrunk.scene.objects=shrunk.scene.objects.filter((object)=>object.id!=='s5')
 assert.equal(sceneOrderCommands(shrunk,nested,'last'),null)
})

for(const [name,change]of[
 ['different document',(document)=>{document.id='other'}],
 ['deleted object',(document)=>{document.scene.objects=document.scene.objects.filter((object)=>object.id!=='s6')}],
 ['reparented object',(document)=>{document.scene.objects.find((object)=>object.id==='s6').parent='s3';document.scene.objects.find((object)=>object.id==='s4').children=['s5'];document.scene.objects.find((object)=>object.id==='s3').children.push('s6')}],
 ['missing parent',(document)=>{document.scene.objects=document.scene.objects.filter((object)=>object.id!=='s4')}],
 ['missing sibling membership',(document)=>{document.scene.objects.find((object)=>object.id==='s4').children=['s5']}],
])test(`captured order lapses for ${name}`,()=>{
 const document=sceneCutSource().document;const target=capture(document,'s6');change(document)
 assert.equal(sceneOrderCommands(document,target,'first'),null)
 assert.equal(sceneOrderCommands(document,target,'last'),null)
})

test('a captured root never follows the object into a new group',()=>{
 const document=sceneCutSource().document;const target=capture(document,'s2')
 document.scene.roots=['s1','s3'];document.scene.objects.find((object)=>object.id==='s2').parent='s3';document.scene.objects.find((object)=>object.id==='s3').children.push('s2')
 assert.equal(sceneOrderCommands(document,target,'first'),null)
 assert.equal(sceneOrderCommands(document,target,'last'),null)
})
