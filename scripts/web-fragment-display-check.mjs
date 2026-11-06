// Real fragment form, canvas, request queue, HTTP application and native store. Display text is literal.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

const work = mkdtempSync(join(tmpdir(), 'chemifree-fragment-display-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'abbreviation-browser-create', title: 'Unicode abbreviation' }) })
  assert.equal(create.status, 201)
  const id = (await create.json()).document.id
  const seed = await fetch(`${base}/api/v1/documents/${id}/transactions`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'ethyl-browser-seed', expected_revision: 0, commands: [
    { op: 'add_atom', element: 'O', position: { x: 100, y: 100 } },
    { op: 'add_atom', element: 'C', position: { x: 260, y: 100 }, bond_to: { atom: '$0', order: 'single' } },
    { op: 'add_atom', element: 'O', position: { x: 300, y: 140 }, bond_to: { atom: '$1', order: 'single' } },
    { op: 'add_atom', element: 'N', position: { x: 300, y: 60 }, bond_to: { atom: '$1', order: 'single' } },
  ] }) })
  assert.equal(seed.status, 200)
  const original = (await seed.json()).document
  const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { FragmentDisplayPanel } from '/src/components/fragment-display-panel.tsx'
import { StructureCanvas } from '/src/editor/structure-canvas.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open(${JSON.stringify(id)}) }, [])
  return <main>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo Et</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo Et</button>
    <button disabled={editor.busy} onClick={() => void editor.save()}>Save Et drawing</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a2', 'a3', 'a4']))}>Select fragment atoms</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['b1']))}>Select boundary bond</button>
    <button disabled={editor.busy} onClick={() => void editor.apply([{ op: 'update_atom', atom: 'a3', charge: 1 }])}>Charge hidden terminal</button>
    <button disabled={editor.busy} onClick={() => void editor.apply([{op:'update_settings',label_size_pt:24,label_weight:'bold',label_style:'italic',label_underline:true,label_outline:true,label_shadow:true}])}>Style label</button>
    <output aria-label="Et snapshot">{JSON.stringify(editor.current)}</output>
    <output aria-label="Et selection">{JSON.stringify([...editor.selection])}</output>
    <output aria-label="Et error">{editor.error?.message}</output>
    {editor.current && <>
      <FragmentDisplayPanel document={editor.current.document} editor={editor} />
      <div style={{ width: 800, height: 550 }}><StructureCanvas document={editor.current.document} tool="select" element="C" bondKind="single"
        selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={() => {}}
        onCommands={(commands, selected) => void editor.apply(commands, selected ? { kind: 'gesture', before: selected } : undefined)}
        onMark={() => {}} disabled={editor.busy} /></div>
    </>}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
  server = await createServer({
    root: fileURLToPath(new URL('../apps/web', import.meta.url)),
    configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
    server: { host: '127.0.0.1', port: 5173, strictPort: true, proxy: { '/api': base } },
    plugins: [{ name: 'abbreviation-check', configureServer(vite) {
      vite.middlewares.use('/abbreviation-check', async (_request, response) => {
        response.setHeader('content-type', 'text/html')
        response.end(await vite.transformIndexHtml('/abbreviation-check', '<!doctype html><html><head><title>Et check</title></head><body><div id="root"></div><script type="module" src="/abbreviation-harness.tsx"></script></body></html>'))
      })
    }, resolveId(path) { if (path === '/abbreviation-harness.tsx') return path }, load(path) { if (path === '/abbreviation-harness.tsx') return fixture } }],
  })
  await server.listen()
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  const button = (name) => page.getByRole('button', { name, exact: true })
  const snapshot = async () => JSON.parse(await page.getByLabel('Et snapshot', { exact: true }).textContent())
  const compact = (text = 'Et') => page.getByRole('img', { name: `Display label ${text} at a2; text does not define chemistry; retained fragment graph`, exact: true })
  async function documentPoint(x, y) {
    return page.locator('svg[data-structure-canvas]').evaluate((svg, at) => {
      const point = svg.createSVGPoint()
      point.x = at.x; point.y = at.y
      const screen = point.matrixTransform(svg.getScreenCTM())
      return { x: screen.x, y: screen.y }
    }, { x, y })
  }
  stage = 'explicit fragment selection, anchor and Unicode display text'
  await page.goto('http://127.0.0.1:5173/abbreviation-check')
  await button('Select fragment atoms').click()
  const text = 'Ph 한 e\u0301 🧪'
  await page.getByRole('textbox', {name:'Fragment display text', exact:true}).fill(text)
  await button('Contract fragment at a2').click()
  await compact(text).waitFor()
  const input = () => page.getByRole('textbox', {name:'Fragment display at a2', exact:true})
  const custom = (await snapshot()).document
  assert.deepEqual(custom.fragment_displays,[{instance:'s1',anchor:'a2',members:['a2','a3','a4'],text}])
  for (const field of ['atoms','bonds','scene','stereo','settings']) assert.deepEqual(custom[field],original[field])
  assert.equal(await page.locator('[data-atom-label="a3"],[data-atom-label="a4"]').count(),0)
  async function inkInsideSharedBox(labelText) {
    await page.locator('svg[data-caption-bounds-ready="true"]').waitFor()
    const measured = await compact(labelText).evaluate((group) => {
      const ink=group.querySelector('text[data-label-metric-key]').getBBox()
      const [x,y,width,height]=group.getAttribute('data-box').split(' ').map(Number)
      return {ink:{x:ink.x,y:ink.y,width:ink.width,height:ink.height},box:{x,y,width,height}}
    })
    assert.ok(measured.ink.x >= measured.box.x && measured.ink.y >= measured.box.y)
    assert.ok(measured.ink.x+measured.ink.width <= measured.box.x+measured.box.width)
    assert.ok(measured.ink.y+measured.ink.height <= measured.box.y+measured.box.height)
  }
  await inkInsideSharedBox(text)
  const hiddenPoint=await documentPoint(300,140)
  await page.mouse.click(hiddenPoint.x,hiddenPoint.y)
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection',{exact:true}).textContent()),[],'hidden member centers cannot select the fragment')
  await compact(text).click()
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection',{exact:true}).textContent()).sort(),['a2','a3','a4','b2','b3'])
  stage = 'label drag moves every retained member with a common delta'
  const beforeMove=(await snapshot()).document.scene.instances[0].positions
  const at=await documentPoint(260,100)
  const destination=await documentPoint(270,110)
  await page.mouse.move(at.x,at.y)
  await page.mouse.down()
  await page.mouse.move(destination.x,destination.y,{steps:4})
  await page.mouse.up()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Et snapshot"]').textContent).document.revision > 2)
  const afterMove=(await snapshot()).document.scene.instances[0].positions
  for (const id of ['a2','a3','a4']) {
    const before=beforeMove.find((item)=>item.atom===id)
    const after=afterMove.find((item)=>item.atom===id)
    assert.ok(Math.abs(after.x-before.x-10)<0.1)
    assert.ok(Math.abs(after.y-before.y-10)<0.1)
  }
  assert.deepEqual(afterMove.find((item)=>item.atom==='a1'),beforeMove.find((item)=>item.atom==='a1'))
  await button('Undo Et').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Et snapshot"]').textContent).document.scene.instances[0].positions.find((p)=>p.atom==='a2').x===260)
  stage = 'Unicode metrics at a larger decorated size and literal edit'
  await button('Style label').click()
  await inkInsideSharedBox(text)
  assert.deepEqual((await snapshot()).document.atoms,original.atoms)
  await input().fill(' ')
  assert.equal(await button('Apply fragment display text').isDisabled(),true)
  await input().fill('한'.repeat(43))
  assert.equal(await button('Apply fragment display text').isDisabled(),true)
  const edited='Et'
  await input().fill(edited)
  await button('Apply fragment display text').click()
  await compact(edited).waitFor()
  await inkInsideSharedBox(edited)
  assert.deepEqual((await snapshot()).document.atoms,original.atoms,'Et text does not replace retained O/N atoms')
  stage = 'explicit expand, undo and bounded wrong-anchor refusal'
  await button('Expand retained fragment at a2').click()
  await compact(edited).waitFor({state:'detached'})
  assert.deepEqual((await snapshot()).document.fragment_displays,[])
  assert.deepEqual((await snapshot()).document.atoms,original.atoms)
  await button('Undo Et').click()
  await compact(edited).waitFor()
  await button('Save Et drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Et snapshot"]').textContent).session.unsaved_changes===false)
  const native=JSON.parse(readFileSync(join(work,'documents',id+'.json'),'utf8'))
  assert.equal(native.format_version,24)
  await page.reload()
  await compact(edited).waitFor()
  await inkInsideSharedBox(edited)
  assert.deepEqual((await snapshot()).document.atoms,original.atoms)
  assert.deepEqual((await snapshot()).document.fragment_displays,[{instance:'s1',anchor:'a2',members:['a2','a3','a4'],text:edited}])
  await button('Expand retained fragment at a2').click()
  await compact(edited).waitFor({state:'detached'})
  await button('Select fragment atoms').click()
  await page.getByRole('textbox',{name:'Fragment display text',exact:true}).fill('literal')
  const beforeFailure=(await snapshot()).document
  await button('Contract fragment at a3').click()
  await page.getByLabel('Et error',{exact:true}).filter({hasText:/./}).waitFor()
  assert.deepEqual((await snapshot()).document,beforeFailure)
  assert.deepEqual(errors,[])
  passed=true
  console.log('Fragment display production form, retained graph, rigid drag, Unicode metrics, refusal and native roundtrip passed.')
} finally {
  await browser?.close()
  await server?.close()
  if (api?.pid && api.exitCode === null && api.signalCode === null) {
    const exited=once(api,'exit')
    api.kill('SIGTERM')
    const force=setTimeout(()=>api.kill('SIGKILL'),5000)
    await exited
    clearTimeout(force)
  }
  if (passed) rmSync(work,{recursive:true,force:true})
  else console.error('Fragment browser failed at '+stage+'; artifacts: '+work+'\n'+log)
}
