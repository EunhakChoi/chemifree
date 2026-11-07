// Real graphic endpoint form, editor queue, HTTP application and native store.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

const work = mkdtempSync(join(tmpdir(), 'chemifree-graphic-creation-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'abbreviation-browser-create', title: 'Graphic creation' }) })
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
    {op:'add_atom',element:'O',position:{x:30,y:0},bond_to:{atom:'$0',order:'single'}},
    {op:'add_graphic',shape:'line',points:[{x:10,y:40},{x:40,y:50}]},
    {op:'add_graphic',shape:'arrow',points:[{x:10,y:70},{x:40,y:80}]},
    {op:'add_graphic',shape:'rectangle',points:[{x:60,y:40},{x:90,y:80}]},
  ])
  const graphics=seeded.scene.objects.filter((object)=>object.kind==='graphic')
  const line=graphics.find((object)=>object.shape==='line')
  const arrow=graphics.find((object)=>object.shape==='arrow')
  const rectangle=graphics.find((object)=>object.shape==='rectangle')
  const original=await transact(seeded.revision,[
    {op:'group',objects:[line.id,arrow.id]},
    {op:'add_reference',source:line.id,target:{kind:'atom',id:originalAtom(seeded)}},
    {op:'add_reference',source:rectangle.id,target:{kind:'object',id:arrow.id}},
  ])
  function originalAtom(document) {return document.atoms[0].id}
  const group=original.scene.objects.find((object)=>object.kind==='group')
  const fixture=`
import React,{useEffect,useState} from 'react'
import {createRoot} from 'react-dom/client'
import {GraphicToolControls} from '/src/components/graphic-tool-controls.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor()
 const [tool,setTool]=useState('graphic')
 const [shape,setShape]=useState(null)
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.graphicCreationHarness={editor,setTool,setShape}
 return <main>
  <button disabled={editor.busy} onClick={()=>editor.setSelection(new Set([${JSON.stringify(group.id)}]))}>Select existing group</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo creation</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo creation</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save graphics</button>
  <GraphicToolControls shape={shape} onShapeChange={setShape} disabled={editor.busy}/>
  <output aria-label="Creation snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Creation busy">{String(editor.busy)}</output>
  <output aria-label="Creation error">{editor.error?.message}</output>
  {editor.current&&<div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool={tool} graphicShape={shape} serviceGeneration={editor.serviceGeneration} onGraphicCreate={(creation)=>void editor.createGraphic(creation)} element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({
    root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),
    server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'graphic-creation-fixture',configureServer(vite){vite.middlewares.use('/graphic-creation-check',async(_request,response)=>{
      response.setHeader('content-type','text/html')
      response.end(await vite.transformIndexHtml('/graphic-creation-check','<!doctype html><html><head><title>Graphic creation</title></head><body><div id="root"></div><script type="module" src="/graphic-creation-harness.tsx"></script></body></html>'))
    })},resolveId(path){if(path==='/graphic-creation-harness.tsx')return path},load(path){if(path==='/graphic-creation-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1280,height:900}})
  const errors=[]
  const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Creation snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Creation busy"]').textContent==='false')
  const canvas=page.locator('svg[data-structure-canvas]')
  const preview=()=>page.locator('[data-graphic-create-preview]')
  const choose=(shape)=>page.getByRole('group',{name:'Graphic shape',exact:true}).getByRole('button',{name:shape[0].toUpperCase()+shape.slice(1),exact:true}).click()
  async function screenPoint(point){
    await canvas.scrollIntoViewIfNeeded()
    return canvas.evaluate((svg,point)=>{const at=new DOMPoint(point.x,point.y).matrixTransform(svg.getScreenCTM());return {x:at.x,y:at.y}},point)
  }
  async function begin(from,to){
    const a=await screenPoint(from);const b=await screenPoint(to)
    await canvas.evaluate((svg)=>svg.addEventListener('pointerdown',(event)=>{window.creationPointerId=event.pointerId},{once:true}))
    await page.mouse.move(a.x,a.y);await page.mouse.down()
    assert.equal(await canvas.evaluate((svg)=>svg.hasPointerCapture(window.creationPointerId)),true)
    await page.mouse.move(b.x,b.y,{steps:4})
  }
  function graphUnchanged(current){
    for(const field of ['atoms','bonds','stereo','settings','molecule'])assert.deepEqual(current[field],original[field])
    assert.deepEqual(current.scene.instances,original.scene.instances)
    assert.deepEqual(current.scene.references,original.scene.references)
    assert.deepEqual(current.scene.objects.filter((object)=>original.scene.objects.some((before)=>before.id===object.id)),original.scene.objects)
  }
  async function previewPoints(shape,points){
    // SVG getBBox excludes stroke: horizontal/vertical line groups can have a zero dimension.
    await page.locator('[data-graphic-create-preview="'+shape+'"]').waitFor({state:'attached'})
    const [a,b]=points
    const expected=shape==='ellipse'?{cx:(a.x+b.x)/2,cy:(a.y+b.y)/2,rx:Math.abs(a.x-b.x)/2,ry:Math.abs(a.y-b.y)/2}:shape==='rectangle'?{x:Math.min(a.x,b.x),y:Math.min(a.y,b.y),width:Math.abs(a.x-b.x),height:Math.abs(a.y-b.y)}:{x1:a.x,y1:a.y,x2:b.x,y2:b.y}
    const mark=preview().locator(shape==='ellipse'?'ellipse.stroke-foreground':shape==='rectangle'?'rect.stroke-foreground':'line.stroke-foreground')
    for(const [name,value] of Object.entries(expected))assert.equal(Number(await mark.getAttribute(name)),value,'preview '+shape+' '+name)
    const ink=await mark.evaluate((mark)=>({stroke:getComputedStyle(mark).stroke,width:Number.parseFloat(getComputedStyle(mark).strokeWidth)}))
    assert.notEqual(ink.stroke,'none','preview has painted stroke')
    assert.ok(ink.width>0,'preview stroke has positive width')
    for(const point of points){const at=await screenPoint(point);const box=await canvas.boundingBox();assert.ok(at.x>=box.x&&at.x<=box.x+box.width&&at.y>=box.y&&at.y<=box.y+box.height,'preview point is in the visible canvas')}
    if(shape==='arrow')assert.equal((await preview().locator('polygon.fill-foreground').getAttribute('points')).split(' ')[0],b.x+','+b.y)
  }
  async function waitRevision(revision){await page.waitForFunction((revision)=>JSON.parse(document.querySelector('[aria-label="Creation snapshot"]').textContent).document.revision===revision,revision);await idle()}
  stage='explicit shape choice before canvas creation'
  await page.goto('http://127.0.0.1:5173/graphic-creation-check')
  await page.getByText('Choose Line, Arrow, Rectangle, Ellipse, Polygon or Spline before drawing.',{exact:false}).waitFor()
  await button('Select existing group').click()
  const beforeChoice=(await snapshot()).document
  const beforeChoiceWrites=writes.length
  await begin({x:0,y:0},{x:30,y:0});await page.mouse.up()
  assert.equal(await preview().count(),0)
  assert.equal(writes.length,beforeChoiceWrites)
  assert.deepEqual((await snapshot()).document,beforeChoice)
  await button('Reset zoom (100%)').click();await button('Zoom in').click();await button('Zoom in').click()
  stage='shape previews over existing atoms, bonds and graphics have no side effects'
  const added=[]
  for(const [shape,points] of [['line',[{x:0,y:0},{x:30,y:0}]],['arrow',[{x:25,y:45},{x:10,y:40}]],['rectangle',[{x:90,y:80},{x:60,y:40}]],['ellipse',[{x:100,y:60},{x:140,y:80}]]]){
    await choose(shape)
    const before=(await snapshot()).document
    const beforeWrites=writes.length
    await begin(points[0],points[1]);await previewPoints(shape,points)
    assert.deepEqual((await snapshot()).document,before,'preview never edits existing objects or chemistry')
    assert.equal(writes.length,beforeWrites)
    await page.mouse.up();await waitRevision(before.revision+1)
    assert.equal(writes.length,beforeWrites+1)
    assert.deepEqual(writes.at(-1).commands,[{op:'add_graphic',shape,points}])
    assert.equal(writes.at(-1).expected_revision,before.revision)
    const current=(await snapshot()).document
    const graphic=current.scene.objects.find((object)=>!before.scene.objects.some((old)=>old.id===object.id))
    assert.deepEqual(graphic,{id:graphic.id,kind:'graphic',parent:null,shape,points,line_width_pt:null})
    assert.ok(current.scene.roots.includes(graphic.id),'selected groups do not become an implicit parent')
    graphUnchanged(current);added.push(graphic)
  }
  stage='creation history and native roundtrip preserve exact point order'
  const beforeUndo=(await snapshot()).document.revision
  await button('Undo creation').click();await waitRevision(beforeUndo+1)
  assert.equal((await snapshot()).document.scene.objects.some((object)=>object.id===added.at(-1).id),false)
  await button('Redo creation').click();await waitRevision(beforeUndo+2)
  assert.deepEqual((await snapshot()).document.scene.objects.find((object)=>object.id===added.at(-1).id),added.at(-1))
  stage='threshold, escape, pointercancel, lost capture and blur discard previews'
  await choose('line')
  let beforeCancel=(await snapshot()).document
  let beforeCancelWrites=writes.length
  const at=await screenPoint({x:0,y:0})
  await page.mouse.move(at.x,at.y);await page.mouse.down();await page.mouse.move(at.x+1,at.y+1);await page.mouse.up()
  assert.equal(writes.length,beforeCancelWrites)
  stage='round-trip graphic drags clear preview below threshold before release'
  for(const shape of ['line','arrow','rectangle','ellipse']){
    await choose(shape)
    for(const pixels of [1,2]){
      const before=(await snapshot()).document
      const count=writes.length
      await begin({x:0,y:0},{x:40,y:20});await previewPoints(shape,[{x:0,y:0},{x:40,y:20}])
      const origin=await screenPoint({x:0,y:0})
      await page.mouse.move(origin.x+pixels,origin.y,{steps:4})
      await preview().waitFor({state:'detached',timeout:5000})
      assert.equal(await canvas.evaluate((svg)=>svg.hasPointerCapture(window.creationPointerId)),true,'returning below threshold keeps gesture ownership until release')
      assert.deepEqual((await snapshot()).document,before,'round-trip preview stays outside document state')
      assert.equal(writes.length,count,'round-trip pointer moves never write')
      await page.mouse.up();await idle()
      assert.deepEqual((await snapshot()).document,before,'release below threshold creates no graphic')
      assert.equal(writes.length,count,'release below threshold sends no transaction')
    }
  }
  await choose('line')
  stage='escape, pointercancel, lost capture and blur discard previews'
  for(const cancel of ['escape','pointercancel','lostcapture','blur']){
    await begin({x:0,y:0},{x:40,y:20});await preview().waitFor()
    if(cancel==='escape')await page.keyboard.press('Escape')
    else if(cancel==='pointercancel')await canvas.evaluate((svg)=>svg.dispatchEvent(new PointerEvent('pointercancel',{pointerId:window.creationPointerId,bubbles:true})))
    else if(cancel==='lostcapture')await canvas.evaluate((svg)=>svg.releasePointerCapture(window.creationPointerId))
    else await page.evaluate(()=>window.dispatchEvent(new Event('blur')))
    await page.mouse.up();await preview().waitFor({state:'detached'})
    assert.deepEqual((await snapshot()).document,beforeCancel,cancel+' leaves the snapshot unchanged')
    assert.equal(writes.length,beforeCancelWrites,cancel+' sends no transaction')
  }
  stage='pan has priority over creation even over existing chemical content'
  for(const mode of ['space','middle']){
    await canvas.focus();const a=await screenPoint({x:0,y:0});const beforeView=await canvas.getAttribute('viewBox')
    if(mode==='space')await page.keyboard.down('Space')
    await page.mouse.move(a.x,a.y);await page.mouse.down({button:mode==='middle'?'middle':'left'})
    await page.mouse.move(a.x+30,a.y+15,{steps:3});await page.mouse.up({button:mode==='middle'?'middle':'left'})
    if(mode==='space')await page.keyboard.up('Space')
    assert.notEqual(await canvas.getAttribute('viewBox'),beforeView)
    assert.equal(await preview().count(),0)
    assert.equal(writes.length,beforeCancelWrites)
  }
  stage='shape, tool, revision and document changes invalidate the press context'
  for(const change of ['shape','tool','revision']){
    await page.evaluate(()=>{window.graphicCreationHarness.setTool('graphic');window.graphicCreationHarness.setShape('line')})
    await begin({x:0,y:0},{x:40,y:20});await preview().waitFor()
    const beforeWrites=writes.length
    if(change==='shape')await page.evaluate(()=>window.graphicCreationHarness.setShape('arrow'))
    else if(change==='tool')await page.evaluate(()=>window.graphicCreationHarness.setTool('select'))
    else await page.evaluate(()=>void window.graphicCreationHarness.editor.apply([{op:'rename',title:'Changed during graphic creation'}]))
    await preview().waitFor({state:'detached'});await page.mouse.up();await idle()
    assert.equal(writes.length,beforeWrites+(change==='revision'?1:0))
    assert.equal((await snapshot()).document.scene.objects.length,beforeCancel.scene.objects.length)
  }
  await page.evaluate(()=>{window.graphicCreationHarness.setTool('graphic');window.graphicCreationHarness.setShape('line')})
  const otherResponse=await fetch(base+'/api/v1/documents',{method:'POST',headers,body:JSON.stringify({request_id:crypto.randomUUID(),title:'Another graphic document'})})
  assert.equal(otherResponse.status,201);const otherId=(await otherResponse.json()).document.id
  const beforeSwitch=writes.length
  await begin({x:0,y:0},{x:40,y:20});await preview().waitFor()
  await page.evaluate((id)=>void window.graphicCreationHarness.editor.open(id),otherId)
  await page.waitForFunction((id)=>JSON.parse(document.querySelector('[aria-label="Creation snapshot"]').textContent).document.id===id,otherId)
  await preview().waitFor({state:'detached'});await page.mouse.up()
  assert.equal(writes.length,beforeSwitch)
  await page.evaluate((id)=>void window.graphicCreationHarness.editor.open(id),id)
  await page.waitForFunction((id)=>JSON.parse(document.querySelector('[aria-label="Creation snapshot"]').textContent).document.id===id,id);await idle()
  stage='queued edits reject stale revisions and freeze the original payload'
  const beforeQueue=(await snapshot()).document
  const beforeQueueWrites=writes.length
  await page.evaluate(()=>{
    const editor=window.graphicCreationHarness.editor
    const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,shape:'arrow',points:[{x:5,y:5},{x:20,y:15}]}
    void editor.apply([{op:'rename',title:'Queued change before creation'}])
    void editor.createGraphic(creation)
  })
  await waitRevision(beforeQueue.revision+1)
  assert.equal(writes.length,beforeQueueWrites+1,'only the earlier rename posts')
  const freezeRevision=(await snapshot()).document.revision
  await page.evaluate(()=>{
    const editor=window.graphicCreationHarness.editor
    const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,shape:'line',points:[{x:5,y:5},{x:20,y:15}]}
    void editor.createGraphic(creation)
    creation.points[1].x=999
  })
  await waitRevision(freezeRevision+1)
  assert.deepEqual(writes.at(-1).commands,[{op:'add_graphic',shape:'line',points:[{x:5,y:5},{x:20,y:15}]}])
  graphUnchanged((await snapshot()).document)
  await button('Save graphics').click()
  await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Creation snapshot"]').textContent).session.unsaved_changes===false)
  const beforeRestart=(await snapshot()).document
  await page.reload();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Creation snapshot"]').textContent??'null')?.document?.scene?.objects?.length>0)
  for(const graphic of added)assert.deepEqual((await snapshot()).document.scene.objects.find((object)=>object.id===graphic.id),graphic)
  stage='a replacement service cannot receive an automatic replay of a creation gesture'
  await choose('line');await begin({x:0,y:0},{x:50,y:15});await preview().waitFor()
  const exited=once(api,'exit');api.kill('SIGTERM');await exited
  api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']})
  await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  const restartWrites=writes.length
  await page.mouse.up();await page.getByLabel('Creation error',{exact:true}).filter({hasText:/./}).waitFor();await idle()
  assert.equal(writes.length,restartWrites+1,'only the expired-session request is sent')
  assert.deepEqual((await snapshot()).document,beforeRestart)
  stage='actual workspace Graphic toolbar and shape choice create one page root'
  await page.goto('http://127.0.0.1:5173/')
  await page.getByText('No document open').waitFor()
  await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click()
  await page.getByRole('button',{name:'Graphic',exact:true}).click()
  await page.getByText('Choose Line, Arrow, Rectangle, Ellipse, Polygon or Spline before drawing.',{exact:false}).waitFor()
  await choose('rectangle')
  const beforeReal=(await(await fetch(base+'/api/v1/documents/'+id)).json()).document
  await begin({x:0,y:0},{x:30,y:20});await previewPoints('rectangle',[{x:0,y:0},{x:30,y:20}])
  const committed=page.waitForResponse((response)=>response.request().method()==='POST'&&response.url().endsWith('/transactions'))
  await page.mouse.up();const response=await committed
  assert.equal(response.status(),200)
  const real=(await response.json()).document
  assert.deepEqual(response.request().postDataJSON().commands,[{op:'add_graphic',shape:'rectangle',points:[{x:0,y:0},{x:30,y:20}]}])
  assert.equal(real.revision,beforeReal.revision+1)
  graphUnchanged(real)
  await choose('ellipse')
  await begin({x:50,y:0},{x:80,y:20});await previewPoints('ellipse',[{x:50,y:0},{x:80,y:20}])
  const ellipseCommit=page.waitForResponse((response)=>response.request().method()==='POST'&&response.url().endsWith('/transactions'))
  await page.mouse.up();const ellipseResponse=await ellipseCommit;assert.equal(ellipseResponse.status(),200)
  assert.deepEqual(ellipseResponse.request().postDataJSON().commands,[{op:'add_graphic',shape:'ellipse',points:[{x:50,y:0},{x:80,y:20}]}])
  graphUnchanged((await ellipseResponse.json()).document)
  const native=JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8'))
  assert.ok(native.document.objects.some((object)=>object.state.kind==='graphic'))
  assert.deepEqual(errors,[])
  passed=true
  console.log('Graphic tool explicit choices, root creation over existing content, preview/cancel/pan, stale/restart rejection and native roundtrip passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Graphic creation browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
