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

const work = mkdtempSync(join(tmpdir(), 'chemifree-graphic-width-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'graphic-width-browser-create', title: 'Graphic widths' }) })
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
    {op:'add_reference',source:rectangle.id,target:{kind:'object',id:arrow.id}},
  ])
  const group=original.scene.objects.find((object)=>object.kind==='group')
  const fixture=`
import React,{useEffect} from 'react'
import {createRoot} from 'react-dom/client'
import {GraphicPointsPanel} from '/src/components/graphic-points-panel.tsx'
import {SceneClipboardPanel} from '/src/components/scene-clipboard-panel.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {graphicPointsCommands} from '/src/editor/graphic-points.ts'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor()
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.widthHarness=editor
 return <main>
  ${graphics.map((graphic)=>`<button disabled={editor.busy} onClick={()=>editor.setSelection(new Set([${JSON.stringify(graphic.id)}]))}>Select ${graphic.shape}</button>`).join('\n')}
  <button onClick={()=>editor.setSelection(new Set())}>Clear graphic selection</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo width</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo width</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save widths</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'update_settings',line_width_pt:2}])}>Document width 2</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'move_objects',objects:[${JSON.stringify(group.id)}],delta:{x:10,y:5}}])}>Move styled group</button>
  <button disabled={editor.busy} onClick={()=>void editor.apply([{op:'transform_selection',selection:{objects:[${JSON.stringify(group.id)}],atoms:[],bonds:[]},pivot:{x:0,y:0},transform:{type:'rotate',degrees:90}}])}>Rotate styled group</button>
  <output aria-label="Width snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Width busy">{String(editor.busy)}</output>
  <output aria-label="Width error">{editor.error?.message}</output>
  {editor.current&&<><GraphicPointsPanel document={editor.current.document} editor={editor}/>
   <SceneClipboardPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool="select" element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onGraphicPoints={(edit)=>void editor.apply((latest)=>graphicPointsCommands(latest,edit))} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'graphic-width-fixture',configureServer(vite){vite.middlewares.use('/graphic-width-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/graphic-width-check','<!doctype html><html><head><title>Graphic width</title></head><body><div id="root"></div><script type="module" src="/graphic-width-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/graphic-width-harness.tsx')return path},load(path){if(path==='/graphic-width-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1280,height:900}})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Width snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Width busy"]').textContent==='false')
  const graphicAt=async(target)=>(await snapshot()).document.scene.objects.find((object)=>object.id===target)
  const input=()=>page.getByRole('textbox',{name:'Graphic line width (pt)',exact:true})
  const canvas=page.locator('svg[data-structure-canvas]')
  async function waitRevision(revision){await page.waitForFunction((revision)=>JSON.parse(document.querySelector('[aria-label="Width snapshot"]').textContent).document.revision===revision,revision);await idle()}
  function widthOnly(before,after,target,width){
    assert.deepEqual(after.settings,before.settings,'graphic setter does not edit document settings')
    for(const field of ['atoms','bonds','stereo','molecule'])assert.deepEqual(after[field],before[field])
    const expected=structuredClone(before.scene);expected.objects.find((object)=>object.id===target).line_width_pt=width
    assert.deepEqual(after.scene,expected,'only the selected graphic width changes')
  }
  async function appliedWidth(target,text,expected){
    const before=(await snapshot()).document
    await input().fill(text);await button('Apply graphic line width').click();await waitRevision(before.revision+1)
    const after=(await snapshot()).document;widthOnly(before,after,target,expected)
    assert.deepEqual(writes.at(-1).commands,[{op:'set_graphic_line_width',object:target,line_width_pt:expected}])
  }
  async function renderedWidth(graphic,expected,selected=false){
    const tag=graphic.shape==='rectangle'?'rect':'line'
    const mark=page.locator('[data-scene-object="'+graphic.id+'"]')
    assert.equal(Number(await mark.locator(tag+'.stroke-foreground').getAttribute('stroke-width')),expected)
    const scale=Number(await canvas.getAttribute('data-viewport-scale'))
    assert.equal(Number(await mark.locator(tag+'[stroke="transparent"]').getAttribute('stroke-width')),Math.max(10*(1/scale),expected),'hit stroke includes the effective width')
    if(graphic.shape==='arrow')assert.equal(Number(await mark.locator('polygon.fill-foreground').getAttribute('stroke-width')),expected,'arrowhead uses the same effective width')
    if(selected){const halo=await mark.locator(tag).evaluateAll((items)=>items.find((item)=>item.classList.contains('stroke-primary/40'))?.getAttribute('stroke-width'));assert.equal(Number(halo),Math.max(8*(1/scale),expected+6*(1/scale)),'selection uses effective width plus the shared halo')}
  }
  async function screenPoint(point){await canvas.scrollIntoViewIfNeeded();return canvas.evaluate((svg,point)=>{const at=new DOMPoint(point.x,point.y).matrixTransform(svg.getScreenCTM());return{x:at.x,y:at.y}},point)}
  stage='inherited graphic widths and strict numeric/IME input'
  await page.goto('http://127.0.0.1:5173/graphic-width-check')
  await button('Select line').click()
  for(const graphic of graphics)assert.equal((await graphicAt(graphic.id)).line_width_pt,null)
  assert.equal(await input().inputValue(),'')
  assert.equal(await button('Use document width').isDisabled(),true)
  await renderedWidth(line,original.settings.line_width_pt,true)
  const beforeInvalid=writes.length
  for(const invalid of ['0','.099','10.01','NaN','Infinity','0x1','1 pt']){await input().fill(invalid);assert.equal(await button('Apply graphic line width').isDisabled(),true);assert.equal(await input().getAttribute('aria-invalid'),'true')}
  assert.equal(writes.length,beforeInvalid)
  await input().fill('10');await input().dispatchEvent('compositionstart')
  assert.equal(await button('Apply graphic line width').isDisabled(),true)
  await page.getByRole('form',{name:'Graphic line width at '+line.id,exact:true}).evaluate((form)=>form.requestSubmit())
  assert.equal(writes.length,beforeInvalid)
  await input().dispatchEvent('compositionend')
  const before=(await snapshot()).document
  await button('Apply graphic line width').click();await waitRevision(before.revision+1)
  widthOnly(before,(await snapshot()).document,line.id,10)
  await renderedWidth(line,10,true);await renderedWidth(arrow,1.2);await renderedWidth(rectangle,1.2)
  assert.equal(Number(await canvas.locator('[data-bonds]').first().getAttribute('stroke-width')),1.2,'chemical bond width is unchanged')
  assert.equal(await button('Apply graphic line width').isDisabled(),true)
  stage='explicit overrides are preserved by Copy/Cut offers without removing the source'
  for(const action of ['Copy graphics…','Cut graphics…']){
    const captured=(await snapshot()).document
    const count=writes.length
    await button(action).click()
    const dialog=page.getByRole('dialog')
    const text=dialog.getByRole('textbox',{name:'Native graphics text',exact:true})
    await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'))
    const payload=JSON.parse(await text.inputValue())
    assert.equal(payload.version,3)
    assert.equal(payload.objects.find((object)=>object.id===line.id).line_width_pt,10)
    assert.match(await dialog.getByRole('list',{name:'Clipboard graphic line widths'}).textContent(),/10 pt \(override\)/)
    assert.deepEqual((await snapshot()).document,captured)
    assert.equal(writes.length,count,'clipboard preparation is not a mutation')
    await dialog.getByRole('button',{name:'Close',exact:true}).click()
  }
  stage='explicit inheritance restoration enables lossless native clipboard text'
  const restoreBefore=(await snapshot()).document
  await button('Use document width').click();await waitRevision(restoreBefore.revision+1)
  widthOnly(restoreBefore,(await snapshot()).document,line.id,null)
  assert.equal(Object.hasOwn(writes.at(-1).commands[0],'line_width_pt'),true)
  assert.equal(writes.at(-1).commands[0].line_width_pt,null)
  await button('Copy graphics…').click()
  const copyDialog=page.getByRole('dialog')
  await copyDialog.getByRole('textbox',{name:'Native graphics text',exact:true}).waitFor()
  await page.waitForFunction(()=>document.querySelector('[aria-label="Native graphics text"]')?.value?.length>0||[...document.querySelectorAll('textarea')].some((item)=>item.value.includes('chemifree.clipboard')))
  const payload=JSON.parse(await copyDialog.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue())
  assert.equal(payload.objects.find((object)=>object.id===line.id).line_width_pt,undefined,'clipboard schema remains unchanged')
  await copyDialog.getByRole('button',{name:'Close',exact:true}).click()
  await appliedWidth(line.id,'10',10)
  await button('Select arrow').click();await appliedWidth(arrow.id,'3',3);await renderedWidth(arrow,3,true)
  await button('Select rectangle').click();await appliedWidth(rectangle.id,'.1',.1);await renderedWidth(rectangle,.1,true)
  await appliedWidth(rectangle.id,'',null)
  stage='document settings affect inherited graphics while explicit overrides remain fixed'
  const beforeDocument=(await snapshot()).document
  await button('Document width 2').click();await waitRevision(beforeDocument.revision+1)
  assert.deepEqual((await snapshot()).document.scene,beforeDocument.scene)
  for(const field of ['atoms','bonds','stereo','molecule'])assert.deepEqual((await snapshot()).document[field],beforeDocument[field])
  await renderedWidth(line,10);await renderedWidth(arrow,3);await renderedWidth(rectangle,2,true)
  await button('Select line').click()
  await button('Undo width').click();await waitRevision(beforeDocument.revision+2)
  await renderedWidth(rectangle,1.2)
  await button('Redo width').click();await waitRevision(beforeDocument.revision+3)
  await renderedWidth(rectangle,2)
  stage='thick unselected ink is hit at zoom and uses the selected halo'
  await button('Reset zoom (100%)').click()
  for(let index=0;index<4;index+=1)await button('Zoom in').click()
  await button('Clear graphic selection').click()
  const currentLine=await graphicAt(line.id);const[a,b]=currentLine.points
  const length=Math.hypot(b.x-a.x,b.y-a.y)
  const edge={x:(a.x+b.x)/2-(b.y-a.y)*4/length,y:(a.y+b.y)/2+(b.x-a.x)*4/length}
  const at=await screenPoint(edge);const hitWrites=writes.length
  await page.mouse.click(at.x,at.y)
  await page.waitForFunction((id)=>window.widthHarness.selection.has(id),line.id)
  assert.equal(writes.length,hitWrites,'selection on thick ink does not edit the document')
  await renderedWidth(line,10,true)
  stage='point updates, moves and transforms retain each graphic override'
  const beforeMove=(await snapshot()).document
  await button('Move styled group').click();await waitRevision(beforeMove.revision+1)
  for(const graphic of [line,arrow])assert.equal((await graphicAt(graphic.id)).line_width_pt,graphic.id===line.id?10:3)
  await renderedWidth(line,10,true);await renderedWidth(arrow,3)
  await button('Undo width').click();await waitRevision(beforeMove.revision+2)
  const beforeRotate=(await snapshot()).document
  await button('Rotate styled group').click();await waitRevision(beforeRotate.revision+1)
  await renderedWidth(line,10,true);await renderedWidth(arrow,3)
  await button('Undo width').click();await waitRevision(beforeRotate.revision+2)
  await button('Reset zoom (100%)').click()
  const beforeEndpoint=(await snapshot()).document
  const target={x:a.x+5,y:a.y-5}
  await canvas.scrollIntoViewIfNeeded()
  const handle=page.locator('[data-endpoint-object="'+line.id+'"][data-graphic-endpoint="0"]')
  const box=await handle.boundingBox();const to=await screenPoint(target)
  assert.equal(await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.getAttribute('data-endpoint-object'),{x:box.x+box.width/2,y:box.y+box.height/2}),line.id,'endpoint gesture starts on its visible handle')
  await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down();await page.mouse.move(to.x,to.y,{steps:4})
  await renderedWidth(line,10,true)
  assert.equal((await snapshot()).document.revision,beforeEndpoint.revision)
  await page.mouse.up();await waitRevision(beforeEndpoint.revision+1)
  assert.equal((await graphicAt(line.id)).line_width_pt,10)
  await button('Undo width').click();await waitRevision(beforeEndpoint.revision+2)
  stage='queued width edits lapse after earlier revisions without changing style'
  await input().fill('4')
  const beforeQueue=(await snapshot()).document;const queueWrites=writes.length
  await page.evaluate(()=>{void window.widthHarness.apply([{op:'rename',title:'Width queue revision'}]);document.querySelector('form[aria-label^="Graphic line width at"]').requestSubmit()})
  await waitRevision(beforeQueue.revision+1)
  assert.equal(writes.length,queueWrites+1)
  assert.equal((await graphicAt(line.id)).line_width_pt,10)
  stage='native 26 save/reopen preserves explicit and inherited graphic widths'
  await button('Save widths').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Width snapshot"]').textContent).session.unsaved_changes===false)
  const native=JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8'));assert.equal(native.format_version,26)
  const stored=(await snapshot()).document
  await page.reload();await button('Select line').click()
  assert.deepEqual((await snapshot()).document.scene,stored.scene)
  await renderedWidth(line,10,true);await renderedWidth(arrow,3);await renderedWidth(rectangle,2)
  stage='restarted services cannot replay a captured width edit'
  await input().fill('4')
  const beforeRestart=(await snapshot()).document
  const exited=once(api,'exit');api.kill('SIGTERM');await exited
  api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']})
  await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  const restartWrites=writes.length
  await button('Apply graphic line width').click();await page.getByLabel('Width error',{exact:true}).filter({hasText:/./}).waitFor();await idle()
  assert.equal(writes.length,restartWrites+1)
  assert.deepEqual((await snapshot()).document,beforeRestart)
  await page.reload();await button('Select line').click();await renderedWidth(line,10,true)
  assert.deepEqual(errors,[])
  passed=true
  console.log('Graphic width form, inheritance, stroke/selection/hit, geometry preservation, styled clipboard offers and native roundtrip passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Graphic width browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
