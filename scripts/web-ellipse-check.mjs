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

const work = mkdtempSync(join(tmpdir(), 'chemifree-ellipse-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'ellipse-browser-create', title: 'Ellipse drawing' }) })
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
import {SceneOrderBoundaryButtons} from '/src/components/scene-order-boundary-buttons.tsx'
import {SelectionRotationForm} from '/src/components/selection-rotation-form.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {graphicPointsCommands} from '/src/editor/graphic-points.ts'
import {selectionTransform} from '/src/editor/selection-geometry.ts'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor()
 const [tool,setTool]=useState('graphic')
 const [shape,setShape]=useState(null)
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.ellipseHarness={editor,setTool}
 const ellipse=editor.current?.document.scene.objects.find((object)=>object.kind==='graphic'&&object.shape==='ellipse')
 const group=ellipse&&editor.current?.document.scene.objects.find((object)=>object.kind==='group'&&object.children.includes(ellipse.id))
 return <main>
  <button onClick={()=>setTool('select')}>Use select</button>
  <button onClick={()=>editor.setSelection(new Set())}>Clear ellipse selection</button>
  <button disabled={!ellipse||editor.busy} onClick={()=>editor.setSelection(new Set([ellipse.id]))}>Select ellipse</button>
  <button disabled={!ellipse||editor.busy} onClick={()=>editor.setSelection(new Set([ellipse.id,'s2']))}>Select mixed ellipse</button>
  <button disabled={!ellipse||editor.busy} onClick={()=>void editor.apply([{op:'group',objects:['s2',ellipse.id]},{op:'add_reference',source:ellipse.id,target:{kind:'object',id:'s2'}}])}>Group ellipse and line</button>
  <button disabled={!group||editor.busy} onClick={()=>editor.setSelection(new Set([group.id]))}>Select ellipse group</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply((latest)=>selectionTransform(latest,editor.selectionAtPress(),'s1',{type:'reflect_vertical'}))}>Reflect ellipse</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'update_settings',line_width_pt:2}])}>Document width 2</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'update_settings',line_width_pt:1.2}])}>Document width default</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo ellipse</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo ellipse</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save ellipse</button>
  <output className="block break-all" aria-label="Ellipse snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Ellipse busy">{String(editor.busy)}</output>
  <output aria-label="Ellipse selection">{JSON.stringify([...editor.selection].sort())}</output>
  <GraphicToolControls shape={shape} onShapeChange={setShape} disabled={editor.busy}/>
  {editor.current&&<>
   <GraphicPointsPanel document={editor.current.document} editor={editor}/>
   <SceneOrderBoundaryButtons document={editor.current.document} editor={editor}/>
   <SelectionRotationForm key={editor.current.document.id} document={editor.current.document} editor={editor} activePlacement="s1"/>
   <SceneClipboardPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool={tool} graphicShape={shape} serviceGeneration={editor.serviceGeneration} onGraphicCreate={(creation)=>void editor.createGraphic(creation)} onGraphicPoints={(edit)=>void editor.apply((latest)=>graphicPointsCommands(latest,edit))} element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'ellipse-fixture',configureServer(vite){vite.middlewares.use('/ellipse-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/ellipse-check','<!doctype html><html><head><title>Ellipse</title></head><body><div id="root"></div><script type="module" src="/ellipse-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/ellipse-harness.tsx')return path},load(path){if(path==='/ellipse-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1400,height:1000}})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  await page.addInitScript(()=>{window.ellipseClipboard={text:'',writes:[],reads:[]};Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText(text){window.ellipseClipboard.writes.push({text,active:navigator.userActivation.isActive});window.ellipseClipboard.text=text;return Promise.resolve()},readText(){window.ellipseClipboard.reads.push(navigator.userActivation.isActive);return Promise.resolve(window.ellipseClipboard.text)}}})})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Ellipse snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Ellipse busy"]').textContent==='false')
  const canvas=page.locator('svg[data-structure-canvas]')
  const graphic=(id)=>page.locator('[data-scene-object="'+id+'"]')
  async function revision(value){await page.waitForFunction((value)=>JSON.parse(document.querySelector('[aria-label="Ellipse snapshot"]').textContent).document.revision===value,value);await idle()}
  async function screenPoint(point){await canvas.scrollIntoViewIfNeeded();return canvas.evaluate((svg,point)=>{const at=new DOMPoint(point.x,point.y).matrixTransform(svg.getScreenCTM());return{x:at.x,y:at.y}},point)}
  async function click(point){const at=await screenPoint(point);await page.mouse.click(at.x,at.y)}
  async function begin(from,to){const a=await screenPoint(from);const b=await screenPoint(to);await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:4})}
  function graphSame(document){for(const key of ['atoms','bonds','stereo','molecule'])assert.deepEqual(document[key],seeded[key]);assert.deepEqual(document.scene.instances,seeded.scene.instances)}
  async function ellipseAttrs(id,expected,width){const mark=graphic(id).locator('ellipse.stroke-foreground');for(const[key,value]of Object.entries(expected))assert.equal(Number(await mark.getAttribute(key)),value,key);assert.equal(await mark.getAttribute('fill'),'none');assert.equal(Number(await mark.getAttribute('stroke-width')),width);assert.equal(await graphic(id).locator('line.stroke-foreground,rect.stroke-foreground').count(),0,'ellipse is not a line or bounding rectangle');const painted=await mark.evaluate((node)=>getComputedStyle(node).stroke);assert.notEqual(painted,'none')}
  async function undo(){const before=(await snapshot()).document;await button('Undo ellipse').click();await revision(before.revision+1)}
  async function redo(){const before=(await snapshot()).document;await button('Redo ellipse').click();await revision(before.revision+1)}
  stage='explicit ellipse gesture creates ordered reversed corners with SVG-only preview'
  await page.goto('http://127.0.0.1:5173/ellipse-check');await page.waitForFunction(()=>!!window.ellipseHarness.editor.current)
  await page.getByRole('group',{name:'Graphic shape',exact:true}).getByRole('button',{name:'Ellipse',exact:true}).click()
  await button('Reset zoom (100%)').click()
  const creationBefore=(await snapshot()).document;const creationWrites=writes.length
  await begin({x:120,y:80},{x:40,y:40})
  const preview=page.locator('[data-graphic-create-preview="ellipse"] ellipse.stroke-foreground');await preview.waitFor();assert.equal(Number(await preview.getAttribute('cx')),80);assert.equal(Number(await preview.getAttribute('rx')),40)
  assert.deepEqual((await snapshot()).document,creationBefore);assert.equal(writes.length,creationWrites)
  await page.mouse.up();await revision(creationBefore.revision+1)
  const ellipse=(await snapshot()).document.scene.objects.find((object)=>object.kind==='graphic'&&object.shape==='ellipse');const eid=ellipse.id
  assert.deepEqual(writes.at(-1).commands,[{op:'add_graphic',shape:'ellipse',points:[{x:120,y:80},{x:40,y:40}]}]);assert.equal(ellipse.parent,null);assert.equal(ellipse.line_width_pt,null);graphSame((await snapshot()).document)
  await undo();assert.equal((await snapshot()).document.scene.objects.some((object)=>object.id===eid),false);await redo();assert.deepEqual((await snapshot()).document.scene.objects.find((object)=>object.id===eid),ellipse)
  await button('Use select').click();await button('Select ellipse').click();await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},1.2)
  const thinWrites=writes.length
  await button('Clear ellipse selection').click()
  await page.waitForFunction(()=>window.ellipseHarness.editor.selection.size===0)
  const thinHit=await screenPoint({x:123,y:60})
  assert.deepEqual(await page.evaluate(({x,y})=>{const node=document.elementFromPoint(x,y);return{object:node?.closest('[data-scene-object]')?.getAttribute('data-scene-object'),stroke:node?.getAttribute('stroke')}},thinHit),{object:eid,stroke:'transparent'},'the transparent curve hit stroke covers a point outside thin ink')
  await page.mouse.click(thinHit.x,thinHit.y);await page.waitForFunction((id)=>window.ellipseHarness.editor.selection.has(id),eid);assert.equal(writes.length,thinWrites)
  stage='ellipse stroke follows document inheritance and explicit width without changing chemistry'
  let before=(await snapshot()).document;await button('Document width 2').click();await revision(before.revision+1);await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},2)
  before=(await snapshot()).document;await page.getByRole('textbox',{name:'Graphic line width (pt)',exact:true}).fill('10');await button('Apply graphic line width').click();await revision(before.revision+1)
  before=(await snapshot()).document;await button('Document width default').click();await revision(before.revision+1);await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},10)
  before=(await snapshot()).document;await button('Use document width').click();await revision(before.revision+1);await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},1.2)
  before=(await snapshot()).document;await page.getByRole('textbox',{name:'Graphic line width (pt)',exact:true}).fill('10');await button('Apply graphic line width').click();await revision(before.revision+1)
  stage='stroke-only native ellipse hit testing at zoom excludes center and bounding corners'
  await button('Reset zoom (100%)').click();for(let index=0;index<4;index+=1)await button('Zoom in').click()
  const hitWrites=writes.length
  for(const point of [{x:80,y:60},{x:120,y:80}]){await button('Clear ellipse selection').click();await click(point);await page.waitForFunction(()=>window.ellipseHarness.editor.selection.size===0);assert.equal(writes.length,hitWrites)}
  const edge=await screenPoint({x:124,y:60});assert.equal(await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.closest('[data-scene-object]')?.getAttribute('data-scene-object'),edge),eid,'the native ellipse stroke receives this pointer')
  await page.mouse.click(edge.x,edge.y);await page.waitForFunction((id)=>window.ellipseHarness.editor.selection.has(id),eid)
  const scale=Number(await canvas.getAttribute('data-viewport-scale'));assert.equal(Number(await graphic(eid).locator('ellipse[stroke="transparent"]').getAttribute('stroke-width')),Math.max(10*(1/scale),10));assert.equal(Number(await graphic(eid).locator('ellipse').evaluateAll((nodes)=>nodes.find((node)=>node.classList.contains('stroke-primary/40')).getAttribute('stroke-width'))),Math.max(8*(1/scale),10+6*(1/scale)))
  async function points(value){await button('Select ellipse').click();const before=(await snapshot()).document;for(const[index,point]of value.entries()){await page.getByRole('textbox',{name:'Endpoint '+(index+1)+' x (pt)',exact:true}).fill(String(point.x));await page.getByRole('textbox',{name:'Endpoint '+(index+1)+' y (pt)',exact:true}).fill(String(point.y))}await button('Apply graphic endpoints').click();await revision(before.revision+1);assert.deepEqual((await snapshot()).document.scene.objects.find((object)=>object.id===eid).points,value)}
  stage='zero axes and coincident corners retain exact finite geometry and remain numerically editable'
  for(const[value,radii,handles]of [[[{x:80,y:40},{x:80,y:80}],{rx:0,ry:20},2],[[{x:40,y:60},{x:120,y:60}],{rx:40,ry:0},2],[[{x:80,y:60},{x:80,y:60}],{rx:0,ry:0},0]]){
    await points(value);await ellipseAttrs(eid,{cx:80,cy:60,...radii},10);assert.equal(await page.locator('[data-endpoint-object="'+eid+'"]').count(),handles)
    await button('Clear ellipse selection').click();await click({x:80,y:60});await page.waitForFunction(()=>window.ellipseHarness.editor.selection.size===0)
  }
  await points([{x:120,y:80},{x:40,y:40}]);await button('Reset zoom (100%)').click()
  stage='endpoint and rigid body previews retain ellipse type, width and graph'
  const moveBefore=(await snapshot()).document;const moveWrites=writes.length
  await begin({x:120,y:60},{x:130,y:65});await ellipseAttrs(eid,{cx:90,cy:65,rx:40,ry:20},10);assert.equal((await snapshot()).document.revision,moveBefore.revision);assert.equal(writes.length,moveWrites)
  await page.mouse.up();await revision(moveBefore.revision+1)
  const endpointBefore=(await snapshot()).document
  const handle=page.locator('[data-endpoint-object="'+eid+'"][data-graphic-endpoint="0"]');await canvas.scrollIntoViewIfNeeded();const box=await handle.boundingBox();const to=await screenPoint({x:140,y:95});assert.equal(await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.getAttribute('data-endpoint-object'),{x:box.x+box.width/2,y:box.y+box.height/2}),eid)
  await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();await page.mouse.move(to.x,to.y,{steps:4});await ellipseAttrs(eid,{cx:95,cy:70,rx:45,ry:25},10);assert.equal((await snapshot()).document.revision,endpointBefore.revision)
  await page.mouse.up();await revision(endpointBefore.revision+1);graphSame((await snapshot()).document);await undo();await undo()
  stage='axis-aligned ellipse arbitrary rotation refuses the entire union; quarter-turn/flip remain supported'
  await button('Select mixed ellipse').click();await page.getByRole('textbox',{name:'Rotation angle (degrees)',exact:true}).fill('45');const refuseBefore=await snapshot();const refusal=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await button('Rotate').click();const refused=await refusal;assert.equal(refused.status(),422);assert.equal((await refused.json()).reason,'transform_ellipse_unsupported');await idle();assert.deepEqual((await snapshot()).document,refuseBefore.document);assert.equal((await snapshot()).session.undo_depth,refuseBefore.session.undo_depth)
  await button('Select ellipse').click();await page.getByRole('textbox',{name:'Rotation angle (degrees)',exact:true}).fill('450');before=(await snapshot()).document;await button('Rotate').click();await revision(before.revision+1);await ellipseAttrs(eid,{cx:80,cy:60,rx:20,ry:40},10);await undo()
  before=(await snapshot()).document;await button('Reflect ellipse').click();await revision(before.revision+1);assert.deepEqual((await snapshot()).document.scene.objects.find((object)=>object.id===eid).points,[{x:40,y:80},{x:120,y:40}]);await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},10);await undo()
  stage='ellipse subtree order and styled v4 Copy/Paste/Cut preserve graph, refs and exact box order'
  before=(await snapshot()).document;await button('Group ellipse and line').click();await revision(before.revision+1);await button('Select ellipse group').click();const group=(await snapshot()).document.scene.objects.find((object)=>object.kind==='group'&&object.children.includes(eid))
  before=(await snapshot()).document;await page.getByRole('group',{name:'Scene order boundaries',exact:true}).getByRole('button',{name:'Last',exact:true}).click();await revision(before.revision+1);assert.equal((await snapshot()).document.scene.roots.at(-1),group.id)
  before=(await snapshot()).document;await page.getByRole('group',{name:'Scene order boundaries',exact:true}).getByRole('button',{name:'First',exact:true}).click();await revision(before.revision+1);assert.equal((await snapshot()).document.scene.roots[0],group.id);await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},10)
  const copyBefore=(await snapshot()).document;const copyWrites=writes.length;await button('Copy graphics…').click();await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));const copyText=await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue();const payload=JSON.parse(copyText);assert.equal(payload.version,4);assert.equal(payload.objects.find((object)=>object.id===eid).line_width_pt,10);assert.equal(payload.objects.filter((object)=>object.kind==='graphic').every((object)=>Object.hasOwn(object,'line_width_pt')),true);assert.match(await page.getByRole('list',{name:'Clipboard graphic line widths'}).textContent(),/ellipse .*10 pt \(override\)/);await button('Copy to clipboard').click();await page.getByRole('status').filter({hasText:'Native graphics text copied to clipboard.'}).waitFor();await button('Close').click();assert.equal(writes.length,copyWrites);assert.deepEqual((await snapshot()).document,copyBefore)
  await button('Paste graphics…').click();await button('Read clipboard').click();await page.waitForFunction((text)=>document.querySelector('textarea')?.value===text,copyText);await page.getByRole('textbox',{name:'Offset x (pt)',exact:true}).fill('100');await page.getByRole('textbox',{name:'Offset y (pt)',exact:true}).fill('0');const pasteResponse=page.waitForResponse((response)=>response.url().endsWith('/operations')&&response.request().postDataJSON()?.operation==='document.clipboard_scene_paste');await button('Paste graphics').click();const pasteEnvelope=await(await pasteResponse).json();assert.equal(pasteEnvelope.outcome,'ok');await revision(copyBefore.revision+1)
  const pasted=(await snapshot()).document;const mapping=pasteEnvelope.result.mapping.objects;const pastedId=mapping.find((item)=>item.source===eid).target;const target=pasted.scene.objects.find((object)=>object.id===pastedId);assert.deepEqual(target.points,[{x:220,y:80},{x:140,y:40}]);assert.equal(target.shape,'ellipse');assert.equal(target.line_width_pt,10);for(const object of copyBefore.scene.objects)assert.deepEqual(pasted.scene.objects.find((item)=>item.id===object.id),object);graphSame(pasted);await ellipseAttrs(pastedId,{cx:180,cy:60,rx:40,ry:20},10)
  await undo();assert.deepEqual((await snapshot()).document.scene,copyBefore.scene);await redo();assert.deepEqual((await snapshot()).document.scene,pasted.scene);await undo()
  await button('Select ellipse group').click();const cutBefore=(await snapshot()).document;await button('Cut graphics…').click();await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));assert.equal(JSON.parse(await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue()).version,4);await button('Cut to clipboard').click();await revision(cutBefore.revision+1);const cutAfter=(await snapshot()).document;assert.equal(cutAfter.scene.objects.some((object)=>object.id===eid||object.id===group.id||object.id==='s2'),false);graphSame(cutAfter);await undo();assert.deepEqual((await snapshot()).document.scene,cutBefore.scene);await redo();await undo();assert.deepEqual((await snapshot()).document.scene,cutBefore.scene)
  const transport=await page.evaluate(()=>window.ellipseClipboard);assert.equal(transport.writes.length,2);assert.equal(transport.writes.every((write)=>write.active),true);assert.deepEqual(transport.reads,[true])
  stage='native 30 stores the ellipse and its styled order/history across a real process restart'
  await button('Save ellipse').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Ellipse snapshot"]').textContent).session.unsaved_changes===false);const saved=(await snapshot()).document;assert.equal(JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8')).format_version,30)
  const exited=once(api,'exit');api.kill('SIGTERM');await exited;api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']});await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  await page.reload();await page.waitForFunction(()=>!!window.ellipseHarness.editor.current);assert.deepEqual((await snapshot()).document.scene,saved.scene);await ellipseAttrs(eid,{cx:80,cy:60,rx:40,ry:20},10)
  stage='actual Scene Add ellipse and compact explicit shape controls retain graph and accessibility'
  await page.goto('http://127.0.0.1:5173/');await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click();const addition=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await button('Add ellipse').click();const added=await addition;assert.equal(added.status(),200);assert.deepEqual(added.request().postDataJSON().commands,[{op:'add_graphic',shape:'ellipse',points:[{x:125,y:20},{x:149,y:36}]}]);const actual=(await added.json()).document;const fresh=actual.scene.objects.find((object)=>!saved.scene.objects.some((old)=>old.id===object.id));await ellipseAttrs(fresh.id,{cx:137,cy:28,rx:12,ry:8},1.2);graphSame(actual)
  await page.setViewportSize({width:320,height:900});await button('Graphic').click();await page.getByRole('group',{name:'Graphic shape',exact:true}).getByRole('button',{name:'Ellipse',exact:true}).click();assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'six explicit choices fit a compact viewport');assert.deepEqual(errors,[]);passed=true
  console.log('Ellipse SVG stroke/hit, reversed/zero axes, gesture/endpoint/move/order, graph/stereo preservation, v4 styled clipboard and native 30 roundtrip passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Ellipse browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
