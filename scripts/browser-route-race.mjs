// Reproduces the Playwright race behind #8 against a plain HTTP server (no Chemifree code):
// a `times: 1` route answers one request, and the page sends its next request right away.
// Without a guard, that next request is now and then paused by the browser while Playwright
// turns interception off, and it never reaches the server.
//
//   npm run test:browser:routes                 # with the guard: expects no stalls
//   npm run test:browser:routes -- --unguarded  # without it: expects stalls, exits 1
//
// A request counts as stalled when it has no response within STALL_MS (the server answers
// at once). Uses CHEMIFREE_BROWSER like the browser smoke.
import { createServer } from 'node:http'
import { chromium } from 'playwright-core'

import { keepRequestInterception } from './lib/request-interception.mjs'

const CYCLES = 1000
const STALL_MS = 1000
const guarded = !process.argv.includes('--unguarded')

const server = createServer((request, response) => {
  response.writeHead(200, { 'content-type': request.url === '/' ? 'text/html' : 'application/json' })
  response.end(request.url === '/' ? '<!doctype html><title>race</title>' : '{"ok":true}')
})
await new Promise((resolveListen) => server.listen(0, '127.0.0.1', resolveListen))
const base = `http://127.0.0.1:${server.address().port}`

const browser = await chromium.launch({ executablePath: process.env.CHEMIFREE_BROWSER, headless: true })
let stalled = 0
try {
  const context = await browser.newContext()
  if (guarded) await keepRequestInterception(context)
  const page = await context.newPage()
  await page.goto(base)
  for (let cycle = 0; cycle < CYCLES; cycle += 1) {
    await page.route('**/refused', (route) => route.fulfill({ status: 401, body: '{}' }), { times: 1 })
    const answered = await page.evaluate(async (stallMs) => {
      await fetch('/refused')
      try {
        await fetch(`/next?${Math.random()}`, { signal: AbortSignal.timeout(stallMs) })
        return true
      } catch {
        return false
      }
    }, STALL_MS)
    if (!answered) stalled += 1
  }
} finally {
  await browser.close()
  server.close()
}

console.log(`${guarded ? 'Guarded' : 'Unguarded'}: ${stalled} of ${CYCLES} requests stalled after a route expired.`)
if (stalled > 0) process.exitCode = 1
