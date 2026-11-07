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

const work = mkdtempSync(join(tmpdir(), 'chemifree-graphic-points-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'abbreviation-browser-create', title: 'Graphic endpoints' }) })
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
import {GraphicPointsPanel} from '/src/components/graphic-points-panel.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {graphicPointsCommands} from '/src/editor/graphic-points.ts'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
 const editor=useDocumentEditor()
 const [tool,setTool]=useState('select')
 window.graphicCanvasTool=setTool
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.graphicHarness=editor
 return <main>
  ${graphics.map((graphic)=>`<button disabled={editor.busy} onClick={()=>editor.setSelection(new Set([${JSON.stringify(graphic.id)}]))}>Select ${graphic.shape}</button>`).join('\n')}
  <button onClick={()=>editor.setSelection(new Set([${JSON.stringify(line.id)},${JSON.stringify(arrow.id)}]))}>Select two graphics</button>
  <button onClick={()=>editor.setSelection(new Set([${JSON.stringify(group.id)}]))}>Select graphic group</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo endpoints</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo endpoints</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save geometry</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'move_objects',objects:[${JSON.stringify(group.id)}],delta:{x:10,y:5}}])}>Move graphic group</button>
  <output aria-label="Graphic snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Graphic busy">{String(editor.busy)}</output>
  <output aria-label="Graphic error">{editor.error?.message}</output>
  {editor.current&&<><GraphicPointsPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool={tool} element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onGraphicPoints={(edit)=>void editor.apply((latest)=>graphicPointsCommands(latest,edit))} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({
    root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),
    server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'graphic-points-fixture',configureServer(vite){vite.middlewares.use('/graphic-points-check',async(_request,response)=>{
      response.setHeader('content-type','text/html')
      response.end(await vite.transformIndexHtml('/graphic-points-check','<!doctype html><html><head><title>Graphic points</title></head><body><div id="root"></div><script type="module" src="/graphic-points-harness.tsx"></script></body></html>'))
    })},resolveId(path){if(path==='/graphic-points-harness.tsx')return path},load(path){if(path==='/graphic-points-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage()
  const errors=[]
  const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Graphic snapshot',{exact:true}).textContent())
  const graphicAt=async(target)=>(await snapshot()).document.scene.objects.find((object)=>object.id===target)
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Graphic busy"]').textContent==='false')
  async function fill(points) {
    for(const [index,point] of points.entries())for(const axis of ['x','y'])await page.getByRole('textbox',{name:'Endpoint '+(index+1)+' '+axis+' (pt)',exact:true}).fill(String(point[axis]))
  }
  async function expectPoints(target,points) {
    await page.waitForFunction(({target,points})=>JSON.stringify(JSON.parse(document.querySelector('[aria-label="Graphic snapshot"]').textContent).document.scene.objects.find((object)=>object.id===target)?.points)===JSON.stringify(points),{target,points})
    await idle()
    await expectPainted(target,points)
  }
  async function expectPainted(target,points) {
    const shape=graphics.find((graphic)=>graphic.id===target).shape
    await page.waitForFunction(({target,points,shape})=>{
      const mark=document.querySelector('[data-scene-object="'+target+'"]')
      const [a,b]=points
      const names=shape==='rectangle'?{x:Math.min(a.x,b.x),y:Math.min(a.y,b.y),width:Math.abs(b.x-a.x),height:Math.abs(b.y-a.y)}:{x1:a.x,y1:a.y,x2:b.x,y2:b.y}
      const ink=mark?.querySelector(shape==='rectangle'?'rect.stroke-foreground':'line.stroke-foreground')
      return ink!==null&&ink!==undefined&&Object.entries(names).every(([name,value])=>Number(ink.getAttribute(name))===value)
    },{target,points,shape})
    const mark=page.locator('[data-scene-object="'+target+'"]')
    const [a,b]=points
    if(shape==='rectangle') {
      const rect=mark.locator('rect.stroke-foreground')
      for(const [name,value] of Object.entries({x:Math.min(a.x,b.x),y:Math.min(a.y,b.y),width:Math.abs(b.x-a.x),height:Math.abs(b.y-a.y)}))assert.equal(Number(await rect.getAttribute(name)),value,'painted rectangle '+name)
    } else {
      const line=mark.locator('line.stroke-foreground')
      for(const [name,value] of Object.entries({x1:a.x,y1:a.y,x2:b.x,y2:b.y}))assert.equal(Number(await line.getAttribute(name)),value,'painted '+shape+' '+name)
      if(shape==='arrow'&&(a.x!==b.x||a.y!==b.y))assert.equal((await mark.locator('polygon.fill-foreground').getAttribute('points')).split(' ')[0],b.x+','+b.y,'arrowhead follows the stored second point')
    }
  }
  function unchangedStructure(current) {
    for(const field of ['atoms','bonds','stereo','settings'])assert.deepEqual(current[field],original[field])
    assert.deepEqual(current.scene.references,original.scene.references)
    assert.deepEqual(current.scene.roots,original.scene.roots)
    assert.deepEqual(current.scene.instances,original.scene.instances)
    assert.deepEqual(current.scene.objects.map(({points,...object})=>object),original.scene.objects.map(({points,...object})=>object))
  }
  stage='selected stored endpoints, invalid values and IME'
  await page.goto('http://127.0.0.1:5173/graphic-points-check')
  await button('Select line').click()
  assert.equal(await page.locator('[data-graphic-endpoint]').count(),2)
  assert.equal(await button('Apply graphic endpoints').isDisabled(),true)
  const x1=page.getByRole('textbox',{name:'Endpoint 1 x (pt)',exact:true})
  const beforeInvalid=writes.length
  for(const invalid of ['', 'NaN','Infinity','100001','0x10','1pt']) {
    await x1.fill(invalid)
    assert.equal(await button('Apply graphic endpoints').isDisabled(),true)
    assert.equal(await x1.getAttribute('aria-invalid'),'true')
  }
  assert.equal(writes.length,beforeInvalid)
  const movedLine=[{x:15.5,y:-20},{x:80,y:35}]
  await fill(movedLine)
  await x1.dispatchEvent('compositionstart')
  assert.equal(await button('Apply graphic endpoints').isDisabled(),true)
  await page.getByRole('form',{name:'Edit line '+line.id+' endpoints',exact:true}).evaluate((form)=>form.requestSubmit())
  assert.equal(writes.length,beforeInvalid,'composing inputs cannot post')
  await x1.dispatchEvent('compositionend')
  await button('Apply graphic endpoints').click()
  await expectPoints(line.id,movedLine)
  unchangedStructure((await snapshot()).document)
  assert.deepEqual(writes.at(-1).commands,[{op:'update_graphic_points',object:line.id,points:movedLine}])
  assert.equal(await button('Apply graphic endpoints').isDisabled(),true)
  stage='arrow direction, reversed rectangle and explicit coincident endpoints'
  const expected=new Map([[line.id,movedLine]])
  for(const [graphic,points] of [[arrow,[{x:90,y:75},{x:20,y:65}]],[rectangle,[{x:100,y:90},{x:45,y:30}]]]) {
    await button('Select '+graphic.shape).click()
    await fill(points)
    await button('Apply graphic endpoints').click()
    await expectPoints(graphic.id,points)
    expected.set(graphic.id,points)
    unchangedStructure((await snapshot()).document)
  }
  const rectanglePoints=expected.get(rectangle.id)
  const coincident=[{x:5,y:5},{x:5,y:5}]
  await fill(coincident)
  await button('Apply graphic endpoints').click()
  await expectPoints(rectangle.id,coincident)
  assert.equal(await page.locator('[data-graphic-endpoint]').count(),0,'coincident endpoints use the numeric form')
  await page.getByRole('form',{name:'Edit rectangle '+rectangle.id+' endpoints',exact:true}).waitFor()
  await button('Undo endpoints').click()
  await expectPoints(rectangle.id,rectanglePoints)
  await button('Redo endpoints').click()
  await expectPoints(rectangle.id,coincident)
  await button('Undo endpoints').click()
  await expectPoints(rectangle.id,rectanglePoints)
  stage='queued revision change refuses a stale endpoint form without a write'
  await button('Select line').click()
  await fill([{x:99,y:99},{x:120,y:120}])
  const beforeQueue=writes.length
  await page.evaluate(()=>{
    const editor=window.graphicHarness
    void editor.apply([{op:'rename',title:'Queued revision'}])
    document.querySelector('form[aria-label^="Edit line"]').requestSubmit()
  })
  await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Graphic snapshot"]').textContent).document.title==='Queued revision')
  await idle()
  assert.equal(writes.length,beforeQueue+1,'only the preceding rename posts; captured endpoint edit lapses')
  assert.deepEqual((await graphicAt(line.id)).points,movedLine)
  stage='group movement still carries edited graphics and their references'
  await button('Move graphic group').click()
  const groupMoved=movedLine.map((point)=>({x:point.x+10,y:point.y+5}))
  await expectPoints(line.id,groupMoved)
  unchangedStructure((await snapshot()).document)
  await button('Undo endpoints').click()
  await expectPoints(line.id,movedLine)
  stage='native save/reopen keeps shape, grouping, references and endpoint order'
  await button('Save geometry').click()
  await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Graphic snapshot"]').textContent).session.unsaved_changes===false)
  const native=JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8'))
  assert.equal(native.document.objects.filter((object)=>object.state.kind==='graphic').length,3)
  await page.reload()
  await button('Select rectangle').click()
  await expectPoints(rectangle.id,rectanglePoints)
  unchangedStructure((await snapshot()).document)
  for(const [target,points] of expected)assert.deepEqual((await graphicAt(target)).points,points)
  stage='single selected endpoint handles and tool/multiselection bounds'
  const canvas=page.locator('svg[data-structure-canvas]')
  const handle=(target,index)=>page.locator('[data-endpoint-object="'+target+'"][data-graphic-endpoint="'+index+'"]')
  await button('Select two graphics').click()
  assert.equal(await page.locator('[data-graphic-endpoint]').count(),0)
  await button('Select graphic group').click()
  assert.equal(await page.locator('[data-graphic-endpoint]').count(),0)
  await button('Select line').click()
  assert.equal(await page.locator('[data-graphic-endpoint]').count(),2)
  await page.evaluate(()=>window.graphicCanvasTool('pan'))
  await page.waitForFunction(()=>document.querySelectorAll('[data-graphic-endpoint]').length===0)
  await page.evaluate(()=>window.graphicCanvasTool('select'))
  await handle(line.id,0).waitFor()
  async function screenPoint(point) {
    await canvas.scrollIntoViewIfNeeded()
    return canvas.evaluate((svg,point)=>{
      const at=svg.createSVGPoint();at.x=point.x;at.y=point.y
      const screen=at.matrixTransform(svg.getScreenCTM())
      return {x:screen.x,y:screen.y}
    },point)
  }
  async function beginEndpoint(target,index,at) {
    await canvas.scrollIntoViewIfNeeded()
    const box=await handle(target,index).boundingBox()
    assert.ok(box,'handle is exposed before a raw mouse gesture')
    await canvas.evaluate((svg)=>svg.addEventListener('pointerdown',(event)=>{window.graphicPointerId=event.pointerId},{once:true}))
    await page.mouse.move(box.x+box.width/2,box.y+box.height/2)
    await page.mouse.down()
    assert.equal(await canvas.evaluate((svg)=>svg.hasPointerCapture(window.graphicPointerId)),true)
    const to=await screenPoint(at)
    await page.mouse.move(to.x,to.y,{steps:4})
  }
  stage='zoomed endpoint previews preserve the opposite point and release one transaction'
  await button('Reset zoom (100%)').click()
  await button('Zoom in').click();await button('Zoom in').click()
  for(const [graphic,index,at] of [[line,0,{x:25.5,y:-35}],[arrow,1,{x:5,y:95}],[rectangle,0,{x:25,y:15}]]) {
    await button('Select '+graphic.shape).click()
    const before=(await snapshot()).document
    const previous=before.scene.objects.find((object)=>object.id===graphic.id).points
    const points=structuredClone(previous);points[index]=at
    const beforeWrites=writes.length
    await beginEndpoint(graphic.id,index,at)
    await expectPainted(graphic.id,points)
    assert.deepEqual((await snapshot()).document,before,'dragging previews never change saved document state')
    assert.equal(writes.length,beforeWrites,'pointer moves never transact')
    await page.mouse.up()
    await expectPoints(graphic.id,points)
    assert.equal(writes.length,beforeWrites+1,'one release sends one transaction')
    assert.deepEqual(writes.at(-1).commands,[{op:'update_graphic_points',object:graphic.id,points}])
    assert.deepEqual((await graphicAt(graphic.id)).points[1-index],previous[1-index],'opposite endpoint remains exact')
    expected.set(graphic.id,points)
    unchangedStructure((await snapshot()).document)
  }
  stage='threshold, Escape, pointercancel and lost capture all discard endpoint previews'
  await button('Select line').click()
  const fixed=expected.get(line.id)
  const noEditRevision=(await snapshot()).document.revision
  const noEditWrites=writes.length
  await canvas.scrollIntoViewIfNeeded()
  const small=await handle(line.id,0).boundingBox()
  await page.mouse.move(small.x+small.width/2,small.y+small.height/2)
  await page.mouse.down();await page.mouse.move(small.x+small.width/2+1,small.y+small.height/2+1);await page.mouse.up()
  stage='round-trip endpoint drags restore geometry below threshold before release'
  for(const index of [0,1])for(const pixels of [1,2]){
    const before=(await snapshot()).document
    const count=writes.length
    const point=fixed[index]
    const previewed=structuredClone(fixed);previewed[index]={x:point.x+15,y:point.y+10}
    await beginEndpoint(line.id,index,previewed[index]);await expectPainted(line.id,previewed)
    const origin=await screenPoint(point)
    await page.mouse.move(origin.x+pixels,origin.y,{steps:4})
    await page.waitForFunction(({target,index,point})=>{
      const ink=document.querySelector('[data-scene-object="'+target+'"] line.stroke-foreground')
      return Number(ink?.getAttribute('x'+(index+1)))===point.x&&Number(ink?.getAttribute('y'+(index+1)))===point.y
    },{target:line.id,index,point},{timeout:5000})
    assert.equal(await canvas.evaluate((svg)=>svg.hasPointerCapture(window.graphicPointerId)),true,'returning below threshold does not cancel endpoint ownership')
    await expectPainted(line.id,fixed)
    assert.deepEqual((await snapshot()).document,before,'round-trip endpoint preview never commits')
    assert.equal(writes.length,count)
    await page.mouse.up();await idle()
    assert.deepEqual((await snapshot()).document,before,'release below threshold is a no-op')
    assert.equal(writes.length,count,'round-trip endpoint release sends no transaction')
  }
  for(const cancel of ['escape','pointercancel','lostcapture']) {
    stage='endpoint cancel via '+cancel
    await beginEndpoint(line.id,0,{x:fixed[0].x+15,y:fixed[0].y+10})
    if(cancel==='escape')await page.keyboard.press('Escape')
    else if(cancel==='pointercancel')await canvas.evaluate((svg)=>svg.dispatchEvent(new PointerEvent('pointercancel',{pointerId:window.graphicPointerId,bubbles:true})))
    else await canvas.evaluate((svg)=>svg.releasePointerCapture(window.graphicPointerId))
    await page.mouse.up()
    const immediate=await canvas.evaluate((svg,target)=>{
      const current=JSON.parse(document.querySelector('[aria-label="Graphic snapshot"]').textContent).document
      const graphic=current.scene.objects.find((object)=>object.id===target)
      return {revision:current.revision,points:graphic.points,paintedX:Number(svg.querySelector('[data-scene-object="'+target+'"] line.stroke-foreground').getAttribute('x1')),captured:svg.hasPointerCapture(window.graphicPointerId)}
    },line.id)
    assert.equal(immediate.revision,noEditRevision,cancel+' leaves the document revision unchanged')
    assert.deepEqual(immediate.points,fixed,cancel+' never commits its preview')
    assert.equal(immediate.captured,false,cancel+' releases pointer capture')
    await expectPainted(line.id,fixed)
    assert.deepEqual((await graphicAt(line.id)).points,fixed)
    assert.equal(writes.length,noEditWrites,cancel+' sends no transaction')
    console.log('Endpoint cancel '+cancel+': '+JSON.stringify({...immediate,settledPaintedX:fixed[0].x,writes:writes.length-noEditWrites}))
  }
  assert.equal((await snapshot()).document.revision,noEditRevision)
  assert.equal(writes.length,noEditWrites)
  stage='Space and middle button pan take precedence over endpoint handles'
  for(const mode of ['space','middle']) {
    await canvas.scrollIntoViewIfNeeded()
    await canvas.focus()
    const box=await handle(line.id,0).boundingBox()
    const beforeView=await canvas.getAttribute('viewBox')
    if(mode==='space')await page.keyboard.down('Space')
    await page.mouse.move(box.x+box.width/2,box.y+box.height/2)
    await page.mouse.down({button:mode==='middle'?'middle':'left'})
    await page.mouse.move(box.x+box.width/2+25,box.y+box.height/2+15,{steps:3})
    await page.mouse.up({button:mode==='middle'?'middle':'left'})
    if(mode==='space')await page.keyboard.up('Space')
    assert.notEqual(await canvas.getAttribute('viewBox'),beforeView)
    assert.deepEqual((await graphicAt(line.id)).points,fixed)
  }
  assert.equal(writes.length,noEditWrites)
  stage='document revision changes cancel a captured endpoint gesture'
  await beginEndpoint(line.id,0,{x:fixed[0].x+20,y:fixed[0].y+20})
  const beforeRename=writes.length
  await page.evaluate(()=>void window.graphicHarness.apply([{op:'rename',title:'Revision changed during endpoint drag'}]))
  await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Graphic snapshot"]').textContent).document.title==='Revision changed during endpoint drag')
  await idle();await page.mouse.up()
  await expectPainted(line.id,fixed)
  assert.equal(writes.length,beforeRename+1,'only the preceding edit posts')
  stage='endpoint edits save and reopen with exact retained scene structure'
  await button('Save geometry').click()
  await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Graphic snapshot"]').textContent).session.unsaved_changes===false)
  await page.reload();await button('Select line').click()
  for(const [target,points] of expected)await expectPoints(target,points)
  unchangedStructure((await snapshot()).document)
  stage='service restart rejects a press-time endpoint edit without automatic retry'
  const beforeRestart=(await snapshot()).document
  await beginEndpoint(line.id,0,{x:fixed[0].x+30,y:fixed[0].y+10})
  const exited=once(api,'exit');api.kill('SIGTERM');await exited
  api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']})
  await new Promise((ready,reject)=>{
    const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000)
    api.once('error',(error)=>{clearTimeout(timer);reject(error)})
    api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})
  })
  const beforeRestartWrites=writes.length
  await page.mouse.up()
  await page.getByLabel('Graphic error',{exact:true}).filter({hasText:/./}).waitFor()
  await idle()
  assert.equal(writes.length,beforeRestartWrites+1,'only the expired-session request is sent, with no retry in the replacement process')
  assert.deepEqual((await snapshot()).document,beforeRestart)
  await page.reload();await button('Select line').click()
  await expectPoints(line.id,fixed)
  assert.equal(await page.getByLabel('Graphic error',{exact:true}).textContent(),'')
  assert.deepEqual(errors,[])
  passed=true
  console.log('Graphic endpoint production form, invalid/IME and revision guards, retained scene and native roundtrip passed.')
} finally {
  await browser?.close()
  await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){
    const exited=once(api,'exit');api.kill('SIGTERM')
    const force=setTimeout(()=>api.kill('SIGKILL'),5000)
    await exited;clearTimeout(force)
  }
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Graphic points browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
