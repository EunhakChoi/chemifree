// The real import form, editor and client; only HTTP and browser clipboard are scripted.
// This checks exact transport, reviewed creation and recovery, not CDXML parser chemistry.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { CdxmlImport } from '/src/components/cdxml-import.tsx'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
import '/src/index.css'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('original') }, [])
  window.cdxmlHarness = { editor }
  return <><output aria-label="Current document">{editor.current?.document.id ?? 'loading'}</output>
    <CdxmlImport editor={editor} /></>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{
    name: 'cdxml-import-clipboard-fixture',
    configureServer(vite) {
      vite.middlewares.use('/cdxml-import-check', async (_request, response) => {
        const html = await vite.transformIndexHtml('/cdxml-import-check', '<!doctype html><html><head><title>CDXML import check</title></head><body><div id="root"></div><script type="module" src="/cdxml-import-harness.tsx"></script></body></html>')
        response.setHeader('content-type', 'text/html')
        response.end(html)
      })
    },
    resolveId(id) { if (id === '/cdxml-import-harness.tsx') return id },
    load(id) { if (id === '/cdxml-import-harness.tsx') return fixture },
  }],
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
  const snapshot = (id, title = id) => ({ api_version: 'v1', document: { id, title, revision: 0, atoms: [], bonds: [] },
    session: { saved_revision: 0, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null })
  const original = snapshot('original')
  const requests = []
  const creationPosts = []
  const commits = new Map()
  let token = 'cdxml-process-1'
  let loseCreation = false
  let dirtyOriginal = false
  let draftWrites = 0
  const source = '\ufeff<?xml version="1.0"?>\r\n<CDXML><page id="7"><fragment id="9"/><fragment id="12"/></page></CDXML>\r\n'
  const choices = [
    { page_id: 7, fragment_id: 9, page_path: 'page7', fragment_path: 'page7/fragment9' },
    { page_id: 7, fragment_id: 12, page_path: 'page7', fragment_path: 'page7/fragment12' },
  ]
  const mapping = { document: { source_id: null, source_path: 'document', target_id: 'created' },
    page: { source_id: 7, source_path: 'page7', target_id: 'p1' },
    fragment: { source_id: 12, source_path: 'page7/fragment12', target_id: 'm1' }, atoms: [], bonds: [] }
  const losses = [{ feature: 'other_content', source_path: 'page7/fragment9', disposition: 'dropped', semantic_impact: 'document', reason: 'Only the chosen molecule is projected.' }]
  const respond = (route, value, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(value) })
  await context.route('**/api/v1/**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    if (path === '/api/v1/session') return respond(route, { header: 'x-chemifree-session', token })
    if (path === '/api/v1/operations' && request.method() === 'POST' && request.postDataJSON().parameters?.mode === 'create') {
      creationPosts.push({ body: request.postDataJSON(), token: request.headers()['x-chemifree-session'] })
    }
    if (request.method() !== 'GET' && request.headers()['x-chemifree-session'] !== token) return respond(route, { api_version: 'v1', code: 'unauthorized', message: 'Session changed.' }, 401)
    if (path === '/api/v1/documents' || path === '/api/v1/trash') return respond(route, { api_version: 'v1', documents: [], unreadable: [] })
    if (path === '/api/v1/documents/original/autosave') {
      draftWrites += 1
      return respond(route, { api_version: 'v1', code: 'storage_failed', message: 'Draft is not writable.' }, 403)
    }
    if (path.startsWith('/api/v1/documents/')) {
      const id = path.split('/').at(-1)
      if (id === 'original') {
        const shown = structuredClone(original)
        if (dirtyOriginal) {
          shown.document.revision = 1
          shown.session.unsaved_changes = true
          shown.session.undo_depth = 1
        }
        return respond(route, shown)
      }
      const created = [...commits.values()].find((value) => value.document.id === id)
      assert.ok(created, `unexpected document read ${path}`)
      return respond(route, created)
    }
    assert.equal(path, '/api/v1/operations')
    const body = request.postDataJSON()
    requests.push(body)
    assert.equal(body.operation, 'document.import')
    assert.equal(body.parameters.format, 'cdxml', 'the bridge never sniffs another format')
    const envelope = (result, loss_report = []) => ({ contract: 1, envelope: 'operation_result', request_id: body.request_id,
      operation: body.operation, profile: 'preserve-semantics', document_revision: null,
      outcome: loss_report.length ? 'partial' : 'ok', diagnostics: [], loss_report, result })
    if (body.parameters.mode === 'read') {
      if (!body.parameters.source.includes('<CDXML>')) return respond(route, { ...envelope(null), outcome: 'error',
        diagnostics: [{ code: 'invalid_cdxml', message: 'Not a CDXML document.' }] })
      return respond(route, envelope({ choices }))
    }
    assert.equal(body.parameters.source, source, 'BOM, CRLF and trailing source bytes are retained')
    assert.equal(body.parameters.page_id, 7)
    assert.equal(body.parameters.fragment_id, 12)
    if (body.parameters.mode === 'preview') return respond(route, envelope({ document: { atoms: [], bonds: [] }, mapping, preview_digest: 'a'.repeat(64) }, losses))
    assert.equal(body.parameters.mode, 'create')
    assert.equal(body.parameters.preview_digest, 'a'.repeat(64))
    assert.equal(body.parameters.accept_loss, true)
    let created = commits.get(body.request_id)
    if (!created) {
      created = snapshot(`created-${commits.size + 1}`, body.parameters.title)
      commits.set(body.request_id, created)
    }
    if (loseCreation) return route.abort('connectionreset')
    return respond(route, envelope(created, losses))
  })
  await page.addInitScript(() => {
    window.cdxmlClipboard = { text: '', reads: [], deny: false, defer: false }
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: {
      readText() {
        const state = window.cdxmlClipboard
        state.reads.push(navigator.userActivation.isActive)
        if (state.deny) return Promise.reject(new DOMException('Denied', 'NotAllowedError'))
        if (state.defer) return new Promise((resolve) => { state.finish = resolve })
        return Promise.resolve(state.text)
      },
    } })
  })
  await page.goto(`http://127.0.0.1:${address.port}/cdxml-import-check`)
  await page.getByLabel('Current document').filter({ hasText: 'original' }).waitFor()
  assert.equal(requests.length, 0)
  assert.deepEqual(await page.evaluate(() => window.cdxmlClipboard.reads), [])
  const field = page.getByLabel('CDXML source text', { exact: true })
  const read = page.getByRole('button', { name: 'Read CDXML clipboard', exact: true })
  const inspect = page.getByRole('button', { name: 'Read CDXML pages and molecules', exact: true })
  await page.evaluate(() => { window.cdxmlClipboard.deny = true })
  await read.click()
  await page.getByRole('alert').filter({ hasText: 'Denied' }).waitFor()
  await field.fill('manual CDXML text')
  assert.equal(await inspect.isEnabled(), true, 'manual entry survives denied clipboard permissions')
  assert.equal(requests.length, 0)
  await page.evaluate(() => { window.cdxmlClipboard.deny = false; window.cdxmlClipboard.defer = true })
  await read.click()
  await field.fill('manual edit wins')
  await page.evaluate(() => { window.cdxmlClipboard.finish('stale clipboard text'); window.cdxmlClipboard.defer = false })
  assert.equal(await field.inputValue(), 'manual edit wins')
  await field.fill('é'.repeat(120 * 1024 / 2))
  assert.equal(await inspect.isEnabled(), true)
  await field.fill('é'.repeat(120 * 1024 / 2 + 1))
  assert.equal(await inspect.isDisabled(), true, 'UTF-8 byte cap, not character count')
  assert.equal(requests.length, 0)
  await field.fill('VmpDRDAxMDA=')
  await inspect.click()
  await page.getByRole('alert').filter({ hasText: 'Not a CDXML' }).waitFor()
  assert.equal(commits.size, 0)

  async function prepare() {
    await page.evaluate((text) => { window.cdxmlClipboard.text = text }, source)
    await read.click()
    // Textarea rendering follows browser line-ending rules; transport still uses
    // the original clipboard string, asserted at every service boundary above.
    await page.waitForFunction((expected) => document.getElementById('cdxml-text').value === expected, source.replaceAll('\r\n', '\n'))
    const before = requests.length
    await inspect.click()
    await page.getByLabel('Page and molecule', { exact: true }).waitFor()
    assert.equal(requests[before].parameters.source, source)
    await page.getByLabel('Page and molecule', { exact: true }).selectOption('1')
    await page.getByLabel('New drawing title', { exact: true }).fill('Clipboard molecule')
    await page.getByRole('button', { name: 'Preview import', exact: true }).click()
    await page.getByTestId('cdxml-preview').waitFor()
    await page.getByText('Only the chosen molecule is projected.', { exact: false }).waitFor()
  }
  await prepare()
  assert.equal(commits.size, 0, 'read and preview never create')
  loseCreation = true
  await page.getByRole('button', { name: 'Accept losses and create drawing', exact: true }).click()
  const check = page.getByRole('button', { name: 'Check CDXML creation outcome', exact: true })
  await check.waitFor()
  await page.waitForFunction(() => !window.cdxmlHarness.editor.busy)
  assert.equal(commits.size, 1)
  assert.equal(await field.isDisabled(), true, 'uncertain creation cannot be redirected to new source')
  const first = requests.find((request) => request.parameters.mode === 'create')
  loseCreation = false
  await check.click()
  await page.getByLabel('Current document').filter({ hasText: 'created-1' }).waitFor()
  assert.equal(commits.size, 1)
  const creations = requests.filter((request) => request.parameters.mode === 'create')
  assert.deepEqual(creations, [first, first], 'outcome replay keeps the exact creation request and ID')
  assert.deepEqual(original, snapshot('original'), 'the existing drawing was never appended or edited')
  assert.equal(await field.inputValue(), '')

  await prepare()
  loseCreation = true
  await page.getByRole('button', { name: 'Accept losses and create drawing', exact: true }).click()
  await check.waitFor()
  await page.waitForFunction(() => !window.cdxmlHarness.editor.busy)
  const beforeRestart = requests.length
  token = 'cdxml-process-2'
  await page.evaluate(async () => { await window.cdxmlHarness.editor.inspectValence('original').catch(() => {}) })
  await page.waitForFunction(() => window.cdxmlHarness.editor.serviceGeneration > 0)
  await check.click()
  await page.getByRole('button', { name: 'Refresh Saved documents', exact: true }).waitFor()
  assert.equal(requests.length, beforeRestart, 'a restarted service never receives another creation attempt')
  assert.equal(commits.size, 2)
  await page.getByRole('button', { name: 'Refresh Saved documents', exact: true }).click()
  await page.getByRole('button', { name: 'I checked Saved documents; clear this attempt', exact: true }).click()
  dirtyOriginal = true
  await page.evaluate(() => { void window.cdxmlHarness.editor.open('original') })
  await page.waitForFunction(() => window.cdxmlHarness.editor.current?.session.unsaved_changes === true)
  await prepare()
  loseCreation = false
  await page.getByRole('button', { name: 'Accept losses and create drawing', exact: true }).click()
  await page.getByLabel('Current document').filter({ hasText: 'created-3' }).waitFor()
  await page.waitForFunction(() => window.cdxmlHarness.editor.error?.code === 'draft_not_written')
  assert.ok(draftWrites > 0)
  assert.equal(commits.size, 3)
  assert.equal(await check.count(), 0, 'confirmed creation is not retried because the old drawing draft failed')
  assert.equal(await field.inputValue(), '')
  await prepare()
  const postsBeforeFirstSendRestart = creationPosts.length
  token = 'cdxml-process-3' // Unobserved restart after preview, before the first create POST.
  await page.getByRole('button', { name: 'Accept losses and create drawing', exact: true }).click()
  await page.getByRole('button', { name: 'Refresh Saved documents', exact: true }).waitFor()
  assert.equal(creationPosts.length, postsBeforeFirstSendRestart + 1)
  assert.equal(creationPosts.at(-1).token, 'cdxml-process-2', 'only the old authorization is attempted')
  assert.equal(commits.size, 3, 'a first-send restart never creates an unreviewed new-process drawing')
  assert.equal(await page.getByLabel('Current document').textContent(), 'created-3')
  assert.deepEqual(await page.evaluate(() => window.cdxmlClipboard.reads), [true, true, true, true, true, true])
  assert.deepEqual(errors, [])
  console.log('CDXML exact text, explicit source/consent, new-drawing creation and process-bound recovery checks passed.')
} finally {
  await browser?.close()
  await server.close()
}
