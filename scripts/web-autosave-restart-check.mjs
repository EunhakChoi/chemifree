// Real editor/autosave hooks and DocumentClient; only HTTP and browser time are controlled.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'
import { ringSource } from './lib/ring-fixture.mjs'

const fixture = `
import React, { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { useDocumentEditor } from '/src/hooks/use-document-editor.ts'
function Harness() {
  const editor = useDocumentEditor()
  useEffect(() => { void editor.open('drawing') }, [])
  window.autosaveHarness = editor
  return <main>
    <output aria-label="Drawing">{editor.current?.document.title ?? 'loading'}</output>
    <output aria-label="Editor error">{editor.error?.code ?? 'none'}</output>
    <output aria-label="Autosave">{editor.autosave.state.kind}</output>
    <button onClick={() => void editor.apply([{ op: 'rename', title: 'Edit' }])}>Edit</button>
    <button onClick={() => editor.autosave.retryNow()}>Retry autosave</button>
    {editor.error && <button onClick={() => void editor.reload()}>Reload</button>}
  </main>
}
createRoot(document.getElementById('root')).render(<Harness />)
`
const server = await createServer({
  root: fileURLToPath(new URL('../apps/web', import.meta.url)),
  configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, strictPort: false },
  plugins: [{ name: 'autosave-restart-fixture', configureServer(vite) {
    vite.middlewares.use('/autosave-check', async (_request, response) => {
      const html = await vite.transformIndexHtml('/autosave-check', '<!doctype html><html><head><title>Autosave check</title></head><body><div id="root"></div><script type="module" src="/autosave-harness.tsx"></script></body></html>')
      response.setHeader('content-type', 'text/html')
      response.end(html)
    })
  }, resolveId(id) { if (id === '/autosave-harness.tsx') return id },
  load(id) { if (id === '/autosave-harness.tsx') return fixture } }],
})
const deferred = () => {
  let resolve
  const promise = new Promise((done) => { resolve = done })
  return { promise, resolve }
}
const json = (route, body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })
let browser
try {
  await server.listen()
  const address = server.httpServer.address()
  assert.ok(address && typeof address !== 'string')
  browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })

  async function scenario({ restart, heldDraft = false, failedRead = false, restartDuringRead = false }) {
    const context = await browser.newContext()
    try {
      await keepRequestInterception(context)
      const page = await context.newPage()
      const errors = []
      page.on('pageerror', (error) => errors.push(error.message))
      await page.clock.install({ time: new Date('2026-01-01T00:00:00Z') })
      await page.clock.pauseAt(new Date('2026-01-01T00:00:01Z'))
      const source = ringSource()
      source.document.title = 'Unsaved original'
      source.session.unsaved_changes = true
      source.session.saved_revision = 2
      source.session.undo_depth = 1
      const replacement = structuredClone(source)
      replacement.document.title = 'Replacement process'
      replacement.document.atoms[0].element = 'N'
      // Same ID and revision deliberately do not establish the same drawing.
      const savedReplacement = structuredClone(replacement)
      savedReplacement.session.draft_revision = replacement.document.revision
      const savedSource = structuredClone(source)
      savedSource.session.draft_revision = source.document.revision
      let token = 'process-1'
      let editing = false
      let reads = 0
      let drafts = 0
      const draftArrived = deferred()
      const releaseDraft = deferred()
      const draftFinished = deferred()
      const refreshed = deferred()
      const readArrived = deferred()
      const releaseRead = deferred()
      await context.route('**/api/v1/**', async (route) => {
        const request = route.request()
        const path = new URL(request.url()).pathname
        if (path === '/api/v1/session') return json(route, { header: 'x-chemifree-token', token })
        if (path === '/api/v1/documents') return json(route, { api_version: 'v1', documents: [] })
        if (path === '/api/v1/trash') return json(route, { api_version: 'v1', documents: [] })
        if (path === '/api/v1/operations' && restartDuringRead) {
          token = 'process-2'
          return json(route, { api_version: 'v1', code: 'unauthorized', message: 'Old process token' }, 401)
        }
        if (path.endsWith('/transactions')) {
          editing = true
          if (restart) {
            token = 'process-2'
            return json(route, { api_version: 'v1', code: 'unauthorized', message: 'Old process token' }, 401)
          }
          return json(route, { api_version: 'v1', code: 'revision_conflict', message: 'Changed elsewhere', current_revision: 4 }, 409)
        }
        if (path.endsWith('/autosave')) {
          drafts += 1
          draftArrived.resolve()
          if (heldDraft && drafts === 1) await releaseDraft.promise
          // A different same-revision reply makes the old hook re-read automatically.
          await json(route, restart || drafts === 1 ? savedReplacement : savedSource)
          draftFinished.resolve()
          return
        }
        assert.equal(path, '/api/v1/documents/drawing', 'unexpected HTTP request')
        reads += 1
        if (restart) return json(route, editing ? replacement : source)
        if (reads === 1) return json(route, source)
        if (restartDuringRead) {
          readArrived.resolve()
          await releaseRead.promise
          await json(route, savedReplacement)
          refreshed.resolve()
          return
        }
        await json(route, failedRead
          ? { api_version: 'v1', code: 'storage_failed', message: 'Read failed' }
          : savedSource, failedRead ? 503 : 200)
        refreshed.resolve()
      })
      await page.goto(`http://127.0.0.1:${address.port}/autosave-check`)
      await page.getByLabel('Drawing', { exact: true }).filter({ hasText: 'Unsaved original' }).waitFor()
      await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'waiting' }).waitFor()
      if (heldDraft) {
        await page.clock.runFor(1000)
        await draftArrived.promise
      }
      await page.getByRole('button', { name: 'Edit', exact: true }).click()
      const error = restart ? 'service_restarted' : 'revision_conflict'
      await page.getByLabel('Editor error', { exact: true }).filter({ hasText: error }).waitFor()
      if (restart) {
        const afterEditReads = reads
        if (heldDraft) {
          releaseDraft.resolve()
          await draftFinished.promise
          await page.evaluate(() => window.autosaveHarness.autosave.flush())
        }
        await page.getByRole('button', { name: 'Retry autosave', exact: true }).click()
        await page.clock.runFor(5000)
        await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'stopped' }).waitFor()
        assert.equal(drafts, heldDraft ? 1 : 0, 'no draft is sent to the replacement process before reconciliation')
        assert.equal(reads, afterEditReads, 'an obsolete draft reply must not trigger a background read')
        assert.equal(await page.getByLabel('Editor error', { exact: true }).textContent(), error)
        assert.deepEqual(await page.evaluate(() => window.autosaveHarness.current.document), source.document,
          'restart and late draft replies preserve the entire unsaved drawing')
        assert.equal(await page.getByRole('button', { name: 'Reload', exact: true }).count(), 1)
        await page.getByRole('button', { name: 'Reload', exact: true }).click()
        await page.getByLabel('Drawing', { exact: true }).filter({ hasText: 'Replacement process' }).waitFor()
        await page.getByLabel('Editor error', { exact: true }).filter({ hasText: 'none' }).waitFor()
        // Explicit reload adopts the new process, after which drafting can resume.
        const nextDraft = page.waitForResponse((response) => response.url().endsWith('/autosave'))
        await page.getByRole('button', { name: 'Retry autosave', exact: true }).click()
        await nextDraft
        await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'saved' }).waitFor()
        assert.equal(drafts, heldDraft ? 2 : 1)
      } else {
        await page.clock.runFor(1000)
        if (restartDuringRead) {
          await readArrived.promise
          await page.evaluate(async () => {
            try {
              await window.autosaveHarness.inspectIdentity('drawing', 'chemical', null)
              throw new Error('Inspection must report the restart')
            } catch (error) {
              if (error.code !== 'service_restarted') throw error
            }
          })
          releaseRead.resolve()
        }
        await refreshed.promise
        const expectedState = restartDuringRead ? 'stopped' : failedRead ? 'failed' : 'saved'
        await page.waitForFunction((expected) => !window.autosaveHarness.busy &&
          window.autosaveHarness.autosave.state.kind === expected, expectedState)
        assert.equal(await page.getByLabel('Editor error', { exact: true }).textContent(), restartDuringRead ? 'service_restarted' : error,
          'background refresh success and failure preserve the existing page error')
        assert.deepEqual(await page.evaluate(() => window.autosaveHarness.current.document), source.document)
      }
      assert.deepEqual(errors, [])
    } finally {
      await context.close()
    }
  }
  await scenario({ restart: true })
  await scenario({ restart: true, heldDraft: true })
  await scenario({ restart: false })
  await scenario({ restart: false, failedRead: true })
  await scenario({ restart: false, restartDuringRead: true })

  // A background refresh queued in the old generation must not stop a successful
  // same-content edit recovery ahead of it. The real queue creates this ordering.
  const context = await browser.newContext()
  try {
    await keepRequestInterception(context)
    const page = await context.newPage()
    const errors = []
    page.on('pageerror', (error) => errors.push(error.message))
    await page.clock.install({ time: new Date('2026-01-01T00:00:00Z') })
    await page.clock.pauseAt(new Date('2026-01-01T00:00:01Z'))
    const source = ringSource()
    source.document.title = 'Unsaved original'
    source.session.unsaved_changes = true
    source.session.saved_revision = 2
    source.session.undo_depth = 1
    const edited = structuredClone(source)
    edited.document.title = 'Edit'
    edited.document.revision = 4
    edited.session.undo_depth = 2
    edited.session.draft_revision = 3
    const saved = structuredClone(edited)
    saved.session.draft_revision = 4
    const mismatched = structuredClone(source)
    mismatched.document.title = 'Different draft view'
    mismatched.session.draft_revision = 3
    const editArrived = deferred()
    const releaseEdit = deferred()
    let token = 'process-1'
    let edits = 0
    let drafts = 0
    let reads = 0
    await context.route('**/api/v1/**', async (route) => {
      const path = new URL(route.request().url()).pathname
      if (path === '/api/v1/session') return json(route, { header: 'x-chemifree-token', token })
      if (path === '/api/v1/documents' || path === '/api/v1/trash') return json(route, { api_version: 'v1', documents: [] })
      if (path.endsWith('/transactions')) {
        edits += 1
        if (edits === 1) {
          editArrived.resolve()
          await releaseEdit.promise
          token = 'process-2'
          return json(route, { api_version: 'v1', code: 'unauthorized', message: 'Old process token' }, 401)
        }
        assert.equal(route.request().headers()['x-chemifree-token'], 'process-2')
        return json(route, edited)
      }
      if (path.endsWith('/autosave')) {
        drafts += 1
        return json(route, drafts === 1 ? mismatched : saved)
      }
      assert.equal(path, '/api/v1/documents/drawing')
      reads += 1
      return json(route, source)
    })
    await page.goto(`http://127.0.0.1:${address.port}/autosave-check`)
    await page.getByLabel('Drawing', { exact: true }).filter({ hasText: 'Unsaved original' }).waitFor()
    await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'waiting' }).waitFor()
    await page.getByRole('button', { name: 'Edit', exact: true }).click()
    await editArrived.promise
    await page.clock.runFor(1000)
    await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'failed' }).waitFor()
    releaseEdit.resolve()
    await page.getByLabel('Drawing', { exact: true }).filter({ hasText: 'Edit' }).waitFor()
    await page.waitForFunction(() => !window.autosaveHarness.busy)
    await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'waiting' }).waitFor()
    assert.equal(edits, 2, 'same-content restart recovery resends the original edit once')
    assert.equal(reads, 2, 'the obsolete queued refresh is discarded before HTTP')
    assert.equal(await page.getByLabel('Editor error', { exact: true }).textContent(), 'none',
      'obsolete refresh must not report a new restart after successful edit recovery')
    const nextDraft = page.waitForResponse((response) => response.url().endsWith('/autosave'))
    await page.clock.runFor(1000)
    await nextDraft
    await page.getByLabel('Autosave', { exact: true }).filter({ hasText: 'saved' }).waitFor()
    assert.equal(drafts, 2, 'drafting resumes after same-content recovery without another Reload')
    assert.deepEqual(await page.evaluate(() => window.autosaveHarness.current.document), edited.document)
    assert.deepEqual(errors, [])
  } finally {
    await context.close()
  }
  console.log('Autosave process ownership, late replies, explicit reload and background error preservation passed.')
} finally {
  await browser?.close()
  await server.close()
}
