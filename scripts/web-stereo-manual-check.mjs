// Exercise the real React hook with a rerender before its zero-delay manual request.
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createServer } from 'vite'
import { chromium } from 'playwright-core'
import { sceneCutSource } from './lib/scene-cut-fixture.mjs'

const document = sceneCutSource().document
document.bonds[0].mark = { kind: 'wedge', narrow: 'begin' }
const fixture = `
import React, {useState} from 'react'
import {createRoot} from 'react-dom/client'
import {flushSync} from 'react-dom'
import {useStereoAgreement} from '/src/hooks/use-stereo-agreement.ts'
const drawing = ${JSON.stringify(document)}
let requests = 0
function Harness() {
  const [render, setRender] = useState(0)
  const {view, checkNow} = useStereoAgreement({document: drawing, serviceGeneration: 0, dragging: false, automatic: false,
    inspect: async (id) => { requests += 1; window.asked = {id, revision: drawing.revision}; return {snapshot: drawing} }})
  return <main>
    <button onClick={() => {checkNow(); flushSync(() => setRender((value) => value + 1))}}>Check now</button>
    <output aria-label="UI render">{render}</output>
    <output aria-label="Comparison state">{view.kind}</output>
    <output aria-label="Inspection requests">{requests}</output>
  </main>
}
createRoot(document.getElementById('root')).render(<React.StrictMode><Harness/></React.StrictMode>)
`
let server
let browser
try {
  server = await createServer({
    root: fileURLToPath(new URL('../apps/web', import.meta.url)),
    configFile: fileURLToPath(new URL('../apps/web/vite.config.ts', import.meta.url)),
    server: {host: '127.0.0.1', port: 0},
    plugins: [{
      name: 'manual-stereo-render-fixture',
      configureServer(vite) {
        vite.middlewares.use('/stereo-manual-check', async (_request, response) => {
          response.setHeader('content-type', 'text/html')
          response.end(await vite.transformIndexHtml('/stereo-manual-check', '<html><head><title>Manual stereo render</title></head><body><div id="root"></div><script type="module" src="/stereo-manual-harness.tsx"></script></body></html>'))
        })
      },
      resolveId(path) { if (path === '/stereo-manual-harness.tsx') return path },
      load(path) { if (path === '/stereo-manual-harness.tsx') return fixture },
    }],
  })
  await server.listen()
  browser = await chromium.launch({executablePath: process.env.CHEMIFREE_BROWSER, headless: true})
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.goto(server.resolvedUrls.local[0] + 'stereo-manual-check')
  await page.getByLabel('Comparison state', {exact: true}).filter({hasText: 'unchecked'}).waitFor()
  assert.equal(await page.getByLabel('Inspection requests', {exact: true}).textContent(), '0')
  await page.getByRole('button', {name: 'Check now', exact: true}).click()
  await page.getByLabel('UI render', {exact: true}).filter({hasText: '1'}).waitFor()
  await page.getByLabel('Comparison state', {exact: true}).filter({hasText: 'ready'}).waitFor()
  assert.equal(await page.getByLabel('Inspection requests', {exact: true}).textContent(), '1')
  assert.deepEqual(await page.evaluate(() => window.asked), {id: document.id, revision: document.revision})
  assert.deepEqual(errors, [])
  console.log('Manual stereo Check now survives a synchronous unchanged React render in StrictMode; exactly one inspection completes.')
} finally {
  await browser?.close()
  await server?.close()
}
