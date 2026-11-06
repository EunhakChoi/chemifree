// Real caption panel, canvas, editor queue, HTTP application and native store. No chemical inference.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

const work = mkdtempSync(join(tmpdir(), 'chemifree-caption-'))
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
  const create = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'caption-browser-create', title: 'Unicode caption' }) })
  assert.equal(create.status, 201)
  const id = (await create.json()).document.id
  const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { CaptionPanel } from '/src/components/caption-panel.tsx'
import { StructureCanvas } from '/src/editor/structure-canvas.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open(${JSON.stringify(id)}) }, [])
  return <main>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo caption</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo caption</button>
    <button disabled={editor.busy} onClick={() => void editor.save()}>Save caption drawing</button>
    <button disabled={editor.busy} onClick={() => void editor.reload()}>Reload caption drawing</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['s2']))}>Select caption</button>
    <output aria-label="Caption snapshot">{JSON.stringify(editor.current)}</output>
    <output aria-label="Caption error">{editor.error?.message}</output>
    {editor.current && <>
      <CaptionPanel key={editor.current.document.id + ':' + editor.serviceGeneration} document={editor.current.document} editor={editor} />
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
    plugins: [{ name: 'caption-check', configureServer(vite) {
      vite.middlewares.use('/caption-check', async (_request, response) => {
        response.setHeader('content-type', 'text/html')
        response.end(await vite.transformIndexHtml('/caption-check', '<!doctype html><html><head><title>Caption check</title></head><body><div id="root"></div><script type="module" src="/caption-harness.tsx"></script></body></html>'))
      })
    }, resolveId(path) { if (path === '/caption-harness.tsx') return path }, load(path) { if (path === '/caption-harness.tsx') return fixture } }],
  })
  await server.listen()
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  const button = (name) => page.getByRole('button', { name, exact: true })
  const snapshot = async () => JSON.parse(await page.getByLabel('Caption snapshot', { exact: true }).textContent())
  const caption = async () => (await snapshot()).document.scene.objects.find((object) => object.kind === 'caption')
  stage = 'create explicit Unicode runs through the production form'
  await page.goto('http://127.0.0.1:5173/caption-check')
  await button('Add caption…').click()
  const text = '한글 e\u0301 👩‍🔬 H'
  await page.getByLabel('Run 1 text', { exact: true }).fill(text)
  await button('Add run').click()
  await page.getByLabel('Run 2 text', { exact: true }).fill('2')
  await page.getByRole('group', { name: 'Run 2 script', exact: true }).getByRole('button', { name: 'Subscript', exact: true }).click()
  await button('Add run').click()
  await page.getByLabel('Run 3 text', { exact: true }).fill('+')
  await page.getByRole('group', { name: 'Run 3 script', exact: true }).getByRole('button', { name: 'Superscript', exact: true }).click()
  await button('Create caption').click()
  await page.locator('[data-caption-text="s2"]').waitFor()
  const expected = [{ text, script: 'baseline' }, { text: '2', script: 'subscript' }, { text: '+', script: 'superscript' }]
  assert.deepEqual((await caption()).runs, expected)
  stage = 'measure actual glyph and script bounds with the loaded fonts'
  await page.waitForFunction(() => {
    const group = document.querySelector('[data-caption]')
    const text = group?.querySelector('text')
    const hit = group?.querySelector('rect')
    if (!text || !hit) return false
    const ink = text.getBBox()
    const box = hit.getBBox()
    const svg = group.ownerSVGElement.viewBox.baseVal
    return ink.width > 30 && box.x <= ink.x && box.y <= ink.y && box.x + box.width >= ink.x + ink.width && box.y + box.height >= ink.y + ink.height && svg.x <= box.x && svg.y <= box.y && svg.x + svg.width >= box.x + box.width && svg.y + svg.height >= box.y + box.height
  })
  const scriptY = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => Number(node.getAttribute('y'))))
  assert.deepEqual(scriptY, [0, 4, -5])
  stage = 'edit through selection and preserve one undo step'
  await page.getByRole('img', { name: `Caption s2: ${text}2+`, exact: true }).click()
  await button('Edit selected caption…').click()
  await page.getByLabel('Run 1 text', { exact: true }).fill('NFC é and NFD e\u0301')
  await button('Apply caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text]')?.textContent === 'NFC é and NFD e\u03012+')
  await button('Undo caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+`, text)
  assert.deepEqual((await caption()).runs, expected)
  await button('Redo caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text]')?.textContent === 'NFC é and NFD e\u03012+')
  stage = 'save, reload and reopen the exact native runs'
  const edited = await caption()
  await button('Save caption drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Caption snapshot"]').textContent)?.session.unsaved_changes === false)
  const native = JSON.parse(readFileSync(join(work, 'documents', `${id}.json`), 'utf8'))
  assert.equal(native.format_version, 9)
  assert.deepEqual(native.document.objects.find((object) => object.id === 's2').state.runs, edited.runs)
  await page.reload()
  await page.locator('[data-caption-text="s2"]').waitFor()
  assert.deepEqual(await caption(), edited)
  assert.equal(await page.getByLabel('Caption error', { exact: true }).textContent(), '')
  assert.deepEqual(errors, [])
  passed = true
  console.log('Caption production form/canvas/native roundtrip passed.')
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
  else console.error(`Caption browser failed at ${stage}; artifacts: ${work}\n${log}`)
}
