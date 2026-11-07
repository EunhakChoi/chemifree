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

const work = mkdtempSync(join(tmpdir(), 'chemifree-polygon-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'polygon-browser-create', title: 'Polygon drawing' }) })
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
 window.polygonHarness={editor,setTool,setShape,setGeneration}
 const polygon=editor.current?.document.scene.objects.find((o)=>o.kind==='graphic'&&o.shape==='polygon')
 return <main>
  <button onClick={()=>setTool('select')}>Use select</button><button onClick={()=>setTool('graphic')}>Use graphic</button>
  <button onClick={()=>editor.setSelection(new Set())}>Clear selection</button>
  <button disabled={!polygon||editor.busy} onClick={()=>editor.setSelection(new Set([polygon.id]))}>Select polygon</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'update_settings',line_width_pt:2}])}>Change document</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo polygon</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo polygon</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save polygon</button>
  <output className="block break-all" aria-label="Polygon snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Polygon error">{editor.error?.message}</output>
  <output aria-label="Polygon busy">{String(editor.busy)}</output>
  <GraphicToolControls shape={shape} onShapeChange={setShape} disabled={editor.busy}/>
  {editor.current&&<>
   <GraphicPointsPanel document={editor.current.document} editor={editor}/>
   <SelectionRotationForm key={editor.current.document.id} document={editor.current.document} editor={editor} activePlacement="s1"/>
   <SceneClipboardPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool={tool} graphicShape={shape} serviceGeneration={generation??editor.serviceGeneration} onPolygonCreate={(creation)=>void editor.createPolygon(creation)} onGraphicCreate={(creation)=>void editor.createGraphic(creation)} onGraphicPoints={(edit)=>void editor.apply((latest)=>graphicPointsCommands(latest,edit))} element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'polygon-fixture',configureServer(vite){vite.middlewares.use('/polygon-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/polygon-check','<!doctype html><html><head><title>Polygon</title></head><body><div id="root"></div><script type="module" src="/polygon-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/polygon-harness.tsx')return path},load(path){if(path==='/polygon-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1400,height:1000}})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  await page.addInitScript(()=>{window.polygonClipboard={text:'',writes:[],reads:[]};Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText(text){window.polygonClipboard.writes.push({text,active:navigator.userActivation.isActive});window.polygonClipboard.text=text;return Promise.resolve()},readText(){window.polygonClipboard.reads.push(navigator.userActivation.isActive);return Promise.resolve(window.polygonClipboard.text)}}})})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Polygon snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Polygon busy"]').textContent==='false')
  const canvas=page.locator('svg[data-structure-canvas]')
  const graphic=(id)=>page.locator('[data-scene-object="'+id+'"]')
  async function revision(value){await page.waitForFunction((value)=>JSON.parse(document.querySelector('[aria-label="Polygon snapshot"]').textContent).document.revision===value,value);await idle()}
  async function screenPoint(point){await canvas.scrollIntoViewIfNeeded();return canvas.evaluate((svg,point)=>{const at=new DOMPoint(point.x,point.y).matrixTransform(svg.getScreenCTM());return{x:at.x,y:at.y}},point)}
  async function click(point){const at=await screenPoint(point);const box=await canvas.boundingBox();assert.equal(at.x>=box.x&&at.x<=box.x+box.width&&at.y>=box.y&&at.y<=box.y+box.height,true,'test click must be inside the visible canvas: '+JSON.stringify({point,at,box}));await page.mouse.click(at.x,at.y)}
  async function begin(from,to){const a=await screenPoint(from);const b=await screenPoint(to);await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:4})}
  function graphSame(document){for(const key of ['atoms','bonds','stereo','molecule'])assert.deepEqual(document[key],seeded[key]);assert.deepEqual(document.scene.instances,seeded.scene.instances)}
  const count=async(n)=>page.waitForFunction((n)=>Number(document.querySelector('[aria-label="Polygon draft vertices"]').textContent)===n,n)
  const shape=async(name)=>page.getByRole('group',{name:'Graphic shape',exact:true}).getByRole('button',{name,exact:true}).click()
  const outline=(id)=>graphic(id).locator('polygon.stroke-foreground')
  const pointsOf=async(id)=>(await snapshot()).document.scene.objects.find((o)=>o.id===id).points
  async function undo(){const d=(await snapshot()).document;await button('Undo polygon').click();await revision(d.revision+1)}
  async function redo(){const d=(await snapshot()).document;await button('Redo polygon').click();await revision(d.revision+1)}
  stage='click drafts have no writes, explicit finish and remove/cancel'
  await page.goto('http://127.0.0.1:5173/polygon-check');await page.waitForFunction(()=>!!window.polygonHarness.editor.current)
  await shape('Polygon');await button('Reset zoom (100%)').click()
  const vertices=[{x:40,y:40},{x:120,y:40},{x:120,y:100},{x:40,y:100}]
  const beforeDraft=(await snapshot()).document;const beforeWrites=writes.length
  await click(vertices[0]);await count(1);assert.equal(await button('Finish polygon').isDisabled(),true)
  await click(vertices[1]);await count(2);await button('Remove last polygon vertex').click();await count(1)
  await button('Cancel polygon').click();await count(0);assert.deepEqual((await snapshot()).document,beforeDraft);assert.equal(writes.length,beforeWrites)
  await begin(vertices[0],vertices[1]);await page.mouse.up();await count(0)
  for(const p of vertices)await click(p)
  await count(4);assert.equal(await page.locator('[data-polygon-draft] polygon.stroke-foreground').getAttribute('points'),'40,40 120,40 120,100 40,100');assert.equal(await page.locator('[data-polygon-draft] polygon.stroke-foreground').getAttribute('fill'),'none')
  assert.deepEqual((await snapshot()).document,beforeDraft);assert.equal(writes.length,beforeWrites)
  await button('Finish polygon').click();await revision(beforeDraft.revision+1);await count(0)
  const polygon=(await snapshot()).document.scene.objects.find((o)=>o.kind==='graphic'&&o.shape==='polygon');const pid=polygon.id
  assert.deepEqual(writes.at(-1).commands,[{op:'add_polygon',vertices}]);assert.deepEqual(polygon.points,vertices);assert.equal(polygon.parent,null);assert.equal(polygon.line_width_pt,null);graphSame((await snapshot()).document)
  await undo();assert.equal((await snapshot()).document.scene.objects.some((o)=>o.id===pid),false);await redo();assert.deepEqual(await pointsOf(pid),vertices)
  stage='draft cancels on shape/tool/revision changes and escape without writes'
  for(const cancel of [async()=>shape('Line'),async()=>button('Use select').click(),async()=>page.keyboard.press('Escape')]){
   await button('Use graphic').click();await shape('Polygon');await click({x:180,y:60});await count(1);const n=writes.length;await cancel();assert.equal(await page.locator('[data-polygon-draft]').count(),0);assert.equal(writes.length,n)
  }
  await button('Use graphic').click();await shape('Polygon');await click({x:180,y:60});await count(1);await page.evaluate(()=>window.polygonHarness.setGeneration(window.polygonHarness.editor.serviceGeneration+1));await count(0);await page.evaluate(()=>window.polygonHarness.setGeneration(null));await click({x:180,y:60});await count(1);let before=(await snapshot()).document;await button('Change document').click();await revision(before.revision+1);await count(0)
  stage='switching document discards the local polygon draft'
  const otherResponse=await fetch(base+'/api/v1/documents',{method:'POST',headers,body:JSON.stringify({request_id:crypto.randomUUID(),title:'Other polygon document'})});assert.equal(otherResponse.status,201);const otherId=(await otherResponse.json()).document.id
  await click({x:180,y:60});await count(1);const switchWrites=writes.length;await page.evaluate((id)=>void window.polygonHarness.editor.open(id),otherId);await page.waitForFunction((id)=>window.polygonHarness.editor.current?.document.id===id,otherId);await count(0);assert.equal(writes.length,switchWrites);await page.evaluate((id)=>void window.polygonHarness.editor.open(id),id);await page.waitForFunction((id)=>window.polygonHarness.editor.current?.document.id===id,id);await idle()
  stage='captured queued Finish lapses after an earlier revision and freezes every vertex'
  before=(await snapshot()).document;let queueWrites=writes.length
  await page.evaluate(()=>{const editor=window.polygonHarness.editor;const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,vertices:[{x:200,y:20},{x:220,y:20},{x:220,y:40}]};void editor.apply([{op:'rename',title:'Earlier queued polygon edit'}]);void editor.createPolygon(creation)})
  await revision(before.revision+1);assert.equal(writes.length,queueWrites+1);assert.deepEqual(await pointsOf(pid),vertices)
  before=(await snapshot()).document
  await page.evaluate(()=>{const editor=window.polygonHarness.editor;const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,vertices:[{x:200,y:20},{x:220,y:20},{x:220,y:40}]};void editor.createPolygon(creation);creation.vertices[2].x=999;creation.vertices.push({x:1,y:2})})
  await revision(before.revision+1);assert.deepEqual(writes.at(-1).commands,[{op:'add_polygon',vertices:[{x:200,y:20},{x:220,y:20},{x:220,y:40}]}]);await undo()
  stage='polygon tool panning wins over existing graph content without adding a vertex'
  const panWrites=writes.length;await canvas.focus();const panAt=await screenPoint({x:0,y:0});const oldView=await canvas.getAttribute('viewBox');await page.keyboard.down('Space');await page.mouse.move(panAt.x,panAt.y);await page.mouse.down();await page.mouse.move(panAt.x+30,panAt.y+15,{steps:3});await page.mouse.up();await page.keyboard.up('Space');assert.notEqual(await canvas.getAttribute('viewBox'),oldView);await count(0);assert.equal(writes.length,panWrites)
  stage='native closed stroke hits closing edge but not interior; endpoint handles never truncate polygon'
  await button('Use select').click();await button('Zoom in').click();await button('Select polygon').click();assert.equal(await page.locator('[data-endpoint-object="'+pid+'"]').count(),0);assert.equal(await outline(pid).getAttribute('fill'),'none');assert.equal(Number(await outline(pid).getAttribute('stroke-width')),2)
  await button('Clear selection').click();await page.waitForFunction(()=>window.polygonHarness.editor.selection.size===0)
  const center=await screenPoint({x:80,y:70});assert.notEqual(await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.closest('[data-scene-object]')?.getAttribute('data-scene-object'),center),pid)
  const edge=await screenPoint({x:40,y:70});assert.equal(await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.closest('[data-scene-object]')?.getAttribute('data-scene-object'),edge),pid)
  await page.mouse.click(edge.x,edge.y);await page.waitForFunction((id)=>window.polygonHarness.editor.selection.has(id),pid)
  stage='marquee requires all vertices and includes the complete polygon bounds'
  await button('Reset zoom (100%)').click();await button('Clear selection').click();await begin({x:30,y:30},{x:130,y:45});await page.mouse.up();await page.waitForFunction((id)=>!window.polygonHarness.editor.selection.has(id),pid);await begin({x:30,y:30},{x:130,y:110});await page.mouse.up();await page.waitForFunction((id)=>window.polygonHarness.editor.selection.has(id),pid)
  stage='all vertices move in preview and commit; width and chemistry remain unchanged'
  before=(await snapshot()).document;const moveWrites=writes.length;await begin({x:40,y:70},{x:50,y:75});assert.equal(await outline(pid).getAttribute('points'),'50,45 130,45 130,105 50,105');assert.deepEqual((await snapshot()).document,before);assert.equal(writes.length,moveWrites);await page.mouse.up();await revision(before.revision+1);assert.deepEqual(await pointsOf(pid),vertices.map((p)=>({x:p.x+10,y:p.y+5})));graphSame((await snapshot()).document);await undo()
  stage='vertex list rejects invalid/IME and applies all rows as one update'
  await button('Select polygon').click();const vx=page.getByRole('textbox',{name:'Vertex 3 x (pt)',exact:true});await vx.fill('100001');assert.equal(await button('Apply polygon vertices').isDisabled(),true);await vx.fill('125');await vx.dispatchEvent('compositionstart');assert.equal(await button('Apply polygon vertices').isDisabled(),true);await vx.dispatchEvent('compositionend');before=(await snapshot()).document;await button('Apply polygon vertices').click();await revision(before.revision+1);const edited=vertices.map((p,i)=>i===2?{...p,x:125}:p);assert.deepEqual(await pointsOf(pid),edited);assert.deepEqual(writes.at(-1).commands,[{op:'update_polygon_vertices',object:pid,vertices:edited}]);await undo()
  await button('Add vertex row').click();assert.equal(await button('Apply polygon vertices').isDisabled(),true);await page.getByRole('textbox',{name:'Vertex 5 x (pt)',exact:true}).fill('80');await page.getByRole('textbox',{name:'Vertex 5 y (pt)',exact:true}).fill('60');before=(await snapshot()).document;await button('Apply polygon vertices').click();await revision(before.revision+1);assert.deepEqual(await pointsOf(pid),[...vertices,{x:80,y:60}]);await undo();await button('Remove last vertex row').click();assert.equal(await button('Remove last vertex row').isDisabled(),true);before=(await snapshot()).document;await button('Apply polygon vertices').click();await revision(before.revision+1);assert.deepEqual(await pointsOf(pid),vertices.slice(0,3));await undo()
  stage='arbitrary rotation uses the complete vertex bounds and preserves styles/graph'
  await page.getByRole('textbox',{name:'Rotation angle (degrees)',exact:true}).fill('45');before=(await snapshot()).document;await button('Rotate').click();await revision(before.revision+1);assert.equal((await pointsOf(pid)).length,4);assert.deepEqual(writes.at(-1).commands[0].pivot,{x:80,y:70});graphSame((await snapshot()).document);await undo()
  stage='64 repeated vertices allowed; 65th click is ignored and Finish writes once'
  await button('Use graphic').click();await shape('Polygon');const capBefore=(await snapshot()).document;const capWrites=writes.length;for(let i=0;i<65;i++)await click({x:200,y:80});await count(64);assert.equal(writes.length,capWrites);await button('Finish polygon').click();await revision(capBefore.revision+1);assert.equal(writes.at(-1).commands[0].vertices.length,64);await undo()
  stage='styled v5 copy/paste/cut preserves every vertex and inherited/exact widths'
  await button('Use select').click();await button('Select polygon').click();before=(await snapshot()).document;await page.getByRole('textbox',{name:'Graphic line width (pt)',exact:true}).fill('10');await button('Apply graphic line width').click();await revision(before.revision+1);assert.equal(Number(await outline(pid).getAttribute('stroke-width')),10)
  const copied=(await snapshot()).document;await button('Copy graphics…').click();await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));const text=await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue();const payload=JSON.parse(text);assert.equal(payload.version,5);assert.deepEqual(payload.objects[0].points,vertices);assert.equal(payload.objects[0].line_width_pt,10);assert.match(await page.getByRole('list',{name:'Clipboard graphic line widths'}).textContent(),/polygon .*10 pt/);await button('Copy to clipboard').click();await page.getByRole('status').filter({hasText:'Native graphics text copied to clipboard.'}).waitFor();await button('Close').click()
  await button('Paste graphics…').click();await button('Read clipboard').click();await page.waitForFunction((text)=>document.querySelector('textarea')?.value===text,text);await page.getByRole('textbox',{name:'Offset x (pt)',exact:true}).fill('160');await page.getByRole('textbox',{name:'Offset y (pt)',exact:true}).fill('0');const pasteResponse=page.waitForResponse((r)=>r.url().endsWith('/operations')&&r.request().postDataJSON()?.operation==='document.clipboard_scene_paste');await button('Paste graphics').click();const pasted=await(await pasteResponse).json();assert.equal(pasted.outcome,'ok');await revision(copied.revision+1);const pastedId=pasted.result.mapping.objects.find((item)=>item.source===pid).target;assert.deepEqual(await pointsOf(pastedId),vertices.map((p)=>({x:p.x+160,y:p.y})));assert.equal((await snapshot()).document.scene.objects.find((o)=>o.id===pastedId).line_width_pt,10);graphSame((await snapshot()).document);await undo()
  await button('Select polygon').click();before=(await snapshot()).document;await button('Cut graphics…').click();await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));assert.equal(JSON.parse(await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue()).version,5);await button('Cut to clipboard').click();await revision(before.revision+1);assert.equal((await snapshot()).document.scene.objects.some((o)=>o.id===pid),false);graphSame((await snapshot()).document);await undo()
  stage='native29 save/restart/reopen retains complete polygon and history'
  await button('Save polygon').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Polygon snapshot"]').textContent).session.unsaved_changes===false);const saved=(await snapshot()).document;assert.equal(JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8')).format_version,29)
  await button('Use graphic').click();await shape('Polygon');for(const p of [{x:250,y:50},{x:270,y:50},{x:270,y:70}])await click(p);await count(3)
  const exited=once(api,'exit');api.kill('SIGTERM');await exited;api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']});await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  const restartWrites=writes.length;await button('Finish polygon').click();await page.getByLabel('Polygon error',{exact:true}).filter({hasText:/./}).waitFor();await idle();assert.equal(writes.length,restartWrites+1,'expired service request is not replayed');assert.deepEqual((await snapshot()).document,saved)
  await page.reload();await page.waitForFunction(()=>!!window.polygonHarness.editor.current);assert.deepEqual((await snapshot()).document.scene,saved.scene);assert.deepEqual(await pointsOf(pid),vertices)
  stage='real workspace exposes explicit polygon Finish with no chemistry changes'
  await page.goto('http://127.0.0.1:5173/');await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click();await button('Graphic').click();await shape('Polygon');await button('Fit drawing').click();const workspaceWrites=writes.length;for(const p of [{x:0,y:0},{x:20,y:0},{x:0,y:20}])await click(p);await count(3);assert.equal(writes.length,workspaceWrites);const createdResponse=page.waitForResponse((r)=>r.url().endsWith('/transactions'));await button('Finish polygon').click();const created=await createdResponse;assert.equal(created.status(),200);assert.deepEqual(created.request().postDataJSON().commands,[{op:'add_polygon',vertices:[{x:0,y:0},{x:20,y:0},{x:0,y:20}]}]);graphSame((await created.json()).document)
  await page.setViewportSize({width:320,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'six explicit choices and polygon controls fit a compact viewport')
  assert.deepEqual(errors,[]);passed=true
  console.log('Polygon explicit Finish/no-write draft, closed stroke hit, 3–64 vertices, vertex editing, move/rotation, v5 clipboard and native29 restart passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Polygon browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
