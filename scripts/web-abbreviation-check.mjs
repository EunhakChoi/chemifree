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

const work = mkdtempSync(join(tmpdir(), 'chemifree-abbreviation-'))
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
    { op: 'add_atom', element: 'C', position: { x: 130, y: 100 }, bond_to: { atom: '$0', order: 'single' } },
    { op: 'add_atom', element: 'C', position: { x: 145, y: 125 }, bond_to: { atom: '$1', order: 'single' } },
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
  const compact = () => page.getByRole('img', { name: 'Et abbreviation at a2; retained ethyl graph', exact: true })
  async function documentPoint(x, y) {
    return page.locator('svg[data-structure-canvas]').evaluate((svg, at) => {
      const point = svg.createSVGPoint()
      point.x = at.x; point.y = at.y
      const screen = point.matrixTransform(svg.getScreenCTM())
      return { x: screen.x, y: screen.y }
    }, { x, y })
  }
  stage = 'collapse the existing pair through the production panel'
  await page.goto('http://127.0.0.1:5173/abbreviation-check')
  await button('Select ethyl pair').click()
  await button('Collapse Et at a2').click()
  await compact().waitFor()
  const collapsed = (await snapshot()).document
  assert.deepEqual(collapsed.atoms, original.atoms)
  assert.deepEqual(collapsed.bonds, original.bonds)
  assert.deepEqual(collapsed.scene, original.scene)
  assert.equal(await page.locator('[data-bond="b2"]').count(), 0)
  stage = 'Shift-click immediately deselects a collapsed atom pair without an explicit internal bond'
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection', { exact: true }).textContent()).sort(), ['a2', 'a3'])
  await compact().click({ modifiers: ['Shift'] })
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection', { exact: true }).textContent()), [])
  await compact().click({ modifiers: ['Shift'] })
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection', { exact: true }).textContent()).sort(), ['a2', 'a3', 'b2'])
  await compact().click({ modifiers: ['Shift'] })
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection', { exact: true }).textContent()), [])
  assert.deepEqual((await snapshot()).document, collapsed, 'selection toggles do not edit graph, display or history')
  stage = 'hidden terminal and bond have no ink or hit'
  const hidden = await documentPoint(145, 125)
  await page.mouse.click(hidden.x, hidden.y)
  assert.deepEqual(JSON.parse(await page.getByLabel('Et selection', { exact: true }).textContent()), [])
  stage = 'covered boundary selection still selects the entire compact pair'
  await button('Select boundary bond').click()
  await compact().click()
  const targets = JSON.parse(await page.getByLabel('Et selection', { exact: true }).textContent())
  assert.deepEqual(targets.sort(), ['a2', 'a3', 'b1', 'b2'])
  const start = await documentPoint(130, 100)
  const end = await documentPoint(140, 105)
  await page.mouse.move(start.x, start.y)
  await page.mouse.down()
  await page.mouse.move(end.x, end.y, { steps: 8 })
  await page.mouse.up()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Et snapshot"]').textContent).document.revision === 3)
  const moved = (await snapshot()).document
  assert.deepEqual(moved.abbreviations, collapsed.abbreviations)
  assert.ok(Math.abs(moved.atoms[1].position.x - 140) < 1 && Math.abs(moved.atoms[1].position.y - 105) < 1)
  assert.ok(Math.abs(moved.atoms[2].position.x - moved.atoms[1].position.x - 15) < 1e-9)
  assert.ok(Math.abs(moved.atoms[2].position.y - moved.atoms[1].position.y - 25) < 1e-9)
  stage = 'natural text remains inside the reserved label box and external bond stays outside'
  const bounds = await compact().evaluate((group) => {
    const text = group.querySelector('text').getBBox()
    const [x, y, width, height] = group.getAttribute('data-box').split(' ').map(Number)
    const box = { x, y, width, height }
    return { text: { x: text.x, y: text.y, width: text.width, height: text.height }, box }
  })
  assert.ok(bounds.text.x >= bounds.box.x && bounds.text.y >= bounds.box.y)
  assert.ok(bounds.text.x + bounds.text.width <= bounds.box.x + bounds.box.width)
  assert.ok(bounds.text.y + bounds.text.height <= bounds.box.y + bounds.box.height)
  stage = 'a hidden member edit expands and undo restores compact presentation'
  await button('Charge hidden terminal').click()
  await compact().waitFor({ state: 'detached' })
  assert.equal((await snapshot()).document.atoms[2].charge, 1)
  await button('Undo Et').click()
  await compact().waitFor()
  assert.equal((await snapshot()).document.atoms[2].charge, 0)
  stage = 'v9 save and page reopen retain the display and complete stored geometry'
  await button('Save Et drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Et snapshot"]').textContent).session.unsaved_changes === false)
  const native = JSON.parse(readFileSync(join(work, 'documents', `${id}.json`), 'utf8'))
  assert.equal(native.format_version, 15)
  assert.deepEqual(native.document.abbreviations, [{ instance: 's1', anchor: 'a2', terminal: 'a3' }])
  await page.reload()
  await compact().waitFor()
  assert.deepEqual((await snapshot()).document.atoms, moved.atoms)
  await button('Expand Et at a2').click()
  await compact().waitFor({ state: 'detached' })
  assert.equal(await page.locator('[data-bond="b2"]').count(), 1)
  assert.deepEqual((await snapshot()).document.atoms, moved.atoms)
  assert.equal(await page.getByLabel('Et error', { exact: true }).textContent(), '')
  assert.deepEqual(errors, [])
  passed = true
  console.log('Et production panel/canvas/native roundtrip passed.')
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
