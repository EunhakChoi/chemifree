// Real graphic width form, renderer, editor queue, HTTP application and native store.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

const work = mkdtempSync(join(tmpdir(), 'chemifree-spline-'))
let api
let server
let browser
let log = ''
let stage = 'build'
let passed = false
try {
  execFileSync('cargo', ['build', '--locked', '--package', 'chemifree-api'], { stdio: 'inherit' })
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { encoding: 'utf8' }))
  const env = { ...process.env, CHEMIFREE_DATA_DIR: work }
  delete env.CHEMIFREE_MODEL_DATA_ROOT
  delete env.CHEMIFREE_MODEL_TRUSTED_REGISTRY
  api = spawn(resolve(metadata.target_directory, 'debug', `chemifree-api${process.platform === 'win32' ? '.exe' : ''}`), ['--port', '0'], { env, stdio: ['ignore', 'ignore', 'pipe'] })
  const base = await new Promise((ready, reject) => {
    const timer = setTimeout(() => reject(new Error(`API startup timed out: ${log}`)), 15_000)
    api.once('error', (error) => { clearTimeout(timer); reject(error) })
    api.once('exit', (code) => { clearTimeout(timer); reject(new Error(`API exited (${code}): ${log}`)) })
    api.stderr.on('data', (data) => {
      log += data.toString()
      const match = log.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) { clearTimeout(timer); ready(match[1]) }
    })
  })
  const token = await (await fetch(`${base}/api/v1/session`)).json()
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'spline-browser-create', title: 'Spline drawing' }) })
  assert.equal(create.status, 201)
  const id = (await create.json()).document.id
  const headers={'content-type':'application/json',[token.header]:token.token,origin:base}
  async function transact(revision,commands) {
    const response=await fetch(base+'/api/v1/documents/'+id+'/transactions',{method:'POST',headers,body:JSON.stringify({request_id:crypto.randomUUID(),expected_revision:revision,commands})})
    assert.equal(response.status,200,await response.clone().text())
    return (await response.json()).document
  }
  const seeded=await transact(0,[
    {op:'add_atom',element:'C',position:{x:0,y:0}},
    {op:'add_atom',element:'F',position:{x:20,y:0},bond_to:{atom:'$0',order:'single'}},
    {op:'add_atom',element:'Cl',position:{x:0,y:20},bond_to:{atom:'$0',order:'single'}},
    {op:'add_atom',element:'Br',position:{x:-20,y:0},bond_to:{atom:'$0',order:'single'}},
    {op:'add_atom',element:'I',position:{x:0,y:-20},bond_to:{atom:'$0',order:'single'}},
    {op:'set_bond_mark',bond:'b1',mark:{kind:'wedge',narrow:'begin'}},
    {op:'set_atom_configuration',atom:'a1',configuration:{kind:'tetrahedral',neighbors:['a2','a3','a4','a5'],turn:'clockwise'}},
    {op:'create_stereo_group',kind:'absolute',atoms:['a1'],number:null},
    {op:'add_graphic',shape:'line',points:[{x:0,y:100},{x:30,y:100}]},
    {op:'add_graphic',shape:'rectangle',points:[{x:150,y:100},{x:180,y:120}]},
  ])
  const fixture=`
import React,{useEffect,useState} from 'react'
import {createRoot} from 'react-dom/client'
import {GraphicToolControls} from '/src/components/graphic-tool-controls.tsx'
import {GraphicPointsPanel} from '/src/components/graphic-points-panel.tsx'
import {SceneClipboardPanel} from '/src/components/scene-clipboard-panel.tsx'
import {SelectionRotationForm} from '/src/components/selection-rotation-form.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {graphicPointsCommands} from '/src/editor/graphic-points.ts'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor();const [tool,setTool]=useState('graphic');const [shape,setShape]=useState(null);const [generation,setGeneration]=useState(null)
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.splineHarness={editor,setTool,setShape,setGeneration}
 const spline=editor.current?.document.scene.objects.find((o)=>o.kind==='graphic'&&o.shape==='spline')
 return <main>
  <button onClick={()=>setTool('select')}>Use select</button><button onClick={()=>setTool('graphic')}>Use graphic</button>
  <button onClick={()=>editor.setSelection(new Set())}>Clear selection</button>
  <button disabled={!spline||editor.busy} onClick={()=>editor.setSelection(new Set([spline.id]))}>Select spline</button>
  <button disabled={!spline||editor.busy} onClick={()=>void editor.apply([{op:'group',objects:['s2',spline.id]},{op:'add_reference',source:spline.id,target:{kind:'object',id:'s2'}}])}>Group spline and line</button>
  <button disabled={!spline||editor.busy} onClick={()=>{const group=editor.current.document.scene.objects.find((o)=>o.kind==='group'&&o.children.includes(spline.id));if(group)editor.setSelection(new Set([group.id]))}}>Select spline group</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'update_settings',line_width_pt:2}])}>Change document</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo spline</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo spline</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save spline</button>
  <output className="block break-all" aria-label="Spline snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Spline error">{editor.error?.message}</output>
  <output aria-label="Spline busy">{String(editor.busy)}</output>
  <GraphicToolControls shape={shape} onShapeChange={setShape} disabled={editor.busy}/>
  {editor.current&&<>
   <GraphicPointsPanel document={editor.current.document} editor={editor}/>
   <SelectionRotationForm key={editor.current.document.id} document={editor.current.document} editor={editor} activePlacement="s1"/>
   <SceneClipboardPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool={tool} graphicShape={shape} serviceGeneration={generation??editor.serviceGeneration} onSplineCreate={(creation)=>void editor.createSpline(creation)} onGraphicCreate={(creation)=>void editor.createGraphic(creation)} onGraphicPoints={(edit)=>void editor.apply((latest)=>graphicPointsCommands(latest,edit))} element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'spline-fixture',configureServer(vite){vite.middlewares.use('/spline-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/spline-check','<!doctype html><html><head><title>Spline</title></head><body><div id="root"></div><script type="module" src="/spline-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/spline-harness.tsx')return path},load(path){if(path==='/spline-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1400,height:1000}})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  await page.addInitScript(()=>{window.splineClipboard={text:'',writes:[],reads:[]};Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText(text){window.splineClipboard.writes.push({text,active:navigator.userActivation.isActive});window.splineClipboard.text=text;return Promise.resolve()},readText(){window.splineClipboard.reads.push(navigator.userActivation.isActive);return Promise.resolve(window.splineClipboard.text)}}})})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Spline snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Spline busy"]').textContent==='false')
  const canvas=page.locator('svg[data-structure-canvas]')
  const graphic=(id)=>page.locator('[data-scene-object="'+id+'"]')
  async function revision(value){await page.waitForFunction((value)=>JSON.parse(document.querySelector('[aria-label="Spline snapshot"]').textContent).document.revision===value,value);await idle()}
  async function screenPoint(point){await canvas.scrollIntoViewIfNeeded();return canvas.evaluate((svg,point)=>{const at=new DOMPoint(point.x,point.y).matrixTransform(svg.getScreenCTM());return{x:at.x,y:at.y}},point)}
  async function click(point){const at=await screenPoint(point);const box=await canvas.boundingBox();assert.equal(at.x>=box.x&&at.x<=box.x+box.width&&at.y>=box.y&&at.y<=box.y+box.height,true,'test click must be inside the visible canvas: '+JSON.stringify({point,at,box}));const hit=await canvas.evaluate((svg,at)=>{const node=document.elementFromPoint(at.x,at.y);return{inside:svg.contains(node),tag:node?.tagName,object:node?.closest('[data-scene-object]')?.getAttribute('data-scene-object')}},at);assert.equal(hit.inside,true,'test pointer target must belong to the canvas: '+JSON.stringify({point,at,box,hit}));await page.mouse.click(at.x,at.y)}
  async function begin(from,to){const a=await screenPoint(from);const b=await screenPoint(to);await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:4})}
  function graphSame(document){for(const key of ['atoms','bonds','stereo','molecule'])assert.deepEqual(document[key],seeded[key]);assert.deepEqual(document.scene.instances,seeded.scene.instances)}
  const count=async(n)=>page.waitForFunction((n)=>Number(document.querySelector('[aria-label="Spline draft points"]').textContent)===n,n)
  const shape=async(name)=>page.getByRole('group',{name:'Graphic shape',exact:true}).getByRole('button',{name,exact:true}).click()
  const stroke=(id)=>graphic(id).locator('path.stroke-foreground')
  const pointsOf=async(id)=>(await snapshot()).document.scene.objects.find((o)=>o.id===id).points
  async function undo(){const d=(await snapshot()).document;await button('Undo spline').click();await revision(d.revision+1)}
  async function redo(){const d=(await snapshot()).document;await button('Redo spline').click();await revision(d.revision+1)}
  const points=[{x:40,y:100},{x:40,y:20},{x:120,y:20},{x:120,y:100}]
  stage='four explicit ordered points remain a local draft until Finish'
  await page.goto('http://127.0.0.1:5173/spline-check');await page.waitForFunction(()=>!!window.splineHarness.editor.current)
  await shape('Spline');await button('Reset zoom (100%)').click()
  const beforeDraft=(await snapshot()).document;const beforeWrites=writes.length
  for(const [i,p]of points.entries()){await click(p);await count(i+1);assert.equal(await button('Finish spline').isDisabled(),i<3)}
  await click({x:200,y:80});await count(4);await button('Remove last spline point').click();await count(3);assert.equal(await button('Finish spline').isDisabled(),true);await click(points[3]);await count(4)
  const draftStroke=page.locator('[data-spline-draft] path.stroke-foreground');assert.equal(await draftStroke.getAttribute('d'),'M 40 100 C 40 20 120 20 120 100');assert.equal(await draftStroke.getAttribute('fill'),'none');assert.deepEqual((await snapshot()).document,beforeDraft);assert.equal(writes.length,beforeWrites)
  await button('Cancel spline').click();await count(0);assert.equal(writes.length,beforeWrites)
  await begin(points[0],points[1]);await page.mouse.up();await count(0)
  for(const p of points)await click(p)
  await button('Finish spline').click();await revision(beforeDraft.revision+1);await count(0)
  const spline=(await snapshot()).document.scene.objects.find((o)=>o.kind==='graphic'&&o.shape==='spline');const sid=spline.id
  assert.deepEqual(writes.at(-1).commands,[{op:'add_spline',points}]);assert.equal(spline.parent,null);assert.equal(spline.line_width_pt,null);assert.deepEqual(spline.points,points);graphSame((await snapshot()).document)
  await undo();assert.equal((await snapshot()).document.scene.objects.some((o)=>o.id===sid),false);await redo();assert.deepEqual(await pointsOf(sid),points)
  stage='shape/tool/revision/service/document/escape changes discard drafts without creation'
  for(const cancel of [async()=>shape('Line'),async()=>button('Use select').click(),async()=>page.keyboard.press('Escape'),async()=>page.evaluate(()=>window.splineHarness.setGeneration(window.splineHarness.editor.serviceGeneration+1))]){
    await page.evaluate(()=>window.splineHarness.setGeneration(null));await button('Use graphic').click();await shape('Spline');await click(points[0]);await count(1);const n=writes.length;await cancel();assert.equal(await page.locator('[data-spline-draft]').count(),0);assert.equal(writes.length,n)
  }
  await page.evaluate(()=>window.splineHarness.setGeneration(null));await button('Use graphic').click();await shape('Spline');await click(points[0]);await count(1);let before=(await snapshot()).document;await button('Change document').click();await revision(before.revision+1);await count(0)
  const otherResponse=await fetch(base+'/api/v1/documents',{method:'POST',headers,body:JSON.stringify({request_id:crypto.randomUUID(),title:'Other spline document'})});assert.equal(otherResponse.status,201);const otherId=(await otherResponse.json()).document.id;await click(points[0]);await count(1);const switchWrites=writes.length;await page.evaluate((id)=>void window.splineHarness.editor.open(id),otherId);await page.waitForFunction((id)=>window.splineHarness.editor.current?.document.id===id,otherId);await count(0);assert.equal(writes.length,switchWrites);await page.evaluate((id)=>void window.splineHarness.editor.open(id),id);await page.waitForFunction((id)=>window.splineHarness.editor.current?.document.id===id,id);await idle()
  stage='queued creation rejects stale revisions and freezes all four submitted points'
  before=(await snapshot()).document;const queueWrites=writes.length
  await page.evaluate((points)=>{const editor=window.splineHarness.editor;const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,points};void editor.apply([{op:'rename',title:'Earlier queued spline edit'}]);void editor.createSpline(creation)},points)
  await revision(before.revision+1);assert.equal(writes.length,queueWrites+1)
  before=(await snapshot()).document;await page.evaluate((points)=>{const editor=window.splineHarness.editor;const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,points};void editor.createSpline(creation);creation.points[2].x=999;creation.points.push({x:0,y:0})},points);await revision(before.revision+1);assert.deepEqual(writes.at(-1).commands,[{op:'add_spline',points}]);await undo()
  stage='pan takes priority over spline capture on existing chemical content'
  const panWrites=writes.length;await canvas.focus();const panAt=await screenPoint({x:0,y:0});const oldView=await canvas.getAttribute('viewBox');await page.keyboard.down('Space');await page.mouse.move(panAt.x,panAt.y);await page.mouse.down();await page.mouse.move(panAt.x+30,panAt.y+15,{steps:3});await page.mouse.up();await page.keyboard.up('Space');assert.notEqual(await canvas.getAttribute('viewBox'),oldView);await count(0);assert.equal(writes.length,panWrites)
  stage='actual cubic stroke hit follows the curve, not controls, closing chord or filled interior'
  await button('Use select').click();await button('Select spline').click();await button('Reset zoom (100%)').click();await button('Zoom in').click()
  assert.equal(await page.locator('[data-endpoint-object="'+sid+'"]').count(),0);assert.equal(await stroke(sid).getAttribute('d'),'M 40 100 C 40 20 120 20 120 100');assert.equal(await stroke(sid).getAttribute('fill'),'none');assert.equal(await graphic(sid).locator('polygon,polyline,line').count(),0,'no arrowhead or control polygon is saved/rendered');assert.equal(Number(await stroke(sid).getAttribute('stroke-width')),2)
  const nativeHit=async(point)=>{const at=await screenPoint(point);return page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.closest('[data-scene-object]')?.getAttribute('data-scene-object'),at)}
  for(const p of [{x:80,y:20},{x:80,y:100},{x:80,y:70}])assert.notEqual(await nativeHit(p),sid,'no control polygon, closing edge or fill is painted')
  assert.equal(await nativeHit({x:80,y:40}),sid,'t=.5 of the cubic receives a native SVG stroke hit')
  await button('Clear selection').click();await click({x:80,y:40});await page.waitForFunction((id)=>window.splineHarness.editor.selection.has(id),sid)
  stage='marquee and transform deliberately use the complete four-point AABB, not curve extrema'
  await button('Reset zoom (100%)').click();await button('Clear selection').click();await begin({x:30,y:35},{x:130,y:110});await page.mouse.up();await page.waitForFunction((id)=>!window.splineHarness.editor.selection.has(id),sid);await begin({x:30,y:10},{x:130,y:110});await page.mouse.up();await page.waitForFunction((id)=>window.splineHarness.editor.selection.has(id),sid)
  await page.getByRole('textbox',{name:'Rotation angle (degrees)',exact:true}).fill('45');before=(await snapshot()).document;await button('Rotate').click();await revision(before.revision+1);assert.deepEqual(writes.at(-1).commands[0].pivot,{x:80,y:60});assert.equal((await pointsOf(sid)).length,4);graphSame((await snapshot()).document);await undo()
  stage='rigid movement previews every control/end point and commits once'
  before=(await snapshot()).document;const moveWrites=writes.length;await begin({x:80,y:40},{x:90,y:45});assert.equal(await stroke(sid).getAttribute('d'),'M 50 105 C 50 25 130 25 130 105');assert.deepEqual((await snapshot()).document,before);assert.equal(writes.length,moveWrites);await page.mouse.up();await revision(before.revision+1);assert.deepEqual(await pointsOf(sid),points.map((p)=>({x:p.x+10,y:p.y+5})));graphSame((await snapshot()).document);await undo()
  stage='numeric editing preserves explicit point roles and refuses invalid/IME drafts'
  await button('Select spline').click();const control=page.getByRole('textbox',{name:'C2 x (pt)',exact:true});await control.fill('100001');assert.equal(await button('Apply spline points').isDisabled(),true);await control.fill('125');await control.dispatchEvent('compositionstart');assert.equal(await button('Apply spline points').isDisabled(),true);await control.dispatchEvent('compositionend');before=(await snapshot()).document;await button('Apply spline points').click();await revision(before.revision+1);const edited=points.map((p,i)=>i===2?{...p,x:125}:p);assert.deepEqual(await pointsOf(sid),edited);assert.deepEqual(writes.at(-1).commands,[{op:'update_spline_points',object:sid,points:edited}]);await undo()
  stage='coincident/reversed definitions remain literal and retain numeric fallback'
  for(const definition of [Array(4).fill({x:80,y:60}),points.toReversed()]){before=(await snapshot()).document;for(const [index,p]of definition.entries())for(const axis of ['x','y'])await page.getByRole('textbox',{name:['P0','C1','C2','P3'][index]+' '+axis+' (pt)',exact:true}).fill(String(p[axis]));await button('Apply spline points').click();await revision(before.revision+1);assert.deepEqual(await pointsOf(sid),definition);assert.equal(await page.locator('[data-endpoint-object="'+sid+'"]').count(),0);await undo()}
  stage='styled group/reference v6 Copy/Paste/Cut preserves every defining point and graph'
  before=(await snapshot()).document;await page.getByRole('textbox',{name:'Graphic line width (pt)',exact:true}).fill('10');await button('Apply graphic line width').click();await revision(before.revision+1);assert.equal(Number(await stroke(sid).getAttribute('stroke-width')),10)
  before=(await snapshot()).document;await button('Group spline and line').click();await revision(before.revision+1);await button('Select spline group').click();const copied=(await snapshot()).document;const group=copied.scene.objects.find((o)=>o.kind==='group'&&o.children.includes(sid))
  await button('Copy graphics…').click();await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));const text=await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue();const payload=JSON.parse(text);assert.equal(payload.version,6);assert.deepEqual(payload.objects.find((o)=>o.id===sid).points,points);assert.equal(payload.objects.find((o)=>o.id===sid).line_width_pt,10);assert.match(await page.getByRole('list',{name:'Clipboard graphic line widths'}).textContent(),/spline .*10 pt/);await button('Copy to clipboard').click();await page.getByRole('status').filter({hasText:'Native graphics text copied to clipboard.'}).waitFor();await button('Close').click()
  await button('Paste graphics…').click();await button('Read clipboard').click();await page.waitForFunction((text)=>document.querySelector('textarea')?.value===text,text);await page.getByRole('textbox',{name:'Offset x (pt)',exact:true}).fill('160');await page.getByRole('textbox',{name:'Offset y (pt)',exact:true}).fill('0');const pasteResponse=page.waitForResponse((r)=>r.url().endsWith('/operations')&&r.request().postDataJSON()?.operation==='document.clipboard_scene_paste');await button('Paste graphics').click();const pasted=await(await pasteResponse).json();assert.equal(pasted.outcome,'ok');await revision(copied.revision+1);const mapping=pasted.result.mapping.objects;const targetOf=(id)=>mapping.find((item)=>item.source===id).target;const afterPaste=(await snapshot()).document;assert.deepEqual(await pointsOf(targetOf(sid)),points.map((p)=>({x:p.x+160,y:p.y})));assert.equal(afterPaste.scene.objects.find((o)=>o.id===targetOf(sid)).line_width_pt,10);assert.deepEqual(afterPaste.scene.objects.find((o)=>o.id===targetOf(group.id)).children,group.children.map(targetOf));assert.equal(afterPaste.scene.references.some((r)=>r.source===targetOf(sid)&&r.target.id===targetOf('s2')),true);for(const o of copied.scene.objects)assert.deepEqual(afterPaste.scene.objects.find((p)=>p.id===o.id),o);graphSame(afterPaste);await undo()
  await button('Select spline group').click();before=(await snapshot()).document;await button('Cut graphics…').click();await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));assert.equal(JSON.parse(await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue()).version,6);await button('Cut to clipboard').click();await revision(before.revision+1);assert.equal((await snapshot()).document.scene.objects.some((o)=>o.id===sid||o.id===group.id||o.id==='s2'),false);graphSame((await snapshot()).document);await undo();assert.deepEqual((await snapshot()).document.scene,copied.scene)
  stage='native30 save/restart/reopen retains spline, group, reference and complete history; unknown creation never replays'
  await button('Save spline').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Spline snapshot"]').textContent).session.unsaved_changes===false);const saved=(await snapshot()).document;assert.equal(JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8')).format_version,30)
  await button('Use graphic').click();await shape('Spline');for(const p of points)await click(p);await count(4)
  const exited=once(api,'exit');api.kill('SIGTERM');await exited;api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']});await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  const restartWrites=writes.length;await button('Finish spline').click();await page.getByLabel('Spline error',{exact:true}).filter({hasText:/./}).waitFor();await idle();assert.equal(writes.length,restartWrites+1,'expired service creation is not replayed');assert.deepEqual((await snapshot()).document,saved)
  await page.reload();await page.waitForFunction(()=>!!window.splineHarness.editor.current);assert.deepEqual((await snapshot()).document.scene,saved.scene);assert.deepEqual(await pointsOf(sid),points)
  stage='real workspace explicit spline over chemical content leaves the graph unchanged'
  await page.goto('http://127.0.0.1:5173/');await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click();await button('Graphic').click();await shape('Spline');await button('Fit drawing').click();const workspacePoints=[{x:0,y:0},{x:0,y:10},{x:20,y:10},{x:20,y:0}];const workspaceWrites=writes.length;for(const [index,p]of workspacePoints.entries()){await click(p);await count(index+1);assert.equal(await canvas.evaluate((svg)=>document.activeElement===svg),true,'graphic point capture retains keyboard focus')};await count(4);assert.equal(writes.length,workspaceWrites);const createdResponse=page.waitForResponse((r)=>r.url().endsWith('/transactions'));await button('Finish spline').click();const created=await createdResponse;assert.equal(created.status(),200);assert.deepEqual(created.request().postDataJSON().commands,[{op:'add_spline',points:workspacePoints}]);graphSame((await created.json()).document)
  await page.setViewportSize({width:320,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'six explicit choices and spline controls fit a compact viewport');assert.deepEqual(errors,[]);passed=true
  console.log('Spline explicit P0/C1/C2/P3, open cubic stroke/hit, defining-point bounds, numeric edits/move/rotation, grouped styled v6 clipboard and native30 restart passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Spline browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
