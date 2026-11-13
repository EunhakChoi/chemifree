// Checks the local product server (P87) end to end. chemifree-api serves the built web app on its
// own origin; a document is created and saved through the browser, and the CLI reads it back from
// the same data directory. The lifecycle rules hold: one server per data directory (exit 4), a port
// in use (exit 3), recovery after SIGKILL (the lock is released), and after SIGTERM (unsaved work is
// kept as a draft that the next start offers). Each check prints `ok <name>`. The first failure
// prints `FAILED <name>: <message>` and the most recent server's standard error, then exits 1.
//
// The check builds nothing. It runs the binaries and the web build that the scripts produced:
//   npm run build:product && npm run test:product
// CI runs the debug binaries instead, so the browser job needs no release compile:
//   npm run build:web && cargo build --locked --package chemifree-api --package chemifree-cli
//   npm run test:product -- --profile debug
// Chromium is Playwright's build (`npx playwright-core install chromium`) unless CHEMIFREE_BROWSER
// names a Chrome or Chromium executable, as in scripts/browser-smoke.mjs.

import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'

const REPO_ROOT = fileURLToPath(new URL('..', import.meta.url))
const WEB_DIR = resolve(REPO_ROOT, 'apps', 'web', 'dist')
const CSP = "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
const SECURITY_HEADERS = {
  'content-security-policy': CSP,
  'x-content-type-options': 'nosniff',
  'referrer-policy': 'no-referrer',
  'cross-origin-opener-policy': 'same-origin',
}
const IMMUTABLE_CACHE = 'public, max-age=31536000, immutable'
const LISTENING = /listening on (http:\/\/127\.0\.0\.1:(\d+))/
const PORT_IN_USE_STATUS = 3
const DATA_DIR_IN_USE_STATUS = 4
const STARTUP_MS = 15_000
const EXIT_MS = 15_000
const HTTP_MS = 10_000
const UI_MS = 15_000
const CLI_MS = 30_000
// Variables that would change what the child processes read: the check passes every setting by flag.
const CLEARED_ENV = [
  'CHEMIFREE_DATA_DIR',
  'CHEMIFREE_WEB_DIR',
  'CHEMIFREE_COMPUTE_CONCURRENCY',
  'CHEMIFREE_MODEL_DATA_ROOT',
  'CHEMIFREE_MODEL_TRUSTED_REGISTRY',
  'CHEMIFREE_LOG_REQUESTS',
]
const USAGE = 'usage: node scripts/product-server-check.mjs [--profile release|debug]'
const TIMED_OUT = Symbol('timed out')

const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms))
const show = (value) => JSON.stringify(value) ?? String(value)
const messageOf = (error) => (error instanceof Error ? error.message : String(error))
const countOf = (list) => (Array.isArray(list) ? list.length : null)
const listItem = (page, id) => page.locator(`li[data-document-id=${JSON.stringify(id)}]`)
const isDocumentCreate = (response) =>
  response.request().method() === 'POST' && new URL(response.url()).pathname === '/api/v1/documents'

function expectEqual(actual, expected, what) {
  if (actual !== expected) throw new Error(`${what} is ${show(actual)}, expected ${show(expected)}`)
}

function expectHeaders(response, what, expected) {
  for (const [name, value] of Object.entries(expected)) {
    expectEqual(response.headers.get(name), value, `${what} ${name} header`)
  }
}

/** Resolves like `promise`, or with TIMED_OUT after `timeoutMs`. The timer never outlives the wait. */
function timeLimit(promise, timeoutMs) {
  let timer
  const limit = new Promise((resolveLimit) => {
    timer = setTimeout(() => resolveLimit(TIMED_OUT), timeoutMs)
  })
  return Promise.race([promise, limit]).finally(() => clearTimeout(timer))
}

/** Fetches with a time limit, so a server that stops answering fails the check instead of hanging it. */
function fetchLimited(url) {
  return fetch(url, { signal: AbortSignal.timeout(HTTP_MS) })
}

async function getJson(url) {
  const response = await fetchLimited(url)
  expectEqual(response.status, 200, `GET ${url} status`)
  return response.json()
}

/** The profile the arguments name: `release` by default, `debug` on request, otherwise null. */
function parseProfile(args) {
  if (args.length === 0) return 'release'
  if (args.length === 2 && args[0] === '--profile' && (args[1] === 'release' || args[1] === 'debug')) {
    return args[1]
  }
  return null
}

function childEnvironment() {
  const env = { ...process.env }
  for (const name of CLEARED_ENV) delete env[name]
  return env
}

function createContext(profile) {
  return {
    profile,
    current: 'setup',
    reported: false,
    cleanup: null,
    workDir: null,
    dataDir: null,
    otherDataDir: null,
    browser: null,
    browserLaunch: null,
    apiPath: null,
    cliPath: null,
    childEnv: childEnvironment(),
    servers: new Set(),
    lastServer: null,
    primary: null,
    documentId: null,
  }
}

/** The directory cargo builds into, as `cargo metadata` reports it (CARGO_TARGET_DIR included). */
function cargoTargetDirectory() {
  const result = spawnSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  })
  if (result.error) throw new Error(`cargo metadata could not run: ${result.error.message}`)
  if (result.status !== 0) throw new Error(`cargo metadata failed: ${result.stderr.trim()}`)
  const { target_directory: targetDirectory } = JSON.parse(result.stdout)
  if (typeof targetDirectory !== 'string') throw new Error('cargo metadata reported no target_directory')
  return targetDirectory
}

/** Sets the binary paths for the profile and returns the required paths that do not exist. */
function locateBuild(ctx) {
  const suffix = process.platform === 'win32' ? '.exe' : ''
  const binDir = join(cargoTargetDirectory(), ctx.profile)
  ctx.apiPath = join(binDir, `chemifree-api${suffix}`)
  ctx.cliPath = join(binDir, `chemifree${suffix}`)
  return [ctx.apiPath, ctx.cliPath, join(WEB_DIR, 'index.html')].filter((path) => !existsSync(path))
}

function buildHint(profile) {
  return profile === 'release'
    ? 'run "npm run build:product" first'
    : 'run "cargo build --locked --package chemifree-api --package chemifree-cli && npm run build:web" first'
}

/**
 * Refuses to start a process once cleanup has begun. A process started after cleanup took its snapshot
 * of running servers would outlive the run, because the run has already decided what to stop.
 */
function assertRunning(ctx) {
  if (ctx.cleanup !== null) throw new Error('the run is ending, so no process is started')
}

/**
 * Starts a process whose standard error is recorded. `exited` resolves with how it ended once its
 * output is closed. It listens for `close`, not `exit`, because standard error can still be flowing
 * after `exit`. A process that never started resolves with its spawn error.
 */
function spawnRecorded(ctx, command, args, env) {
  assertRunning(ctx)
  const child = spawn(command, args, { env, stdio: ['ignore', 'ignore', 'pipe'] })
  const server = { child, args, stderr: '', outcome: null, baseUrl: null, port: null, exited: null }
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (text) => {
    server.stderr += text
  })
  server.exited = new Promise((resolveExit) => {
    child.once('close', (code, signal) => resolveExit({ code, signal }))
    child.on('error', (error) => {
      // An error from a process that did start (a failed kill) does not end it, so it is not an outcome.
      if (child.pid === undefined) resolveExit({ code: null, signal: null, error })
    })
  })
  server.exited.then((outcome) => {
    server.outcome = outcome
    ctx.servers.delete(server)
  })
  ctx.servers.add(server)
  ctx.lastServer = server
  return server
}

/** Starts chemifree-api on `dataDir` with the web app. Port 0 lets the OS choose the port. */
function spawnApi(ctx, dataDir, port) {
  const args = ['--data-dir', dataDir, '--web-dir', WEB_DIR, '--port', String(port)]
  return spawnRecorded(ctx, ctx.apiPath, args, ctx.childEnv)
}

function describeOutcome(outcome) {
  if (outcome.error !== undefined) return outcome.error.message
  return outcome.signal === null ? `exit status ${outcome.code}` : `signal ${outcome.signal}`
}

/** Polls the server's standard error for the listening line, which names the port it bound. */
async function waitForListening(server) {
  const deadline = Date.now() + STARTUP_MS
  let match = LISTENING.exec(server.stderr)
  while (match === null) {
    if (server.outcome !== null) {
      throw new Error(`chemifree-api ended (${describeOutcome(server.outcome)}) before it listened`)
    }
    if (Date.now() >= deadline) throw new Error(`chemifree-api did not listen within ${STARTUP_MS / 1000} s`)
    await sleep(20)
    match = LISTENING.exec(server.stderr)
  }
  return match
}

async function startServer(ctx, dataDir) {
  const server = spawnApi(ctx, dataDir, 0)
  const match = await waitForListening(server)
  server.baseUrl = match[1]
  server.port = Number(match[2])
  return server
}

/** How the process ended. A process still running after EXIT_MS is killed and the wait fails. */
async function waitForExit(server) {
  const outcome = await timeLimit(server.exited, EXIT_MS)
  if (outcome === TIMED_OUT) {
    server.child.kill('SIGKILL')
    throw new Error(`chemifree-api did not exit within ${EXIT_MS / 1000} s`)
  }
  return outcome
}

/** Sends `signal` to a server that is still running, then waits for it to end. */
function stopServer(server, signal) {
  if (server.outcome === null) server.child.kill(signal)
  return waitForExit(server)
}

/** Runs the CLI on the check's data directory. A run that times out is an error, not an empty result. */
function runCli(ctx, args) {
  assertRunning(ctx)
  const result = spawnSync(ctx.cliPath, args, {
    env: { ...ctx.childEnv, CHEMIFREE_DATA_DIR: ctx.dataDir },
    encoding: 'utf8',
    timeout: CLI_MS,
  })
  if (result.error) throw new Error(`chemifree could not run: ${result.error.message}`)
  return { status: result.status, stdout: result.stdout, stderr: result.stderr }
}

/** Added to every page before its own scripts run: records each Content-Security-Policy violation. */
function watchCsp() {
  window.__cspViolations = []
  document.addEventListener('securitypolicyviolation', (event) => {
    window.__cspViolations.push(`${event.violatedDirective} ${event.blockedURI}`)
  })
}

/**
 * Clicks the structure canvas at fractions of its width and height. The click is a locator action:
 * Playwright scrolls the canvas into view first, and it fails if another element covers the point.
 */
async function clickCanvas(page, fractionX, fractionY) {
  const canvas = page.locator('svg[data-structure-canvas]')
  await canvas.waitFor()
  const box = await canvas.boundingBox()
  if (box === null) throw new Error('the structure canvas has no box')
  await canvas.click({ position: { x: box.width * fractionX, y: box.height * fractionY } })
}

/** Waits until the save-state badge reads exactly `text`. */
async function waitForSaveState(page, text) {
  await page.waitForFunction(
    (expected) => document.querySelector('[data-testid="save-state"]')?.textContent?.trim() === expected,
    text,
  )
}

/**
 * Runs `body` on a new page of the primary server, then fails if the page reported a console error,
 * an uncaught page error, a crash or a Content-Security-Policy violation. Nothing is filtered: any
 * such message is a finding. The page's context is closed on every path.
 */
async function withPage(ctx, body) {
  const context = await ctx.browser.newContext()
  try {
    await context.addInitScript(watchCsp)
    const page = await context.newPage()
    page.setDefaultTimeout(UI_MS)
    const problems = []
    page.on('console', (message) => {
      if (message.type() === 'error') problems.push(`console error: ${message.text()}`)
    })
    page.on('pageerror', (error) => problems.push(`page error: ${error.message}`))
    page.on('crash', () => problems.push('the page crashed'))
    await page.goto(`${ctx.primary.baseUrl}/`)
    await body(page)
    const violations = await page.evaluate(() => window.__cspViolations)
    if (!Array.isArray(violations)) throw new Error('the CSP listener did not run on the page')
    problems.push(...violations.map((violation) => `CSP violation: ${violation}`))
    if (problems.length > 0) throw new Error(`the page reported ${problems.join('; ')}`)
  } finally {
    await context.close()
  }
}

/** The index and a hashed script it names carry the cache and security headers, and so does the API. */
async function checkHeaders(ctx) {
  const base = ctx.primary.baseUrl
  const index = await fetchLimited(`${base}/`)
  const html = await index.text()
  expectEqual(index.status, 200, 'GET / status')
  const contentType = index.headers.get('content-type') ?? ''
  if (!contentType.startsWith('text/html')) throw new Error(`GET / content-type is ${show(contentType)}`)
  expectHeaders(index, 'GET /', { ...SECURITY_HEADERS, 'cache-control': 'no-cache' })

  const script = /\/assets\/[^"']+\.js/.exec(html)
  if (script === null) throw new Error('index.html names no script under /assets/')
  const asset = await fetchLimited(`${base}${script[0]}`)
  await asset.arrayBuffer()
  expectEqual(asset.status, 200, `GET ${script[0]} status`)
  expectHeaders(asset, `GET ${script[0]}`, { ...SECURITY_HEADERS, 'cache-control': IMMUTABLE_CACHE })

  const health = await fetchLimited(`${base}/api/v1/health`)
  await health.text()
  expectEqual(health.status, 200, 'GET /api/v1/health status')
  expectHeaders(health, 'GET /api/v1/health', SECURITY_HEADERS)
}

/** A document is created and saved through the browser UI, with no console error and no CSP violation. */
async function checkUiSave(ctx) {
  await withPage(ctx, async (page) => {
    await page.getByText('No document open', { exact: true }).waitFor()
    const created = page.waitForResponse(isDocumentCreate)
    await page.getByRole('button', { name: 'New document', exact: true }).click()
    const reply = await created
    expectEqual(reply.status(), 201, 'POST /api/v1/documents status')
    const body = await reply.json()
    if (typeof body.document?.id !== 'string') throw new Error('the created document has no id')
    ctx.documentId = body.document.id

    await page.getByLabel('Document title').waitFor()
    await clickCanvas(page, 0.3, 0.55)
    // The edit must make the session unsaved first; otherwise Save would prove nothing.
    await waitForSaveState(page, 'Unsaved changes')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await waitForSaveState(page, 'Saved')
  })
}

/** The CLI reads the saved document from the same data directory: two atoms and one bond. */
function checkCliRead(ctx) {
  const result = runCli(ctx, ['documents', 'show', ctx.documentId, '--json'])
  expectEqual(result.status, 0, `chemifree documents show exit status (stderr: ${result.stderr.trim()})`)
  const saved = JSON.parse(result.stdout).document
  expectEqual(saved?.id, ctx.documentId, 'the CLI document id')
  expectEqual(countOf(saved?.atoms), 2, 'atoms in the saved document')
  expectEqual(countOf(saved?.bonds), 1, 'bonds in the saved document')
}

/** A second server on the same data directory is refused with status 4 and names the holder. */
async function checkSecondServer(ctx) {
  const server = spawnApi(ctx, ctx.dataDir, 0)
  const outcome = await waitForExit(server)
  expectEqual(outcome.code, DATA_DIR_IN_USE_STATUS, 'second server exit status')
  if (!server.stderr.includes('already used by another Chemifree API server')) {
    throw new Error(`second server stderr does not refuse the data directory: ${server.stderr.trim()}`)
  }
}

/** A server asked for the port in use exits with status 3. It uses a data directory of its own. */
async function checkPortConflict(ctx) {
  const server = spawnApi(ctx, ctx.otherDataDir, ctx.primary.port)
  const outcome = await waitForExit(server)
  expectEqual(outcome.code, PORT_IN_USE_STATUS, 'port conflict exit status')
  if (!server.stderr.includes('is already in use')) {
    throw new Error(`port conflict stderr does not report the port: ${server.stderr.trim()}`)
  }
}

/** SIGKILL releases the lock, so the next start on the same data directory listens and lists the document. */
async function checkSigkillRestart(ctx) {
  const killed = await stopServer(ctx.primary, 'SIGKILL')
  expectEqual(killed.signal, 'SIGKILL', 'the killed server ended by signal')
  ctx.primary = await startServer(ctx, ctx.dataDir)
  const body = await getJson(`${ctx.primary.baseUrl}/api/v1/documents`)
  if (!Array.isArray(body.documents) || !body.documents.some((item) => item.id === ctx.documentId)) {
    throw new Error(`the restarted server does not list document ${ctx.documentId}`)
  }
}

/**
 * Unsaved work survives a clean stop. SIGTERM writes it as a draft, the next start offers it as
 * recoverable, and neither page reports a console error, page error or CSP violation.
 */
async function checkSigtermRecovery(ctx) {
  const id = ctx.documentId
  await withPage(ctx, async (page) => {
    await listItem(page, id).waitFor()
    await listItem(page, id).getByRole('button', { name: 'Open', exact: true }).click()
    await page.getByLabel('Document title').waitFor()
    await clickCanvas(page, 0.7, 0.35)
    await waitForSaveState(page, 'Unsaved changes')
  })
  // The edit lives only in the server's session until it is saved or drafted.
  const session = await getJson(`${ctx.primary.baseUrl}/api/v1/documents/${id}`)
  expectEqual(session.session?.unsaved_changes, true, 'the server holds the unsaved edit before SIGTERM')
  const stopped = await stopServer(ctx.primary, 'SIGTERM')
  expectEqual(stopped.code, 0, 'exit status after SIGTERM')
  const draft = join(ctx.dataDir, 'drafts', `${id}.json`)
  if (!existsSync(draft)) throw new Error(`SIGTERM left no recovery draft at ${draft}`)

  ctx.primary = await startServer(ctx, ctx.dataDir)
  await withPage(ctx, async (page) => {
    await listItem(page, id).getByText('Unsaved work found', { exact: true }).waitFor()
    await listItem(page, id).getByRole('button', { name: 'Open', exact: true }).click()
    const notice = page.getByTestId('recovery-notice')
    await notice.waitFor()
    expectEqual(await notice.getAttribute('data-state'), 'recoverable', 'the recovery notice state')
  })
  const last = await stopServer(ctx.primary, 'SIGTERM')
  expectEqual(last.code, 0, 'exit status of the restarted server after SIGTERM')
}

const CHECKS = [
  { name: 'headers', run: checkHeaders },
  { name: 'ui-save', run: checkUiSave },
  { name: 'cli-read', run: checkCliRead },
  { name: 'second-server', run: checkSecondServer },
  { name: 'port-conflict', run: checkPortConflict },
  { name: 'sigkill-restart', run: checkSigkillRestart },
  { name: 'sigterm-recovery', run: checkSigtermRecovery },
]

/** Prints the failure once: `FAILED <name>: <message>`, then the most recent server's standard error. */
function reportFailure(ctx, name, error) {
  if (ctx.reported) return
  ctx.reported = true
  console.error(`FAILED ${name}: ${messageOf(error)}`)
  if (ctx.lastServer !== null) {
    console.error(`last chemifree-api stderr (${ctx.lastServer.args.join(' ')}):`)
    console.error(ctx.lastServer.stderr.trimEnd() || '(none)')
  }
}

/** Runs the setup and then the checks in order. Returns the exit status; the caller cleans up. */
async function run(ctx) {
  try {
    const missing = locateBuild(ctx)
    if (missing.length > 0) {
      console.error(`product-server-check: missing ${missing.join(', ')}; ${buildHint(ctx.profile)}`)
      return 1
    }
    ctx.workDir = mkdtempSync(join(tmpdir(), 'chemifree-product-'))
    ctx.dataDir = join(ctx.workDir, 'data')
    ctx.otherDataDir = join(ctx.workDir, 'other-data')
    // Playwright's own signal handlers call process.exit once the browser closes, which can end the
    // process before this check's cleanup removes the work directory. The check handles signals itself.
    ctx.browserLaunch = chromium.launch({
      executablePath: process.env.CHEMIFREE_BROWSER || undefined,
      headless: true,
      handleSIGHUP: false,
      handleSIGINT: false,
      handleSIGTERM: false,
    })
    ctx.browser = await ctx.browserLaunch
    console.log(`product-server-check (${ctx.profile}): node ${process.version}, chromium ${ctx.browser.version()}`)
    ctx.primary = await startServer(ctx, ctx.dataDir)
  } catch (error) {
    reportFailure(ctx, 'setup', error)
    return 1
  }
  for (const check of CHECKS) {
    ctx.current = check.name
    try {
      await check.run(ctx)
    } catch (error) {
      reportFailure(ctx, check.name, error)
      return 1
    }
    console.log(`ok ${check.name}`)
  }
  console.log(`product-server-check: all ${CHECKS.length} checks passed (${ctx.profile})`)
  return 0
}

/**
 * Closes the browser, or the launch still in progress when the run ends. Returns the problem if
 * closing failed, otherwise null.
 */
async function closeBrowser(ctx) {
  if (ctx.browserLaunch === null) return null
  let browser
  try {
    browser = await ctx.browserLaunch
  } catch {
    // The setup reported the failed launch, and a failed launch leaves no browser to close.
    return null
  }
  try {
    await browser.close()
    return null
  } catch (error) {
    return `closing the browser: ${messageOf(error)}`
  }
}

/** Kills any server still running, closes the browser and removes the work directory. */
async function cleanup(ctx) {
  const problems = []
  const survivors = [...ctx.servers]
  for (const server of survivors) server.child.kill('SIGKILL')
  await Promise.all(survivors.map((server) => timeLimit(server.exited, EXIT_MS)))
  const browserProblem = await closeBrowser(ctx)
  if (browserProblem !== null) problems.push(browserProblem)
  if (ctx.workDir !== null) {
    try {
      rmSync(ctx.workDir, { recursive: true, force: true })
    } catch (error) {
      problems.push(`removing ${ctx.workDir}: ${messageOf(error)}`)
    }
  }
  return problems
}

/** Cleanup runs once, whichever path asks for it first. */
function cleanupOnce(ctx) {
  ctx.cleanup ??= cleanup(ctx)
  return ctx.cleanup
}

/** Prints what cleanup could not do. Every path that ends the run reports it. */
function reportCleanup(problems) {
  for (const problem of problems) console.error(`cleanup: ${problem}`)
}

/** Failures outside a check, and signals to the check itself, clean up before the process ends. */
function installProcessHandlers(ctx) {
  const abort = (cause, status) => {
    reportFailure(ctx, ctx.current, cause)
    cleanupOnce(ctx)
      .then(reportCleanup, (error) => console.error(`cleanup failed: ${messageOf(error)}`))
      .finally(() => process.exit(status))
  }
  process.on('uncaughtException', (error) => abort(error, 1))
  process.on('unhandledRejection', (reason) => abort(reason, 1))
  process.on('SIGINT', () => abort(new Error('interrupted by SIGINT'), 130))
  process.on('SIGTERM', () => abort(new Error('terminated by SIGTERM'), 143))
}

async function main() {
  const profile = parseProfile(process.argv.slice(2))
  if (profile === null) {
    console.error(USAGE)
    return 2
  }
  const ctx = createContext(profile)
  installProcessHandlers(ctx)
  const status = await run(ctx)
  const problems = await cleanupOnce(ctx)
  for (const problem of problems) console.error(`cleanup: ${problem}`)
  return problems.length > 0 && status === 0 ? 1 : status
}

process.exit(await main())
