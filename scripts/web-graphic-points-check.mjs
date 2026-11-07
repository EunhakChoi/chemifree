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
import React,{useEffect} from 'react'
import {createRoot} from 'react-dom/client'
import {GraphicPointsPanel} from '/src/components/graphic-points-panel.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
 const editor=useDocumentEditor()
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.graphicHarness=editor
 return <main>
  ${graphics.map((graphic)=>`<button disabled={editor.busy} onClick={()=>editor.setSelection(new Set([${JSON.stringify(graphic.id)}]))}>Select ${graphic.shape}</button>`).join('\n')}
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo endpoints</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo endpoints</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save geometry</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'move_objects',objects:[${JSON.stringify(group.id)}],delta:{x:10,y:5}}])}>Move graphic group</button>
  <output aria-label="Graphic snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Graphic busy">{String(editor.busy)}</output>
  <output aria-label="Graphic error">{editor.error?.message}</output>
  {editor.current&&<><GraphicPointsPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool="select" element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
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
    const shape=graphics.find((graphic)=>graphic.id===target).shape
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
