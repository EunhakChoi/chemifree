import assert from 'node:assert/strict'
import {register} from 'node:module'
import {test} from 'node:test'
import {sceneCutSource,sceneCutCopy} from './lib/scene-cut-fixture.mjs'
import {pngFixture,pngChunk} from './lib/web-png-fixture.mjs'
register('./lib/web-alias-hooks.mjs',import.meta.url)
const {embeddedPngMetadata,pngMetadata,readPngFile,MAX_PNG_BYTES}=await import('../apps/web/src/lib/png-image.ts')
const {isDocumentResponse}=await import('../apps/web/src/lib/document-contract.ts')
const {imageCreationCommands,imageGeometryCommands,imageGeometryValid,imagePixelTotal,parseImageSize}=await import('../apps/web/src/editor/image.ts')
const {selectionGeometry,selectionMove,selectionTransform,marqueeSelection}=await import('../apps/web/src/editor/selection-geometry.ts')
const {sceneGeometry,sceneGeometryCommands}=await import('../apps/web/src/editor/scene-geometry.ts')
const {sceneClipboardCopyMatches,sceneClipboardGraphicStyles}=await import('../apps/web/src/lib/scene-clipboard-contract.ts')
const {graphicPointsCommands,selectedGraphic}=await import('../apps/web/src/editor/graphic-points.ts')
const bytes=pngFixture();const data=bytes.toString('base64')
function fixture(png=bytes){const source=sceneCutSource();const image=source.document.scene.objects.find((o)=>o.id==='s6');delete image.shape;delete image.points;Object.assign(image,{kind:'image',mime_type:'image/png',data_base64:png.toString('base64'),origin:{x:40,y:50},size_pt:{x:20,y:30},pixel_width:png.readUInt32BE(16),pixel_height:png.readUInt32BE(20)});return{source,image}}
test('PNG preflight accepts bounded static RGB/RGBA without native decoding; rejects huge headers, animation and metadata inflation paths',()=>{
 for(const png of [bytes,pngFixture({color:2}),pngFixture({extra:[pngChunk('sRGB',Buffer.from([0])),pngChunk('gAMA',Buffer.from([0,0,177,143])),pngChunk('pHYs',Buffer.alloc(9))]})])assert.deepEqual(embeddedPngMetadata(png.toString('base64')),{pixelWidth:2,pixelHeight:2,byteLength:png.length})
 for(const png of [pngFixture({headerWidth:2049}),pngFixture({headerWidth:1024,headerHeight:1025}),pngFixture({headerWidth:0}),pngFixture({depth:16}),pngFixture({color:3}),pngFixture({interlace:1}),pngFixture({extra:[pngChunk('acTL',Buffer.alloc(8))]}),pngFixture({extra:[pngChunk('iCCP',Buffer.alloc(10))]}),pngFixture({extra:[pngChunk('tEXt',Buffer.alloc(10))]}),pngFixture({extra:[pngChunk('IHDR',bytes.subarray(16,29))]}),Buffer.concat([bytes,Buffer.from([0])]),bytes.subarray(0,bytes.length-1)])assert.equal(pngMetadata(png),null)
 for(const value of ['data:image/png;base64,'+data,'https://example.test/a.png',data+'\n',data.slice(0,-1)+'!','',null,'A'.repeat(Math.ceil(MAX_PNG_BYTES/3)*4+4)])assert.equal(embeddedPngMetadata(value),null)
})
test('oversized or wrong-MIME files are refused before arrayBuffer; exact compressed bound is read but still validated',async()=>{
 let reads=0;const file=(size,type,content=bytes)=>({size,type,async arrayBuffer(){reads++;return content.buffer.slice(content.byteOffset,content.byteOffset+content.byteLength)}})
 await assert.rejects(readPngFile(file(MAX_PNG_BYTES+1,'image/png')),/128 KiB/);await assert.rejects(readPngFile(file(bytes.length,'image/svg+xml')),/image\/png/);assert.equal(reads,0)
 const selected=await readPngFile(file(MAX_PNG_BYTES,'image/png'));assert.equal(reads,1);assert.equal(selected.dataBase64,data)
 await assert.rejects(readPngFile(file(30,'image/png',Buffer.alloc(30))),/static/)
})
test('snapshot validates canonical PNG bytes, derived dimensions, rect and current aggregate pixels before rendering',()=>{
 const v=fixture();assert.equal(isDocumentResponse(v.source),true)
 for(const change of [(o)=>o.mime_type='image/svg+xml',(o)=>o.data_base64='https://example.test/png',(o)=>o.pixel_width=3,(o)=>o.size_pt.x=0,(o)=>o.origin.x=99999,(o)=>o.size_pt.y=Infinity,(o)=>o.data_base64=pngFixture({headerWidth:1000000}).toString('base64')]){const x=structuredClone(v.source);change(x.document.scene.objects.find((o)=>o.id==='s6'));assert.equal(isDocumentResponse(x),false)}
 const png=pngFixture({width:1024,height:1024});const large=fixture(png);large.source.document.scene.objects.find((o)=>o.id==='s4').children=['s6','s5'];for(let n=8;n<=10;n++){large.source.document.scene.roots.push('s'+n);large.source.document.scene.objects.push({...large.image,id:'s'+n,parent:null})}assert.equal(imagePixelTotal(large.source.document),4194304);assert.equal(isDocumentResponse(large.source),true);large.source.document.scene.roots.push('s11');large.source.document.scene.objects.push({...large.image,id:'s11',parent:null});assert.equal(isDocumentResponse(large.source),false)
})
test('explicit rect supports the full coordinate interval, refuses invalid size/corners and has no pixel-to-pt inference',()=>{
 for(const value of ['1','0.1','200000','+2e5'])assert.notEqual(parseImageSize(value),null)
 for(const value of ['','0','-1','Infinity','NaN','0x10','200001'])assert.equal(parseImageSize(value),null)
 assert.equal(imageGeometryValid({x:-100000,y:-100000},{x:200000,y:200000}),true);assert.equal(imageGeometryValid({x:0,y:0},{x:100001,y:1}),false)
 const doc=sceneCutSource().document;const creation={documentId:doc.id,revision:doc.revision,serviceGeneration:2,dataBase64:data,origin:{x:40,y:50},sizePt:{x:20,y:30}}
 assert.deepEqual(imageCreationCommands(doc,creation),[{op:'add_image',mime_type:'image/png',data_base64:data,origin:creation.origin,size_pt:creation.sizePt}]);assert.equal(imageCreationCommands({...doc,revision:doc.revision+1},creation),null);assert.equal(imageCreationCommands({...doc,id:'other'},creation),null);assert.equal(imageCreationCommands(doc,{...creation,sizePt:{x:0,y:1}}),null)
})
test('image geometry edit preserves embedded bytes and target context; graphics endpoints cannot mutate images',()=>{
 const {source,image}=fixture();const doc=source.document;const edit={documentId:doc.id,revision:doc.revision,image,origin:{x:80,y:90},sizePt:{x:40,y:10}}
 assert.deepEqual(imageGeometryCommands(doc,edit),[{op:'update_image_geometry',object:image.id,origin:edit.origin,size_pt:edit.sizePt}]);assert.equal(imageGeometryCommands(doc,{...edit,origin:image.origin,sizePt:image.size_pt}),null)
 for(const change of [(d)=>d.revision++,(d)=>d.id='other',(d)=>d.scene.objects.find((o)=>o.id===image.id).parent=null,(d)=>d.scene.objects=d.scene.objects.filter((o)=>o.id!==image.id)]){const changed=structuredClone(doc);change(changed);assert.equal(imageGeometryCommands(changed,edit),null)}
 assert.equal(graphicPointsCommands(doc,{documentId:doc.id,revision:doc.revision,graphic:image,points:[{x:0,y:0},{x:1,y:1}]}),null);assert.equal(selectedGraphic(doc,new Set([image.id])),null);assert.equal(image.data_base64,data)
})
test('whole-image and group motion includes image IDs; transparent rectangle bounds drive marquee/arrange and mixed transforms refuse atomically',()=>{
 const {source,image}=fixture();const doc=source.document;const scope=selectionGeometry(doc,new Set(['s4']),null);assert.equal(scope.objects.has(image.id),true)
 assert.deepEqual(selectionMove(doc,new Set([image.id]),null,{x:10,y:-2}),[{op:'move_selection',selection:{objects:[image.id],atoms:[],bonds:[]},delta:{x:10,y:-2}}])
 assert.equal(marqueeSelection(doc,null,{x:39,y:49},{x:61,y:81},new Set(),false,true).has(image.id),true);assert.equal(marqueeSelection(doc,null,{x:39,y:49},{x:61,y:79},new Set(),false,true).has(image.id),false)
 assert.deepEqual(sceneGeometry(doc,new Set([image.id])).bounds,{left:40,top:50,right:60,bottom:80});assert.notEqual(sceneGeometryCommands(doc,new Set(['s4','s7']),{kind:'arrange',mode:'align_left'}),null)
 for(const selected of [[image.id],['s4'],[image.id,'a1']])for(const transform of [{type:'rotate',degrees:0},{type:'rotate',degrees:45},{type:'reflect_vertical'}])assert.equal(selectionTransform(doc,new Set(selected),'s1',transform),null)
})
test('graphic clipboard never silently drops an image or a containing group; unknown image payloads never preview as graphics',()=>{
 const v=fixture();const copy=sceneCutCopy(v.source);assert.equal(sceneClipboardCopyMatches(copy.payload,copy.copied,copy.roots,{objects:['s4'],atoms:[],bonds:[]},v.source.document,v.source.document.settings),false)
 const outside=sceneCutSource();const unselected=outside.document.scene.objects.find((o)=>o.id==='s7');delete unselected.shape;delete unselected.points;Object.assign(unselected,{...v.image,id:'s7',parent:'s3'});const ordinary=sceneCutCopy(outside);assert.equal(sceneClipboardCopyMatches(ordinary.payload,ordinary.copied,ordinary.roots,{objects:['s4'],atoms:[],bonds:[]},outside.document,outside.document.settings),true,'an unselected image never prevents copying the chosen graphic forest')
 const payload=JSON.parse(copy.payload);payload.version=6;for(const o of payload.objects)if(o.kind==='graphic')o.line_width_pt=null;assert.equal(sceneClipboardGraphicStyles(JSON.stringify(payload)),null)
})
