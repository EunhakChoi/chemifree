import assert from 'node:assert/strict'
import {register} from 'node:module'
import {test} from 'node:test'
import {sceneCutSource,sceneCutCopy} from './lib/scene-cut-fixture.mjs'
register('./lib/web-alias-hooks.mjs',import.meta.url)
const {isDocumentResponse}=await import('../apps/web/src/lib/document-contract.ts')
const {appendSplinePoint,splineCreationCommands,splineDraftCurrent,splinePointsCommands,splinePoints}=await import('../apps/web/src/editor/spline.ts')
const {graphicPointsCommands,selectedGraphic,translatedGraphicObject}=await import('../apps/web/src/editor/graphic-points.ts')
const {graphicCreationCommands}=await import('../apps/web/src/editor/graphic-creation.ts')
const {marqueeSelection,selectionTransform}=await import('../apps/web/src/editor/selection-geometry.ts')
const {sceneClipboardCopyMatches,sceneClipboardGraphicStyles,sceneClipboardPasteMapping}=await import('../apps/web/src/lib/scene-clipboard-contract.ts')
const points=[{x:40,y:100},{x:40,y:20},{x:120,y:20},{x:120,y:100}]
function fixture(width=10){const source=sceneCutSource();const spline=source.document.scene.objects.find((o)=>o.id==='s6');spline.shape='spline';spline.points=structuredClone(points);spline.line_width_pt=width;const copy=sceneCutCopy(source);const payload=JSON.parse(copy.payload);payload.version=6;for(const o of payload.objects)if(o.kind==='graphic')o.line_width_pt??=null;return {source,spline,copy,payload}}
function matches(v){return sceneClipboardCopyMatches(JSON.stringify(v.payload),v.copy.copied,v.copy.roots,{objects:['s4'],atoms:[],bonds:[]},v.source.document,v.source.document.settings)}
test('spline snapshots require exactly four bounded defining points; old cardinalities remain shape-specific',()=>{
 for(const count of [0,1,2,3,4,5,64,65]){const v=fixture();v.spline.points=Array.from({length:count},()=>({x:1,y:1}));assert.equal(isDocumentResponse(v.source),count===4);v.spline.shape='line';assert.equal(isDocumentResponse(v.source),count===2);v.spline.shape='polygon';assert.equal(isDocumentResponse(v.source),count>=3&&count<=64)}
 for(const bad of [{x:100001,y:0},{x:Infinity,y:0},{x:0,y:NaN}]){const v=fixture();v.spline.points[2]=bad;assert.equal(isDocumentResponse(v.source),false);assert.equal(splinePoints(v.spline.points),null)}
 for(const value of [points,points.toReversed(),Array(4).fill({x:0,y:0}),[{x:-100000,y:100000},{x:0,y:0},{x:1,y:1},{x:100000,y:-100000}]])assert.deepEqual(splinePoints(value),value)
})
test('four explicit clicks remain immutable and capped until Finish; stale contexts lapse',()=>{
 const {source}=fixture();const doc=source.document;let draft={documentId:doc.id,revision:doc.revision,serviceGeneration:7,points:[]};const first=draft
 for(const p of points)draft=appendSplinePoint(draft,p)
 assert.equal(first.points.length,0);assert.deepEqual(draft.points,points);assert.equal(appendSplinePoint(draft,{x:999,y:999}),draft);assert.equal(splineCreationCommands(doc,{...draft,points:points.slice(0,3)}),null)
 assert.equal(splineDraftCurrent(doc,'spline',7,draft),true)
 for(const [d,shape,g]of [[{...doc,id:'other'},'spline',7],[{...doc,revision:doc.revision+1},'spline',7],[doc,'polygon',7],[doc,'spline',8]])assert.equal(splineDraftCurrent(d,shape,g,draft),false)
 assert.deepEqual(splineCreationCommands(doc,draft),[{op:'add_spline',points}]);assert.equal(splineCreationCommands({...doc,revision:doc.revision+1},draft),null)
 assert.equal(graphicCreationCommands(doc,{...draft,shape:'spline'}),null)
})
test('spline updates all four points atomically and rejects stale/no-op; legacy endpoint commands never truncate it',()=>{
 const {source,spline}=fixture();const doc=source.document;const next=points.map((p)=>({x:p.x+1,y:p.y-2}));const edit={documentId:doc.id,revision:doc.revision,graphic:spline,points:next}
 assert.deepEqual(splinePointsCommands(doc,edit),[{op:'update_spline_points',object:spline.id,points:next}]);assert.equal(splinePointsCommands(doc,{...edit,points}),null)
 for(const change of [(d)=>d.revision++,(d)=>d.id='other',(d)=>d.scene.objects.find((o)=>o.id===spline.id).points[2].x++,(d)=>d.scene.objects=d.scene.objects.filter((o)=>o.id!==spline.id)]){const d=structuredClone(doc);change(d);assert.equal(splinePointsCommands(d,edit),null)}
 assert.equal(graphicPointsCommands(doc,edit),null);assert.equal(selectedGraphic(doc,new Set([spline.id])),null)
 const moved=translatedGraphicObject(spline,{x:12,y:-5});assert.deepEqual(moved.points,points.map((p)=>({x:p.x+12,y:p.y-5})));assert.equal(moved.line_width_pt,10);assert.deepEqual(spline.points,points)
})
test('bounds deliberately include both controls, even outside the actual curve',()=>{
 const {source,spline}=fixture();const doc=source.document
 assert.equal(marqueeSelection(doc,null,{x:30,y:35},{x:130,y:110},new Set(),false,true).has(spline.id),false)
 assert.equal(marqueeSelection(doc,null,{x:30,y:10},{x:130,y:110},new Set(),false,true).has(spline.id),true)
 assert.deepEqual(selectionTransform(doc,new Set([spline.id]),null,{type:'rotate',degrees:45})[0].pivot,{x:80,y:60})
})
test('v6 spline group copy preserves every point, width and reference; legacy profiles forbid spline',()=>{
 for(const width of [null,.1,10]){const v=fixture(width);assert.equal(matches(v),true);assert.deepEqual(sceneClipboardGraphicStyles(JSON.stringify(v.payload))[0],{id:'s6',shape:'spline',lineWidth:width});for(const version of [2,3,4,5]){const p=structuredClone(v.payload);p.version=version;if(version===2)for(const o of p.objects)delete o.line_width_pt;assert.equal(sceneClipboardGraphicStyles(JSON.stringify(p)),null)}}
 for(const change of [(p)=>p.version=7,(p)=>delete p.objects[1].line_width_pt,(p)=>p.objects[1].line_width_pt=10.01,(p)=>p.objects[1].points=points.slice(0,3),(p)=>p.objects[1].points.push({x:0,y:0}),(p)=>p.objects[1].closed=true,(p)=>p.objects[0].line_width_pt=null]){const v=fixture();change(v.payload);assert.equal(sceneClipboardGraphicStyles(JSON.stringify(v.payload)),null);assert.equal(matches(v),false)}
 const v=fixture();assert.equal(sceneClipboardGraphicStyles(JSON.stringify(v.payload).replace('"line_width_pt":10','"line_width_pt":1,"line_width_pt":10')),null)
})
test('v6 old-shape input parses while copies canonicalize to2–5; paste verifies every defining point and graph',()=>{
 for(const [shape,pts,version,width]of [['line',points.slice(0,2),2,null],['line',points.slice(0,2),3,10],['ellipse',points.slice(0,2),4,null],['polygon',points,5,null]]){const v=fixture(width);v.spline.shape=shape;v.spline.points=pts;v.payload.objects[1].shape=shape;v.payload.objects[1].points=pts;assert.notEqual(sceneClipboardGraphicStyles(JSON.stringify(v.payload)),null);assert.equal(matches(v),false);v.payload.version=version;if(version===2)for(const o of v.payload.objects)delete o.line_width_pt;assert.equal(matches(v),true)}
 const outside=sceneCutSource();const unselected=outside.document.scene.objects.find((o)=>o.id==='s7');unselected.shape='spline';unselected.points=points;unselected.line_width_pt=10;const copy=sceneCutCopy(outside);assert.equal(sceneClipboardCopyMatches(copy.payload,copy.copied,copy.roots,{objects:['s4'],atoms:[],bonds:[]},outside.document,outside.document.settings),true,'unselected spline does not change canonical copy profile')
 const before=sceneCutSource().document;const payload={format:'chemifree.clipboard',version:6,kind:'graphics',settings:before.settings,roots:['s20'],objects:[{kind:'graphic',id:'s20',shape:'spline',points,line_width_pt:10}],references:[]};const after=structuredClone(before);after.revision++;after.selection={objects:['s8'],atoms:[],bonds:[]};after.scene.roots.push('s8');after.scene.objects.push({kind:'graphic',id:'s8',parent:null,shape:'spline',points:points.map((p)=>({x:p.x+10,y:p.y+20})),line_width_pt:10});const mapping={objects:[{source:'s20',target:'s8'}]};const check=(d)=>sceneClipboardPasteMapping(JSON.stringify(payload),mapping,{x:10,y:20},before,d,before.settings);assert.deepEqual(check(after),mapping.objects)
 for(const change of [(d)=>d.scene.objects.at(-1).points[2].x++,(d)=>d.scene.objects.at(-1).points.reverse(),(d)=>d.scene.objects.at(-1).line_width_pt=null,(d)=>d.atoms[0].charge=1]){const d=structuredClone(after);change(d);assert.equal(check(d),null)}
})
