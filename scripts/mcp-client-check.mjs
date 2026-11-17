// Checks chemifree-mcp (P82) with a real MCP client. The SDK's stdio client starts the built adapter on
// a temporary data directory where a running chemifree-api has recorded its address, then calls the
// adapter's nine tools. Each read reply is compared with the API's own reply to the same operation. The
// document is created and given one unsaved edit over HTTP first, as the web editor does, so the snapshot
// has to include work that only the running server holds. The edit tools then create, change, save and
// close documents: a repeated request_id replays its reply, a stale revision is refused whichever client
// made the last edit, and a snapshot after chemifree-api has been stopped and started again on the same
// data directory is read through the same adapter process. A failed step prints
// `FAILED <step>: <message>` and the API's standard error, then the run exits 1. A passing run prints
// one line.
//
// The check builds nothing. It runs the binaries that cargo built for the profile (debug by default):
//   cargo build --locked --package chemifree-api --package chemifree-mcp
//   npm run test:mcp -- --profile debug
// CARGO_TARGET_DIR is honored, as in scripts/product-server-check.mjs.

import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js'

const REPO_ROOT = fileURLToPath(new URL('..', import.meta.url))
const LISTENING = /listening on (http:\/\/127\.0\.0\.1:\d+)/
const STARTUP_MS = 15_000
const EXIT_MS = 15_000
const HTTP_MS = 10_000
// The tools that tools/list must report, exactly, each with its readOnlyHint: the three read tools are
// read-only, and the six edit tools are not. The names are compared sorted.
const READ_ONLY_HINTS = new Map([
  ['chemifree_capabilities', true],
  ['chemifree_document_close', false],
  ['chemifree_document_create', false],
  ['chemifree_document_save', false],
  ['chemifree_document_snapshot', true],
  ['chemifree_document_transaction', false],
  ['chemifree_documents_list', true],
  ['chemifree_history_redo', false],
  ['chemifree_history_undo', false],
])
const TOOL_NAMES = [...READ_ONLY_HINTS.keys()].sort()
// The two forms of a tool error for a stopped server: nothing answers at the address, or a connection that was
// made fails before its reply. Both name the loopback address, with the port that the API printed.
const UNREACHABLE_TEXT = /^No running Chemifree API server answered at http:\/\/127\.0\.0\.1:\d+\./
const TRANSPORT_FAILURE_TEXT = /^The request to the Chemifree API server at http:\/\/127\.0\.0\.1:\d+ failed: /
const USAGE = 'usage: node scripts/mcp-client-check.mjs [--profile debug|release]'
const TIMED_OUT = Symbol('timed out')

const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms))
const messageOf = (error) => (error instanceof Error ? error.message : String(error))
const textOf = (result) => result.content?.[0]?.text ?? '(no text)'

/** Resolves like `promise`, or with TIMED_OUT after `timeoutMs`. The timer never outlives the wait. */
function timeLimit(promise, timeoutMs) {
  let timer
  const limit = new Promise((resolveLimit) => {
    timer = setTimeout(() => resolveLimit(TIMED_OUT), timeoutMs)
  })
  return Promise.race([promise, limit]).finally(() => clearTimeout(timer))
}

/** The profile the arguments name: `debug` by default, `release` on request, otherwise null. */
function parseProfile(args) {
  if (args.length === 0) return 'debug'
  if (args.length === 2 && args[0] === '--profile' && (args[1] === 'debug' || args[1] === 'release')) {
    return args[1]
  }
  return null
}

/** The children's environment. The check passes its settings by flag, so no CHEMIFREE_ setting is inherited. */
function childEnvironment() {
  return Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith('CHEMIFREE_')))
}

/** The directory cargo builds into, as `cargo metadata` reports it (CARGO_TARGET_DIR included). */
function cargoTargetDirectory() {
  const result = spawnSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  })
  if (result.error) throw new Error(`cargo metadata could not run: ${result.error.message}`)
  if (result.status !== 0) {
    const how = result.status === null ? `signal ${result.signal}` : `exit status ${result.status}`
    throw new Error(`cargo metadata failed (${how}): ${result.stderr.trim() || 'no error output'}`)
  }
  const { target_directory: targetDirectory } = JSON.parse(result.stdout)
  if (typeof targetDirectory !== 'string') throw new Error('cargo metadata reported no target_directory')
  return targetDirectory
}

/** The paths of the two binaries for the profile. A missing binary fails the run, with the build to run. */
function locateBuild(profile) {
  const suffix = process.platform === 'win32' ? '.exe' : ''
  const binDir = join(cargoTargetDirectory(), profile)
  const paths = {
    api: join(binDir, `chemifree-api${suffix}`),
    mcp: join(binDir, `chemifree-mcp${suffix}`),
  }
  const missing = Object.values(paths).filter((path) => !existsSync(path))
  if (missing.length > 0) {
    const release = profile === 'release' ? ' --release' : ''
    throw new Error(
      `missing ${missing.join(', ')}; run "cargo build${release} --locked --package chemifree-api --package chemifree-mcp" first`,
    )
  }
  return paths
}

/**
 * Starts chemifree-api on `dataDir` with `--port 0`, so the OS picks a free loopback port and nothing is
 * probed first. Its standard error is recorded, and `api.outcome` is set once the process has ended.
 */
function spawnApi(state, apiPath, dataDir) {
  const child = spawn(apiPath, ['--data-dir', dataDir, '--port', '0'], {
    env: childEnvironment(),
    stdio: ['ignore', 'ignore', 'pipe'],
  })
  const api = { child, stderr: '', outcome: null, exited: null }
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (text) => {
    api.stderr += text
  })
  api.exited = new Promise((resolveExit) => {
    // `close`, not `exit`: standard error can still be arriving after `exit`.
    child.once('close', (code, signal) => {
      api.outcome = { code, signal }
      resolveExit(api.outcome)
    })
    child.on('error', (error) => {
      // An error from a process that did start (a failed kill) does not end it.
      if (child.pid === undefined) {
        api.outcome = { code: null, signal: null, error }
        resolveExit(api.outcome)
      }
    })
  })
  state.api = api
  return api
}

function describeOutcome(outcome) {
  if (outcome.error !== undefined) return outcome.error.message
  return outcome.signal === null ? `exit status ${outcome.code}` : `signal ${outcome.signal}`
}

/** The address that the API prints once it listens. Fails if the API ends first or does not listen in time. */
async function waitForListening(api) {
  const deadline = Date.now() + STARTUP_MS
  for (;;) {
    const match = LISTENING.exec(api.stderr)
    if (match !== null) return match[1]
    if (api.outcome !== null) {
      throw new Error(`chemifree-api ended (${describeOutcome(api.outcome)}) before it listened`)
    }
    if (Date.now() >= deadline) throw new Error(`chemifree-api did not listen within ${STARTUP_MS / 1000} s`)
    await sleep(20)
  }
}

/** Stops the API with SIGTERM, then SIGKILL if it has not ended within EXIT_MS. An ended process is left alone. */
async function stopApi(api) {
  if (api.outcome !== null) return
  api.child.kill('SIGTERM')
  if ((await timeLimit(api.exited, EXIT_MS)) !== TIMED_OUT) return
  api.child.kill('SIGKILL')
  const killed = await timeLimit(api.exited, EXIT_MS)
  throw new Error(
    killed === TIMED_OUT
      ? `chemifree-api (pid ${api.child.pid}) did not exit even after SIGKILL`
      : `chemifree-api did not exit within ${EXIT_MS / 1000} s of SIGTERM, so it was stopped with SIGKILL`,
  )
}

/** One call to the API, sent as the web editor sends it: JSON bodies typed, and the session header on writes. */
function send(baseUrl, method, path, { session, body } = {}) {
  const headers = {}
  if (body !== undefined) headers['content-type'] = 'application/json'
  if (session !== undefined) headers[session.header] = session.token
  return fetch(`${baseUrl}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(HTTP_MS),
  })
}

/** A call that must answer `status` with a JSON body, which is returned parsed. */
async function requestJson(baseUrl, method, path, status, options = {}) {
  const response = await send(baseUrl, method, path, options)
  const text = await response.text()
  assert.equal(response.status, status, `${method} ${path} answered ${response.status}: ${text}`)
  return JSON.parse(text)
}

/** One request of the operation contract, as the MCP adapter sends it (contract 1, preserve-semantics). */
function operate(baseUrl, session, operation, parameters) {
  return requestJson(baseUrl, 'POST', '/api/v1/operations', 200, {
    session,
    body: { contract: 1, request_id: randomUUID(), profile: 'preserve-semantics', operation, parameters },
  })
}

/** The command that adds one carbon atom at `x`, in the shape that the web editor sends. */
const carbonAt = (x) => ({ op: 'add_atom', element: 'C', position: { x, y: 300 } })

/** A tool call that must not be a tool error. Its result is returned. */
async function callSucceeds(client, name, args) {
  const result = await client.callTool({ name, arguments: args })
  assert.notEqual(result.isError, true, `${name} is a tool error: ${textOf(result)}`)
  return result
}

/**
 * The whole check, in order. `state.step` names the step that runs, so a failure says where it happened.
 * The API and the client are recorded in `state` as they start, so cleanup stops them on every path.
 * Returns the success line.
 */
async function run(state) {
  state.step = 'setup'
  const paths = locateBuild(state.profile)
  state.workDir = mkdtempSync(join(tmpdir(), 'chemifree-mcp-'))
  const dataDir = join(state.workDir, 'data')

  state.step = 'start chemifree-api'
  const baseUrl = await waitForListening(spawnApi(state, paths.api, dataDir))

  state.step = 'create and edit a document over HTTP'
  const session = await requestJson(baseUrl, 'GET', '/api/v1/session', 200)
  const created = await requestJson(baseUrl, 'POST', '/api/v1/documents', 201, {
    session,
    body: { request_id: randomUUID() },
  })
  const documentId = created.document.id
  const edited = await requestJson(baseUrl, 'POST', `/api/v1/documents/${documentId}/transactions`, 200, {
    session,
    body: {
      request_id: randomUUID(),
      expected_revision: created.document.revision,
      commands: [{ op: 'add_atom', element: 'C', position: { x: 300, y: 300 } }],
    },
  })
  assert.equal(edited.document.revision, 1)
  assert.equal(edited.session.unsaved_changes, true)

  state.step = 'read capabilities and snapshot over HTTP'
  const httpCatalog = await operate(baseUrl, session, 'capabilities.inspect', {})
  const httpSnapshot = await operate(baseUrl, session, 'document.snapshot', { document_id: documentId })
  assert.equal(httpSnapshot.outcome, 'ok')
  assert.equal(httpSnapshot.document_revision, edited.document.revision)

  state.step = 'initialize'
  state.client = new Client({ name: 'chemifree-mcp-check', version: '0.0.0' })
  await state.client.connect(
    new StdioClientTransport({
      command: paths.mcp,
      args: ['--data-dir', dataDir],
      env: childEnvironment(),
    }),
  )
  assert.equal(state.client.getServerVersion()?.name, 'chemifree')

  state.step = 'tools/list'
  const { tools } = await state.client.listTools()
  assert.deepEqual(tools.map((tool) => tool.name).sort(), TOOL_NAMES)
  assert.deepEqual(
    Object.fromEntries(tools.map((tool) => [tool.name, tool.annotations?.readOnlyHint])),
    Object.fromEntries(READ_ONLY_HINTS),
  )
  // A save writes the document's file, a transaction may delete what it names, and an undo or a redo acts on the
  // undo history that the web editor shares. Create and close are not destructive.
  assert.deepEqual(
    tools
      .filter((tool) => tool.annotations?.destructiveHint === true)
      .map((tool) => tool.name)
      .sort(),
    [
      'chemifree_document_save',
      'chemifree_document_transaction',
      'chemifree_history_redo',
      'chemifree_history_undo',
    ],
  )
  const transaction = tools.find((tool) => tool.name === 'chemifree_document_transaction')
  for (const argument of ['document_id', 'expected_revision', 'commands']) {
    assert.ok(
      transaction?.inputSchema?.required?.includes(argument),
      `chemifree_document_transaction requires ${argument}`,
    )
  }

  state.step = 'chemifree_capabilities'
  const capabilities = await state.client.callTool({ name: 'chemifree_capabilities', arguments: {} })
  assert.notEqual(capabilities.isError, true, textOf(capabilities))
  assert.equal(capabilities.structuredContent?.operation, 'capabilities.inspect')
  assert.equal(capabilities.structuredContent?.outcome, 'ok')
  // The catalog is the reply's `result`, and it is the catalog the API reports for the same operation.
  assert.deepEqual(capabilities.structuredContent?.result, httpCatalog.result)

  state.step = 'chemifree_documents_list'
  const listed = await state.client.callTool({ name: 'chemifree_documents_list', arguments: {} })
  assert.notEqual(listed.isError, true, textOf(listed))
  // The data directory is new, so the one document this check created is the whole list.
  assert.deepEqual(listed.structuredContent?.documents?.map((document) => document.id), [documentId])

  state.step = 'chemifree_document_snapshot'
  const snapshot = await state.client.callTool({
    name: 'chemifree_document_snapshot',
    arguments: { document_id: documentId },
  })
  assert.notEqual(snapshot.isError, true, textOf(snapshot))
  assert.equal(snapshot.structuredContent?.document_revision, httpSnapshot.document_revision)
  // Nothing was saved, so the atom from the edit is in the snapshot only because the running server holds it.
  assert.equal(snapshot.structuredContent?.result?.document?.atoms?.length, 1)

  state.step = 'chemifree_document_snapshot (unknown id)'
  const unknown = await state.client.callTool({
    name: 'chemifree_document_snapshot',
    arguments: { document_id: randomUUID() },
  })
  assert.equal(unknown.isError, true, `an unknown document is not a tool error: ${textOf(unknown)}`)
  assert.equal(textOf(unknown), 'document.snapshot: outcome error, revision n/a')

  // The edit tools, through the same adapter process. Each reply is checked against the API's own state.
  state.step = 'chemifree_document_create (repeated request_id)'
  const createArguments = { request_id: 'mcp-s2-create-1' }
  const createReply = await callSucceeds(state.client, 'chemifree_document_create', createArguments)
  const createAgainReply = await callSucceeds(state.client, 'chemifree_document_create', createArguments)
  const newDocumentId = createReply.structuredContent?.result?.document?.id
  assert.equal(typeof newDocumentId, 'string', `the reply names no document: ${textOf(createReply)}`)
  assert.equal(
    createAgainReply.structuredContent?.result?.document?.id,
    newDocumentId,
    'a repeated request_id returns the first reply',
  )
  const listedAfterCreates = await callSucceeds(state.client, 'chemifree_documents_list', {})
  assert.equal(
    listedAfterCreates.structuredContent?.documents?.length,
    2,
    'the document from the HTTP check and this one, and no other: the repeat created nothing',
  )

  state.step = 'chemifree_document_transaction (agent edit at revision 0)'
  const addFirst = {
    document_id: newDocumentId,
    expected_revision: 0,
    commands: [carbonAt(300)],
    request_id: 'mcp-s2-tx-1',
  }
  const transacted = await callSucceeds(state.client, 'chemifree_document_transaction', addFirst)
  assert.equal(textOf(transacted), 'document.transaction: outcome ok, revision 1')
  const afterEdit = await requestJson(baseUrl, 'GET', `/api/v1/documents/${newDocumentId}`, 200)
  assert.equal(afterEdit.document.revision, 1)
  assert.equal(afterEdit.document.atoms.length, 1)

  state.step = 'chemifree_document_transaction (repeated request_id)'
  const replayed = await callSucceeds(state.client, 'chemifree_document_transaction', addFirst)
  assert.deepEqual(replayed.structuredContent, transacted.structuredContent)
  assert.equal(replayed.structuredContent?.document_revision, 1)

  state.step = 'web-style transaction at a stale revision (HTTP)'
  const webStale = await requestJson(baseUrl, 'POST', `/api/v1/documents/${newDocumentId}/transactions`, 409, {
    session,
    body: { request_id: randomUUID(), expected_revision: 0, commands: [carbonAt(340)] },
  })
  assert.equal(webStale.code, 'revision_conflict')
  assert.equal(webStale.current_revision, 1)

  state.step = 'web-style edit to revision 2 (HTTP)'
  const webEdit = await requestJson(baseUrl, 'POST', `/api/v1/documents/${newDocumentId}/transactions`, 200, {
    session,
    body: { request_id: randomUUID(), expected_revision: 1, commands: [carbonAt(340)] },
  })
  assert.equal(webEdit.document.revision, 2)

  state.step = 'chemifree_document_transaction (stale revision after the web edit)'
  const staleEdit = await state.client.callTool({
    name: 'chemifree_document_transaction',
    arguments: { document_id: newDocumentId, expected_revision: 1, commands: [carbonAt(380)] },
  })
  assert.equal(staleEdit.isError, true, `a stale edit is not a tool error: ${textOf(staleEdit)}`)
  assert.equal(staleEdit.structuredContent?.outcome, 'error')
  assert.equal(staleEdit.structuredContent?.diagnostics?.[0]?.code, 'revision_conflict')
  assert.equal(staleEdit.structuredContent?.diagnostics?.[0]?.details?.current_revision, 2)

  state.step = 'chemifree_document_save at revision 2'
  const saved = await callSucceeds(state.client, 'chemifree_document_save', {
    document_id: newDocumentId,
    expected_revision: 2,
  })
  assert.equal(saved.structuredContent?.result?.session?.unsaved_changes, false)

  state.step = 'chemifree_document_close'
  const closed = await callSucceeds(state.client, 'chemifree_document_close', { document_id: newDocumentId })
  assert.equal(closed.structuredContent?.outcome, 'ok')
  assert.equal(closed.structuredContent?.document_revision, null)

  state.step = 'chemifree_document_snapshot after the close'
  const reopened = await callSucceeds(state.client, 'chemifree_document_snapshot', { document_id: newDocumentId })
  assert.equal(reopened.structuredContent?.document_revision, 2)
  assert.equal(reopened.structuredContent?.result?.document?.atoms?.length, 2)
  assert.equal(reopened.structuredContent?.result?.session?.unsaved_changes, false)

  // The client keeps the address of the server it last reached. A call made while nothing answers there
  // drops that address, so the next call looks the server up in the data directory again.
  state.step = 'chemifree_document_snapshot while chemifree-api is stopped'
  await stopApi(state.api)
  const stopped = await state.client.callTool({
    name: 'chemifree_document_snapshot',
    arguments: { document_id: newDocumentId },
  })
  assert.equal(stopped.isError, true, `a stopped server is not a tool success: ${textOf(stopped)}`)
  const stoppedText = textOf(stopped)
  assert.ok(
    UNREACHABLE_TEXT.test(stoppedText) || TRANSPORT_FAILURE_TEXT.test(stoppedText),
    `a stopped server is named as unreachable or as a failed request: ${stoppedText}`,
  )

  state.step = 'chemifree-api restarted on the same data directory'
  await waitForListening(spawnApi(state, paths.api, dataDir))

  state.step = 'chemifree_document_snapshot after the restart'
  const restarted = await callSucceeds(state.client, 'chemifree_document_snapshot', {
    document_id: newDocumentId,
  })
  assert.equal(restarted.structuredContent?.document_revision, 2)
  assert.equal(restarted.structuredContent?.result?.document?.atoms?.length, 2)

  return (
    `mcp-client-check (${state.profile}): chemifree-mcp initialized and lists ${TOOL_NAMES.length} tools; ` +
    `its reads match chemifree-api (revision ${httpSnapshot.document_revision}), and an unknown document is a ` +
    'tool error; its edits replay by request_id, refuse a stale revision, save and close, and read back after ' +
    'a restart of chemifree-api'
  )
}

/** Prints `FAILED <step>: <message>`, then the API's standard error, which usually explains a failed start or call. */
function reportFailure(state, error) {
  console.error(`FAILED ${state.step}: ${messageOf(error)}`)
  if (state.api !== null) {
    console.error('last chemifree-api stderr:')
    console.error(state.api.stderr.trimEnd() || '(none)')
  }
}

/** Closes the MCP client, stops the API and removes the work directory. Returns what could not be done. */
async function cleanup(state) {
  const problems = []
  if (state.client !== null) {
    try {
      await state.client.close()
    } catch (error) {
      problems.push(`closing the MCP client: ${messageOf(error)}`)
    }
  }
  if (state.api !== null) {
    try {
      await stopApi(state.api)
    } catch (error) {
      problems.push(messageOf(error))
    }
  }
  if (state.workDir !== null) {
    try {
      rmSync(state.workDir, { recursive: true, force: true })
    } catch (error) {
      problems.push(`removing ${state.workDir}: ${messageOf(error)}`)
    }
  }
  return problems
}

/** Runs cleanup once and prints its problems once. Every path that ends the run awaits this one promise. */
function cleanupOnce(state) {
  state.cleanup ??= cleanup(state).then((problems) => {
    for (const problem of problems) console.error(`cleanup: ${problem}`)
    return problems
  })
  return state.cleanup
}

/** A signal ends the run: cleanup first, then the process exits with 130 for SIGINT or 143 for SIGTERM. */
function installSignalHandlers(state) {
  for (const [signal, status] of [
    ['SIGINT', 130],
    ['SIGTERM', 143],
  ]) {
    process.on(signal, () => {
      state.stopStatus = status
      console.error(`mcp-client-check: ${signal} ends the run`)
      void cleanupOnce(state).then(() => process.exit(status))
    })
  }
}

async function main() {
  const profile = parseProfile(process.argv.slice(2))
  if (profile === null) {
    console.error(USAGE)
    return 2
  }
  const state = { profile, step: 'setup', api: null, client: null, workDir: null, cleanup: null, stopStatus: null }
  installSignalHandlers(state)
  let status = 0
  let summary = ''
  try {
    summary = await run(state)
  } catch (error) {
    status = 1
    // An interrupted step fails because the signal closed its connection, so the signal is the report.
    if (state.stopStatus === null) reportFailure(state, error)
  }
  const problems = await cleanupOnce(state)
  if (problems.length > 0) status = 1
  if (state.stopStatus !== null) return state.stopStatus
  if (status === 0) console.log(summary)
  return status
}

process.exit(await main())
