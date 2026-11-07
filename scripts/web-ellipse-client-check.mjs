import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource,sceneCutCopy } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs',import.meta.url)
const {isDocumentResponse}=await import('../apps/web/src/lib/document-contract.ts')
const {sceneClipboardCopyMatches,sceneClipboardGraphicStyles,sceneClipboardPasteMapping}=await import('../apps/web/src/lib/scene-clipboard-contract.ts')
const {graphicPointsCommands}=await import('../apps/web/src/editor/graphic-points.ts')

function ellipseCopy(width=null){
 const source=sceneCutSource();const ellipse=source.document.scene.objects.find((object)=>object.id==='s6');ellipse.shape='ellipse';ellipse.line_width_pt=width
 const copy=sceneCutCopy(source);const payload=JSON.parse(copy.payload);payload.version=4
 for(const object of payload.objects)if(object.kind==='graphic')object.line_width_pt??=null
 return{source,copy,payload}
}
function matches(value){return sceneClipboardCopyMatches(JSON.stringify(value.payload),value.copy.copied,value.copy.roots,{objects:['s4'],atoms:[],bonds:[]},value.source.document,value.source.document.settings)}

for(const width of [null,.1,10,1.2])test(`ellipse clipboard uses required-width v4 even for ${width===null?'all inheritance':width}`,()=>{
 const value=ellipseCopy(width)
 assert.equal(matches(value),true)
 assert.deepEqual(sceneClipboardGraphicStyles(JSON.stringify(value.payload)),[{id:'s6',shape:'ellipse',lineWidth:width},{id:'s5',shape:'arrow',lineWidth:null}])
 for(const version of [2,3]){const legacy=structuredClone(value);legacy.payload.version=version;if(version===2)for(const object of legacy.payload.objects)delete object.line_width_pt;assert.equal(matches(legacy),false);assert.equal(sceneClipboardGraphicStyles(JSON.stringify(legacy.payload)),null)}
})

for(const [name,change]of[
 ['missing width',(p)=>{delete p.objects[1].line_width_pt}],
 ['range',(p)=>{p.objects[1].line_width_pt=10.01}],
 ['unknown radius',(p)=>{p.objects[1].rx=4}],
 ['group width',(p)=>{p.objects[0].line_width_pt=null}],
 ['future version',(p)=>{p.version=7}],
 ['unrecognized shape',(p)=>{p.objects[1].shape='circle'}],
])test(`ellipse v4 strictly rejects ${name}`,()=>{
 const value=ellipseCopy();change(value.payload);assert.equal(matches(value),false);assert.equal(sceneClipboardGraphicStyles(JSON.stringify(value.payload)),null)
})

test('v4 old-shape input remains readable while copy chooses its canonical v2/v3 profile',()=>{
 for(const width of [null,10]){
  const value=ellipseCopy(width);value.source.document.scene.objects.find((object)=>object.id==='s6').shape='line';value.payload.objects[1].shape='line'
  assert.equal(sceneClipboardGraphicStyles(JSON.stringify(value.payload))[0].shape,'line')
  assert.equal(matches(value),false,'old-shape copies retain their v2/v3 output profile')
  value.payload.version=width===null?2:3
  if(width===null)for(const object of value.payload.objects)delete object.line_width_pt
  assert.equal(matches(value),true)
 }
 const source=sceneCutSource();source.document.scene.objects.find((object)=>object.id==='s7').shape='ellipse'
 const copy=sceneCutCopy(source)
 assert.equal(sceneClipboardCopyMatches(copy.payload,copy.copied,copy.roots,{objects:['s4'],atoms:[],bonds:[]},source.document,source.document.settings),true,'an unselected ellipse does not change the copied profile')
})

test('ellipse opposite corners preserve reversed/zero axes in API snapshots and endpoint commands',()=>{
 for(const points of [[{x:90,y:80},{x:50,y:40}],[{x:50,y:40},{x:50,y:80}],[{x:50,y:40},{x:90,y:40}],[{x:50,y:40},{x:50,y:40}]]){
  const value=ellipseCopy();const ellipse=value.source.document.scene.objects.find((object)=>object.id==='s6');ellipse.points=points
  assert.equal(isDocumentResponse(value.source),true)
  const edit={documentId:value.source.document.id,revision:value.source.document.revision,graphic:ellipse,points:[{x:-100000,y:100000},{x:100000,y:-100000}]}
  assert.deepEqual(graphicPointsCommands(value.source.document,edit),[{op:'update_graphic_points',object:'s6',points:edit.points}])
 }
})

test('ellipse v4 paste retains exact point order, width and all outside graph/scene state',()=>{
 const before=sceneCutSource().document
 const payload={format:'chemifree.clipboard',version:4,kind:'graphics',settings:before.settings,roots:['s20'],objects:[{kind:'graphic',id:'s20',shape:'ellipse',points:[{x:90,y:80},{x:50,y:40}],line_width_pt:10}],references:[]}
 const after=structuredClone(before);after.revision+=1;after.selection={objects:['s8'],atoms:[],bonds:[]};after.scene.roots.push('s8');after.scene.objects.push({kind:'graphic',id:'s8',parent:null,shape:'ellipse',points:[{x:100,y:100},{x:60,y:60}],line_width_pt:10})
 const mapping={objects:[{source:'s20',target:'s8'}]};const paste=(document)=>sceneClipboardPasteMapping(JSON.stringify(payload),mapping,{x:10,y:20},before,document,before.settings)
 assert.deepEqual(paste(after),mapping.objects)
 for(const change of [(d)=>{d.scene.objects.at(-1).shape='rectangle'},(d)=>{d.scene.objects.at(-1).line_width_pt=null},(d)=>{d.scene.objects.at(-1).points.reverse()},(d)=>{d.atoms[0].charge=1}]){const changed=structuredClone(after);change(changed);assert.equal(paste(changed),null)}
 const duplicate=JSON.stringify(payload).replace('"line_width_pt":10','"line_width_pt":1,"line_width_pt":10')
 assert.equal(sceneClipboardGraphicStyles(duplicate),null)
})
