import assert from 'node:assert/strict'
import {register} from 'node:module'
import {test} from 'node:test'
import {sceneCutSource,sceneCutCopy} from './lib/scene-cut-fixture.mjs'
register('./lib/web-alias-hooks.mjs',import.meta.url)
const {isDocumentResponse}=await import('../apps/web/src/lib/document-contract.ts')
const {appendPolygonVertex,polygonCreationCommands,polygonDraftCurrent,polygonVerticesCommands,polygonVerticesValid}=await import('../apps/web/src/editor/polygon.ts')
const {graphicPointsCommands,selectedGraphic,translatedGraphicObject}=await import('../apps/web/src/editor/graphic-points.ts')
const {marqueeSelection,selectionTransform}=await import('../apps/web/src/editor/selection-geometry.ts')
const {graphicCreationCommands}=await import('../apps/web/src/editor/graphic-creation.ts')
const {sceneClipboardCopyMatches,sceneClipboardGraphicStyles,sceneClipboardPasteMapping}=await import('../apps/web/src/lib/scene-clipboard-contract.ts')
const vertices=[{x:40,y:50},{x:80,y:70},{x:55,y:90},{x:40,y:50}]
function fixture(width=10){const source=sceneCutSource();const polygon=source.document.scene.objects.find((o)=>o.id==='s6');polygon.shape='polygon';polygon.points=structuredClone(vertices);polygon.line_width_pt=width;const copy=sceneCutCopy(source);const payload=JSON.parse(copy.payload);payload.version=5;for(const o of payload.objects)if(o.kind==='graphic')o.line_width_pt??=null;return {source,polygon,copy,payload}}
function matches(v){return sceneClipboardCopyMatches(JSON.stringify(v.payload),v.copy.copied,v.copy.roots,{objects:['s4'],atoms:[],bonds:[]},v.source.document,v.source.document.settings)}
test('polygon snapshots require exactly 3–64 bounded vertices; old graphics remain exact2',()=>{
 for(const count of [0,1,2,3,64,65]){const v=fixture();v.polygon.points=Array.from({length:count},()=>({x:1,y:1}));assert.equal(isDocumentResponse(v.source),count>=3&&count<=64);v.polygon.shape='line';assert.equal(isDocumentResponse(v.source),count===2)}
 for(const point of [{x:100001,y:0},{x:Infinity,y:0},{x:0,y:NaN}]){const v=fixture();v.polygon.points[2]=point;assert.equal(isDocumentResponse(v.source),false)}
 for(const points of [vertices,[{x:0,y:0},{x:0,y:0},{x:0,y:0}],[{x:0,y:0},{x:2,y:2},{x:0,y:2},{x:2,y:0}]])assert.equal(polygonVerticesValid(points),true)
})
test('draft appends ordered immutable clicks with a hard 64 cap; Finish captures doc/revision/service',()=>{
 const {source}=fixture();const doc=source.document;let draft={documentId:doc.id,revision:doc.revision,serviceGeneration:7,vertices:[]};const first=draft
 for(let n=0;n<65;n++)draft=appendPolygonVertex(draft,{x:n,y:-n})
 assert.equal(first.vertices.length,0);assert.equal(draft.vertices.length,64);assert.deepEqual(draft.vertices.at(-1),{x:63,y:-63});assert.equal(appendPolygonVertex(draft,{x:9,y:9}),draft)
 assert.equal(polygonDraftCurrent(doc,'polygon',7,draft),true)
 for(const [d,shape,generation]of [[{...doc,id:'other'},'polygon',7],[{...doc,revision:doc.revision+1},'polygon',7],[doc,'line',7],[doc,'polygon',8]])assert.equal(polygonDraftCurrent(d,shape,generation,draft),false)
 assert.deepEqual(polygonCreationCommands(doc,draft),[{op:'add_polygon',vertices:draft.vertices}]);assert.equal(polygonCreationCommands({...doc,revision:doc.revision+1},draft),null)
 assert.equal(polygonCreationCommands(doc,{...draft,vertices:vertices.slice(0,2)}),null)
 assert.equal(graphicCreationCommands(doc,{...draft,shape:'polygon',points:vertices}),null)
})
test('polygon editing is atomic, captured, no-op aware; legacy endpoints never truncate vertices',()=>{
 const {source,polygon}=fixture();const doc=source.document;const next=vertices.map((p)=>({x:p.x+1,y:p.y-2}));const edit={documentId:doc.id,revision:doc.revision,graphic:polygon,vertices:next}
 assert.deepEqual(polygonVerticesCommands(doc,edit),[{op:'update_polygon_vertices',object:polygon.id,vertices:next}]);assert.equal(polygonVerticesCommands(doc,{...edit,vertices:vertices}),null)
 for(const change of [(d)=>d.revision++,(d)=>d.id='other',(d)=>d.scene.objects.find((o)=>o.id===polygon.id).points.pop(),(d)=>d.scene.objects=d.scene.objects.filter((o)=>o.id!==polygon.id)]){const d=structuredClone(doc);change(d);assert.equal(polygonVerticesCommands(d,edit),null)}
 assert.equal(graphicPointsCommands(doc,{...edit,points:next}),null);assert.equal(selectedGraphic(doc,new Set([polygon.id])),null)
 const moved=translatedGraphicObject(polygon,{x:12,y:-5});assert.deepEqual(moved.points,vertices.map((p)=>({x:p.x+12,y:p.y-5})));assert.equal(moved.line_width_pt,10);assert.deepEqual(polygon.points,vertices)
})
test('v5 polygon copy retains every vertex and width; legacy profiles and malformed cardinality refuse',()=>{
 for(const width of [null,.1,10]){const v=fixture(width);assert.equal(matches(v),true);assert.deepEqual(sceneClipboardGraphicStyles(JSON.stringify(v.payload))[0],{id:'s6',shape:'polygon',lineWidth:width});for(const version of [2,3,4]){const p=structuredClone(v.payload);p.version=version;if(version===2)for(const o of p.objects)delete o.line_width_pt;assert.equal(sceneClipboardGraphicStyles(JSON.stringify(p)),null)}}
 for(const change of [(p)=>p.version=6,(p)=>delete p.objects[1].line_width_pt,(p)=>p.objects[1].line_width_pt=10.01,(p)=>p.objects[1].points=vertices.slice(0,2),(p)=>p.objects[1].points=Array(65).fill({x:0,y:0}),(p)=>p.objects[1].fill='solid',(p)=>p.objects[0].line_width_pt=null]){const v=fixture();change(v.payload);assert.equal(sceneClipboardGraphicStyles(JSON.stringify(v.payload)),null);assert.equal(matches(v),false)}
 const v=fixture();const duplicate=JSON.stringify(v.payload).replace('"line_width_pt":10','"line_width_pt":1,"line_width_pt":10');assert.equal(sceneClipboardGraphicStyles(duplicate),null)
})
test('v5 old shapes parse but copies canonicalize; paste verifies every vertex and retained graph',()=>{
 const v=fixture();v.polygon.shape='ellipse';v.polygon.points=vertices.slice(0,2);v.payload.objects[1].shape='ellipse';v.payload.objects[1].points=vertices.slice(0,2);assert.notEqual(sceneClipboardGraphicStyles(JSON.stringify(v.payload)),null);assert.equal(matches(v),false);v.payload.version=4;assert.equal(matches(v),true)
 const before=sceneCutSource().document;const payload={format:'chemifree.clipboard',version:5,kind:'graphics',settings:before.settings,roots:['s20'],objects:[{kind:'graphic',id:'s20',shape:'polygon',points:vertices,line_width_pt:10}],references:[]};const after=structuredClone(before);after.revision++;after.selection={objects:['s8'],atoms:[],bonds:[]};after.scene.roots.push('s8');after.scene.objects.push({kind:'graphic',id:'s8',parent:null,shape:'polygon',points:vertices.map((p)=>({x:p.x+10,y:p.y+20})),line_width_pt:10});const mapping={objects:[{source:'s20',target:'s8'}]};const check=(d)=>sceneClipboardPasteMapping(JSON.stringify(payload),mapping,{x:10,y:20},before,d,before.settings);assert.deepEqual(check(after),mapping.objects)
 for(const change of [(d)=>d.scene.objects.at(-1).points[2].x++,(d)=>d.scene.objects.at(-1).points.pop(),(d)=>d.scene.objects.at(-1).line_width_pt=null,(d)=>d.atoms[0].charge=1]){const d=structuredClone(after);change(d);assert.equal(check(d),null)}
})

test('marquee and rotation bounds include every ordered vertex, not just the first two',()=>{
 const {source,polygon}=fixture();polygon.points=[{x:0,y:0},{x:10,y:10},{x:100,y:80}];const doc=source.document
 assert.equal(marqueeSelection(doc,null,{x:-1,y:-1},{x:11,y:11},new Set(),false,true).has(polygon.id),false)
 assert.equal(marqueeSelection(doc,null,{x:-1,y:-1},{x:101,y:81},new Set(),false,true).has(polygon.id),true)
 const commands=selectionTransform(doc,new Set([polygon.id]),null,{type:'rotate',degrees:45});assert.deepEqual(commands[0].pivot,{x:50,y:40})
})
