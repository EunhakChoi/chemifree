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
import { pngFixture, screenshotPixels } from './lib/web-png-fixture.mjs'

const work = mkdtempSync(join(tmpdir(), 'chemifree-image-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'image-browser-create', title: 'Image drawing' }) })
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
  const png=pngFixture();const data=png.toString('base64')
  const fixture=`
import React,{useEffect} from 'react'
import {createRoot} from 'react-dom/client'
import {EmbeddedImagePanel} from '/src/components/embedded-image-panel.tsx'
import {SceneClipboardPanel} from '/src/components/scene-clipboard-panel.tsx'
import {SceneOrderBoundaryButtons} from '/src/components/scene-order-boundary-buttons.tsx'
import {SelectionRotationForm} from '/src/components/selection-rotation-form.tsx'
import {StructureCanvas} from '/src/editor/structure-canvas.tsx'
import {sceneGeometryCommands} from '/src/editor/scene-geometry.ts'
import {useDocumentEditor} from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness(){
 const editor=useDocumentEditor();useEffect(()=>{void editor.open(${JSON.stringify(id)})},[])
 window.imageHarness={editor}
 const image=editor.current?.document.scene.objects.find((o)=>o.kind==='image')
 const group=image&&editor.current?.document.scene.objects.find((o)=>o.kind==='group'&&o.children.includes(image.id))
 return <main>
  <button onClick={()=>editor.setSelection(new Set())}>Clear selection</button>
  <button disabled={!image||editor.busy} onClick={()=>editor.setSelection(new Set([image.id]))}>Select image</button>
  <button disabled={!image||editor.busy} onClick={()=>void editor.apply([{op:'group',objects:['s2',image.id]},{op:'add_reference',source:image.id,target:{kind:'object',id:'s2'}}])}>Group image and line</button>
  <button disabled={!group||editor.busy} onClick={()=>editor.setSelection(new Set([group.id]))}>Select image group</button>
  <button disabled={!image||editor.busy} onClick={()=>void editor.apply((latest)=>sceneGeometryCommands(latest,new Set([image.id,'s3']),{kind:'arrange',mode:'align_left'}))}>Align image and rectangle</button>
  <button disabled={!image||editor.busy} onClick={()=>void editor.apply([{op:'delete_objects',objects:[image.id],mode:'subtree'}])}>Delete image</button>
  <button disabled={editor.busy} onClick={()=>void editor.undo()}>Undo image</button>
  <button disabled={editor.busy} onClick={()=>void editor.redo()}>Redo image</button>
  <button disabled={editor.busy} onClick={()=>void editor.save()}>Save image</button>
  <output className="block break-all" aria-label="Image snapshot">{JSON.stringify(editor.current)}</output>
  <output aria-label="Image busy">{String(editor.busy)}</output>
  <output aria-label="Image error">{editor.error?.message}</output>
  {editor.current&&<>
   <EmbeddedImagePanel document={editor.current.document} editor={editor}/>
   <SceneOrderBoundaryButtons document={editor.current.document} editor={editor}/>
   <SelectionRotationForm key={editor.current.document.id} document={editor.current.document} editor={editor} activePlacement="s1"/>
   <SceneClipboardPanel document={editor.current.document} editor={editor}/>
   <div style={{width:800,height:550}}><StructureCanvas document={editor.current.document} tool="select" serviceGeneration={editor.serviceGeneration} element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={()=>{}} onCommands={(commands)=>void editor.apply(commands)} onMark={()=>{}} disabled={editor.busy}/></div></>}
 </main>
}
createRoot(document.getElementById('root')).render(<Harness/> )
`
  server=await createServer({root:fileURLToPath(new URL('../apps/web',import.meta.url)),configFile:fileURLToPath(new URL('../apps/web/vite.config.ts',import.meta.url)),server:{host:'127.0.0.1',port:5173,strictPort:true,proxy:{'/api':base}},
    plugins:[{name:'image-fixture',configureServer(vite){vite.middlewares.use('/image-check',async(_request,response)=>{response.setHeader('content-type','text/html');response.end(await vite.transformIndexHtml('/image-check','<!doctype html><html><head><title>Image</title></head><body><div id="root"></div><script type="module" src="/image-harness.tsx"></script></body></html>'))})},resolveId(path){if(path==='/image-harness.tsx')return path},load(path){if(path==='/image-harness.tsx')return fixture}}],
  })
  await server.listen()
  browser=await chromium.launch({executablePath:process.env.CHEMIFREE_BROWSER,headless:true})
  const page=await browser.newPage({viewport:{width:1400,height:1000},colorScheme:'light'})
  const errors=[];const writes=[]
  page.on('pageerror',(error)=>errors.push(error.message))
  page.on('request',(request)=>{if(request.method()==='POST'&&request.url().endsWith('/transactions'))writes.push(request.postDataJSON())})
  await page.addInitScript(()=>{window.imageClipboard={text:'',writes:[],reads:[]};window.imageFileReads=0;const originalRead=File.prototype.arrayBuffer;File.prototype.arrayBuffer=function(){window.imageFileReads++;const result=originalRead.call(this);if(window.deferImageRead){window.deferImageRead=false;return new Promise((resolve,reject)=>{window.releaseImageRead=()=>result.then(resolve,reject)})}return result};Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText(text){window.imageClipboard.writes.push({text,active:navigator.userActivation.isActive});window.imageClipboard.text=text;return Promise.resolve()},readText(){window.imageClipboard.reads.push(navigator.userActivation.isActive);return Promise.resolve(window.imageClipboard.text)}}})})
  const button=(name)=>page.getByRole('button',{name,exact:true})
  const snapshot=async()=>JSON.parse(await page.getByLabel('Image snapshot',{exact:true}).textContent())
  const idle=()=>page.waitForFunction(()=>document.querySelector('[aria-label="Image busy"]').textContent==='false')
  const canvas=page.locator('svg[data-structure-canvas]')
  const graphic=(id)=>page.locator('[data-scene-object="'+id+'"]')
  async function revision(value){await page.waitForFunction((value)=>JSON.parse(document.querySelector('[aria-label="Image snapshot"]').textContent).document.revision===value,value);await idle()}
  async function screenPoint(point){await canvas.scrollIntoViewIfNeeded();return canvas.evaluate((svg,point)=>{const at=new DOMPoint(point.x,point.y).matrixTransform(svg.getScreenCTM());return{x:at.x,y:at.y}},point)}
  async function click(point){const at=await screenPoint(point);const box=await canvas.boundingBox();assert.equal(at.x>=box.x&&at.x<=box.x+box.width&&at.y>=box.y&&at.y<=box.y+box.height,true,'test click must be inside the visible canvas: '+JSON.stringify({point,at,box}));const hit=await canvas.evaluate((svg,at)=>{const node=document.elementFromPoint(at.x,at.y);return{inside:svg.contains(node),tag:node?.tagName,object:node?.closest('[data-scene-object]')?.getAttribute('data-scene-object')}},at);assert.equal(hit.inside,true,'test pointer target must belong to the canvas: '+JSON.stringify({point,at,box,hit}));await page.mouse.click(at.x,at.y)}
  async function begin(from,to){const a=await screenPoint(from);const b=await screenPoint(to);await page.mouse.move(a.x,a.y);await page.mouse.down();await page.mouse.move(b.x,b.y,{steps:4})}
  function graphSame(document){for(const key of ['atoms','bonds','stereo','molecule'])assert.deepEqual(document[key],seeded[key]);assert.deepEqual(document.scene.instances,seeded.scene.instances)}
  const add=()=>page.getByRole('form',{name:'Add PNG image',exact:true})
  const edit=()=>page.getByRole('form',{name:'Apply image geometry',exact:true})
  const image=(id)=>page.locator('[data-scene-image="'+id+'"]')
  async function undo(){const d=(await snapshot()).document;await button('Undo image').click();await revision(d.revision+1)}
  async function redo(){const d=(await snapshot()).document;await button('Redo image').click();await revision(d.revision+1)}
  async function choose(buffer,name='drawing.png',mimeType='image/png'){await add().getByLabel('PNG file',{exact:true}).setInputFiles({name,mimeType,buffer})}
  async function rectangle(form,origin,size){for(const[label,value]of [['Image origin x (pt)',origin.x],['Image origin y (pt)',origin.y],['Image width (pt)',size.x],['Image height (pt)',size.y]])await form.getByRole('textbox',{name:label,exact:true}).fill(String(value))}
  const currentImage=async(id)=>(await snapshot()).document.scene.objects.find((o)=>o.id===id)
  async function painted(id){const mark=image(id).locator('image');await mark.waitFor();assert.equal(await mark.getAttribute('href'),'data:image/png;base64,'+data);assert.equal(await mark.getAttribute('preserveAspectRatio'),'none');const pixels=screenshotPixels(await mark.screenshot());assert.deepEqual(pixels.at(Math.floor(pixels.width/8),Math.floor(pixels.height/2)),[255,0,0],'the actual embedded bitmap paints opaque red pixels');assert.deepEqual(pixels.at(Math.floor(pixels.width*7/8),Math.floor(pixels.height/2)),[255,255,255],'transparent PNG pixels reveal the card background')}
  stage='file/MIME/size/pixel preflight never writes or decodes a candidate image'
  await page.goto('http://127.0.0.1:5173/image-check');await page.waitForFunction(()=>!!window.imageHarness.editor.current);await button('Reset zoom (100%)').click()
  const original=await snapshot();const originalWrites=writes.length
  await choose(Buffer.alloc(128*1024+1),'too-large.png');await page.getByRole('alert').filter({hasText:'128 KiB'}).waitFor();assert.equal(await page.evaluate(()=>window.imageFileReads),0)
  await choose(png,'wrong.svg','image/svg+xml');await page.getByRole('alert').filter({hasText:'image/png'}).waitFor();assert.equal(await page.evaluate(()=>window.imageFileReads),0)
  await choose(pngFixture({headerWidth:1000000,headerHeight:1000000}),'huge-header.png');await page.getByRole('alert').filter({hasText:'static'}).waitFor();assert.equal(await page.locator('svg image').count(),0);assert.equal(writes.length,originalWrites);assert.deepEqual((await snapshot()).document,original.document)
  stage='a superseded async file read cannot replace the latest file or cross document context'
  await page.evaluate(()=>window.deferImageRead=true);await choose(png,'old-read.png');await page.getByRole('status').filter({hasText:'Reading PNG file…'}).waitFor();await choose(png,'latest.svg','image/svg+xml');await page.getByRole('alert').filter({hasText:'image/png'}).waitFor();await page.evaluate(()=>window.releaseImageRead());assert.equal(await add().getByRole('button',{name:'Add PNG image',exact:true}).isDisabled(),true);assert.equal(await page.getByRole('status').filter({hasText:'old-read.png: image/png'}).count(),0)
  const other=await fetch(base+'/api/v1/documents',{method:'POST',headers,body:JSON.stringify({request_id:crypto.randomUUID(),title:'Other image drawing'})});assert.equal(other.status,201);const otherId=(await other.json()).document.id
  await page.evaluate(()=>window.deferImageRead=true);await choose(png,'old-document.png');await page.getByRole('status').filter({hasText:'Reading PNG file…'}).waitFor();await page.evaluate((id)=>void window.imageHarness.editor.open(id),otherId);await page.waitForFunction((id)=>window.imageHarness.editor.current?.document.id===id,otherId);await page.evaluate(()=>window.releaseImageRead());assert.equal(await add().getByRole('button',{name:'Add PNG image',exact:true}).isDisabled(),true);assert.equal(await page.getByRole('status').filter({hasText:'old-document.png: image/png'}).count(),0);assert.equal(await page.locator('svg image').count(),0);await page.evaluate((id)=>void window.imageHarness.editor.open(id),id);await page.waitForFunction((id)=>window.imageHarness.editor.current?.document.id===id,id);await idle();assert.equal(writes.length,originalWrites)
  stage='server full validation refuses corrupt CRC without decoding or writing an image'
  const corrupt=Buffer.from(png);corrupt[corrupt.length-1]^=1;await choose(corrupt,'corrupt.png');await page.getByRole('status').filter({hasText:'corrupt.png: image/png'}).waitFor();await rectangle(add(),{x:50,y:60},{x:40,y:20});const rejected=page.waitForResponse((r)=>r.url().endsWith('/transactions'));await add().getByRole('button',{name:'Add PNG image',exact:true}).click();const refused=await rejected;assert.equal(refused.status(),422);assert.equal((await refused.json()).reason,'image_invalid');await page.getByLabel('Image error',{exact:true}).filter({hasText:/./}).waitFor();await idle();assert.equal(await page.locator('svg image').count(),0);assert.deepEqual((await snapshot()).document,original.document);assert.equal((await snapshot()).session.undo_depth,original.session.undo_depth)
  stage='explicit pt geometry and IME guards precede one successful embedded-image creation'
  await choose(png);await page.getByRole('status').filter({hasText:'drawing.png: image/png'}).waitFor();assert.match(await add().textContent(),/compressed bytes \(limit 128 KiB\).*2 × 2 pixels/)
  await rectangle(add(),{x:99999,y:60},{x:40,y:20});assert.equal(await add().getByRole('button',{name:'Add PNG image',exact:true}).isDisabled(),true)
  await rectangle(add(),{x:50,y:60},{x:40,y:20});const width=add().getByRole('textbox',{name:'Image width (pt)',exact:true});await width.dispatchEvent('compositionstart');const imeWrites=writes.length;await add().evaluate((form)=>form.requestSubmit());assert.equal(writes.length,imeWrites);assert.equal(await add().getByRole('button',{name:'Add PNG image',exact:true}).isDisabled(),true);await width.dispatchEvent('compositionend')
  const beforeCreate=(await snapshot()).document;await add().getByRole('button',{name:'Add PNG image',exact:true}).click();await revision(beforeCreate.revision+1);const created=(await snapshot()).document.scene.objects.find((o)=>o.kind==='image');const iid=created.id
  assert.deepEqual(writes.at(-1).commands,[{op:'add_image',mime_type:'image/png',data_base64:data,origin:{x:50,y:60},size_pt:{x:40,y:20}}]);assert.equal(created.parent,null);assert.equal(created.pixel_width,2);assert.equal(created.pixel_height,2);graphSame((await snapshot()).document);await painted(iid)
  await undo();assert.equal(await page.locator('svg image').count(),0);await redo();assert.deepEqual(await currentImage(iid),created)
  stage='transparent pixels have a full rectangular hit area, selection outline and rigid move preview at zoom'
  await button('Zoom in').click();await button('Clear selection').click();const transparent=await screenPoint({x:85,y:70});assert.equal(await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.getAttribute('data-image-hit'),transparent),iid);await page.mouse.click(transparent.x,transparent.y);await page.waitForFunction((id)=>window.imageHarness.editor.selection.has(id),iid);assert.equal(await image(iid).locator('[data-image-selection]').count(),1)
  const beforeMove=(await snapshot()).document;const moveWrites=writes.length;await begin({x:85,y:70},{x:95,y:75});const mark=image(iid).locator('image');assert.equal(Number(await mark.getAttribute('x')),60);assert.equal(Number(await mark.getAttribute('y')),65);assert.equal(await mark.getAttribute('href'),'data:image/png;base64,'+data);assert.deepEqual((await snapshot()).document,beforeMove);assert.equal(writes.length,moveWrites);await page.mouse.up();await revision(beforeMove.revision+1);assert.deepEqual((await currentImage(iid)).origin,{x:60,y:65});assert.equal((await currentImage(iid)).data_base64,data);graphSame((await snapshot()).document);await undo()
  stage='marquee contains the complete image rectangle, including transparent pixels'
  await button('Reset zoom (100%)').click();await button('Clear selection').click();await begin({x:45,y:55},{x:95,y:75});await page.mouse.up();await page.waitForFunction((id)=>!window.imageHarness.editor.selection.has(id),iid);await begin({x:45,y:55},{x:95,y:85});await page.mouse.up();await page.waitForFunction((id)=>window.imageHarness.editor.selection.has(id),iid)
  stage='numeric image edit preserves bytes and derived dimensions; invalid/no-op drafts send nothing'
  await button('Select image').click();assert.equal(await edit().getByRole('button',{name:'Apply image geometry',exact:true}).isDisabled(),true);await rectangle(edit(),{x:-100000,y:-100000},{x:200000,y:200000});assert.equal(await edit().getByRole('button',{name:'Apply image geometry',exact:true}).isDisabled(),false);await rectangle(edit(),{x:50,y:60},{x:0,y:20});assert.equal(await edit().getByRole('button',{name:'Apply image geometry',exact:true}).isDisabled(),true);await rectangle(edit(),{x:60,y:65},{x:30,y:25});let before=(await snapshot()).document;await edit().getByRole('button',{name:'Apply image geometry',exact:true}).click();await revision(before.revision+1);assert.deepEqual(await currentImage(iid),{...created,origin:{x:60,y:65},size_pt:{x:30,y:25}});await undo()
  stage='creation queue freezes bytes/rectangles and rejects earlier-revision intents'
  before=(await snapshot()).document;const queueWrites=writes.length;await page.evaluate((data)=>{const editor=window.imageHarness.editor;const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,dataBase64:data,origin:{x:100,y:50},sizePt:{x:10,y:10}};void editor.apply([{op:'rename',title:'Earlier queued image edit'}]);void editor.createImage(creation)},data);await revision(before.revision+1);assert.equal(writes.length,queueWrites+1);assert.equal((await snapshot()).document.scene.objects.filter((o)=>o.kind==='image').length,1)
  before=(await snapshot()).document;await page.evaluate((data)=>{const editor=window.imageHarness.editor;const creation={documentId:editor.current.document.id,revision:editor.current.document.revision,serviceGeneration:editor.serviceGeneration,dataBase64:data,origin:{x:100,y:50},sizePt:{x:10,y:10}};void editor.createImage(creation);creation.dataBase64='changed';creation.origin.x=999;creation.sizePt.x=999},data);await revision(before.revision+1);assert.deepEqual(writes.at(-1).commands,[{op:'add_image',mime_type:'image/png',data_base64:data,origin:{x:100,y:50},size_pt:{x:10,y:10}}]);await undo()
  stage='image rectangle participates in rigid alignment, group motion, reference retention and SVG paint order'
  before=(await snapshot()).document;await button('Align image and rectangle').click();await revision(before.revision+1);assert.equal((await currentImage(iid)).origin.x,50);assert.equal((await snapshot()).document.scene.objects.find((o)=>o.id==='s3').points[0].x,50);graphSame((await snapshot()).document);await undo()
  before=(await snapshot()).document;await button('Group image and line').click();await revision(before.revision+1);await button('Select image group').click();const grouped=(await snapshot()).document;const group=grouped.scene.objects.find((o)=>o.kind==='group'&&o.children.includes(iid));const refs=grouped.scene.references
  const beforeGroupMove=(await snapshot()).document;await begin({x:85,y:70},{x:90,y:75});assert.equal(Number(await image(iid).locator('image').getAttribute('x')),55);assert.equal(Number(await graphic('s2').locator('line.stroke-foreground').getAttribute('x1')),5);await page.mouse.up();await revision(beforeGroupMove.revision+1);assert.equal((await currentImage(iid)).data_base64,data);assert.deepEqual((await snapshot()).document.scene.references,refs);graphSame((await snapshot()).document);await undo()
  await button('Select image').click();before=(await snapshot()).document;await page.getByRole('group',{name:'Scene order boundaries',exact:true}).getByRole('button',{name:'First',exact:true}).click();await revision(before.revision+1);assert.deepEqual((await snapshot()).document.scene.objects.find((o)=>o.id===group.id).children,[iid,'s2']);assert.deepEqual(await canvas.locator('[data-scene-object]').evaluateAll((nodes,ids)=>nodes.map((n)=>n.getAttribute('data-scene-object')).filter((id)=>ids.includes(id)),[iid,'s2']),[iid,'s2']);before=(await snapshot()).document;await page.getByRole('group',{name:'Scene order boundaries',exact:true}).getByRole('button',{name:'Last',exact:true}).click();await revision(before.revision+1);assert.deepEqual((await snapshot()).document.scene.objects.find((o)=>o.id===group.id).children,['s2',iid]);assert.deepEqual((await snapshot()).document.scene.references,refs)
  stage='all image rotations/reflections and image-bearing clipboard forests refuse atomically, with no silent omission'
  await button('Select image group').click();await page.getByRole('textbox',{name:'Rotation angle (degrees)',exact:true}).fill('45');assert.equal(await button('Rotate').isDisabled(),true)
  const refusalBefore=await snapshot()
  for(const transform of [{type:'rotate',degrees:0},{type:'rotate',degrees:45},{type:'reflect_vertical'}]){const reply=await fetch(base+'/api/v1/documents/'+id+'/transactions',{method:'POST',headers,body:JSON.stringify({request_id:crypto.randomUUID(),expected_revision:refusalBefore.document.revision,commands:[{op:'transform_selection',selection:{objects:[group.id],atoms:['a1'],bonds:[]},instance:'s1',pivot:{x:0,y:0},transform}]})});assert.equal(reply.status,422);assert.equal((await reply.json()).reason,'image_unsupported')}
  for(const action of ['Copy graphics…','Cut graphics…']){const response=page.waitForResponse((r)=>r.url().endsWith('/operations')&&r.request().postDataJSON()?.operation==='document.clipboard_scene_copy');await button(action).click();const result=await(await response).json();assert.equal(result.outcome,'error');assert.equal(result.diagnostics[0].details.reason,'image_clipboard_unsupported');await page.getByRole('alert').waitFor();assert.equal(await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue(),'');await button('Close').click();assert.deepEqual((await snapshot()).document,refusalBefore.document);assert.equal((await snapshot()).session.undo_depth,refusalBefore.session.undo_depth)}assert.equal((await page.evaluate(()=>window.imageClipboard)).writes.length,0)
  await page.evaluate(()=>window.imageHarness.editor.setSelection(new Set(['s2'])));const ordinaryCopy=page.waitForResponse((r)=>r.url().endsWith('/operations')&&r.request().postDataJSON()?.operation==='document.clipboard_scene_copy');await button('Copy graphics…').click();const ordinaryResult=await(await ordinaryCopy).json();assert.equal(ordinaryResult.outcome,'ok','unselected images do not block ordinary graphic copying');await page.waitForFunction(()=>document.querySelector('textarea')?.value.includes('chemifree.clipboard'));const ordinaryPayload=JSON.parse(await page.getByRole('textbox',{name:'Native graphics text',exact:true}).inputValue());assert.equal(ordinaryPayload.version,2);assert.deepEqual(ordinaryPayload.objects.map((o)=>o.kind),['graphic']);await button('Close').click();assert.deepEqual((await snapshot()).document,refusalBefore.document)
  stage='delete and undo restore the complete embedded object, order and references'
  await button('Select image').click();before=(await snapshot()).document;const deleting=page.waitForResponse((r)=>r.url().endsWith('/transactions'));await button('Delete image').click();const deleted=await deleting;assert.equal(deleted.status(),200,await deleted.text());assert.deepEqual(deleted.request().postDataJSON().commands,[{op:'delete_objects',objects:[iid],mode:'subtree'}]);await revision(before.revision+1);assert.equal(await page.locator('svg image').count(),0);graphSame((await snapshot()).document);await undo();assert.deepEqual((await snapshot()).document.scene,before.scene);await painted(iid)
  stage='native30 save/process restart preserves exact PNG and refuses stale image creation replay'
  await button('Save image').click();await page.waitForFunction(()=>JSON.parse(document.querySelector('[aria-label="Image snapshot"]').textContent).session.unsaved_changes===false);const saved=(await snapshot()).document;assert.equal(JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8')).format_version,30)
  await choose(png);await page.getByRole('status').filter({hasText:'drawing.png: image/png'}).waitFor();await rectangle(add(),{x:100,y:50},{x:10,y:10})
  const exited=once(api,'exit');api.kill('SIGTERM');await exited;api=spawn(resolve(metadata.target_directory,'debug',process.platform==='win32'?'chemifree-api.exe':'chemifree-api'),['--port',new URL(base).port],{env,stdio:['ignore','ignore','pipe']});await new Promise((ready,reject)=>{const timer=setTimeout(()=>reject(new Error('Replacement API did not start')),15000);api.once('error',(error)=>{clearTimeout(timer);reject(error)});api.stderr.on('data',(data)=>{const line=data.toString();log+=line;if(line.includes('listening on ')){clearTimeout(timer);ready()}})})
  const restartWrites=writes.length;await add().getByRole('button',{name:'Add PNG image',exact:true}).click();await page.getByLabel('Image error',{exact:true}).filter({hasText:/./}).waitFor();await idle();assert.equal(writes.length,restartWrites+1,'expired image creation is never replayed');assert.deepEqual((await snapshot()).document,saved)
  await page.reload();await page.waitForFunction(()=>!!window.imageHarness.editor.current);assert.deepEqual((await snapshot()).document.scene,saved.scene);assert.equal((await currentImage(iid)).data_base64,data);await painted(iid)
  stage='actual workspace exposes file/pt controls and the saved embedded image without external resources'
  await page.goto('http://127.0.0.1:5173/');await page.locator('li[data-document-id="'+id+'"]').getByRole('button',{name:'Open',exact:true}).click();await page.getByRole('region',{name:'Embedded PNG images',exact:true}).waitFor();await button('Fit drawing').click();await image(iid).locator('image').waitFor();assert.equal(await image(iid).locator('image').getAttribute('href'),'data:image/png;base64,'+data);await choose(png,'workspace.png');await page.getByRole('status').filter({hasText:'workspace.png: image/png'}).waitFor();await rectangle(add(),{x:120,y:50},{x:10,y:20});const workspaceCreate=page.waitForResponse((r)=>r.url().endsWith('/transactions'));await add().getByRole('button',{name:'Add PNG image',exact:true}).click();const added=await workspaceCreate;assert.equal(added.status(),200);graphSame((await added.json()).document)
  await page.setViewportSize({width:320,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,'file/point controls fit a compact viewport');assert.deepEqual(errors,[]);passed=true
  console.log('Embedded static PNG preflight, actual opaque/transparent pixels, rectangular hit/move/group/order, geometry/Undo/native30 and typed clipboard/transform refusals passed.')
} finally {
  await browser?.close();await server?.close()
  if(api?.pid&&api.exitCode===null&&api.signalCode===null){const exited=once(api,'exit');api.kill('SIGTERM');const force=setTimeout(()=>api.kill('SIGKILL'),5000);await exited;clearTimeout(force)}
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error('Image browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
