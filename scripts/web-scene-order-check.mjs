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

const work = mkdtempSync(join(tmpdir(), 'chemifree-scene-order-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'scene-order-browser-create', title: 'Scene order' }) })
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
    ...['line','arrow','rectangle','line','arrow','rectangle'].map((shape,index)=>({op:'add_graphic',shape,points:[{x:40,y:index*5},{x:70,y:index*5+10}]})),
    {op:'group',objects:['s5','s6','s7']},
    {op:'add_caption',anchor:{x:40,y:40},alignment:'left',runs:[{text:'Paint order',script:'baseline',weight:'bold',style:'normal'}]},
    {op:'add_graphic',shape:'line',points:[{x:40,y:40},{x:70,y:40}]},
    {op:'group',objects:['s8','s9','s10']},
  ])
  await transact(seeded.revision,[
    {op:'set_graphic_line_width',object:'s2',line_width_pt:.1},
    {op:'set_graphic_line_width',object:'s6',line_width_pt:10},
    {op:'add_reference',source:'s2',target:{kind:'atom',id:'a1'}},
    {op:'add_reference',source:'s7',target:{kind:'object',id:'s6'}},
  ])
  const otherCreate=await fetch(base+'/api/v1/documents',{method:'POST',headers,body:JSON.stringify({request_id:'order-other',title:'Other scene order'})})
  assert.equal(otherCreate.status,201)
  const otherId=(await otherCreate.json()).document.id
  const otherResponse=await fetch(base+'/api/v1/documents/'+otherId+'/transactions',{method:'POST',headers,body:JSON.stringify({request_id:'order-other-objects',expected_revision:0,commands:['line','arrow','rectangle'].map((shape)=>({op:'add_graphic',shape,points:[{x:0,y:0},{x:10,y:10}]}))})})
  assert.equal(otherResponse.status,200)
  const otherBefore=(await otherResponse.json()).document
  const fixture=`
import React,{useEffect} from 'react'
import {createRoot} from 'react-dom/client'
import {SceneOrderBoundaryButtons} from '/src/components/scene-order-boundary-buttons.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor()
 useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.orderHarness=editor
 return <main>
  <button onClick={()=>editor.setSelection(new Set())}>Select none</button>
  <button onClick={()=>editor.setSelection(new Set(['s2','s3']))}>Select multiple</button>
  <button onClick={()=>editor.setSelection(new Set(['a1','b1']))}>Select graph only</button>
  <button onClick={()=>editor.setSelection(new Set(['s3','a1','b1']))}>Select mixed root arrow</button>
  <button onClick={()=>editor.setSelection(new Set(['s3']))}>Select root arrow</button>
  <button onClick={()=>editor.setSelection(new Set(['s11']))}>Select root group</button>
  <button onClick={()=>editor.setSelection(new Set(['s6']))}>Select child arrow</button>
  <button onClick={()=>editor.setSelection(new Set(['s8']))}>Select nested group</button>
  <button onClick={()=>void editor.apply([{op:'add_graphic',shape:'line',points:[{x:0,y:30},{x:30,y:30}]}])}>Append root</button>
  <button onClick={()=>void editor.apply([{op:'group',objects:['s2','s3']}])}>Group root pair</button>
  <button onClick={()=>void editor.apply([{op:'ungroup',group:'s8'}])}>Ungroup nested</button>
  <button onClick={()=>void editor.apply([{op:'delete_objects',objects:['s3'],mode:'subtree'}])}>Delete root arrow</button>
  <button onClick={()=>void editor.apply((latest)=>[{op:'reorder',object:'s3',index:latest.scene.roots.length-1}])}>Direct last</button>
  <button onClick={()=>void editor.apply([{op:'rename',title:'Queued order title'}])}>Rename order</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo order</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo order</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save order</button>
  <button disabled={editor.busy} onClick={()=>void editor.open(${JSON.stringify(id)})}>Open order</button>
  <output aria-label="Order snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Order busy">{String(editor.busy)}</output>
  <output aria-label="Order selection">{JSON.stringify([...editor.selection].sort())}</output>
  {editor.current&&<><SceneOrderBoundaryButtons document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:450}}><StructureCanvas document={editor.current.document} tool="select" element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'scene-order-fixture',configureServer(vite){vite.middlewares.use('/order-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/order-check','<!doctype html><html><head><title>Scene order</title></head><body><div id="root"></div><script type="module" src="/order-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/order-harness.tsx')return path},load(path){if(path==='/order-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1280,height:1000}})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const orderButton=(name)=>page.getByRole('group',{name:'Scene order boundaries',exact:true}).getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Order snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Order busy"]').textContent==='false')
  async function revision(value){await page.waitForFunction((value)=>JSON.parse(document.querySelector('[aria-label="Order snapshot"]').textContent).document.revision===value,value);await idle()}
  const read=async(docId)=>(await(await fetch(base+'/api/v1/documents/'+docId)).json()).document
  async function selected(name,ids){await button(name).click();await page.waitForFunction((ids)=>document.querySelector('[aria-label="Order selection"]').textContent===JSON.stringify(ids.sort()),ids)}
  function preorder(scene){const byId=new Map(scene.objects.map((object)=>[object.id,object]));const result=[];const visit=(id)=>{const object=byId.get(id);result.push(object);if(object.kind==='group')object.children.forEach(visit)};scene.roots.forEach(visit);return result}
  const clean=({revision:_revision,selection:_selection,...document})=>document
  function orderOnly(before,after,objectId,boundary){
    const expected=structuredClone(before);const object=expected.scene.objects.find((object)=>object.id===objectId)
    const siblings=object.parent===null?expected.scene.roots:expected.scene.objects.find((item)=>item.id===object.parent).children
    siblings.splice(siblings.indexOf(objectId),1);siblings.splice(boundary==='First'?0:siblings.length,0,objectId)
    expected.scene.objects=preorder(expected.scene)
    assert.deepEqual(clean(after),clean(expected),'only this sibling list and its canonical traversal order change; metadata, geometry, graph, refs and subtrees stay exact')
    assert.equal(after.revision,before.revision+1)
  }
  async function paint(document){const expected=preorder(document.scene).filter((object)=>object.kind!=='group').map((object)=>object.id);await page.waitForFunction((expected)=>{const svg=document.querySelector('svg[data-structure-canvas]');return svg&&JSON.stringify([...svg.children].map((item)=>item.getAttribute('data-scene-object')??item.getAttribute('data-instance')).filter(Boolean))===JSON.stringify(expected)},expected)}
  async function move(name,objectId,index){const before=await snapshot();const reply=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await orderButton(name).click();const response=await reply;assert.equal(response.status(),200,await response.text());assert.deepEqual(response.request().postDataJSON().commands,[{op:'reorder',object:objectId,index}]);await revision(before.document.revision+1);const after=await snapshot();orderOnly(before.document,after.document,objectId,name);assert.equal(after.session.undo_depth,before.session.undo_depth+1);await paint(after.document);return{before,after}}
  async function noop(name){const before=await snapshot();const count=writes.length;assert.equal(await orderButton(name).isDisabled(),true);await orderButton(name).evaluate((button)=>button.click());await idle();assert.equal(writes.length,count);assert.deepEqual((await snapshot()).document,before.document);assert.equal((await snapshot()).session.undo_depth,before.session.undo_depth)}
  async function undo(){const before=(await snapshot()).document;await button('Undo order').click();await revision(before.revision+1);await paint((await snapshot()).document)}
  async function redo(){const before=(await snapshot()).document;await button('Redo order').click();await revision(before.revision+1);await paint((await snapshot()).document)}
  async function held(op){let release;let arrived;const gate=new Promise((resolve)=>{release=resolve});const started=new Promise((resolve)=>{arrived=resolve});const handler=async(route)=>{if(!route.request().postDataJSON().commands.some((command)=>command.op===op))return route.fallback();const response=await route.fetch();assert.equal(response.status(),200);arrived();await gate;await route.fulfill({response})};await page.route('**/transactions',handler);return{release,started,done:()=>page.unroute('**/transactions',handler)}}
  stage='selection and root boundaries with exact no-op/history behavior'
  await page.goto('http://127.0.0.1:5173/order-check');await page.waitForFunction(()=>!!window.orderHarness.current)
  for(const [name,ids]of[['Select none',[]],['Select multiple',['s2','s3']],['Select graph only',['a1','b1']]]){await selected(name,ids);await noop('First');await noop('Last')}
  await selected('Select mixed root arrow',['s3','a1','b1'])
  const first=await move('First','s3',0);await noop('First');const last=await move('Last','s3',4);await noop('Last')
  await undo();assert.deepEqual((await snapshot()).document.scene,first.after.document.scene)
  assert.deepEqual((await snapshot()).document.selection,{objects:['s3'],atoms:['a1'],bonds:['b1']},'history keeps the press-time graph selection without reordering it')
  await undo();assert.deepEqual((await snapshot()).document.scene,first.before.document.scene)
  await redo();await redo();assert.deepEqual((await snapshot()).document.scene,last.after.document.scene);await undo();await undo()
  stage='root group and nested group boundaries retain entire subtrees and paint order'
  await selected('Select root group',['s11']);await noop('Last');await move('First','s11',0);await move('Last','s11',4)
  await selected('Select child arrow',['s6']);const child=await move('First','s6',0);await noop('First');await move('Last','s6',2);await noop('Last');await undo();await undo();assert.deepEqual((await snapshot()).document.scene,child.before.document.scene)
  await selected('Select nested group',['s8']);await noop('First');await move('Last','s8',2);await move('First','s8',0)
  stage='queued Last uses the latest same-parent sibling count and keeps later live selection'
  await selected('Select root arrow',['s3']);const appendBefore=(await snapshot()).document;const append=await held('add_graphic')
  await button('Append root').click();await append.started;await orderButton('Last').click();await page.evaluate(()=>window.orderHarness.setSelection(new Set(['s2'])));append.release();await revision(appendBefore.revision+2);await append.done()
  assert.deepEqual(writes.at(-1).commands,[{op:'reorder',object:'s3',index:5}]);assert.equal((await snapshot()).document.scene.roots.at(-1),'s3')
  assert.equal(await page.getByLabel('Order selection',{exact:true}).textContent(),'["s2"]')
  await paint((await snapshot()).document);await undo();await undo();assert.deepEqual((await snapshot()).document.scene,appendBefore.scene)
  stage='a preceding reorder reaching the boundary produces no second transaction'
  await selected('Select root arrow',['s3']);const boundaryBefore=(await snapshot()).document;const boundaryWrites=writes.length;const boundary=await held('reorder')
  await button('Direct last').click();await boundary.started;await orderButton('Last').click();boundary.release();await revision(boundaryBefore.revision+1);await boundary.done();assert.equal(writes.length,boundaryWrites+1);await undo()
  stage='reparented roots and children lapse instead of following a new parent'
  await selected('Select root arrow',['s3']);const groupBefore=(await snapshot()).document;const groupWrites=writes.length;const group=await held('group')
  await button('Group root pair').click();await group.started;await orderButton('Last').click();group.release();await revision(groupBefore.revision+1);await group.done();assert.equal(writes.length,groupWrites+1);assert.notEqual((await snapshot()).document.scene.objects.find((object)=>object.id==='s3').parent,null);await undo();assert.deepEqual((await snapshot()).document.scene,groupBefore.scene)
  await selected('Select child arrow',['s6']);const parentBefore=(await snapshot()).document;const parentWrites=writes.length;const parent=await held('ungroup')
  await button('Ungroup nested').click();await parent.started;await orderButton('Last').click();parent.release();await revision(parentBefore.revision+1);await parent.done();assert.equal(writes.length,parentWrites+1);assert.equal((await snapshot()).document.scene.objects.find((object)=>object.id==='s6').parent,'s11');await undo();assert.deepEqual((await snapshot()).document.scene,parentBefore.scene)
  stage='deleted targets lapse without an index or target substitution'
  await selected('Select root arrow',['s3']);const deleteBefore=(await snapshot()).document;const deleteWrites=writes.length;const deletion=await held('delete_objects')
  await button('Delete root arrow').click();await deletion.started;await orderButton('Last').click();deletion.release();await revision(deleteBefore.revision+1);await deletion.done();assert.equal(writes.length,deleteWrites+1);assert.equal((await snapshot()).document.scene.objects.some((object)=>object.id==='s3'),false);await undo();assert.deepEqual((await snapshot()).document.scene,deleteBefore.scene)
  stage='document changes lapse even when the other document reuses object and parent IDs'
  await selected('Select root arrow',['s3']);const docBefore=(await snapshot()).document;const docWrites=writes.length;const rename=await held('rename')
  await button('Rename order').click();await rename.started
  await page.evaluate((id)=>{void window.orderHarness.open(id);document.querySelector('[aria-label="Scene order boundaries"] button:last-child').click()},otherId)
  rename.release();await page.waitForFunction((id)=>window.orderHarness.current?.document.id===id,otherId);await idle();await rename.done();assert.equal(writes.length,docWrites+1);assert.deepEqual(await read(otherId),otherBefore)
  await button('Open order').click();await page.waitForFunction((id)=>window.orderHarness.current?.document.id===id,id);await idle();await undo();assert.deepEqual((await snapshot()).document.scene,docBefore.scene)
  stage='nontrivial root and group order survives native save/restart and paints in that order'
  await selected('Select root group',['s11']);await move('First','s11',0);await selected('Select child arrow',['s6']);await move('First','s6',0)
  await button('Save order').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Order snapshot"]').textContent).session.unsaved_changes===false)
  const saved=(await snapshot()).document;assert.equal(JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8')).format_version,30)
  const exited=once(api,'exit');api.kill('SIGTERM');await exited
  api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']})
  await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  await page.reload();await page.waitForFunction(()=>!!window.orderHarness.current);assert.deepEqual((await snapshot()).document.scene,saved.scene);await paint(saved)
  stage='actual Scene panel includes First/Last alongside Earlier/Later'
  await page.goto('http://127.0.0.1:5173/');await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click()
  assert.equal(await button('Select scene object s6').getAttribute('aria-pressed'),'true');assert.equal(await button('Earlier').isDisabled(),true);assert.equal(await button('Later').isEnabled(),true);assert.equal(await orderButton('First').isDisabled(),true)
  const actual=page.waitForResponse((response)=>response.url().endsWith('/transactions'));await orderButton('Last').click();const actualResponse=await actual;assert.equal(actualResponse.status(),200);assert.deepEqual(actualResponse.request().postDataJSON().commands,[{op:'reorder',object:'s6',index:2}]);const after=(await actualResponse.json()).document;orderOnly(saved,after,'s6','Last');await paint(after)
  assert.deepEqual(errors,[]);passed=true
  console.log('Scene First/Last root/group/subtree order, paint sequence, queued siblings/selection, parent/doc/deletion stale, no-op/history and native preservation passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Scene order browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
