// Real abbreviation panel, canvas, editor queue, HTTP application and native store. No chemical inference.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

const work = mkdtempSync(join(tmpdir(), 'chemifree-display-label-'))
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
    { op: 'add_atom', element: 'C', position: { x: 275, y: 125 }, bond_to: { atom: '$1', order: 'single' } },
  ] }) })
  assert.equal(seed.status, 200)
  const original = (await seed.json()).document
  const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { AbbreviationPanel } from '/src/components/abbreviation-panel.tsx'
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
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['a2', 'a3']))}>Select ethyl pair</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['b1']))}>Select boundary bond</button>
    <button disabled={editor.busy} onClick={() => void editor.apply([{ op: 'update_atom', atom: 'a3', charge: 1 }])}>Charge hidden terminal</button>
    <button disabled={editor.busy} onClick={() => void editor.apply([{op:'update_settings',label_weight:'bold',label_style:'italic',label_underline:true,label_outline:true,label_shadow:true}])}>Style label</button>
    <output aria-label="Et snapshot">{JSON.stringify(editor.current)}</output>
    <output aria-label="Et selection">{JSON.stringify([...editor.selection])}</output>
    <output aria-label="Et error">{editor.error?.message}</output>
    {editor.current && <>
      <AbbreviationPanel document={editor.current.document} editor={editor} />
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
  const compact = (text = 'Et') => page.getByRole('img', { name: `Display label ${text} at a2; text does not define chemistry; underlying ethyl graph`, exact: true })
  async function documentPoint(x, y) {
    return page.locator('svg[data-structure-canvas]').evaluate((svg, at) => {
      const point = svg.createSVGPoint()
      point.x = at.x; point.y = at.y
      const screen = point.matrixTransform(svg.getScreenCTM())
      return { x: screen.x, y: screen.y }
    }, { x, y })
  }
  stage = 'edit Unicode display text through the production form'
  await page.goto('http://127.0.0.1:5173/abbreviation-check')
  await button('Select ethyl pair').click()
  await button('Collapse Et at a2').click()
  await compact().waitFor()
  const text = 'Ph 한 e\u0301 🧪'
  const input = () => page.getByRole('textbox', { name: 'Display label at a2', exact: true })
  await input().fill(text)
  await button('Apply display label').click()
  await compact(text).waitFor()
  const custom = (await snapshot()).document
  assert.equal(custom.abbreviations[0].display_label, text)
  for (const field of ['atoms', 'bonds', 'scene', 'stereo', 'settings']) assert.deepEqual(custom[field], original[field])
  assert.equal(await page.locator('[data-atom-label="a3"]').count(), 0)
  async function inkInsideSharedBox() {
    await page.locator('svg[data-caption-bounds-ready="true"]').waitFor()
    const measured = await compact(text).evaluate((group) => {
      const ink=group.querySelector('text[data-label-metric-key]').getBBox()
      const [x,y,width,height]=group.getAttribute('data-box').split(' ').map(Number)
      return { ink:{x:ink.x,y:ink.y,width:ink.width,height:ink.height}, box:{x,y,width,height} }
    })
    assert.ok(measured.ink.x >= measured.box.x && measured.ink.y >= measured.box.y)
    assert.ok(measured.ink.x+measured.ink.width <= measured.box.x+measured.box.width)
    assert.ok(measured.ink.y+measured.ink.height <= measured.box.y+measured.box.height)
  }
  await inkInsideSharedBox()
  await compact(text).click()
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection', {exact:true}).textContent()).sort(), ['a2','a3','b2'])
  stage = 'default styling and decorated styling share actual Unicode ink bounds'
  await button('Style label').click()
  await inkInsideSharedBox()
  assert.deepEqual((await snapshot()).document.atoms, original.atoms)
  stage = 'invalid input and reset have explicit presentation semantics'
  await input().fill(' ')
  assert.equal(await button('Apply display label').isDisabled(), true)
  await input().fill('한'.repeat(43))
  assert.equal(await button('Apply display label').isDisabled(), true)
  await input().fill(text)
  await button('Reset display label to Et').click()
  await compact().waitFor()
  await button('Undo Et').click()
  await compact(text).waitFor()
  stage = 'graph mutation expands and undo restores exact custom text'
  await button('Charge hidden terminal').click()
  await compact(text).waitFor({state:'detached'})
  assert.equal((await snapshot()).document.atoms[2].charge,1)
  await button('Undo Et').click()
  await compact(text).waitFor()
  assert.deepEqual((await snapshot()).document.atoms,original.atoms)
  stage = 'v23 native save and reopen retain text and full graph'
  await button('Save Et drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Et snapshot"]').textContent).session.unsaved_changes === false)
  const native=JSON.parse(readFileSync(join(work,'documents',`${id}.json`),'utf8'))
  assert.equal(native.format_version,27)
  assert.deepEqual(native.document.display_labels,[{instance:'s1',anchor:'a2',text}])
  await page.reload()
  await compact(text).waitFor()
  await inkInsideSharedBox()
  assert.deepEqual((await snapshot()).document.atoms, original.atoms)
  await button('Expand underlying ethyl at a2').click()
  await compact(text).waitFor({state:'detached'})
  assert.deepEqual((await snapshot()).document.abbreviations,[])
  assert.deepEqual((await snapshot()).document.atoms,original.atoms)
  assert.equal(await page.getByLabel('Et error',{exact:true}).textContent(),'')
  assert.deepEqual(errors,[])
  passed=true
  console.log('Custom display label production form, Unicode ink, history and native roundtrip passed.')
} finally {
  await browser?.close()
  await server?.close()
  if (api?.pid && api.exitCode === null && api.signalCode === null) {
    const exited = once(api, 'exit')
    api.kill('SIGTERM')
    const force = setTimeout(() => api.kill('SIGKILL'), 5_000)
    await exited
    clearTimeout(force)
  }
  if (passed) rmSync(work, { recursive: true, force: true })
  else console.error(`Et browser failed at ${stage}; artifacts: ${work}\n${log}`)
}
