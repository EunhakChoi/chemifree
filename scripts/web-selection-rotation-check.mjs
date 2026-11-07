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

const work = mkdtempSync(join(tmpdir(), 'chemifree-selection-rotation-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'selection-rotation-browser-create', title: 'Arbitrary rotation' }) })
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
    {op:'add_instance',source:'s1',offset:{x:100,y:100}},
    {op:'add_graphic',shape:'line',points:[{x:140,y:100},{x:160,y:100}]},
    {op:'add_graphic',shape:'arrow',points:[{x:180,y:100},{x:200,y:100}]},
    {op:'add_graphic',shape:'rectangle',points:[{x:140,y:140},{x:160,y:160}]},
    {op:'add_caption',anchor:{x:200,y:200},alignment:'left',runs:[{text:'Caption',script:'baseline',weight:'normal',style:'normal'}]},
    {op:'group',objects:['s3','s4']},
    {op:'group',objects:['s5','s6']},
  ])
  await transact(seeded.revision,[
    {op:'set_graphic_line_width',object:'s3',line_width_pt:.1},
    {op:'set_graphic_line_width',object:'s4',line_width_pt:10},
    {op:'add_reference',source:'s3',target:{kind:'atom',id:'a1'}},
    {op:'add_reference',source:'s5',target:{kind:'object',id:'s4'}},
  ])
  const otherCreate=await fetch(base+'/api/v1/documents',{method:'POST',headers,body:JSON.stringify({request_id:'rotation-empty',title:'Empty rotation'})})
  assert.equal(otherCreate.status,201)
  const otherId=(await otherCreate.json()).document.id
  const fixture=`
import React,{useEffect,useState} from 'react'
import {createRoot} from 'react-dom/client'
import {SelectionRotationForm} from '/src/components/selection-rotation-form.tsx'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor()
 const [placement,setPlacement]=useState('s2')
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.rotationHarness={editor,setPlacement}
 return <main>
  <button onClick={()=>editor.setSelection(new Set(['s3','s7','a1','b1']))}>Select mixed overlap</button>
  <button onClick={()=>editor.setSelection(new Set(['s3','a1','b1']))}>Select queue scope</button>
  <button onClick={()=>editor.setSelection(new Set(['s3']))}>Select line</button>
  <button onClick={()=>editor.setSelection(new Set(['s3','s5']))}>Select rectangle pair</button>
  <button onClick={()=>editor.setSelection(new Set(['s8']))}>Select caption group</button>
  <button onClick={()=>void editor.apply([{op:'move_objects',objects:['s3'],delta:{x:5,y:0}}])}>Nudge line</button>
  <button onClick={()=>void editor.apply([{op:'delete_objects',objects:['s3'],mode:'subtree'}])}>Delete line</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo rotation</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo rotation</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save rotation</button>
  <button disabled={editor.busy} onClick={()=>void editor.open(${JSON.stringify(otherId)})}>Open empty</button>
  <button disabled={editor.busy} onClick={()=>void editor.open(${JSON.stringify(id)})}>Open rotation</button>
  <output aria-label="Rotation snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Rotation busy">{String(editor.busy)}</output>
  <output aria-label="Rotation selection">{JSON.stringify([...editor.selection].sort())}</output>
  <output aria-label="Rotation error">{editor.error?.message}</output>
  {editor.current&&<SelectionRotationForm key={editor.current.document.id} document={editor.current.document} editor={editor} activePlacement={placement}/>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'selection-rotation-fixture',configureServer(vite){vite.middlewares.use('/rotation-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/rotation-check','<!doctype html><html><head><title>Selection rotation</title></head><body><div id="root"></div><script type="module" src="/rotation-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/rotation-harness.tsx')return path},load(path){if(path==='/rotation-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1280,height:1000}})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const input=()=>page.getByRole('textbox',{name:'Rotation angle (degrees)',exact:true})
  const form=()=>page.getByRole('form',{name:'Rotate selected geometry',exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Rotation snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Rotation busy"]').textContent==='false')
  async function revision(value){await page.waitForFunction((value)=>JSON.parse(document.querySelector('[aria-label="Rotation snapshot"]').textContent).document.revision===value,value);await idle()}
  async function selected(name,ids){await button(name).click();await page.waitForFunction((ids)=>document.querySelector('[aria-label="Rotation selection"]').textContent===JSON.stringify(ids.sort()),ids)}
  function pointAt(point,pivot,degrees){const angle=degrees*Math.PI/180;const x=point.x-pivot.x;const y=point.y-pivot.y;return{x:pivot.x+x*Math.cos(angle)-y*Math.sin(angle),y:pivot.y+x*Math.sin(angle)+y*Math.cos(angle)}}
  function near(actual,expected){assert.ok(Math.abs(actual.x-expected.x)<1e-8&&Math.abs(actual.y-expected.y)<1e-8,JSON.stringify({actual,expected}))}
  function geometryOnly(before,after,pivot,degrees,graphicIds,atoms=[]){
    for(const key of ['id','title','atoms','bonds','stereo','molecule','settings','abbreviations','fragment_displays'])assert.deepEqual(after[key],before[key],key)
    for(const key of ['pages','roots','references'])assert.deepEqual(after.scene[key],before.scene[key],key)
    const metadata=(object)=>object.kind==='graphic'?{...object,points:[]}:object
    assert.deepEqual(after.scene.objects.map(metadata),before.scene.objects.map(metadata),'shape, group, caption, reference ownership and override metadata stay exact')
    for(const object of before.scene.objects){if(object.kind!=='graphic')continue;const actual=after.scene.objects.find((item)=>item.id===object.id);if(graphicIds.includes(object.id))object.points.forEach((point,index)=>near(actual.points[index],pointAt(point,pivot,degrees)));else assert.deepEqual(actual.points,object.points)}
    for(const placement of before.scene.instances){const actual=after.scene.instances.find((item)=>item.id===placement.id);assert.deepEqual({...actual,positions:[]},{...placement,positions:[]});placement.positions.forEach((position,index)=>{if(placement.id==='s2'&&atoms.includes(position.atom))near(actual.positions[index],pointAt(position,pivot,degrees));else assert.deepEqual(actual.positions[index],position)})}
  }
  async function rotate(text){const before=(await snapshot()).document;await input().fill(text);const response=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await button('Rotate').click();const reply=await response;assert.equal(reply.status(),200,await reply.text());await revision(before.revision+1);return{before,after:(await snapshot()).document,command:writes.at(-1).commands}}
  stage='finite angle validation, no-op turns and IME'
  await page.goto('http://127.0.0.1:5173/rotation-check')
  await button('Select line').waitFor();await page.waitForFunction(()=>!!window.rotationHarness.editor.current)
  await selected('Select line',['s3'])
  const untouched=(await snapshot()).document;const invalidWrites=writes.length
  for(const text of ['', 'NaN','Infinity','1e309','0x10','30 degrees']){await input().fill(text);assert.equal(await button('Rotate').isDisabled(),true);await form().evaluate((form)=>form.requestSubmit())}
  for(const text of ['0','-0','360','-720','-5e-324']){await input().fill(text);assert.equal(await input().getAttribute('aria-invalid'),'false');assert.equal(await button('Rotate').isDisabled(),true);await form().evaluate((form)=>form.requestSubmit())}
  await input().fill('30');await input().dispatchEvent('compositionstart');assert.equal(await button('Rotate').isDisabled(),true);await form().evaluate((form)=>form.requestSubmit());await input().dispatchEvent('compositionend')
  assert.equal(writes.length,invalidWrites);assert.deepEqual((await snapshot()).document,untouched)
  stage='positive, negative and multi-turn mixed rotations with one history step'
  await selected('Select mixed overlap',['s3','s7','a1','b1'])
  for(const [text,angle] of [['30',30],['-37.5',-37.5],['390',30]]){
    const result=await rotate(text)
    assert.deepEqual(result.command,[{op:'transform_selection',selection:{objects:['s7','s3'],atoms:['a1'],bonds:['b1']},instance:'s2',pivot:{x:150,y:100},transform:{type:'rotate',degrees:Number(text)}}])
    geometryOnly(result.before,result.after,{x:150,y:100},angle,['s3','s4'],['a1','a2'])
    await button('Undo rotation').click();await revision(result.after.revision+1);assert.deepEqual((await snapshot()).document.scene,result.before.scene)
    await button('Redo rotation').click();await revision(result.after.revision+2);assert.deepEqual((await snapshot()).document.scene,result.after.scene)
    await button('Undo rotation').click();await revision(result.after.revision+3)
  }
  stage='large finite angles have no separate UI magnitude limit'
  await selected('Select line',['s3']);assert.equal(await input().getAttribute('max'),null)
  const large=await rotate('1e20');assert.equal(large.command[0].transform.degrees,1e20);geometryOnly(large.before,large.after,{x:150,y:100},280,['s3'])
  await button('Undo rotation').click();await revision(large.after.revision+1)
  stage='caption groups stay disabled and arbitrary rectangles refuse the entire union'
  await selected('Select caption group',['s8']);await input().fill('45');assert.equal(await button('Rotate').isDisabled(),true)
  const caption=(await snapshot()).document;const captionWrites=writes.length;await form().evaluate((form)=>form.requestSubmit());assert.equal(writes.length,captionWrites);assert.deepEqual((await snapshot()).document,caption)
  await selected('Select rectangle pair',['s3','s5']);await input().fill('45')
  const rectBefore=await snapshot();const refused=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await button('Rotate').click();const refusal=await refused;assert.equal(refusal.status(),422);const refusalBody=await refusal.json();assert.equal(refusalBody.code,'invalid_command');assert.equal(refusalBody.reason,'transform_rectangle_unsupported');assert.equal(refusalBody.command_index,0);await idle();assert.deepEqual((await snapshot()).document,rectBefore.document);assert.equal((await snapshot()).session.undo_depth,rectBefore.session.undo_depth)
  const quarter=await rotate('450');geometryOnly(quarter.before,quarter.after,{x:150,y:130},90,['s3','s5']);await button('Undo rotation').click();await revision(quarter.after.revision+1)
  stage='queued rotation retains pressed selection and placement while recomputing its pivot'
  await selected('Select queue scope',['s3','a1','b1']);await input().fill('45')
  let release;let arrived
  const gate=new Promise((resolve)=>{release=resolve});const started=new Promise((resolve)=>{arrived=resolve})
  const hold=async(route)=>{if(!route.request().postDataJSON().commands.some((command)=>command.op==='move_objects'))return route.fallback();const response=await route.fetch();arrived();await gate;await route.fulfill({response})}
  await page.route('**/transactions',hold)
  const queueBefore=(await snapshot()).document
  await button('Nudge line').click();await started
  await button('Rotate').click()
  await page.evaluate(()=>{window.rotationHarness.setPlacement('s1');window.rotationHarness.editor.setSelection(new Set(['s4']))})
  release();await revision(queueBefore.revision+2);await page.unroute('**/transactions',hold)
  assert.deepEqual(writes.at(-1).commands,[{op:'transform_selection',selection:{objects:['s3'],atoms:['a1'],bonds:['b1']},instance:'s2',pivot:{x:132.5,y:100},transform:{type:'rotate',degrees:45}}])
  const nudged=structuredClone(queueBefore);nudged.scene.objects.find((object)=>object.id==='s3').points.forEach((point)=>{point.x+=5})
  geometryOnly(nudged,(await snapshot()).document,{x:132.5,y:100},45,['s3'],['a1','a2'])
  await button('Undo rotation').click();await revision(queueBefore.revision+3);await button('Undo rotation').click();await revision(queueBefore.revision+4)
  stage='deleted captured targets lapse in the queue without a second transaction'
  await selected('Select line',['s3']);await input().fill('20')
  let releaseDelete;let deleted
  const deleteGate=new Promise((resolve)=>{releaseDelete=resolve});const deleteStarted=new Promise((resolve)=>{deleted=resolve})
  const holdDelete=async(route)=>{if(!route.request().postDataJSON().commands.some((command)=>command.op==='delete_objects'))return route.fallback();const response=await route.fetch();assert.equal(response.status(),200);deleted();await deleteGate;await route.fulfill({response})}
  await page.route('**/transactions',holdDelete)
  const deleteBefore=(await snapshot()).document;const deleteWrites=writes.length
  await button('Delete line').click();await deleteStarted;await button('Rotate').click();releaseDelete();await revision(deleteBefore.revision+1);await page.unroute('**/transactions',holdDelete)
  assert.equal(writes.length,deleteWrites+1);assert.equal((await snapshot()).document.scene.objects.some((object)=>object.id==='s3'),false)
  await button('Undo rotation').click();await revision(deleteBefore.revision+2)
  assert.deepEqual((await snapshot()).document.scene,deleteBefore.scene)
  stage='document changes reset the draft and native 29 retains rotated geometry and style'
  await selected('Select line',['s3'])
  const persisted=await rotate('22.5');geometryOnly(persisted.before,persisted.after,{x:150,y:100},22.5,['s3'])
  await button('Save rotation').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Rotation snapshot"]').textContent).session.unsaved_changes===false)
  const saved=(await snapshot()).document
  assert.equal(JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8')).format_version,29)
  await input().fill('33');await button('Open empty').click();await page.waitForFunction((id)=>window.rotationHarness.editor.current?.document.id===id,otherId);assert.equal(await input().inputValue(),'');assert.equal(await button('Rotate').isDisabled(),true)
  await button('Open rotation').click();await page.waitForFunction((id)=>window.rotationHarness.editor.current?.document.id===id,id);await idle()
  const exited=once(api,'exit');api.kill('SIGTERM');await exited
  api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']})
  await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  await page.reload();await page.waitForFunction(()=>!!window.rotationHarness.editor.current);assert.deepEqual((await snapshot()).document.scene,saved.scene)
  stage='actual workspace exposes the same arbitrary-angle control beside fixed rotations'
  await page.goto('http://127.0.0.1:5173/')
  await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click()
  assert.deepEqual(saved.selection,{objects:['s3'],atoms:[],bonds:[]})
  assert.equal(await button('Select scene object s3').getAttribute('aria-pressed'),'true','opening the native document restores its selected line')
  await input().fill('13')
  assert.equal(await button('Rotate 90° clockwise').isEnabled(),true)
  const actual=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await button('Rotate').click();const actualResponse=await actual;assert.equal(actualResponse.status(),200)
  assert.deepEqual(actualResponse.request().postDataJSON().commands,[{op:'transform_selection',selection:{objects:['s3'],atoms:[],bonds:[]},pivot:{x:150,y:100},transform:{type:'rotate',degrees:13}}])
  geometryOnly(saved,(await actualResponse.json()).document,{x:150,y:100},13,['s3'])
  assert.deepEqual(errors,[])
  passed=true
  console.log('Arbitrary rotation form/workspace, finite/modulo/IME, mixed pivots/history, atomic refusals, queued scope, style/graph and native preservation passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Selection rotation browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
