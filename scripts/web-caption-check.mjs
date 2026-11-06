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
let page
const errors = []

// Print the actual SVG and font state to stderr: isolated verifier temp files may be removed.
async function captionDiagnostics(target) {
  if (!target || target.isClosed()) return
  try {
    const diagnostic = await target.evaluate(() => {
      const box = (rect) => ({ x: rect.x, y: rect.y, width: rect.width, height: rect.height })
      return {
        fontsStatus: document.fonts.status,
        fonts: [...document.fonts].map((font) => ({ family: font.family, status: font.status })),
        snapshot: document.querySelector('[aria-label="Caption snapshot"]')?.textContent,
        editorError: document.querySelector('[aria-label="Caption error"]')?.textContent,
        canvases: [...document.querySelectorAll('[data-structure-canvas]')].map((canvas) => ({
          viewBox: canvas.getAttribute('viewBox'), scale: canvas.dataset.viewportScale,
          ready: canvas.dataset.captionBoundsReady, pixels: box(canvas.getBoundingClientRect()),
        })),
        captions: [...document.querySelectorAll('[data-caption-text]')].map((text) => ({
          id: text.getAttribute('data-caption-text'), text: text.textContent,
          ink: box(text.getBBox()), hit: text.parentElement.querySelector('rect') ? box(text.parentElement.querySelector('rect').getBBox()) : null,
          pixels: box(text.getBoundingClientRect()), font: getComputedStyle(text).fontFamily,
          spans: [...text.querySelectorAll('tspan')].map((span) => ({ script: span.dataset.captionScript, y: span.getAttribute('y'), size: span.getAttribute('font-size'), ink: box(span.getBBox()) })),
        })),
      }
    })
    console.error('Caption UI diagnostics:', JSON.stringify({ stage, errors, ...diagnostic }))
  } catch (diagnosticError) {
    console.error('Caption UI diagnostics unavailable:', diagnosticError)
  }
}

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
  const alternateCreate = await fetch(`${base}/api/v1/documents`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'caption-browser-alternate', title: 'Alternate caption' }) })
  assert.equal(alternateCreate.status, 201)
  const alternateId = (await alternateCreate.json()).document.id
  const alternateEdit = await fetch(`${base}/api/v1/documents/${alternateId}/transactions`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'caption-browser-alternate-edit', expected_revision: 0, commands: [{ op: 'add_caption', anchor: { x: -2400, y: -900 }, runs: [{ text: 'Alternate 한글 caption outside the old frame', script: 'baseline' }] }] }) })
  assert.equal(alternateEdit.status, 200)
  const alternateSave = await fetch(`${base}/api/v1/documents/${alternateId}/save`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'caption-browser-alternate-save', expected_revision: 1 }) })
  assert.equal(alternateSave.status, 200)
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
    <button disabled={editor.busy} onClick={() => void editor.open(${JSON.stringify(id)})}>Open original caption drawing</button>
    <button disabled={editor.busy} onClick={() => void editor.open(${JSON.stringify(alternateId)})}>Open alternate caption drawing</button>
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
  page = await browser.newPage()
  page.on('pageerror', (error) => errors.push(error.message))
  const button = (name) => page.getByRole('button', { name, exact: true })
  const snapshot = async () => JSON.parse(await page.getByLabel('Caption snapshot', { exact: true }).textContent())
  const caption = async () => (await snapshot()).document.scene.objects.find((object) => object.kind === 'caption')
  stage = 'create explicit Unicode runs through the production form'
  await page.goto('http://127.0.0.1:5173/caption-check')
  await button('Fit drawing').click()
  const beforeCreateViewBox = await page.locator('[data-structure-canvas]').getAttribute('viewBox')
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
  await page.locator('[data-structure-canvas][data-caption-bounds-ready="true"]').waitFor()
  assert.equal(await page.locator('[data-structure-canvas]').getAttribute('viewBox'), beforeCreateViewBox, 'creating a caption preserves the chosen viewport')
  // The new baseline is at y=0, so its ascenders can be above the existing frame.
  // Editing does not auto-fit: use the same explicit control offered by the product.
  await button('Fit drawing').click()
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
  const editedText = 'NFC é and NFD e\u0301 '.repeat(8)
  await page.getByLabel('Run 1 text', { exact: true }).fill(editedText)
  await page.getByLabel('Caption horizontal position (pt)', { exact: true }).fill('1200')
  await page.getByLabel('Caption vertical position (pt)', { exact: true }).fill('450')
  await button('Apply caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+`, editedText)
  await button('Undo caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+`, text)
  assert.deepEqual((await caption()).runs, expected)
  await button('Redo caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+`, editedText)
  stage = 'save, reload and reopen the exact native runs'
  const edited = await caption()
  await button('Save caption drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Caption snapshot"]').textContent)?.session.unsaved_changes === false)
  const native = JSON.parse(readFileSync(join(work, 'documents', `${id}.json`), 'utf8'))
  assert.equal(native.format_version, 8)
  assert.deepEqual(native.document.objects.find((object) => object.id === 's2').state.runs, edited.runs)
  await page.reload()
  await page.locator('[data-caption-text="s2"]').waitFor()
  assert.deepEqual(await caption(), edited)
  async function fullyVisible(target) {
    await target.waitForFunction(() => {
      const text = document.querySelector('[data-caption-text]')
      if (!text) return false
      const canvas = text.ownerSVGElement
      if (canvas.dataset.captionBoundsReady !== 'true') return false
      const ink = text.getBBox()
      const view = canvas.viewBox.baseVal
      const drawn = text.getBoundingClientRect()
      const frame = canvas.getBoundingClientRect()
      return ink.width > 0 && view.x <= ink.x && view.y <= ink.y && view.x + view.width >= ink.x + ink.width && view.y + view.height >= ink.y + ink.height && drawn.left >= frame.left - 1 && drawn.right <= frame.right + 1 && drawn.top >= frame.top - 1 && drawn.bottom <= frame.bottom + 1
    })
  }
  // Reopening must fit actual caption ink, not merely add it to an unused content-bound value.
  await fullyVisible(page)
  stage = 'document switches get one measured fit for the current document'
  await button('Open alternate caption drawing').click()
  await page.waitForFunction((id) => JSON.parse(document.querySelector('[aria-label="Caption snapshot"]').textContent)?.document.id === id, alternateId)
  await fullyVisible(page)
  await button('Open original caption drawing').click()
  await page.waitForFunction((id) => JSON.parse(document.querySelector('[aria-label="Caption snapshot"]').textContent)?.document.id === id, id)
  await fullyVisible(page)

  // Hold real font HTTP replies on cold pages: the actual SVG/font measurement remains pending.
  // No React, caption layout or viewport implementation is replaced by the fixture.
  async function fontRace(userNavigatesViewport) {
    const cold = await browser.newPage()
    cold.on('pageerror', (error) => errors.push(error.message))
    let releaseFonts
    const fontsHeld = new Promise((resolveFonts) => { releaseFonts = resolveFonts })
    let fontRequests = 0
    await cold.route(/\.woff2(?:\?.*)?$/, async (route) => {
      if (route.request().resourceType() !== 'font') return route.continue()
      fontRequests += 1
      await fontsHeld
      await route.continue()
    })
    try {
      await cold.goto('http://127.0.0.1:5173/caption-check', { waitUntil: 'domcontentloaded' })
      await cold.waitForFunction(() => document.fonts.status === 'loading' && document.querySelector('[data-structure-canvas]')?.dataset.captionBoundsReady === 'false' && document.querySelector('[data-caption] rect') !== null)
      assert.ok(fontRequests > 0, 'a real font response must be held')
      if (userNavigatesViewport) {
        const canvas = cold.locator('[data-structure-canvas]')
        await canvas.focus()
        await canvas.press('ArrowRight')
        await canvas.press('+')
        const chosen = await canvas.getAttribute('viewBox')
        releaseFonts()
        await cold.waitForFunction(() => document.querySelector('[data-structure-canvas]')?.dataset.captionBoundsReady === 'true')
        assert.equal(await canvas.getAttribute('viewBox'), chosen, 'late measurements must retain user pan/zoom')
      } else {
        await cold.getByRole('button', { name: 'Open alternate caption drawing', exact: true }).click()
        await cold.waitForFunction((id) => JSON.parse(document.querySelector('[aria-label="Caption snapshot"]').textContent)?.document.id === id && document.querySelector('[data-structure-canvas]')?.dataset.captionBoundsReady === 'false', alternateId)
        releaseFonts()
        await fullyVisible(cold)
        assert.equal(JSON.parse(await cold.getByLabel('Caption snapshot', { exact: true }).textContent()).document.id, alternateId)
      }
    } catch (error) {
      await captionDiagnostics(cold)
      throw error
    } finally {
      releaseFonts()
      await cold.close()
    }
  }
  stage = 'font completion fits the latest document but preserves explicit viewport input'
  await fontRace(false)
  await fontRace(true)

  assert.equal(await page.getByLabel('Caption error', { exact: true }).textContent(), '')
  assert.deepEqual(errors, [])
  passed = true
  console.log('Caption production form/canvas/native roundtrip passed.')
} catch (error) {
  await captionDiagnostics(page)
  throw error
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
