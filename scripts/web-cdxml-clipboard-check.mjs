// Real export panel, hook and client; only HTTP and browser clipboard are replaced.
// This checks UI boundaries, not scientific interchange or OS permission grants.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { InterchangeExportPanel } from '/src/components/interchange-export-panel.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.cdxmlHarness = { editor }
  return <main className="p-4"><output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    <output aria-label="Service generation">{editor.serviceGeneration}</output>
    {editor.current && <InterchangeExportPanel key={editor.current.document.id} document={editor.current.document} editor={editor} />}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{
    name: 'cdxml-clipboard-check-fixture',
    configureServer(vite) {
      vite.middlewares.use('/cdxml-clipboard-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/cdxml-clipboard-check', '<!doctype html><html><head><title>CDXML clipboard check</title><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="root"></div><script type="module" src="/cdxml-clipboard-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/cdxml-clipboard-harness.tsx') return id },
    load(id) { if (id === '/cdxml-clipboard-harness.tsx') return fixture },
  }],
})
let browser
try {
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
  const context = await browser.newContext({ viewport: { width: 390, height: 600 } })
  await keepRequestInterception(context)
  const page = await context.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_style: 'normal', label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
  const atom = (id, x) => ({ id, element: 'C', kind: 'element', query: null, charge: 0, isotope: null,
    hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y: 0 } })
  const state = { api_version: 'v1', document: { id: 'drawing', title: 'Two atoms', revision: 3,
    atoms: [atom('a1', 0), atom('a2', 30)], bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null }],
    stereo: { atoms: [], bonds: [], groups: [] }, molecule: { id: 'm1' }, settings,
    scene: { pages: [{ id: 'p1' }], instances: [{ id: 's1', page: 'p1', molecule: 'm1', positions: [{ atom: 'a1', x: 0, y: 0 }, { atom: 'a2', x: 30, y: 0 }] }],
      roots: ['s1'], objects: [{ kind: 'instance', id: 's1', parent: null, molecule: 'm1' }], references: [] }, selection: { objects: [], atoms: [], bonds: [] } },
    session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null, notes: [] }
  const before = structuredClone(state)
  const content = '<?xml version="1.0" encoding="UTF-8"?>\n<CDXML><page id="1"><fragment id="2"><n id="3" p="0 0"/><n id="4" p="30 0"/><b id="5" B="3" E="4"/></fragment></page></CDXML>\n'
  const loss = { feature: 'drawing_settings', source_path: 'document.settings', disposition: 'dropped', reason: 'Review the settings projection.', semantic_impact: 'document' }
  let digest = 'a'.repeat(64)
  let token = 'cdxml-service'
  let restart = false
  const requests = []
  const response = (route, body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/session') return response(route, { header: 'x-chemifree-session', token })
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return response(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/drawing') return response(route, state)
    if (path === '/api/v1/documents/other') return response(route, { ...state, document: { ...state.document, id: 'other' } })
    assert.equal(path, '/api/v1/operations')
    const request = route.request().postDataJSON()
    requests.push(request)
    if (restart) {
      token = 'restarted-service'
      return route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ api_version: 'v1', code: 'unauthorized', message: 'New process.' }) })
    }
    assert.equal(request.operation, 'document.export', 'copy must not import or transact')
    const reply = { contract: 1, envelope: 'operation_result', request_id: request.request_id, operation: request.operation,
      profile: 'preserve-semantics', document_revision: state.document.revision, diagnostics: [], loss_report: [loss], outcome: 'partial',
      result: { content: request.parameters.format === 'cdxml' ? content : 'VmpDRDAxMDA=', snapshot: state } }
    if (request.parameters.accepted_losses !== digest) return response(route, {
      ...reply, document_revision: null, outcome: 'error', loss_report: [], result: null,
      diagnostics: [{ code: 'loss_consent_required', message: 'Review settings.', details: { features: ['drawing_settings'], losses: [loss], loss_digest: digest } }],
    })
    return response(route, reply)
  })
  await page.addInitScript(() => {
    window.cdxmlClipboard = { writes: [], reads: 0, mode: 'ok', finish: null }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText: async () => { window.cdxmlClipboard.reads += 1; throw new Error('Copy must not read the clipboard.') },
      writeText: (text) => {
        const clipboard = window.cdxmlClipboard
        clipboard.writes.push({ text, active: navigator.userActivation.isActive })
        if (clipboard.mode === 'deny') return Promise.reject(new Error('Permission denied.'))
        if (clipboard.mode === 'hold') return new Promise((resolve) => { clipboard.finish = resolve })
        return Promise.resolve()
      },
    } })
  })
  await page.goto(`http://127.0.0.1:${address.port}/cdxml-clipboard-check`)
  const panel = page.getByRole('complementary', { name: 'CDXML and CDX export' })
  const copy = panel.getByRole('button', { name: 'Copy CDXML to clipboard', exact: true })
  const text = panel.getByLabel('Prepared CDXML text', { exact: true })
  await panel.waitFor()
  assert.equal(requests.length, 0)
  assert.deepEqual(await page.evaluate(() => window.cdxmlClipboard.writes), [])
  assert.equal(await copy.count(), 0)
  await page.evaluate(() => window.cdxmlHarness.editor.setSelection(new Set(['a1'])))
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).waitFor()
  assert.equal(await copy.count(), 0, 'a consent offer is not written content')
  assert.deepEqual(requests[0].parameters, { document_id: 'drawing', format: 'cdxml' }, 'selection cannot turn whole drawing export into fragment copy')
  // The exact digest is renewed if the server changes its consent proposal.
  digest = 'b'.repeat(64)
  await panel.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).click()
  await panel.getByText('The losses changed since you accepted them. Review them again.', { exact: true }).waitFor()
  assert.equal(await copy.count(), 0)
  await panel.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).click()
  await copy.waitFor()
  assert.equal(requests.at(-1).parameters.accepted_losses, digest)
  assert.equal(await text.inputValue(), content)
  assert.deepEqual(await page.evaluate(() => window.cdxmlClipboard.writes), [], 'preparing and accepting losses never writes')
  const exportedRequests = requests.length
  await copy.click()
  await panel.getByText('CDXML text copied.', { exact: true }).waitFor()
  assert.deepEqual(await page.evaluate(() => window.cdxmlClipboard.writes), [{ text: content, active: true }])
  assert.equal(requests.length, exportedRequests, 'the explicit copy writes reviewed bytes directly without a network await')
  assert.deepEqual(state, before, 'copy leaves document/history/server selection unchanged')

  await page.evaluate(() => { window.cdxmlClipboard.mode = 'deny' })
  await copy.click()
  await panel.getByRole('alert').filter({ hasText: 'Permission denied.' }).waitFor()
  await panel.getByRole('button', { name: 'Select CDXML text', exact: true }).click()
  assert.deepEqual(await text.evaluate((node) => [node.selectionStart, node.selectionEnd, document.activeElement === node]), [0, content.length, true])
  await text.press('Tab')
  await text.focus()
  assert.deepEqual(await text.evaluate((node) => [node.selectionStart, node.selectionEnd]), [0, content.length], 'keyboard focus selects prepared text')
  const bounds = await text.boundingBox()
  assert.ok(bounds && bounds.x >= 0 && bounds.x + bounds.width <= 390, 'manual text remains within a narrow viewport')
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)

  await page.evaluate(() => { Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined }) })
  await copy.click()
  await panel.getByRole('alert').filter({ hasText: 'Clipboard writing is unavailable.' }).waitFor()
  assert.equal(await text.inputValue(), content, 'API absence retains manual fallback')
  await panel.getByRole('button', { name: 'Preview CDX export', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and preview CDX', exact: true }).click()
  await panel.getByRole('button', { name: 'Download CDX', exact: true }).waitFor()
  assert.equal(await copy.count(), 0, 'CDX binary transport has no plain-text copy button')
  assert.equal(await text.count(), 0)

  // Reload reinstalls the boundary stub; a held old-document write must not populate a new panel.
  await page.reload()
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).click()
  await copy.waitFor()
  await page.evaluate(() => { window.cdxmlClipboard.mode = 'hold' })
  await copy.click()
  await page.waitForFunction(() => typeof window.cdxmlClipboard.finish === 'function')
  await page.evaluate(() => window.cdxmlHarness.editor.open('other'))
  await page.getByLabel('Current document', { exact: true }).filter({ hasText: 'other' }).waitFor()
  await page.evaluate(() => window.cdxmlClipboard.finish())
  assert.equal(await text.count(), 0)
  assert.equal(await panel.getByText('CDXML text copied.', { exact: true }).count(), 0)

  await page.evaluate(() => window.cdxmlHarness.editor.open('drawing'))
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).click()
  await copy.waitFor()
  // A same-revision style change still invalidates both manual and browser copy.
  state.document.settings.label_size_pt = 14
  await page.evaluate(() => window.cdxmlHarness.editor.open('drawing'))
  await panel.getByText('The drawing or service changed. Preview the export again.', { exact: true }).waitFor()
  assert.equal(await text.count(), 0)
  assert.equal(await copy.count(), 0)
  await panel.getByRole('button', { name: 'Preview CDXML export', exact: true }).click()
  await panel.getByRole('button', { name: 'Accept these losses and preview CDXML', exact: true }).click()
  await copy.waitFor()
  await copy.click()
  await panel.getByText('Copying CDXML text…', { exact: true }).waitFor()
  restart = true
  await page.evaluate(async () => {
    try { await window.cdxmlHarness.editor.inspectIdentity('drawing', 'chemical', null) } catch { /* Expected process change. */ }
  })
  await page.waitForFunction(() => window.cdxmlHarness.editor.serviceGeneration > 0)
  await page.evaluate(() => window.cdxmlClipboard.finish())
  assert.equal(await copy.count(), 0, 'a detected process change invalidates prepared copy')
  assert.equal(await text.count(), 0)
  assert.equal(await panel.getByText('CDXML text copied.', { exact: true }).count(), 0, 'held old-process success cannot restore a stale prepared copy')
  assert.equal(await page.evaluate(() => window.cdxmlClipboard.reads), 0)
  assert.deepEqual(errors, [])
  console.log('CDXML explicit text copy, renewed consent, manual fallback, narrow layout and stale-state checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
