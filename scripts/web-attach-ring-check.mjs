// Production workspace/canvas/hook/client. Only HTTP is scripted; no backend integration claim.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'
import { keepRequestInterception } from './lib/request-interception.mjs'
import { attachAfter, attachSource } from './lib/attach-ring-fixture.mjs'
const fixture = `
import React from 'react'
import { createRoot } from 'react-dom/client'
import { DocumentWorkspace } from '/src/components/document-workspace.tsx'
import '/src/index.css'
createRoot(document.getElementById('root')).render(<DocumentWorkspace />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{ name: 'attach-ring-fixture', configureServer(vite) {
    vite.middlewares.use('/attach-ring-check', async (_request, response) => {
      const html = await vite.transformIndexHtml('/attach-ring-check', '<!doctype html><html><head><title>Attach ring check</title></head><body><div id="root"></div><script type="module" src="/attach-ring-harness.tsx"></script></body></html>')
      response.setHeader('content-type', 'text/html')
      response.end(html)
    })
  }, resolveId(id) { if (id === '/attach-ring-harness.tsx') return id },
  load(id) { if (id === '/attach-ring-harness.tsx') return fixture } }],
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
  let state = attachSource()
  let lost = 2
  let committed
  let historyCalls = 0
  const bodies = []
  const reply = (route, body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return reply(route, { header: 'x-chemifree-session', token: 'attach-service' })
    if (path === '/api/v1/preferences') {
      const computation = { status: 'not_implemented', owner: 'preferences', followup: 'P19', note: 'Not yet.' }
      return reply(route, { api_version: 'v1', revision: 0, ui: { theme: 'system' }, new_documents: {},
        new_document_settings: state.document.settings, computation: { nmr: computation, numeric_precision: computation } })
    }
    if (path === '/api/v1/documents') return reply(route, { api_version: 'v1', documents: [{ id: 'drawing', title: state.document.title,
      revision: state.document.revision, atom_count: 4, bond_count: 3, open: true,
      unsaved_changes: state.session.unsaved_changes, recovery: null, saved_at_ms: 1 }], unreadable: [] })
    if (path === '/api/v1/trash') return reply(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing' || path.endsWith('/autosave')) return reply(route, state)
    if (path === '/api/v1/operations') {
      const body = route.request().postDataJSON()
      assert.equal(body.operation, 'chem.stereo')
      return reply(route, { contract: 1, envelope: 'operation_result', request_id: body.request_id,
        operation: body.operation, profile: 'preserve-semantics', document_revision: null, outcome: 'unsupported', result: null,
        diagnostics: [{ code: 'operation_not_implemented', severity: 'error', path: null, message: 'Inspection unavailable in this transport fixture.', details: {} }], loss_report: [] })
    }
    if (path.endsWith('/undo') || path.endsWith('/redo')) {
      historyCalls += 1
      const undo = path.endsWith('/undo')
      state = undo ? attachSource() : structuredClone(committed)
      state.document.revision = undo ? 5 : 6
      state.session.undo_depth = undo ? 0 : 1
      state.session.redo_depth = undo ? 1 : 0
      state.notes = []
      return reply(route, state)
    }
    assert.equal(path, '/api/v1/documents/drawing/transactions')
    const body = route.request().postDataJSON()
    bodies.push(body)
    assert.equal(body.commands.length, 1)
    const command = body.commands[0]
    assert.equal(command.op, 'attach_ring')
    assert.equal(command.instance, 's1')
    assert.equal(command.atom, 'a1')
    assert.equal(command.size, 5)
    assert.ok(command.direction.dx > 0)
    assert.ok(Math.abs(command.direction.dy) < 1e-8)
    if (!committed) {
      assert.equal(body.expected_revision, 3)
      state = attachAfter(5)
      state.document.selection = structuredClone(body.selection_before)
      committed = structuredClone(state)
    }
    if (lost > 0) { lost -= 1; return route.abort('failed') }
    return reply(route, committed)
  })
  await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
    readText() { throw new Error('Attachment must not read the OS clipboard.') },
    writeText() { throw new Error('Attachment must not write the OS clipboard.') },
  } }))
  await page.goto(`http://127.0.0.1:${address.port}/attach-ring-check`)
  await page.locator('[data-document-id="drawing"]').getByRole('button', { name: 'Open', exact: true }).click()
  await page.getByLabel('Document title', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Insert carbon ring', exact: true }).click()
  await page.getByRole('button', { name: 'Attach to atom', exact: true }).click()
  const five = page.getByRole('button', { name: 'Five-member carbon ring', exact: true })
  const six = page.getByRole('button', { name: 'Six-member carbon ring', exact: true })
  await five.click()
  const canvas = page.locator('[data-structure-canvas]')
  await canvas.scrollIntoViewIfNeeded()
  await canvas.focus()
  for (let index = 0; index < 3; index += 1) await canvas.press('-')
  await canvas.press('ArrowRight')
  const points = await canvas.evaluate((svg) => {
    const at = (x, y) => { const point = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM()); return { x: point.x, y: point.y } }
    return { atom: at(0, 0), end: at(60, 0), empty: at(100, 80) }
  })
  await page.mouse.click(points.atom.x, points.atom.y)
  assert.deepEqual(bodies, [], 'atom click cannot guess a direction')
  await page.mouse.move(points.empty.x, points.empty.y)
  await page.mouse.down()
  await page.mouse.move(points.empty.x + 20, points.empty.y)
  await page.mouse.up()
  assert.deepEqual(bodies, [], 'empty-space drag cannot guess an attachment atom')
  await page.mouse.move(points.atom.x, points.atom.y)
  await page.mouse.down()
  await page.mouse.move(points.end.x, points.end.y)
  assert.equal(await five.isDisabled(), true, 'gesture size is captured and locked')
  await page.mouse.up()
  const check = page.getByRole('button', { name: 'Check ring insertion outcome', exact: true })
  await check.waitFor()
  assert.equal(state.document.atoms.length, 9, 'one five-ring committed despite losing both replies')
  assert.equal(await six.isDisabled(), true)
  assert.equal(await page.getByRole('button', { name: 'Independent', exact: true }).isDisabled(), true)
  assert.equal(await page.getByRole('button', { name: 'Close document', exact: true }).isDisabled(), true)
  await check.click()
  await check.waitFor({ state: 'hidden' })
  assert.deepEqual(bodies, [bodies[0], bodies[0], bodies[0]])
  await page.getByText('stereo_configuration changed atoms[a1]; Undo restores the statement.', { exact: true }).waitFor()
  assert.deepEqual(state.document.stereo, { atoms: [], bonds: [], groups: [] })
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('[data-structure-canvas]')?.querySelectorAll('[data-bond="b9"]').length === 0)
  assert.deepEqual(state.document.stereo, attachSource().document.stereo)
  await page.getByRole('button', { name: 'Redo', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('[data-structure-canvas]')?.querySelector('[data-bond="b9"]'))
  assert.deepEqual(state.document.atoms, attachAfter(5).document.atoms)
  assert.deepEqual(state.document.bonds, attachAfter(5).document.bonds)
  assert.equal(historyCalls, 2)
  assert.deepEqual(errors, [])
  await context.close()
  process.stdout.write('Attached ring production controls, atom drag, stereo notes, history and frozen recovery checks passed.\n')
} finally {
  if (browser) await browser.close()
  await server.close()
}
