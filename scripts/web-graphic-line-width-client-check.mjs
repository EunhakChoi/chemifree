import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'
import { sceneCutSource, sceneCutCopy, sceneCutAfter } from './lib/scene-cut-fixture.mjs'

register('./lib/web-alias-hooks.mjs',import.meta.url)
const {graphicLineWidth,graphicLineWidthCommands,parseGraphicLineWidth}=await import('../apps/web/src/editor/graphic-line-width.ts')
const {isDocumentResponse}=await import('../apps/web/src/lib/document-contract.ts')
const {sceneClipboardCopyMatches,sceneClipboardCutMatches,sceneClipboardPasteMapping}=await import('../apps/web/src/lib/scene-clipboard-contract.ts')
const {stillApplicable}=await import('../apps/web/src/editor/commands.ts')

test('graphic widths accept only bounded finite decimals or blank document inheritance',()=>{
 for(const [text,value]of [['',null],['  ',null],['.1',.1],['10',10],['1e0',1],['+2.5',2.5]])assert.equal(parseGraphicLineWidth(text),value)
 for(const text of ['0','.0999','10.001','NaN','Infinity','0x1','1 pt'])assert.equal(parseGraphicLineWidth(text),undefined,text)
 const graphic=sceneCutSource().document.scene.objects.find((object)=>object.id==='s6')
 assert.equal(graphicLineWidth(graphic,1.2),1.2)
 assert.equal(graphicLineWidth({...graphic,line_width_pt:null},2),2)
 assert.equal(graphicLineWidth({...graphic,line_width_pt:3},2),3)
 assert.equal(graphicLineWidth({...graphic,line_width_pt:3},4),3)
})

test('snapshot width is additive but malformed overrides cannot enter rendering',()=>{
 assert.equal(isDocumentResponse(sceneCutSource()),true)
 for(const value of [null,.1,10]){
  const reply=sceneCutSource();reply.document.scene.objects.find((object)=>object.id==='s6').line_width_pt=value
  assert.equal(isDocumentResponse(reply),true)
 }
 for(const value of [false,'1',0,.099,10.01,NaN,Infinity]){
  const reply=sceneCutSource();reply.document.scene.objects.find((object)=>object.id==='s6').line_width_pt=value
  assert.equal(isDocumentResponse(reply),false)
 }
})

test('width commands preserve explicit override identity and refuse stale graphic, revision or document',()=>{
 const document=sceneCutSource().document
 const graphic=document.scene.objects.find((object)=>object.id==='s6')
 const original=structuredClone(document)
 const edit={documentId:document.id,revision:document.revision,graphic,lineWidth:1.2}
 assert.deepEqual(graphicLineWidthCommands(document,edit),[{op:'set_graphic_line_width',object:'s6',line_width_pt:1.2}],'explicitly matching document width still records an override')
 assert.equal(graphicLineWidthCommands(document,{...edit,lineWidth:null}),null)
 for(const lineWidth of [.099,10.01,Infinity,NaN])assert.equal(graphicLineWidthCommands(document,{...edit,lineWidth}),null)
 assert.equal(graphicLineWidthCommands({...document,id:'other'},edit),null)
 assert.equal(graphicLineWidthCommands({...document,revision:4},edit),null)
 const moved=structuredClone(document);moved.scene.objects.find((object)=>object.id==='s6').points[0].x=5
 assert.equal(graphicLineWidthCommands(moved,edit),null)
 const overridden=structuredClone(document);overridden.scene.objects.find((object)=>object.id==='s6').line_width_pt=3
 assert.deepEqual(graphicLineWidthCommands(overridden,{...edit,graphic:overridden.scene.objects.find((object)=>object.id==='s6'),lineWidth:null}),[{op:'set_graphic_line_width',object:'s6',line_width_pt:null}])
 assert.deepEqual(stillApplicable([{op:'set_graphic_line_width',object:'s6',line_width_pt:3}],document),[{op:'set_graphic_line_width',object:'s6',line_width_pt:3}])
 assert.deepEqual(document,original)
})

test('graphics clipboard accepts inherited API null without dropping any explicit width',()=>{
 const source=sceneCutSource();const copy=sceneCutCopy(source)
 const requested={objects:['s4'],atoms:[],bonds:[]}
 for(const object of source.document.scene.objects)if(object.kind==='graphic')object.line_width_pt=null
 assert.equal(sceneClipboardCopyMatches(copy.payload,copy.copied,copy.roots,requested,source.document,source.document.settings),true)
 for(const width of [source.document.settings.line_width_pt,3]){
  const changed=structuredClone(source.document);changed.scene.objects.find((object)=>object.id==='s6').line_width_pt=width
  assert.equal(sceneClipboardCopyMatches(copy.payload,copy.copied,copy.roots,requested,changed,changed.settings),false,'an incorrect successful service reply cannot erase an override')
 }
 const payload=JSON.parse(copy.payload);payload.objects.find((object)=>object.kind==='graphic').line_width_pt=null
 assert.equal(sceneClipboardCopyMatches(JSON.stringify(payload),copy.copied,copy.roots,requested,source.document,source.document.settings),false,'clipboard schema stays strict even for null')
})

test('clipboard paste/cut normalize only inherited width while preserving outside overrides',()=>{
 const before=sceneCutSource().document
 for(const object of before.scene.objects)if(object.kind==='graphic')object.line_width_pt=null
 before.scene.objects.find((object)=>object.id==='s7').line_width_pt=3
 const payload=JSON.stringify({format:'chemifree.clipboard',version:2,kind:'graphics',settings:before.settings,roots:['s20'],objects:[{id:'s20',kind:'graphic',shape:'line',points:[{x:1,y:2},{x:31,y:2}]}],references:[]})
 const after=structuredClone(before);after.revision+=1
 after.scene.roots.push('s8');after.scene.objects.push({kind:'graphic',id:'s8',parent:null,shape:'line',points:[{x:11,y:22},{x:41,y:22}],line_width_pt:null})
 after.selection={objects:['s8'],atoms:[],bonds:[]}
 const mapping={objects:[{source:'s20',target:'s8'}]}
 assert.deepEqual(sceneClipboardPasteMapping(payload,mapping,{x:10,y:20},before,after,before.settings),mapping.objects)
 const override=structuredClone(after);override.scene.objects.find((object)=>object.id==='s8').line_width_pt=2
 assert.equal(sceneClipboardPasteMapping(payload,mapping,{x:10,y:20},before,override,before.settings),null)
 const lost=structuredClone(after);lost.scene.objects.find((object)=>object.id==='s7').line_width_pt=null
 assert.equal(sceneClipboardPasteMapping(payload,mapping,{x:10,y:20},before,lost,before.settings),null)
 const old=sceneCutSource().document;const cut=sceneCutAfter().document
 cut.scene.objects.find((object)=>object.id==='s7').line_width_pt=null
 assert.equal(sceneClipboardCutMatches(old,cut,['s4','s6','s5']),true)
})
