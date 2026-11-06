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
          ready: canvas.dataset.captionBoundsReady, measuredViewBox: canvas.dataset.captionMeasuredViewBox, pixels: box(canvas.getBoundingClientRect()),
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
  const alternateEdit = await fetch(`${base}/api/v1/documents/${alternateId}/transactions`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'caption-browser-alternate-edit', expected_revision: 0, commands: [{ op: 'add_caption', anchor: { x: -2400, y: -900 }, alignment: 'right', runs: [{ text: '  עברית العربية Alternate 한글 caption outside the old frame  ', script: 'baseline' }] }] }) })
  assert.equal(alternateEdit.status, 200)
  const alternateSave = await fetch(`${base}/api/v1/documents/${alternateId}/save`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify({ request_id: 'caption-browser-alternate-save', expected_revision: 1 }) })
  assert.equal(alternateSave.status, 200)
  const fixture = `
import React, { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { CaptionPanel } from '/src/components/caption-panel.tsx'
import { StructureCanvas } from '/src/editor/structure-canvas.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import { useCanvasViewport } from '/src/hooks/use-canvas-viewport.ts'
import '/src/index.css'
const probeContent = {}
function ViewportProbe() {
  const [identity, setIdentity] = useState(0)
  const [measurement, setMeasurement] = useState<{ ready: boolean; width: number; at?: string }>({ ready: false, width: 100 })
  const view = useCanvasViewport('probe-' + identity, { x: 0, y: 0, width: measurement.width, height: 100 },
    { boundsReady: measurement.ready, measuredViewBox: measurement.at, contentIdentity: probeContent })
  return <section aria-label="Viewport measurement boundary">
    <div ref={view.containerRef} style={{ width: 200, height: 100 }}><output aria-label="Probe viewBox">{view.viewBox}</output></div>
    <button onClick={() => setMeasurement({ ready: true, width: 200, at: view.viewBox })}>Probe first measurement</button>
    <button onClick={() => setMeasurement({ ready: true, width: 400, at: view.viewBox })}>Probe post-fit measurement</button>
    <button onClick={() => setMeasurement({ ready: true, width: 800, at: view.viewBox })}>Probe later measurement</button>
    <button onClick={() => view.pan({ x: 10, y: 0 })}>Probe pan</button>
    <button onClick={() => { setMeasurement({ ready: false, width: 100 }); setIdentity((value) => value + 1) }}>Reset probe document</button>
  </section>
}
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open(${JSON.stringify(id)}) }, [])
  return <main>
    <button disabled={editor.busy} onClick={() => void editor.open(${JSON.stringify(id)})}>Open original caption drawing</button>
    <button disabled={editor.busy} onClick={() => void editor.open(${JSON.stringify(alternateId)})}>Open alternate caption drawing</button>
    <button disabled={editor.busy} onClick={() => void editor.apply(() => [{ op: 'update_settings', label_font: 'monospace' }])}>Use monospace drawing font</button>
    <button disabled={editor.busy} onClick={() => void editor.apply(() => [{ op: 'update_settings', label_size_pt: 30 }])}>Use 30pt drawing size</button>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo caption</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo caption</button>
    <button disabled={editor.busy} onClick={() => void editor.save()}>Save caption drawing</button>
    <button disabled={editor.busy} onClick={() => void editor.reload()}>Reload caption drawing</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set(['s2']))}>Select caption</button>
    <button disabled={editor.busy} onClick={() => editor.setSelection(new Set())}>Clear caption selection</button>
    <output aria-label="Caption snapshot">{JSON.stringify(editor.current)}</output>
    <output aria-label="Caption error">{editor.error?.message}</output>
    {editor.current && <>
      <CaptionPanel key={editor.current.document.id + ':' + editor.serviceGeneration} document={editor.current.document} editor={editor} />
      <div style={{ width: 800, height: 550 }}><StructureCanvas document={editor.current.document} tool="select" element="C" bondKind="single"
        selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={() => {}}
        onCommands={(commands, selected) => void editor.apply(commands, selected ? { kind: 'gesture', before: selected } : undefined)}
        onMark={() => {}} disabled={editor.busy} /></div>
    </>}
    <ViewportProbe />
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
  const waitForCaptionMeasurement = () => page.waitForFunction(() => {
    const canvas = document.querySelector('[data-structure-canvas]')
    return canvas?.dataset.captionBoundsReady === 'true' && canvas.dataset.captionMeasuredViewBox === canvas.getAttribute('viewBox')
  })
  const snapshot = async () => JSON.parse(await page.getByLabel('Caption snapshot', { exact: true }).textContent())
  const caption = async () => (await snapshot()).document.scene.objects.find((object) => object.kind === 'caption')
  // Measurement readiness describes the painted document. An in-flight edit can
  // leave the previous document ready; first wait for the committed snapshot and
  // its actual foreground text, then require current-viewport ink measurement.
  const waitForCaptionEdit = async (previousRevision, runs) => {
    await page.waitForFunction(({ revision, runs }) => {
      const output = document.querySelector('[aria-label="Caption snapshot"]')
      if (!output?.textContent) return false
      const current = JSON.parse(output.textContent).document
      const caption = current.scene.objects.find((object) => object.kind === 'caption')
      const text = document.querySelector('[data-caption-text]')
      return current.revision === revision + 1 && JSON.stringify(caption?.runs) === JSON.stringify(runs) &&
        text?.textContent === runs.map((run) => run.text).join('')
    }, { revision: previousRevision, runs })
    await waitForCaptionMeasurement()
  }
  stage = 'create explicit Unicode runs through the production form'
  await page.goto('http://127.0.0.1:5173/caption-check')
  await button('Fit drawing').click()
  const beforeCreateViewBox = await page.locator('[data-structure-canvas]').getAttribute('viewBox')
  await button('Add caption…').click()
  const text = '   한글 e\u0301 👩‍🔬 H'
  await page.getByLabel('Run 1 text', { exact: true }).fill(text)
  await button('Add run').click()
  await page.getByLabel('Run 2 text', { exact: true }).fill('2')
  await page.getByRole('group', { name: 'Run 2 script', exact: true }).getByRole('button', { name: 'Subscript', exact: true }).click()
  await button('Add run').click()
  await page.getByLabel('Run 3 text', { exact: true }).fill('+   ')
  await page.getByRole('group', { name: 'Run 3 script', exact: true }).getByRole('button', { name: 'Superscript', exact: true }).click()
  await page.getByRole('group', { name: 'Run 1 weight', exact: true }).getByRole('button', { name: 'Bold', exact: true }).click()
  await page.getByRole('group', { name: 'Run 2 style', exact: true }).getByRole('button', { name: 'Italic', exact: true }).click()
  await page.getByRole('group', { name: 'Run 3 weight', exact: true }).getByRole('button', { name: 'Bold', exact: true }).click()
  await page.getByRole('group', { name: 'Run 3 style', exact: true }).getByRole('button', { name: 'Italic', exact: true }).click()
  await page.getByRole('group', { name: 'Run 1 font family', exact: true }).getByRole('button', { name: 'Serif', exact: true }).click()
  await page.getByRole('group', { name: 'Run 2 font family', exact: true }).getByRole('button', { name: 'Monospace', exact: true }).click()
  await page.getByRole('group', { name: 'Run 3 font family', exact: true }).getByRole('button', { name: 'Sans-serif', exact: true }).click()
  await page.getByLabel('Run 1 size (pt)', { exact: true }).fill('24')
  await page.getByLabel('Run 2 size (pt)', { exact: true }).fill('18')
  for (const invalid of ['3.99', '72.01', 'NaN', 'Infinity', '12 pt', '0x10', '0b100', '0o10']) {
    await page.getByLabel('Run 1 size (pt)', { exact: true }).fill(invalid)
    assert.equal(await button('Create caption').isDisabled(), true, invalid)
  }
  for (const valid of ['4', '72', '4.25', '71.9999', '12.5', '.5e1']) {
    await page.getByLabel('Run 1 size (pt)', { exact: true }).fill(valid)
    assert.equal(await button('Create caption').isDisabled(), false, valid)
  }
  await page.getByLabel('Run 1 size (pt)', { exact: true }).fill('24')
  for (const color of ['red', '#abc', '#aabbcc', '#12345678', 'rgba(0,0,0,0)', 'var(--foreground)', 'url(javascript:alert(1))']) {
    await page.getByLabel('Run 1 color', { exact: true }).fill(color)
    assert.equal(await button('Create caption').isDisabled(), true, color)
  }
  await page.getByLabel('Run 1 color', { exact: true }).fill('#2A7BCD')
  await page.getByLabel('Run 2 color', { exact: true }).fill('#000000')
  await button('Create caption').click()
  await page.locator('[data-caption-text="s2"]').waitFor()
  const expected = [{ shadow: false, outline: false, underline: false, text, script: 'baseline', weight: 'bold', style: 'normal', font: 'serif', color: '#2A7BCD', size_pt: 24 }, { shadow: false, outline: false, underline: false, text: '2', script: 'subscript', weight: 'normal', style: 'italic', font: 'monospace', color: '#000000', size_pt: 18 }, { shadow: false, outline: false, underline: false, text: '+   ', script: 'superscript', weight: 'bold', style: 'italic', font: 'sans-serif', color: null, size_pt: null }]
  assert.deepEqual((await caption()).runs, expected)
  assert.deepEqual(await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => [node.getAttribute('font-weight'), node.getAttribute('font-style')])), [['bold', 'normal'], ['normal', 'italic'], ['bold', 'italic']])
  const familyStyles = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).fontFamily))
  assert.equal(familyStyles[0], 'serif')
  assert.equal(familyStyles[1], 'monospace')
  assert.ok(familyStyles[2].includes('Geist'), familyStyles[2])
  stage = 'theme foreground inheritance and opaque overrides preserve geometry'
  await waitForCaptionMeasurement()
  const inkBeforeTheme = await page.locator('[data-caption-text]').evaluate((node) => { const b = node.getBBox(); return [b.x, b.y, b.width, b.height] })
  await page.evaluate(() => { document.documentElement.classList.remove('dark'); document.documentElement.style.setProperty('--foreground', '#102030') })
  const lightInk = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).fill))
  await page.evaluate(() => { document.documentElement.classList.add('dark'); document.documentElement.style.setProperty('--foreground', '#E0D0C0') })
  const darkInk = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).fill))
  assert.deepEqual(lightInk, ['rgb(42, 123, 205)', 'rgb(0, 0, 0)', 'rgb(16, 32, 48)'])
  assert.deepEqual(darkInk, ['rgb(42, 123, 205)', 'rgb(0, 0, 0)', 'rgb(224, 208, 192)'])
  assert.deepEqual(await page.locator('[data-caption-text]').evaluate((node) => { const b = node.getBBox(); return [b.x, b.y, b.width, b.height] }), inkBeforeTheme)
  await page.evaluate(() => { document.documentElement.classList.remove('dark'); document.documentElement.style.removeProperty('--foreground') })
  await button('Select caption').click()
  await button('Edit selected caption…').click()
  await page.getByLabel('Run 1 color', { exact: true }).fill('')
  await button('Apply caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text] tspan')?.getAttribute('fill') === null)
  assert.equal((await caption()).runs[0].color, null)
  await button('Undo caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text] tspan')?.getAttribute('fill') === '#2A7BCD')
  assert.deepEqual((await caption()).runs, expected)
  stage = 'family-only edit resets to dynamic inheritance and undo keeps overrides'
  await button('Select caption').click()
  await button('Edit selected caption…').click()
  await page.getByRole('group', { name: 'Run 1 font family', exact: true }).getByRole('button', { name: 'Drawing font', exact: true }).click()
  await button('Apply caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text] tspan')?.getAttribute('font-family') === null && !document.querySelector('[data-caption-text] tspan')?.classList.contains('font-sans'))
  assert.deepEqual((await caption()).runs, [{ ...expected[0], font: null }, ...expected.slice(1)])
  await button('Use monospace drawing font').click()
  await page.waitForFunction(() => getComputedStyle(document.querySelector('[data-caption-text] tspan')).fontFamily === 'monospace')
  const inheritedFamilies = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).fontFamily))
  assert.deepEqual(inheritedFamilies.slice(0, 2), ['monospace', 'monospace'])
  assert.equal(inheritedFamilies[2], familyStyles[2], 'explicit sans stays fixed when the drawing font changes')
  await button('Undo caption').click()
  await page.waitForFunction(() => getComputedStyle(document.querySelector('[data-caption-text] tspan')).fontFamily.includes('Geist'))
  await button('Undo caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text] tspan')?.getAttribute('font-family') === 'serif')
  assert.deepEqual((await caption()).runs, expected)
  stage = 'drawing size updates inheritance while preserving overrides and uses run size for script offsets'
  await button('Use 30pt drawing size').click()
  await page.waitForFunction(() => Number(document.querySelector('[data-caption-text] tspan:last-child')?.getAttribute('font-size')) === 20)
  assert.deepEqual(await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => [Number(node.getAttribute('font-size')), Number(node.getAttribute('y'))])), [[24, 0], [12, 6], [20, -12.5]])
  assert.deepEqual((await caption()).runs, expected)
  await button('Undo caption').click()
  await page.waitForFunction(() => Number(document.querySelector('[data-caption-text] tspan:last-child')?.getAttribute('font-size')) === 8)
  await button('Select caption').click()
  await button('Edit selected caption…').click()
  await page.getByLabel('Run 1 size (pt)', { exact: true }).fill('')
  await button('Apply caption').click()
  await page.waitForFunction(() => Number(document.querySelector('[data-caption-text] tspan')?.getAttribute('font-size')) === 12)
  assert.equal((await caption()).runs[0].size_pt, null)
  await button('Undo caption').click()
  await page.waitForFunction(() => Number(document.querySelector('[data-caption-text] tspan')?.getAttribute('font-size')) === 24)
  assert.deepEqual((await caption()).runs, expected)
  stage = 'measure actual glyph and script bounds with the loaded fonts'
  await waitForCaptionMeasurement()
  assert.equal(await page.locator('[data-structure-canvas]').getAttribute('viewBox'), beforeCreateViewBox, 'creating a caption preserves the chosen viewport')
  // The new baseline is at y=0, so its ascenders can be above the existing frame.
  // Editing does not auto-fit: use the same explicit control offered by the product.
  await button('Fit drawing').click()
  await page.waitForFunction(() => {
    const group = document.querySelector('[data-caption]')
    const text = group?.querySelector('[data-caption-text]')
    const hit = group?.querySelector('rect')
    if (!text || !hit) return false
    const ink = text.getBBox()
    const box = hit.getBBox()
    const svg = group.ownerSVGElement.viewBox.baseVal
    return ink.width > 30 && box.x <= ink.x && box.y <= ink.y && box.x + box.width >= ink.x + ink.width && box.y + box.height >= ink.y + ink.height && svg.x <= box.x && svg.y <= box.y && svg.x + svg.width >= box.x + box.width && svg.y + svg.height >= box.y + box.height
  })
  stage = 'explicit underline ink, Unicode advances and fixed anchors'
  const glyphMetrics = () => page.locator('[data-caption-text]').evaluate((node) => ({
    advance: node.getComputedTextLength(), x: node.getAttribute('x'), anchor: node.dataset.captionAnchorX,
    spans: [...node.querySelectorAll('tspan')].map((span) => Array.from({ length: span.getNumberOfChars() }, (_, index) => [span.getStartPositionOfChar(index).x, span.getEndPositionOfChar(index).x])),
  }))
  // FEFF is a permitted zero-advance run. Others exercise native cluster shaping,
  // ligatures, RTL, supplementary characters and preserved edge spaces.
  for (const sample of [text, '   ffi office   ', '  العربية אבג  ', 'e\u0301 👩‍🔬 🧪', '\ufeff']) {
    await button('Select caption').click()
    await button('Edit selected caption…').click()
    await page.getByLabel('Run 1 text', { exact: true }).fill(sample)
    await button('Apply caption').click()
    await page.waitForFunction((sample) => document.querySelector('[data-caption-text] tspan')?.textContent === sample, sample)
    await waitForCaptionMeasurement()
    const plain = await glyphMetrics()
    const plainSnapshot = await caption()
    await button('Edit selected caption…').click()
    for (let run = 1; run <= 3; run++) await page.getByRole('group', { name: `Run ${run} underline`, exact: true }).getByRole('button', { name: 'On', exact: true }).click()
    await button('Apply caption').click()
    await page.waitForFunction(() => [...document.querySelectorAll('[data-caption-underline]')].length === 3 && [...document.querySelectorAll('[data-caption-underline]')].every((line) => line.hasAttribute('height')))
    await waitForCaptionMeasurement()
    assert.deepEqual(await glyphMetrics(), plain, `underline leaves all character advances and anchor fixed: ${sample}`)
    assert.deepEqual((await caption()).runs, plainSnapshot.runs.map((run) => ({ ...run, underline: true })))
    await button('Fit drawing').click()
    await page.waitForFunction(() => {
      const text = document.querySelector('[data-caption-text]')
      const hit = text?.parentElement.querySelector('rect')?.getBBox()
      if (!text || !hit) return false
      const view = text.ownerSVGElement.viewBox.baseVal
      return [...text.parentElement.querySelectorAll('[data-caption-underline]')].every((line) => {
        if (Number(line.getAttribute('width')) === 0) return true
        const b = line.getBBox()
        return hit.x <= b.x && hit.y <= b.y && hit.x + hit.width >= b.x + b.width && hit.y + hit.height >= b.y + b.height && view.x <= b.x && view.y <= b.y && view.x + view.width >= b.x + b.width && view.y + view.height >= b.y + b.height
      })
    })
    const geometry = await page.locator('[data-caption-text]').evaluate((node) => [...node.parentElement.querySelectorAll('[data-caption-underline]')].map((line) => {
      const span = node.children.item(Number(line.dataset.captionUnderline))
      const edges = Array.from({ length: span.getNumberOfChars() }, (_, index) => [span.getStartPositionOfChar(index).x, span.getEndPositionOfChar(index).x]).flat()
      const actual = line.getBBox()
      const b = Number(line.getAttribute('width')) === 0 ? { x: Number(line.getAttribute('x')), y: Number(line.getAttribute('y')), width: 0, height: Number(line.getAttribute('height')) } : actual
      return { x: b.x, width: b.width, start: edges.length ? Math.min(...edges) : b.x, end: edges.length ? Math.max(...edges) : b.x, y: b.y, baseline: Number(span.getAttribute('y')), size: Number(span.getAttribute('font-size')), height: b.height, fill: getComputedStyle(line).fill, textFill: getComputedStyle(span).fill }
    }))
    for (const line of geometry) {
      assert.ok(Math.abs(line.x - line.start) < 0.001)
      assert.ok(Math.abs(line.width - (line.end - line.start)) < 0.001)
      assert.ok(Math.abs(line.y - (line.baseline + line.size / 12)) < 0.001)
      assert.ok(Math.abs(line.height - line.size / 16) < 0.001)
      assert.equal(line.fill, line.textFill)
    }
    // Click the actual painted underline; it must select the caption through its hit box.
    await button('Clear caption selection').click()
    assert.equal(await button('Edit selected caption…').isDisabled(), true)
    const line = page.locator('[data-caption-underline="1"]')
    await line.click({ force: true })
    assert.equal(await button('Edit selected caption…').isEnabled(), true)
    await button('Undo caption').click()
    await page.waitForFunction(() => document.querySelectorAll('[data-caption-underline]').length === 0)
  }
  stage = 'hollow run outlines, theme colors, stroke bounds and undo'
  await page.evaluate(() => {
    const canvas = document.querySelector('[data-structure-canvas]')
    const output = document.createElement('output')
    output.hidden = true
    output.dataset.captionReadinessProbe = ''
    document.body.append(output)
    const evidence = { viewChanges: 0, pendingViewports: 0, violations: [] }
    let previous = canvas.getAttribute('viewBox')
    const observer = new MutationObserver(() => {
      const current = canvas.getAttribute('viewBox')
      const measured = canvas.dataset.captionMeasuredViewBox
      const ready = canvas.dataset.captionBoundsReady === 'true'
      if (current !== previous) evidence.viewChanges++
      if (measured !== current && !ready) evidence.pendingViewports++
      if (measured !== current && ready) evidence.violations.push({ current, measured })
      previous = current
      output.textContent = JSON.stringify(evidence)
    })
    observer.observe(canvas, { attributes: true, attributeFilter: ['viewBox', 'data-caption-bounds-ready', 'data-caption-measured-view-box'] })
  })
  for (const sample of ['H', 'g j gj', '   ffi office   ', '  العربية אבג  ', 'e\u0301 👩‍🔬 🧪', '   ', '\ufeff', '\u0301']) {
    await button('Select caption').click()
    await button('Edit selected caption…').click()
    await page.getByLabel('Run 1 text', { exact: true }).fill(sample)
    await page.getByLabel('Run 1 size (pt)', { exact: true }).fill('72')
    await button('Apply caption').click()
    await page.waitForFunction((sample) => document.querySelector('[data-caption-text] tspan')?.textContent === sample, sample)
    await waitForCaptionMeasurement()
    const plain = await glyphMetrics()
    assert.deepEqual(await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).stroke)), ['none', 'none', 'none'], 'filled glyphs do not inherit theme stroke')
    const original = await caption()
    await button('Edit selected caption…').click()
    for (let run = 1; run <= 3; run++) await page.getByRole('group', { name: `Run ${run} outline`, exact: true }).getByRole('button', { name: 'On', exact: true }).click()
    await page.getByRole('group', { name: 'Run 1 underline', exact: true }).getByRole('button', { name: 'On', exact: true }).click()
    await button('Apply caption').click()
    await page.waitForFunction(() => document.querySelectorAll('[data-caption-outline="true"]').length === 3)
    await waitForCaptionMeasurement()
    assert.deepEqual(await glyphMetrics(), plain, `outline retains all advances and fixed anchor: ${sample}`)
    assert.deepEqual((await caption()).runs, original.runs.map((run, index) => ({ ...run, outline: true, underline: index === 0 })))
    await button('Fit drawing').click()
    await waitForCaptionMeasurement()
    const outline = await page.locator('[data-caption-text]').evaluate((node) => {
      const ink = node.getBBox()
      let left = ink.x, top = ink.y, right = ink.x + ink.width, bottom = ink.y + ink.height
      const runs = [...node.querySelectorAll('tspan')].map((span) => {
        const b = span.getBBox(), size = Number(span.getAttribute('font-size')), style = getComputedStyle(span)
        if (/\S/u.test(span.textContent) && b.width > 0 && b.height > 0) {
          left = Math.min(left, b.x - size / 32); top = Math.min(top, b.y - size / 32)
          right = Math.max(right, b.x + b.width + size / 32); bottom = Math.max(bottom, b.y + b.height + size / 32)
        }
        return { fill: style.fill, stroke: style.stroke, width: Number.parseFloat(style.strokeWidth), join: style.strokeLinejoin, size }
      })
      const underline = node.parentElement.querySelector('[data-caption-underline]').getBBox()
      if (underline.width > 0) {
        left = Math.min(left, underline.x); top = Math.min(top, underline.y)
        right = Math.max(right, underline.x + underline.width); bottom = Math.max(bottom, underline.y + underline.height)
      }
      const hit = node.parentElement.querySelector('rect').getBBox(), view = node.ownerSVGElement.viewBox.baseVal
      return { runs, expected: [left - 2, top - 2, right - left + 4, bottom - top + 4], hit: [hit.x, hit.y, hit.width, hit.height],
        fits: view.x <= hit.x && view.y <= hit.y && view.x + view.width >= hit.x + hit.width && view.y + view.height >= hit.y + hit.height,
        underline: { fill: getComputedStyle(node.parentElement.querySelector('[data-caption-underline]')).fill, stroke: getComputedStyle(node.parentElement.querySelector('[data-caption-underline]')).stroke } }
    })
    for (const run of outline.runs) {
      assert.equal(run.fill, 'none')
      assert.equal(run.join, 'round')
      assert.ok(Math.abs(run.width - run.size / 16) < 0.001)
    }
    assert.deepEqual(outline.runs.slice(0, 2).map((run) => run.stroke), ['rgb(42, 123, 205)', 'rgb(0, 0, 0)'])
    assert.equal(outline.underline.fill, 'rgb(42, 123, 205)', 'underline stays filled while glyphs are hollow')
    assert.equal(outline.underline.stroke, 'none')
    outline.hit.forEach((value, index) => assert.ok(Math.abs(value - outline.expected[index]) < 0.001, `stroke participates in hit/selection bounds: ${sample}`))
    assert.equal(outline.fits, true, `fit encloses full stroke: ${sample}`)
    if (sample === 'H') {
      await button('Clear caption selection').click()
      assert.equal(await button('Edit selected caption…').isDisabled(), true)
      const point = await page.locator('[data-caption-text] tspan').first().evaluate((span) => {
        const b = span.getBBox(), radius = Number(span.getAttribute('font-size')) / 32
        const point = new DOMPoint(b.x + b.width / 2, b.y - radius + 0.1).matrixTransform(span.getScreenCTM())
        return { x: point.x, y: point.y }
      })
      await page.mouse.click(point.x, point.y)
      assert.equal(await button('Edit selected caption…').isEnabled(), true, 'outer outline bounds participate in real pointer hit testing')
    }
    await page.evaluate(() => document.documentElement.classList.add('dark'))
    const darkStroke = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).stroke))
    assert.deepEqual(darkStroke.slice(0, 2), ['rgb(42, 123, 205)', 'rgb(0, 0, 0)'])
    assert.notEqual(darkStroke[2], outline.runs[2].stroke, 'inherited outline follows theme')
    await page.evaluate(() => document.documentElement.classList.remove('dark'))
    await button('Undo caption').click()
    await page.waitForFunction(() => document.querySelectorAll('[data-caption-outline="true"]').length === 0)
    assert.deepEqual((await caption()).runs, original.runs)
  }
  stage = 'independent shadows with measured glyph and underline ink'
  for (const sample of ['H', 'g j gj', '   ffi office   ', '  العربية אבג  ', 'e\u0301 👩‍🔬 🧪', '   ', '\ufeff', '\u0301']) {
    await button('Select caption').click()
    await button('Edit selected caption…').click()
    await page.getByLabel('Run 1 text', { exact: true }).fill(sample)
    const prior = await snapshot()
    const plainRuns = prior.document.scene.objects.find((object) => object.kind === 'caption').runs.map((run, index) => index === 0 ? { ...run, text: sample } : run)
    await button('Apply caption').click()
    await waitForCaptionEdit(prior.document.revision, plainRuns)
    const before = await caption(), metrics = await glyphMetrics(), plainRevision = (await snapshot()).document.revision
    await button('Edit selected caption…').click()
    for (let run = 1; run <= 3; run++) await page.getByRole("group", { name: "Run " + run + " shadow", exact: true }).getByRole("button", { name: "On", exact: true }).click()
    await page.getByRole('group', { name: 'Run 1 outline', exact: true }).getByRole('button', { name: 'On', exact: true }).click()
    await page.getByRole('group', { name: 'Run 1 underline', exact: true }).getByRole('button', { name: 'On', exact: true }).click()
    await button('Apply caption').click()
    const shadowRuns = before.runs.map((run, index) => ({ ...run, shadow: true, outline: index === 0, underline: index === 0 }))
    await waitForCaptionEdit(plainRevision, shadowRuns)
    assert.deepEqual(await glyphMetrics(), metrics, 'shadow preserves shaped advances and anchor')
    assert.deepEqual((await caption()).runs, before.runs.map((run, index) => ({ ...run, shadow: true, outline: index === 0, underline: index === 0 })))
    await button('Fit drawing').click()
    await waitForCaptionMeasurement()
    const evidence = await page.locator('[data-caption-text]').evaluate((text) => {
      const ink = text.getBBox()
      let left = ink.x, top = ink.y, right = ink.x + ink.width, bottom = ink.y + ink.height
      for (const span of text.children) {
        const b = span.getBBox(), radius = span.dataset.captionOutline === 'true' ? Number(span.getAttribute('font-size')) / 32 : 0
        if (/\S/u.test(span.textContent) && b.width > 0 && b.height > 0) {
          left = Math.min(left, b.x - radius); top = Math.min(top, b.y - radius)
          right = Math.max(right, b.x + b.width + radius + 1); bottom = Math.max(bottom, b.y + b.height + radius + 1)
        }
      }
      const line = text.parentElement.querySelector('[data-caption-underline]').getBBox()
      if (line.width > 0) {
        left = Math.min(left, line.x); top = Math.min(top, line.y)
        right = Math.max(right, line.x + line.width + 1); bottom = Math.max(bottom, line.y + line.height + 1)
      }
      const shadow = text.parentElement.querySelector('[data-caption-shadow-text]'), hit = text.parentElement.querySelector('rect').getBBox(), view = text.ownerSVGElement.viewBox.baseVal
      return { hit: [hit.x, hit.y, hit.width, hit.height], expected: [left - 2, top - 2, right - left + 4, bottom - top + 4], transform: shadow.parentElement.getAttribute('transform'), opacity: shadow.parentElement.getAttribute('opacity'),
        colors: [...shadow.children].map((span) => getComputedStyle(span).stroke === 'none' ? getComputedStyle(span).fill : getComputedStyle(span).stroke),
        fits: view.x <= hit.x && view.y <= hit.y && view.x + view.width >= hit.x + hit.width && view.y + view.height >= hit.y + hit.height }
    })
    const composition = await page.locator('[data-caption-text]').evaluate(async (text) => {
      const layer = text.parentElement.querySelector('[data-caption-shadow-layer]')
      const underline = text.parentElement.querySelector('[data-caption-underlines]')
      const before = Boolean(layer.compareDocumentPosition(text) & Node.DOCUMENT_POSITION_FOLLOWING) && Boolean(layer.compareDocumentPosition(underline) & Node.DOCUMENT_POSITION_FOLLOWING)
      const clone = layer.cloneNode(true)
      const originals = [layer, ...layer.querySelectorAll('*')], copies = [clone, ...clone.querySelectorAll('*')]
      // Render the component's actual shadow tree into a transparent browser canvas;
      // freeze resolved theme/font styles so the detached SVG paints the same ink.
      originals.forEach((original, index) => {
        const style = getComputedStyle(original)
        for (const property of ['fill', 'stroke', 'stroke-width', 'stroke-linejoin', 'font-family', 'font-size', 'font-weight', 'font-style', 'visibility', 'white-space', 'direction', 'unicode-bidi']) copies[index].style.setProperty(property, style.getPropertyValue(property))
      })
      const bounds = layer.getBBox(), svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
      const width = Math.ceil(bounds.width + 8), height = Math.ceil(bounds.height + 8)
      svg.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
      svg.setAttribute('width', String(width)); svg.setAttribute('height', String(height))
      svg.setAttribute('viewBox', `${bounds.x - 3} ${bounds.y - 3} ${width} ${height}`)
      svg.append(clone)
      const url = URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(svg)], { type: 'image/svg+xml' }))
      try {
        const image = new Image()
        image.src = url
        await image.decode()
        const canvas = document.createElement('canvas')
        canvas.width = width * 3; canvas.height = height * 3
        const context = canvas.getContext('2d')
        context.drawImage(image, 0, 0, canvas.width, canvas.height)
        const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data
        let maximumAlpha = 0
        for (let index = 3; index < pixels.length; index += 4) maximumAlpha = Math.max(maximumAlpha, pixels[index])
        return { before, maximumAlpha }
      } finally {
        URL.revokeObjectURL(url)
      }
    })
    assert.equal(composition.before, true, 'all shadow ink paints before every foreground glyph and underline')
    assert.ok(composition.maximumAlpha >= 80 && composition.maximumAlpha <= 90, 'the complete shadow layer composites once at .35, including descender/underline overlap')
    assert.equal(evidence.transform, 'translate(1 1)'); assert.equal(evidence.opacity, '0.35')
    evidence.hit.forEach((value, index) => assert.ok(Math.abs(value - evidence.expected[index]) < 0.001, "shadow ink bounds: " + sample))
    assert.equal(evidence.fits, true)
    if (sample === 'H') {
      await button('Clear caption selection').click()
      const point = await page.locator('[data-caption-text] tspan').first().evaluate((span) => {
        const b = span.getBBox(), radius = Number(span.getAttribute('font-size')) / 32
        const p = new DOMPoint(b.x + b.width + radius + 0.5, b.y + b.height / 2 + 1).matrixTransform(span.getScreenCTM())
        return { x: p.x, y: p.y }
      })
      await page.mouse.click(point.x, point.y)
      assert.equal(await button('Edit selected caption…').isEnabled(), true, 'translated shadow ink participates in pointer hit testing')
    }
    await page.evaluate(() => document.documentElement.classList.add('dark'))
    const dark = await page.locator('[data-caption-shadow-text] tspan').evaluateAll((spans) => spans.map((span) => getComputedStyle(span).stroke === 'none' ? getComputedStyle(span).fill : getComputedStyle(span).stroke))
    assert.deepEqual(dark.slice(0, 2), evidence.colors.slice(0, 2)); assert.notEqual(dark[2], evidence.colors[2])
    await page.evaluate(() => document.documentElement.classList.remove('dark'))
    const shadowRevision = (await snapshot()).document.revision
    await button('Undo caption').click()
    await waitForCaptionEdit(shadowRevision, before.runs)
    assert.deepEqual((await caption()).runs, before.runs)
  }
  await button('Select caption').click()
  await button('Edit selected caption…').click()
  const readiness = JSON.parse(await page.locator('[data-caption-readiness-probe]').textContent())
  assert.ok(readiness.viewChanges > 0, 'outline fits changed the actual SVG viewport')
  assert.ok(readiness.pendingViewports > 0, 'viewport changes invalidate measured bounds until remeasurement')
  assert.deepEqual(readiness.violations, [], 'ready never exposes bounds measured in another viewport')
  await page.getByLabel('Run 1 text', { exact: true }).fill(text)
  await page.getByLabel('Run 1 size (pt)', { exact: true }).fill('24')
  await button('Apply caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text] tspan')?.textContent === text, text)
  await waitForCaptionMeasurement()
  await button('Fit drawing').click()
  const scriptY = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((node) => Number(node.getAttribute('y'))))
  assert.deepEqual(scriptY, [0, 6, -5])
  stage = 'whole-caption alignment shifts all script chunks and includes edge spaces'
  const leftMetrics = await page.locator('[data-caption-text]').evaluate((node) => ({
    starts: [...node.querySelectorAll('tspan')].map((span) => span.getStartPositionOfChar(0).x),
    advance: [...node.querySelectorAll('tspan')].reduce((sum, span) => sum + span.getComputedTextLength(), 0),
  }))
  assert.ok(leftMetrics.advance > 30)
  const originalAnchor = (await caption()).anchor
  for (const [alignment, fraction] of [['center', 0.5], ['right', 1], ['left', 0]]) {
    await button('Select caption').click()
    await button('Edit selected caption…').click()
    await page.getByRole('group', { name: 'Caption alignment', exact: true }).getByRole('button', { name: alignment[0].toUpperCase() + alignment.slice(1), exact: true }).click()
    await button('Apply caption').click()
    await page.waitForFunction((alignment) => {
      const text = document.querySelector('[data-caption-text]')
      if (text?.dataset.captionAlignment !== alignment || text.ownerSVGElement.dataset.captionBoundsReady !== 'true') return false
      const advance = text.getComputedTextLength()
      const expected = Number(text.dataset.captionAnchorX) - advance * (alignment === 'right' ? 1 : alignment === 'center' ? 0.5 : 0)
      return Math.abs(Number(text.getAttribute('x')) - expected) < 0.02
    }, alignment)
    assert.equal((await caption()).alignment, alignment)
    assert.deepEqual((await caption()).anchor, originalAnchor)
    assert.deepEqual((await caption()).runs, expected)
    const starts = await page.locator('[data-caption-text] tspan').evaluateAll((nodes) => nodes.map((span) => span.getStartPositionOfChar(0).x))
    starts.forEach((x, index) => assert.ok(Math.abs(x - (leftMetrics.starts[index] - leftMetrics.advance * fraction)) < 0.05, `${alignment} run ${index}: all runs shift by the whole advance`))
  }
  // A single alignment-only edit is one undo step; the anchor and mixed styling stay fixed.
  await button('Undo caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text]')?.dataset.captionAlignment === 'right')
  assert.equal((await caption()).alignment, 'right')
  await button('Redo caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text]')?.dataset.captionAlignment === 'left')
  stage = 'style-only edit remeasures current ink and undo restores styles'
  await button('Select caption').click()
  const beforeStyleViewport = await page.locator('[data-structure-canvas]').getAttribute('viewBox')
  await button('Edit selected caption…').click()
  await page.getByRole('group', { name: 'Run 1 weight', exact: true }).getByRole('button', { name: 'Normal', exact: true }).click()
  await page.getByRole('group', { name: 'Run 1 style', exact: true }).getByRole('button', { name: 'Italic', exact: true }).click()
  await button('Apply caption').click()
  await page.waitForFunction(() => {
    const text = document.querySelector('[data-caption-text]')
    if (text?.firstElementChild?.getAttribute('font-weight') !== 'normal' || text.firstElementChild.getAttribute('font-style') !== 'italic') return false
    const hit = text.parentElement.querySelector('rect')?.getBBox()
    const ink = text.getBBox()
    return text.ownerSVGElement.dataset.captionBoundsReady === 'true' && hit && hit.x <= ink.x && hit.y <= ink.y && hit.x + hit.width >= ink.x + ink.width && hit.y + hit.height >= ink.y + ink.height
  })
  assert.deepEqual((await caption()).runs, [{ ...expected[0], weight: 'normal', style: 'italic' }, ...expected.slice(1)])
  assert.equal(await page.locator('[data-structure-canvas]').getAttribute('viewBox'), beforeStyleViewport)
  await button('Undo caption').click()
  await page.waitForFunction(() => document.querySelector('[data-caption-text]')?.firstElementChild?.getAttribute('font-weight') === 'bold')
  assert.deepEqual((await caption()).runs, expected)
  stage = 'edit through selection and preserve one undo step'
  await page.locator('[data-caption-text="s2"]').click()
  await button('Edit selected caption…').click()
  await page.getByRole('group', { name: 'Run 1 shadow', exact: true }).getByRole('button', { name: 'On', exact: true }).click()
  const editedText = '  NFC é and NFD e\u0301 '.repeat(8)
  await page.getByLabel('Run 1 text', { exact: true }).fill(editedText)
  await page.getByRole('group', { name: 'Run 1 outline', exact: true }).getByRole('button', { name: 'On', exact: true }).click()
  await page.getByRole('group', { name: 'Run 1 underline', exact: true }).getByRole('button', { name: 'On', exact: true }).click()
  await page.getByRole('group', { name: 'Caption alignment', exact: true }).getByRole('button', { name: 'Right', exact: true }).click()
  await page.getByLabel('Caption horizontal position (pt)', { exact: true }).fill('1200')
  await page.getByLabel('Caption vertical position (pt)', { exact: true }).fill('450')
  await button('Apply caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+   `, editedText)
  await button('Undo caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+   `, text)
  assert.deepEqual((await caption()).runs, expected)
  await button('Redo caption').click()
  await page.waitForFunction((text) => document.querySelector('[data-caption-text]')?.textContent === `${text}2+   `, editedText)
  stage = 'save, reload and reopen the exact native runs'
  const edited = await caption()
  await button('Save caption drawing').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Caption snapshot"]').textContent)?.session.unsaved_changes === false)
  const native = JSON.parse(readFileSync(join(work, 'documents', `${id}.json`), 'utf8'))
  assert.equal(native.format_version, 17)
  assert.equal(native.document.objects.find((object) => object.id === 's2').state.alignment, 'right')
  assert.deepEqual(native.document.objects.find((object) => object.id === 's2').state.runs, edited.runs)
  await page.reload()
  await page.locator('[data-caption-text="s2"]').waitFor()
  assert.deepEqual(await caption(), edited)
  async function fullyVisible(target) {
    await target.waitForFunction(() => {
      const text = document.querySelector('[data-caption-text]')
      if (!text) return false
      const canvas = text.ownerSVGElement
      if (canvas.dataset.captionBoundsReady !== 'true' || canvas.dataset.captionMeasuredViewBox !== canvas.getAttribute('viewBox')) return false
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

  stage = 'deterministic viewport measurement provenance and user authority'
  const probe = page.getByLabel('Probe viewBox', { exact: true })
  await button('Probe first measurement').click()
  await page.waitForFunction(() => document.querySelector('[aria-label="Probe viewBox"]')?.textContent === '0 0 200 100')
  await button('Probe post-fit measurement').click()
  await page.waitForFunction(() => document.querySelector('[aria-label="Probe viewBox"]')?.textContent === '0 -50 400 200')
  // Confirm in the resulting viewport, then later unrelated measurements cannot auto-fit.
  await button('Probe post-fit measurement').click()
  await button('Probe later measurement').click()
  assert.equal(await probe.textContent(), '0 -50 400 200')
  await button('Reset probe document').click()
  await button('Probe first measurement').click()
  await page.waitForFunction(() => document.querySelector('[aria-label="Probe viewBox"]')?.textContent === '0 0 200 100')
  await button('Probe pan').click()
  assert.equal(await probe.textContent(), '-10 0 200 100')
  await button('Probe post-fit measurement').click()
  assert.equal(await probe.textContent(), '-10 0 200 100', 'a pending correction loses authority after user navigation')

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
