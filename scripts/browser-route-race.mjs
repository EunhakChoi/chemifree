// Regression check for #8: a one-shot route that expires while the page starts its next request
// must not leave that request stalled. It drives a plain HTTP server (no Chemifree code) through
// the same `keepRequestInterception` guard the browser smoke uses.
//
//   npm run test:browser:routes             # with the guard: fails if any request stalls
//   npm run test:browser:routes -- --unguarded
//                                           # without it: reproduces the stall (expected to fail)
//
// Uses playwright-core with CHEMIFREE_BROWSER, like the browser smoke.
import assert from 'node:assert/strict'
import { createServer } from 'node:http'

import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const CYCLES = 1000
/** A request that takes this long against a local server that answers at once has stalled. */
const STALLED_MS = 1000
const guarded = !process.argv.includes('--unguarded')

const server = createServer((request, response) => {
  response.setHeader('content-type', 'application/json')
  response.end(JSON.stringify({ path: request.url }))
})
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
const browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
try {
  const context = await browser.newContext()
  if (guarded) await keepRequestInterception(context)
  const page = await context.newPage()
  await page.goto(`http://127.0.0.1:${server.address().port}/start`)
  const stalled = []
  for (let cycle = 0; cycle < CYCLES; cycle += 1) {
    // As in the smoke's simulated restart: one refused request, then the page's next request.
    await page.route('**/refused', (route) => route.fulfill({ status: 401, body: '{}' }), { times: 1 })
    const ms = await page.evaluate(async (limit) => {
      await fetch('/refused')
      const started = performance.now()
      await fetch('/next', { signal: AbortSignal.timeout(limit * 3) }).catch(() => {})
      return performance.now() - started
    }, STALLED_MS)
    if (ms > STALLED_MS) stalled.push(cycle)
  }
  console.log(`${guarded ? 'Guarded' : 'Unguarded'}: ${stalled.length} of ${CYCLES} requests stalled after a route expired.`)
  assert.deepEqual(stalled, [], 'no request stalls when a route expires')
} finally {
  await browser.close()
  server.close()
}
