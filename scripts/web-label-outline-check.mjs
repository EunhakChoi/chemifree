// Production canvas/editor and backend: actual SVG underline and 400/500/700 ink and shared geometry.
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'
import { installReadyCanvasSampler } from './lib/ready-canvas-sample.mjs'

const work = mkdtempSync(join(tmpdir(), 'chemifree-label-outline-'))
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
  async function write(path, body) {
    const response = await fetch(`${base}/api/v1${path}`, { method: 'POST', headers: { 'content-type': 'application/json', [token.header]: token.token, origin: base }, body: JSON.stringify(body) })
    assert.ok(response.ok, await response.clone().text())
    return response.json()
  }
  const id = (await write('/documents', { request_id: 'weight-create', title: 'Chemical label ink' })).document.id
  const drawn = await write(`/documents/${id}/transactions`, { request_id: 'weight-seed', expected_revision: 0, commands: [
    { op: 'add_atom', element: 'O', position: { x: -300, y: 0 } },
    { op: 'add_atom', element: 'C', position: { x: 0, y: 0 }, bond_to: { atom: '$0', order: 'single' } },
    { op: 'add_atom', element: 'C', position: { x: 40, y: 40 }, bond_to: { atom: '$1', order: 'single' } },
    { op: 'add_atom', element: 'N', position: { x: -400, y: 220 }, hydrogens: { mode: 'specified', count: 3 }, radical: 'unknown', attachment_points: [1] },
    { op: 'add_atom', element: 'C', position: { x: -100, y: 220 }, bond_to: { atom: '$3', order: 'single' } },
    { op: 'add_atom', query: { kind: 'list', negated: true, elements: ['C', 'Cl', 'Br', 'I'] }, position: { x: 600, y: 220 } },
    { op: 'add_caption', anchor: { x: -300, y: 600 }, alignment: 'left', runs: [{ text: 'Independent caption', script: 'baseline', weight: 'normal' }] },
  ] })
  const seeded = await write(`/documents/${id}/transactions`, { request_id: 'weight-atom-states', expected_revision: drawn.document.revision,
    commands: [{ op: 'update_atom', atom: drawn.document.atoms[3].id, isotope: 15, charge: 2 }] })
  await write(`/documents/${id}/transactions`, { request_id: 'weight-compact', expected_revision: seeded.document.revision,
    commands: [{ op: 'collapse_ethyl', instance: 's1', anchor: seeded.document.atoms[1].id, terminal: seeded.document.atoms[2].id }] })
  const coldId = (await write('/documents', { request_id: 'weight-cold-create', title: 'Delayed italic source' })).document.id
  const alternateId = (await write('/documents', { request_id: 'weight-cold-alternate', title: 'Delayed italic destination' })).document.id
  for (const [documentId, alternate] of [[coldId, false], [alternateId, true]]) {
    let seededCold = await write(`/documents/${documentId}/transactions`, { request_id: `weight-cold-seed-${alternate}`, expected_revision: 0, commands: [
      { op: 'update_settings', label_outline: true, label_underline: true, label_style: 'italic', label_weight: 'normal', label_font: 'sans-serif', label_size_pt: 72 },
      alternate
        ? { op: 'add_atom', query: { kind: 'list', negated: true, elements: ['C', 'Cl', 'Br', 'I'] }, position: { x: 4000, y: -2000 } }
        : { op: 'add_atom', element: 'N', position: { x: -2500, y: 1200 }, hydrogens: { mode: 'specified', count: 3 } },
    ] })
    if (!alternate) {
      seededCold = await write(`/documents/${documentId}/transactions`, { request_id: 'weight-cold-atom-states', expected_revision: seededCold.document.revision,
        commands: [{ op: 'update_atom', atom: seededCold.document.atoms[0].id, isotope: 15, charge: 2 }] })
    }
    await write(`/documents/${documentId}/save`, { request_id: `weight-cold-save-${alternate}`, expected_revision: seededCold.document.revision })
  }
  const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { StructureCanvas } from '/src/editor/structure-canvas.tsx'
import { DocumentStyleButton } from '/src/components/style-settings.tsx'
import { TooltipProvider } from '/src/components/ui/tooltip.tsx'
import { settingsOf } from '/src/lib/document-contract.ts'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open(new URLSearchParams(location.search).has('cold') ? ${JSON.stringify(coldId)} : ${JSON.stringify(id)}) }, [])
  return <main>
    <button disabled={editor.busy} onClick={() => void editor.open(${JSON.stringify(alternateId)})}>Open alternate italic drawing</button>
    {['normal','italic'].flatMap((style) => ['normal','bold'].flatMap((weight) => ['sans-serif', 'serif', 'monospace'].flatMap((font) => [4,72].flatMap((size) => [false,true].map((underline) => <button key={style+weight+font+size+underline} disabled={editor.busy} onClick={() => void editor.apply([{op:'update_settings',label_outline:true,label_underline:underline,label_style:style,label_weight:weight,label_font:font,label_size_pt:size}])}>Outline {style} {weight} {font} {size} {String(underline)}</button>)))))}
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo weight</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo weight</button>
    <button disabled={editor.busy} onClick={() => void editor.save()}>Save weight</button>
    <output aria-label="Weight snapshot">{JSON.stringify(editor.current)}</output>
    <output aria-label="Weight selection">{JSON.stringify([...editor.selection])}</output>
    <output aria-label="Weight error">{editor.error?.message}</output>
    {editor.current && <>
      <DocumentStyleButton document={editor.current.document} settings={settingsOf(editor.current.document)} editor={editor} />
      <div style={{width:1000,height:650}}><StructureCanvas document={editor.current.document} tool="select" element="C" bondKind="single"
        selection={editor.selection} onSelectionChange={editor.setSelection} activePlacement="s1" onActivePlacementChange={() => {}}
        onCommands={(commands) => void editor.apply(commands)} onMark={() => {}} disabled={editor.busy}/></div>
    </>}
  </main>
}
createRoot(document.getElementById('root')).render(<TooltipProvider><Harness /></TooltipProvider>)
`
  server = await createServer({
    root: fileURLToPath(new URL('../apps/web', import.meta.url)),
    configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
    server: { host: '127.0.0.1', port: 5173, strictPort: true, proxy: { '/api': base } },
    plugins: [{ name: 'label-outline-check', configureServer(vite) {
      vite.middlewares.use('/label-outline-check', async (_request, response) => {
        response.setHeader('content-type', 'text/html')
        response.end(await vite.transformIndexHtml('/label-outline-check', '<!doctype html><html><head><title>Label style</title></head><body><div id="root"></div><script type="module" src="/weight-harness.tsx"></script></body></html>'))
      })
    }, resolveId(path) { if (path === '/weight-harness.tsx') return path }, load(path) { if (path === '/weight-harness.tsx') return fixture } }],
  })
  await server.listen()
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  const button = (name) => page.getByRole('button', { name, exact: true })
  const snapshot = async () => JSON.parse(await page.getByLabel('Weight snapshot', { exact: true }).textContent())
  await page.goto('http://127.0.0.1:5173/label-outline-check')
  await page.locator('[data-atom-label="a2"]').waitFor()
  await page.evaluate(() => document.fonts.ready)
  // Load diagnostics before the matrix, so sampling itself has no async gap.
  await page.evaluate(async () => { window.__chemicalLabelMeasure = (await import('/src/editor/label.ts')).measureChemicalLabels })
  await page.evaluate(installReadyCanvasSampler)
  const original = (await snapshot()).document
  const normalWeights = await page.locator('[data-atom-label="a4"] [data-chunk="text"] tspan').evaluateAll((spans) => spans.map((span) => span.getAttribute('font-weight')))
  assert.deepEqual(normalWeights, ['500', '400', '400', '500', '400'])
  for (const style of ['normal','italic']) {
  for (const weight of ['normal','bold']) {
  for (const font of ['sans-serif', 'serif', 'monospace']) {
    for (const size of [4, 72]) {
    for (const underline of [false,true]) {
      stage = `actual underline ${style} ${weight} ink and shared geometry: ${font} ${size}pt`
      const beforeRevision = (await snapshot()).document.revision
      await button(`Outline ${style} ${weight} ${font} ${size} ${underline}`).click()
      await page.waitForFunction(({ font, size, style, weight, underline, beforeRevision }) => {
        const current = JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent)
        const svg = document.querySelector('svg[data-structure-canvas]')
        return current.document.revision > beforeRevision && current.document.settings.label_font === font && current.document.settings.label_size_pt === size && current.document.settings.label_style === style && current.document.settings.label_weight === weight && current.document.settings.label_underline === underline && current.document.settings.label_outline === true && !svg.classList.contains('cursor-progress') && svg.dataset.captionMeasuredViewBox === svg.getAttribute('viewBox') && svg.dataset.captionBoundsReady === 'true'
      }, { font, size, style, weight, underline, beforeRevision })
      await page.getByRole('button', { name: 'Fit drawing', exact: true }).click()
      const geometry = await page.locator('svg[data-structure-canvas]').evaluate((svg, expected) => window.__captureReadyChemicalCanvas(svg, expected, () => {
        // Capture all evidence in one JS turn. Fresh metrics are diagnostic only:
        // production rectangles and bounds remain the subject of every assertion.
        const fresh = window.__chemicalLabelMeasure(svg)
        const current = JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent)
        const context = { documentId:current.document.id,revision:current.document.revision,settings:current.document.settings,
          ready:svg.dataset.captionBoundsReady,measuredViewBox:svg.dataset.captionMeasuredViewBox,paintedViewBox:svg.getAttribute('viewBox'),
          fonts:document.fonts.status,busy:svg.classList.contains('cursor-progress') }
        const box = (value) => { const [x,y,width,height] = value.split(' ').map(Number); return {x,y,width,height} }
        const rect = (r) => ({x:r.x,y:r.y,width:r.width,height:r.height})
        const labels = [...svg.querySelectorAll('[data-atom-label]')].map((label) => ({ id: label.dataset.atomLabel, box: box(label.dataset.box),
          ink: rect(label.getBBox()), underlines: [...label.querySelectorAll('[data-label-underline]')].map((line) => ({x:Number(line.getAttribute('x')),y:Number(line.getAttribute('y')),width:Number(line.getAttribute('width')),height:Number(line.getAttribute('height')),fill:getComputedStyle(line).fill,stroke:getComputedStyle(line).stroke})), expectedUnderlines: [...label.querySelectorAll('[data-chunk="text"] tspan')].map((span) => {
            const positions = Array.from({length:span.getNumberOfChars()}, (_, index) => [span.getStartPositionOfChar(index), span.getEndPositionOfChar(index)]).flat()
            const start = Math.min(...positions.map((point) => point.x)), end = Math.max(...positions.map((point) => point.x))
            const size = Number(span.getAttribute('font-size'))
            return {x:start,y:positions[0].y+size/12,width:end-start,height:size/16}
          }), runs: [...label.querySelectorAll('[data-chunk="text"] tspan')].map((span) => ({outline:span.getAttribute('data-label-outline'),fill:getComputedStyle(span).fill,stroke:getComputedStyle(span).stroke,strokeWidth:Number(span.getAttribute('stroke-width')),join:span.getAttribute('stroke-linejoin'),ink:rect(span.getBBox()),style:span.getAttribute('font-style'),weight:span.getAttribute('font-weight'),text:span.textContent,size:Number(span.getAttribute('font-size')),advance:span.getComputedTextLength()})),
          chunks: [...label.querySelectorAll('[data-chunk="text"]')].map((chunk) => ({box:box(chunk.dataset.box),ink:rect(chunk.getBBox()),key:chunk.dataset.labelMetricKey,
            fresh:fresh.chunks.get(chunk.dataset.labelMetricKey),font:getComputedStyle(chunk).font,
            characters:[...chunk.querySelectorAll('tspan')].map((span)=>({text:span.textContent,computedFont:getComputedStyle(span).font,
              positions:Array.from({length:span.getNumberOfChars()},(_,index)=>{ const start=span.getStartPositionOfChar(index),end=span.getEndPositionOfChar(index);return {start:{x:start.x,y:start.y},end:{x:end.x,y:end.y}} })}))})),
          marks: [...label.querySelectorAll('text[data-mark]')].map((mark) => ({part:mark.dataset.mark,box:box(mark.dataset.box),ink:rect(mark.getBBox()),weights:[...mark.querySelectorAll('tspan')].map((span)=>span.getAttribute('font-weight')),styles:[...mark.querySelectorAll('tspan')].map((span)=>span.getAttribute('font-style')),outlines:[...mark.querySelectorAll('tspan')].map((span)=>span.getAttribute('data-label-outline'))})) }))
        return {context, labels, viewBox:rect(svg.viewBox.baseVal), strokes:[...svg.querySelectorAll('[data-bond="b1"], [data-bond="b3"]')].map((line) => ({x1:Number(line.getAttribute('x1')),y1:Number(line.getAttribute('y1')),x2:Number(line.getAttribute('x2')),y2:Number(line.getAttribute('y2'))})),captionWeights:[...svg.querySelectorAll('[data-caption-text] tspan')].map((span) => span.getAttribute('font-weight')),captionStyles:[...svg.querySelectorAll('[data-caption-text] tspan')].map((span) => getComputedStyle(span).fontStyle),stretched:svg.querySelectorAll('[data-atom-label] [textLength]').length}
      }), { documentId:id,revision:beforeRevision+1,settings:{label_font:font,label_size_pt:size,label_style:style,label_weight:weight,label_underline:underline,label_outline:true} })
      assert.equal(geometry.context.ready,'true',JSON.stringify({stage,context:geometry.context}))
      assert.equal(geometry.context.measuredViewBox,geometry.context.paintedViewBox,JSON.stringify({stage,context:geometry.context}))
      assert.equal(geometry.context.fonts,'loaded',JSON.stringify({stage,context:geometry.context}))
      assert.equal(geometry.context.settings.label_underline,underline,JSON.stringify({stage,context:geometry.context}))
      assert.equal(geometry.context.busy,false,JSON.stringify({stage,context:geometry.context}))
      assert.deepEqual([geometry.context.settings.label_font,geometry.context.settings.label_size_pt,geometry.context.settings.label_style,geometry.context.settings.label_weight],
        [font,size,style,weight],JSON.stringify({stage,context:geometry.context}))
      const inside = (inner, outer, message) => {
        assert.ok(inner.x >= outer.x - 0.05 && inner.y >= outer.y - 0.05 && inner.x+inner.width <= outer.x+outer.width+0.05 && inner.y+inner.height <= outer.y+outer.height+0.05, `${message}: ${JSON.stringify({inner,outer})}`)
      }
      for (const label of geometry.labels) {
        assert.equal(label.underlines.length, underline ? label.runs.length : 0, `each chemical run gets exactly one rect, marks get none: ${JSON.stringify({stage,context:geometry.context,label})}`)
        for (let index = 0; index < label.underlines.length; index++) {
          const line = label.underlines[index], expected = label.expectedUnderlines[index]
          for (const field of ['x','y','width','height']) assert.ok(Math.abs(line[field]-expected[field]) < 0.05, `${stage} underline ${field}: ${JSON.stringify({line,expected})}`)
          assert.notEqual(line.fill,'none','underline stays filled')
          assert.equal(line.stroke,'none','underline has no glyph stroke')
          inside(line, label.box, `${stage} underline union`)
        }
        for (const run of label.runs) {
          assert.equal(run.outline,'true')
          assert.equal(run.fill,'none')
          assert.notEqual(run.stroke,'none')
          assert.equal(run.join,'round')
          assert.equal(run.strokeWidth,run.size/16)
          if (run.ink.width>0 && run.ink.height>0) {
            const radius=run.size/32
            inside({x:run.ink.x-radius,y:run.ink.y-radius,width:run.ink.width+radius*2,height:run.ink.height+radius*2},label.box,`${stage} actual outline ink`)
          }
        }
        inside(label.ink, label.box, `${stage} ${label.id} ink; marks=${JSON.stringify(label.marks)}`)
        inside(label.box, geometry.viewBox, `${stage} ${label.id} fit`)
        assert.ok(label.runs.every((run) => run.style === style && (weight === 'bold' ? run.weight === '700' : ['400','500'].includes(run.weight))), JSON.stringify(label))
        for (const chunk of label.chunks) inside(chunk.ink, chunk.box, `${stage} ${label.id} natural chunk`)
        for (const mark of label.marks) {
          inside(mark.ink, mark.box, `${stage} ${label.id} ${mark.part} mark`)
          assert.ok(mark.weights.every((weight)=>weight==='400'),'radical and attachment marks retain their original weight')
          assert.ok(mark.outlines.every((outline)=>outline==='false'),'radical and attachment marks remain filled')
          assert.ok(mark.styles.every((style)=>style==='normal'),'radical and attachment marks retain normal style')
        }
      }
      assert.ok(geometry.labels.some((label) => label.runs.some((run) => run.text === 'Et')))
      assert.ok(geometry.labels.some((label) => label.runs.some((run) => run.text.includes('['))))
      assert.deepEqual(geometry.captionWeights, ['normal'], 'independent caption keeps weight')
      assert.deepEqual(geometry.captionStyles, ['normal'], 'independent caption keeps style')
      assert.equal(geometry.stretched, 0, 'italic natural advances are never stretched')
      for (const stroke of geometry.strokes) {
        for (const end of [{x:stroke.x1,y:stroke.y1},{x:stroke.x2,y:stroke.y2}]) {
          assert.ok(geometry.labels.every(({box}) => !(end.x > box.x && end.x < box.x+box.width && end.y > box.y && end.y < box.y+box.height)), 'bond trim endpoints stay outside every chemical label')
        }
      }
      const label = geometry.labels.find((item) => item.id === 'a4')
      const pickInput = async () => page.locator('svg[data-structure-canvas]').evaluate((svg, box) => {
        const point=svg.createSVGPoint(); point.x=box.x+box.width-0.5; point.y=box.y+box.height/2
        const screen=point.matrixTransform(svg.getScreenCTM()), target=document.elementFromPoint(screen.x, screen.y)
        const roundtrip=screen.matrixTransform(svg.getScreenCTM().inverse())
        return {x:screen.x,y:screen.y,world:{x:roundtrip.x,y:roundtrip.y},viewBox:svg.getAttribute('viewBox'),
          viewport:{width:innerWidth,height:innerHeight},target:target?.outerHTML.slice(0,250),insideCanvas:target!==null&&svg.contains(target),
          disabled:svg.classList.contains('cursor-progress')}
      }, label.box)
      const beforeScroll = await pickInput()
      if (!beforeScroll.insideCanvas) console.log('Label edge before scrolling:', JSON.stringify({stage,input:beforeScroll}))
      // Snapshot outputs occupy real page height. A world point inside the SVG viewBox may
      // still be below the browser viewport; raw mouse coordinates do not scroll it into view.
      await page.locator('[data-atom-label="a4"]').scrollIntoViewIfNeeded()
      const screen = await pickInput()
      assert.ok(screen.insideCanvas && screen.x>=0 && screen.y>=0 && screen.x<screen.viewport.width && screen.y<screen.viewport.height,
        `the exact label-edge mouse point must reach the canvas: ${JSON.stringify({beforeScroll,screen})}`)
      assert.equal(screen.disabled,false,'picking starts only after the style edit is idle')
      assert.ok(screen.world.x>=label.box.x && screen.world.x<=label.box.x+label.box.width && screen.world.y>=label.box.y && screen.world.y<=label.box.y+label.box.height,
        `screen/world conversion preserves the measured-box point: ${JSON.stringify({screen,label})}`)
      await page.evaluate(() => {
        window.__weightPickEvent=null
        document.addEventListener('pointerdown',(event)=>{
          const svg=document.querySelector('svg[data-structure-canvas]'), point=svg.createSVGPoint()
          point.x=event.clientX;point.y=event.clientY
          const world=point.matrixTransform(svg.getScreenCTM().inverse())
          window.__weightPickEvent={client:{x:event.clientX,y:event.clientY},world:{x:world.x,y:world.y},
            target:event.target?.outerHTML.slice(0,250),insideCanvas:svg.contains(event.target),viewBox:svg.getAttribute('viewBox')}
        },{once:true,capture:true})
      })
      await page.mouse.click(screen.x, screen.y)
      const pickEvent=await page.evaluate(()=>window.__weightPickEvent)
      assert.ok(pickEvent?.insideCanvas,`the native pointer event reached the canvas: ${JSON.stringify({screen,pickEvent})}`)
      assert.ok(JSON.parse(await page.getByLabel('Weight selection', { exact:true }).textContent()).includes('a4'),
        `right edge of the measured label is pickable: ${JSON.stringify({beforeScroll,screen,pickEvent,label})}`)
      // Selection can trigger a render after scrolling and pointer hover. Compare
      // the two mounted consumers in the same turn, not a pre-interaction box.
      const selectedGeometry = await page.locator('svg[data-structure-canvas]').evaluate((svg, expected) => window.__captureReadyChemicalCanvas(svg, expected, () => {
        const atom = svg.querySelector('[data-instance="s1"] [data-atom-label="a4"]')
        const underlay = svg.querySelector('[data-instance="s1"] [data-atom-underlay="a4"]')
        const values = atom?.getAttribute('data-box')?.split(' ').map(Number)
        const box = values ? {x:values[0],y:values[1],width:values[2],height:values[3]} : null
        const highlight = underlay ? Object.fromEntries(['x','y','width','height'].map((field)=>[field,Number(underlay.getAttribute(field))])) : null
        const current = JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent)
        return { box,highlight,selection:JSON.parse(document.querySelector('[aria-label="Weight selection"]').textContent),
          documentId:current.document.id,revision:current.document.revision,settings:current.document.settings,
          ready:svg.dataset.captionBoundsReady,measuredViewBox:svg.dataset.captionMeasuredViewBox,paintedViewBox:svg.getAttribute('viewBox'),
          fonts:document.fonts.status,busy:svg.classList.contains('cursor-progress'),
          atom:atom?.outerHTML,underlay:underlay?.outerHTML }
      }), {documentId:geometry.context.documentId,revision:geometry.context.revision,settings:geometry.context.settings})
      const selectionEvidence = JSON.stringify({stage,beforeScroll,screen,pickEvent,before:label,after:selectedGeometry})
      assert.equal(selectedGeometry.ready,'true',selectionEvidence)
      assert.equal(selectedGeometry.measuredViewBox,selectedGeometry.paintedViewBox,selectionEvidence)
      assert.equal(selectedGeometry.fonts,'loaded',selectionEvidence)
      assert.equal(selectedGeometry.busy,false,selectionEvidence)
      assert.equal(selectedGeometry.revision,geometry.context.revision,selectionEvidence)
      assert.deepEqual(selectedGeometry.settings,geometry.context.settings,selectionEvidence)
      assert.ok(selectedGeometry.selection.includes('a4'), `the mounted selection still contains a4: ${selectionEvidence}`)
      assert.ok(selectedGeometry.box && selectedGeometry.highlight && ['x','y','width','height'].every((key) => Math.abs(selectedGeometry.highlight[key]-selectedGeometry.box[key]) < 0.05),
        `selection highlight uses the same mounted measured label box: ${selectionEvidence}`)
      if (selectedGeometry.box && ['x','y','width','height'].some((key)=>Math.abs(selectedGeometry.box[key]-label.box[key])>=0.05)) {
        console.log(`Label metrics changed between geometry and selection samples: ${selectionEvidence}`)
      }
      const current = (await snapshot()).document
      for (const field of ['atoms','bonds','scene','stereo','abbreviations']) assert.deepEqual(current[field], original[field], `appearance keeps ${field}`)
    }
  }
  }
  }
  }
  stage = 'real shaping advances: whitespace, Unicode, RTL and ligatures'
  const shaping = await page.evaluate(async () => {
    const { measureChemicalLabels, compactLabel, labelStyleOf } = await import('/src/editor/label.ts')
    const { DEFAULT_SETTINGS } = await import('/src/lib/document-contract.ts')
    const results = []
    for (const text of ['   ffi office   ', '  العربية אבג  ', 'e\u0301 👩‍🔬 🧪', '   ']) {
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
      svg.setAttribute('width','800'); svg.setAttribute('height','150')
      const group = document.createElementNS(svg.namespaceURI,'g'); group.setAttribute('data-labels','serif'); group.setAttribute('font-family','serif')
      const label = document.createElementNS(svg.namespaceURI,'g'); label.setAttribute('data-atom-label','probe')
      const node = document.createElementNS(svg.namespaceURI,'text'); node.setAttribute('x','0');node.setAttribute('y','0'); node.setAttribute('text-anchor','middle');node.setAttribute('dominant-baseline','central');node.setAttribute('xml:space','preserve');node.style.whiteSpace='pre'
      const span = document.createElementNS(svg.namespaceURI,'tspan'); span.setAttribute('font-size','72');span.setAttribute('font-weight','500');span.setAttribute('font-style','italic');span.setAttribute('data-label-outline','true');span.setAttribute('fill','none');span.setAttribute('stroke','currentColor');span.setAttribute('stroke-width','4.5');span.setAttribute('stroke-linejoin','round');span.textContent=text
      node.append(span);label.append(node);group.append(label);svg.append(group);document.body.append(svg)
      const positions = Array.from({length:span.getNumberOfChars()},(_,i)=>[span.getStartPositionOfChar(i),span.getEndPositionOfChar(i)]).flat()
      const expected={x:Math.min(...positions.map((p)=>p.x)),y:positions[0].y+6,width:Math.max(...positions.map((p)=>p.x))-Math.min(...positions.map((p)=>p.x)),height:4.5}
      const settings={...DEFAULT_SETTINGS,label_font:'serif',label_size_pt:72,label_style:'italic',label_outline:true,label_underline:true}
      node.setAttribute('data-label-metric-key',compactLabel(text,labelStyleOf(settings)).chunks[0].metricKey)
      const metrics=measureChemicalLabels(svg)
      const layout=compactLabel(text,labelStyleOf(settings,metrics))
      results.push({text,expected,actual:layout.chunks[0].underlines[0]})
      svg.remove()
    }
    return results
  })
  for (const {text,expected,actual} of shaping) {
    assert.ok(actual, `a nonzero advance gets an underline: ${text}`)
    for(const field of ['x','y','width','height']) assert.ok(Math.abs(actual[field]-expected[field])<0.01, `${text}: ${field}`)
  }
  stage = 'production underline selector, undo/redo, save and reopen'
  await button('Document style').click()
  await page.getByLabel('Outline labels', {exact:true}).click()
  await page.getByRole('option', {name:'Off',exact:true}).click()
  await page.getByRole('button', {name:'Done',exact:true}).click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent).document.settings.label_outline === false)
  await button('Undo weight').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent).document.settings.label_outline === true)
  await button('Redo weight').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent).document.settings.label_outline === false)
  await button('Undo weight').click()
  await button('Save weight').click()
  await page.waitForFunction(() => JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent).session.unsaved_changes === false)
  const native = JSON.parse(readFileSync(join(work,'documents',`${id}.json`),'utf8'))
  assert.equal(native.format_version,30)
  assert.equal(native.document.settings.label_outline,true)
  await page.reload()
  await page.waitForFunction(() => document.querySelector('svg[data-structure-canvas]')?.dataset.captionBoundsReady === 'true')
  assert.equal((await snapshot()).document.settings.label_outline,true)
  assert.deepEqual((await snapshot()).document.atoms,seeded.document.atoms)
  stage = 'delayed italic 400/500 font completion follows the switched document and preserves its viewport'
  const cold = await browser.newPage()
  cold.on('pageerror', (error) => errors.push(error.message))
  let releaseFonts
  const heldFonts = new Promise((resolveFonts) => { releaseFonts = resolveFonts })
  let heldRequests = 0
  await cold.route(/\.woff2(?:\?.*)?$/, async (route) => {
    if (route.request().resourceType() !== 'font') return route.continue()
    heldRequests += 1
    await heldFonts
    await route.continue()
  })
  try {
    await cold.goto('http://127.0.0.1:5173/label-outline-check?cold', { waitUntil: 'domcontentloaded' })
    await cold.waitForFunction((id) => {
      const current = JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent)
      return current?.document.id === id && document.fonts.status === 'loading' &&
        !document.fonts.check('italic 500 72px "Geist Variable"') &&
        document.querySelector('[data-atom-label] [font-weight="500"][font-style="italic"]') !== null &&
        document.querySelector('[data-structure-canvas]')?.dataset.captionBoundsReady === 'false'
    }, coldId)
    assert.ok(heldRequests > 0, 'a real bundled variable-font response used by italic 500 labels is held')
    await cold.getByRole('button', { name: 'Open alternate italic drawing', exact: true }).click()
    await cold.waitForFunction((id) => JSON.parse(document.querySelector('[aria-label="Weight snapshot"]').textContent)?.document.id === id &&
      document.querySelector('[data-structure-canvas]')?.dataset.captionBoundsReady === 'false' &&
      document.querySelector('[data-atom-label]')?.textContent.includes('['), alternateId)
    const canvas = cold.locator('svg[data-structure-canvas]')
    await canvas.focus()
    await canvas.press('ArrowRight')
    await canvas.press('+')
    const chosenViewport = await canvas.getAttribute('viewBox')
    const fallback = await cold.locator('[data-atom-label]').evaluate((label) => ({ box: label.dataset.box,
      advance: label.querySelector('[data-chunk="text"]').getComputedTextLength() }))
    releaseFonts()
    await cold.waitForFunction(() => document.fonts.status === 'loaded' && document.fonts.check('italic 500 72px "Geist Variable"') &&
      document.querySelector('[data-structure-canvas]')?.dataset.captionBoundsReady === 'true')
    assert.equal(await canvas.getAttribute('viewBox'), chosenViewport, 'late source/font measurements cannot replace the switched document user viewport')
    const loaded = await cold.locator('[data-atom-label]').evaluate((label) => {
      const text = label.querySelector('[data-chunk="text"]'), span = text.querySelector('tspan')
      return { box: label.dataset.box, advance: text.getComputedTextLength(), weight: getComputedStyle(span).fontWeight, style: getComputedStyle(span).fontStyle, family: getComputedStyle(span).fontFamily }
    })
    assert.equal(loaded.weight, '500')
    assert.equal(loaded.style, 'italic')
    assert.match(loaded.family, /Geist/)
    assert.notEqual(loaded.advance, fallback.advance, 'the network release must change the actual query font metrics')
    assert.notEqual(loaded.box, fallback.box, 'italic bounds must be remeasured after the font arrives')
    assert.equal(JSON.parse(await cold.getByLabel('Weight snapshot', { exact: true }).textContent()).document.id, alternateId)
    await cold.getByRole('button', { name: 'Fit drawing', exact: true }).click()
    await cold.waitForFunction(() => {
      const svg = document.querySelector('svg[data-structure-canvas]')
      const label = svg?.querySelector('[data-atom-label]')
      if (!label || svg.dataset.captionBoundsReady !== 'true' || svg.dataset.captionMeasuredViewBox !== svg.getAttribute('viewBox')) return false
      const [x,y,width,height] = label.dataset.box.split(' ').map(Number), ink = label.getBBox(), view = svg.viewBox.baseVal
      return ink.width > 0 && x <= ink.x + 0.05 && y <= ink.y + 0.05 && x+width >= ink.x+ink.width-0.05 && y+height >= ink.y+ink.height-0.05 &&
        view.x <= x && view.y <= y && view.x+view.width >= x+width && view.y+view.height >= y+height
    })
  } finally {
    releaseFonts()
    await cold.close()
  }
  assert.equal(await page.getByLabel('Weight error',{exact:true}).textContent(),'')
  assert.deepEqual(errors,[])
  passed=true
  console.log('Chemical label outline: 3 families × 4/72pt × normal/bold × normal/italic × underline off/on; hollow glyph strokes, real run advances, Unicode shaping, fit/trim/hit, graph/Et, captions, UI/history/native and delayed fonts passed.')
} finally {
  await browser?.close()
  await server?.close()
  if(api?.pid && api.exitCode===null && api.signalCode===null){
    const exited=once(api,'exit'); api.kill('SIGTERM'); const force=setTimeout(()=>api.kill('SIGKILL'),5000); await exited;clearTimeout(force)
  }
  if(passed)rmSync(work,{recursive:true,force:true})
  else console.error(`Label-outline browser failed at ${stage}; artifacts: ${work}\n${log}`)
}
