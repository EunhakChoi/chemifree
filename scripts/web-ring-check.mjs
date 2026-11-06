// Real canvas, editor queue and client; HTTP is scripted. Not chemistry validation.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'
import { ringAfter, ringSelection, ringSource } from './lib/ring-fixture.mjs'

const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { StructureCanvas } from '/src/editor/structure-canvas.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.ringHarness = editor
  return <main>
    <output aria-label="Current atom count">{editor.current?.document.atoms.length ?? 'loading'}</output>
    <output aria-label="Pending ring">{String(editor.clipboardPending)}</output>
    <output aria-label="Editor error">{editor.error?.message}</output>
    <button disabled={editor.busy} onClick={() => void editor.undo()}>Undo</button>
    <button disabled={editor.busy} onClick={() => void editor.redo()}>Redo</button>
    {editor.clipboardPending && <button onClick={() => void editor.checkClipboardPaste()}>Check ring insertion outcome</button>}
    {editor.current && <div style={{ width: 760, height: 600 }}><StructureCanvas document={editor.current.document}
      tool="ring" element="C" bondKind="single" selection={editor.selection} onSelectionChange={editor.setSelection}
      activePlacement="s1" onActivePlacementChange={() => {}} onCommands={() => { throw new Error('Ring must use its dedicated attempt') }}
      onMark={() => {}} onRing={(instance, center) => void editor.insertRing(editor.current.document.id, instance, center)} disabled={editor.busy}/></div>}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{ name: 'ring-check-fixture', configureServer(vite) {
    vite.middlewares.use('/ring-check', async (_request, response) => {
      const html = await vite.transformIndexHtml('/ring-check', '<!doctype html><html><head><title>Ring check</title></head><body><div id="root"></div><script type="module" src="/ring-harness.tsx"></script></body></html>')
      response.setHeader('content-type', 'text/html')
      response.end(html)
    })
  }, resolveId(id) { if (id === '/ring-harness.tsx') return id },
  load(id) { if (id === '/ring-harness.tsx') return fixture } }],
})
let browser
try {
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const context = await browser.newContext()
  await keepRequestInterception(context)
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  let state = ringSource()
  let lost = 2
  const bodies = []
  const replies = new Map()
  const reply = (route, body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return reply(route, { header: 'x-chemifree-session', token: 'ring-service' })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return reply(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return reply(route, state)
    if (path.endsWith('/autosave')) return reply(route, state)
    if (path.endsWith('/undo') || path.endsWith('/redo')) {
      const revision = state.document.revision + 1
      const undo = path.endsWith('/undo')
      state = undo ? ringSource() : ringAfter()
      state.document.revision = revision
      state.document.selection = ringSelection()
      state.session.undo_depth = undo ? 0 : 1
      state.session.redo_depth = undo ? 1 : 0
      return reply(route, state)
    }
    assert.equal(path, '/api/v1/documents/drawing/transactions')
    const body = route.request().postDataJSON()
    bodies.push(body)
    assert.deepEqual(body.commands, [{ op: 'insert_ring', instance: 's1', center: { x: 100, y: 100 } }])
    let confirmed = replies.get(body.request_id)
    if (!confirmed) {
      assert.equal(body.expected_revision, 3)
      assert.deepEqual(body.selection_before, ringSelection())
      state = ringAfter()
      confirmed = structuredClone(state)
      replies.set(body.request_id, confirmed)
    }
    if (lost > 0) { lost -= 1; return route.abort('failed') }
    return reply(route, confirmed)
  })
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
    readText() { throw new Error('Ring insertion must not read the OS clipboard.') },
    writeText() { throw new Error('Ring insertion must not write the OS clipboard.') },
  } }))
  await page.goto(`http://127.0.0.1:${address.port}/ring-check`)
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '2' }).waitFor()
  await page.evaluate(() => window.ringHarness.setSelection(new Set(['s7', 'a1', 'b1'])))
  const canvas = page.locator('[data-structure-canvas]')
  await canvas.focus()
  for (let index = 0; index < 5; index += 1) await canvas.press('-')
  await canvas.press('ArrowRight')
  const point = await canvas.evaluate((svg) => {
    const location = new DOMPoint(100, 100).matrixTransform(svg.getScreenCTM())
    return { x: location.x, y: location.y }
  })
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await page.mouse.move(point.x + 20, point.y + 20)
  await page.mouse.up()
  assert.deepEqual(bodies, [], 'dragging the ring tool does not insert')
  await page.mouse.click(point.x, point.y)
  await page.getByLabel('Pending ring', { exact: true }).filter({ hasText: 'true' }).waitFor()
  assert.equal(state.document.atoms.length, 8, 'backend committed one ring despite losing both replies')
  assert.equal(await page.getByRole('button', { name: 'Undo', exact: true }).isDisabled(), true)
  await page.getByRole('button', { name: 'Check ring insertion outcome', exact: true }).click()
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '8' }).waitFor()
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  assert.equal(replies.size, 1)
  assert.deepEqual(await page.evaluate(() => [...window.ringHarness.selection]), ['s7', 'a1', 'b1'])
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '2' }).waitFor()
  await page.getByRole('button', { name: 'Redo', exact: true }).click()
  await page.getByLabel('Current atom count', { exact: true }).filter({ hasText: '8' }).waitFor()
  assert.deepEqual(state.document.atoms, ringAfter().document.atoms)
  assert.deepEqual(state.document.bonds, ringAfter().document.bonds)
  assert.deepEqual(errors, [])
  console.log('Ring canvas, queue, outcome recovery and history checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
