import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { once } from 'node:events'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

// Compare the real entry adapters over a throwaway data directory. Document editing and
// local persistence are exercised; no chemistry perception or analysis is implied.
execFileSync('cargo', ['build', '--workspace', '--locked'], { stdio: 'inherit' })
const metadata = JSON.parse(
  execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], {
    encoding: 'utf8',
  }),
)
const suffix = process.platform === 'win32' ? '.exe' : ''
const apiPath = resolve(metadata.target_directory, 'debug', `chemifree-api${suffix}`)
const cliPath = resolve(metadata.target_directory, 'debug', `chemifree${suffix}`)
const dataDir = mkdtempSync(join(tmpdir(), 'chemifree-contract-'))
const env = { ...process.env, CHEMIFREE_DATA_DIR: dataDir }
const running = new Set()

async function startApi(extraEnv = {}) {
  const child = spawn(apiPath, ['--port', '0'], { env: { ...env, ...extraEnv }, stdio: ['ignore', 'ignore', 'pipe'] })
  running.add(child)
  let log = ''
  const baseUrl = await new Promise((resolveUrl, reject) => {
    const timer = setTimeout(() => reject(new Error(`API startup timed out. ${log}`)), 15_000)
    child.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    child.once('exit', (code) => {
      clearTimeout(timer)
      reject(new Error(`API exited before startup (${code}). ${log}`))
    })
    child.stderr.on('data', (data) => {
      log += data.toString()
      const match = log.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) {
        clearTimeout(timer)
        resolveUrl(match[1])
      }
    })
  })
  return { child, baseUrl, log: () => log }
}

/** Resolves once `child` has written a line matching `pattern` to standard error. */
function logged(api, pattern) {
  if (pattern.test(api.log())) return Promise.resolve()
  return new Promise((resolveLine, reject) => {
    const timer = setTimeout(() => reject(new Error(`no log line matching ${pattern}. ${api.log()}`)), 5_000)
    const read = () => {
      if (pattern.test(api.log())) {
        clearTimeout(timer)
        api.child.stderr.off('data', read)
        resolveLine()
      }
    }
    api.child.stderr.on('data', read)
  })
}

async function stop(child, signal = 'SIGTERM') {
  if (child.pid && child.exitCode === null && child.signalCode === null) {
    const stopped = once(child, 'exit')
    child.kill(signal)
    const force = setTimeout(() => child.kill('SIGKILL'), 5_000)
    await stopped
    clearTimeout(force)
  }
  running.delete(child)
}

function cli(...args) {
  const result = spawnSync(cliPath, args, { env, encoding: 'utf8' })
  return { status: result.status, body: JSON.parse(result.stdout) }
}

/**
 * Starts a CLI process without waiting; `done` resolves with its result and duration.
 * Durations use the monotonic clock: the wall clock can be stepped by time synchronization.
 */
function cliProcess(args, extraEnv = {}) {
  const started = performance.now()
  const child = spawn(cliPath, args, { env: { ...env, ...extraEnv }, stdio: ['ignore', 'pipe', 'pipe'] })
  running.add(child)
  let stdout = ''
  let stderr = ''
  child.stdout.on('data', (data) => (stdout += data))
  child.stderr.on('data', (data) => (stderr += data))
  const done = once(child, 'exit').then(([status, signal]) => {
    running.delete(child)
    return { status, signal, stderr, body: stdout ? JSON.parse(stdout) : null, ms: Math.round(performance.now() - started) }
  })
  return { child, done }
}

const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms))
const DELAY = 'CHEMIFREE_TEST_COMMIT_DELAY_MS'

let api
try {
  api = await startApi()

  async function call(path, { method = 'GET', body, headers = {}, timeout = 5_000 } = {}) {
    const response = await fetch(`${api.baseUrl}${path}`, {
      method,
      headers: body === undefined ? headers : { 'content-type': 'application/json', ...headers },
      // A string body is sent as written, for JSON that JSON.stringify cannot produce.
      body: body === undefined ? undefined : typeof body === 'string' ? body : JSON.stringify(body),
      signal: AbortSignal.timeout(timeout),
    })
    assert.match(response.headers.get('content-type'), /application\/json/)
    assert.equal(response.headers.get('access-control-allow-origin'), null)
    const json = await response.json()
    assert.equal(json.api_version, 'v1')
    return { status: response.status, body: json }
  }

  // Inspection parity.
  for (const [endpoint, command] of [
    ['health', 'status'],
    ['capabilities', 'capabilities'],
  ]) {
    const http = await call(`/api/v1/${endpoint}`)
    assert.equal(http.status, 200)
    assert.deepEqual(http.body, cli(command, '--json').body, `HTTP ${endpoint} = CLI ${command}`)
  }
  const capabilities = (await call('/api/v1/capabilities')).body.capabilities
  assert.deepEqual(
    capabilities.filter((item) => item.status === 'available').map((item) => item.id),
    ['runtime_inspection', 'document_editing', 'format_conversion', 'persistence'],
  )
  assert.equal(new Set(capabilities.map((item) => item.id)).size, 8)

  // The opt-in request log: one line per request with method, path, and status, and
  // nothing from the headers (such as the session token).
  const logging = await startApi({ CHEMIFREE_LOG_REQUESTS: '1' })
  try {
    assert.equal((await fetch(`${logging.baseUrl}/api/v1/health`)).status, 200)
    const loggedSession = await (await fetch(`${logging.baseUrl}/api/v1/session`)).json()
    await logged(logging, /^\+\d+ GET \/api\/v1\/session 200 \d+ms$/m)
    assert.match(logging.log(), /^\+\d+ GET \/api\/v1\/health 200 \d+ms$/m)
    assert.ok(!logging.log().includes(loggedSession.token), 'the session token is never logged')
  } finally {
    await stop(logging.child)
  }
  assert.doesNotMatch(api.log(), /GET \/api\/v1\/health/, 'nothing is logged without the option')

  // Write protection.
  const { token, header } = (await call('/api/v1/session')).body
  assert.equal(header, 'x-chemifree-session')
  assert.match(token, /^[0-9a-f]{64}$/)
  let auth = { [header]: token, origin: new URL(api.baseUrl).origin }
  /** Stops the API (SIGKILL simulates a crash) and starts a new process with a new token. */
  async function restartApi(signal = 'SIGKILL', extraEnv = {}) {
    await stop(api.child, signal)
    api = await startApi(extraEnv)
    const session = (await call('/api/v1/session')).body
    auth = { [session.header]: session.token, origin: new URL(api.baseUrl).origin }
  }
  const unauthenticated = await call('/api/v1/documents', { method: 'POST', body: { request_id: 'c0' } })
  assert.deepEqual([unauthenticated.status, unauthenticated.body.code], [401, 'unauthorized'])
  const crossSite = await call('/api/v1/documents', {
    method: 'POST',
    body: { request_id: 'c0' },
    headers: { ...auth, origin: 'https://example.com' },
  })
  assert.deepEqual([crossSite.status, crossSite.body.code], [403, 'forbidden_origin'])
  const tokenLeak = await call('/api/v1/session', { headers: { origin: 'https://example.com' } })
  assert.equal(tokenLeak.status, 403)
  assert.deepEqual((await call('/api/v1/documents')).body.documents, [])

  // Create → edit → undo/redo → save → close → reopen over HTTP.
  const write = (path, body, timeout) => call(path, { method: 'POST', body, headers: auth, timeout })
  const created = await write('/api/v1/documents', { request_id: 'create-1', title: 'Contract ethanol' })
  assert.equal(created.status, 201)
  const id = created.body.document.id
  const doc = `/api/v1/documents/${id}`
  const draw = {
    request_id: 'edit-1',
    expected_revision: 0,
    commands: [
      { op: 'add_atom', element: 'C', position: { x: 0, y: 0 } },
      { op: 'add_atom', element: 'C', position: { x: 26, y: 15 }, bond_to: { atom: 'a1', order: 'single' } },
      { op: 'add_atom', element: 'O', position: { x: 52, y: 0 }, bond_to: { atom: 'a2', order: 'single' } },
    ],
  }
  const edited = await write(`${doc}/transactions`, draw)
  assert.equal(edited.status, 200)
  assert.equal(edited.body.document.revision, 1)
  const replayed = await write(`${doc}/transactions`, draw)
  assert.deepEqual(replayed.body, edited.body, 'a retried request_id replays its result')
  const reused = await write(`${doc}/transactions`, { ...draw, commands: draw.commands.slice(0, 1) })
  assert.deepEqual([reused.status, reused.body.code], [409, 'request_id_reused'])
  const stale = await write(`${doc}/transactions`, { ...draw, request_id: 'edit-stale' })
  assert.deepEqual([stale.status, stale.body.code, stale.body.current_revision], [409, 'revision_conflict', 1])
  const invalid = await write(`${doc}/transactions`, {
    request_id: 'edit-bad',
    expected_revision: 1,
    commands: [{ op: 'add_bond', begin: 'a1', end: 'a2', order: 'double' }],
  })
  assert.deepEqual([invalid.status, invalid.body.code, invalid.body.command_index], [422, 'invalid_command', 0])
  // A key given twice in `positions` makes the body malformed: `invalid_request` without a
  // command index, like any other malformed body, and nothing changes. The CLI agrees below.
  const repeatedKey = '[{"op":"add_atom","element":"N","positions":{"s1":{"x":0,"y":0},"s1":{"x":5,"y":5}}}]'
  const twice = await write(`${doc}/transactions`,
    `{"request_id":"edit-twice","expected_revision":1,"commands":${repeatedKey}}`)
  assert.deepEqual([twice.status, twice.body.code, 'command_index' in twice.body], [422, 'invalid_request', false])
  assert.match(twice.body.message, /duplicate key "s1"/)
  assert.equal((await call(doc)).body.document.revision, 1, 'nothing changed')

  const changed = await write(`${doc}/transactions`, {
    request_id: 'edit-2',
    expected_revision: 1,
    commands: [
      { op: 'update_bond', bond: 'b2', order: 'double' },
      { op: 'update_atom', atom: 'a3', charge: -1, isotope: 18 },
    ],
  })
  assert.equal(changed.body.document.revision, 2)
  const undone = await write(`${doc}/undo`, { request_id: 'undo-1', expected_revision: 2 })
  assert.deepEqual(undone.body.document.atoms, edited.body.document.atoms)
  assert.deepEqual(undone.body.document.bonds, edited.body.document.bonds)
  const redone = await write(`${doc}/redo`, { request_id: 'redo-1', expected_revision: 3 })
  assert.deepEqual(redone.body.document.atoms, changed.body.document.atoms)
  const unsavedClose = await write(`${doc}/close`, { request_id: 'close-0' })
  assert.deepEqual([unsavedClose.status, unsavedClose.body.code], [409, 'unsaved_changes'])

  // Save failure: make the target path a directory so the atomic replace cannot commit.
  const file = join(dataDir, 'documents', `${id}.json`)
  const committed = readFileSync(file)
  rmSync(file)
  mkdirSync(file)
  const failed = await write(`${doc}/save`, { request_id: 'save-1', expected_revision: 4 })
  assert.deepEqual([failed.status, failed.body.code, failed.body.reason], [503, 'storage_failed', 'io'])
  assert.equal((await call(doc)).body.session.unsaved_changes, true)
  rmSync(file, { recursive: true })
  writeFileSync(file, committed)
  const saved = await write(`${doc}/save`, { request_id: 'save-1', expected_revision: 4 })
  assert.equal(saved.status, 200)
  assert.deepEqual(saved.body.session, {
    saved_revision: 4,
    unsaved_changes: false,
    undo_depth: 2,
    redo_depth: 0,
    draft_revision: null,
    draft_failure: null,
  })
  const closed = await write(`${doc}/close`, { request_id: 'close-1' })
  assert.deepEqual(closed.body, { api_version: 'v1', id, discarded_changes: false })
  const reopened = await call(doc)
  assert.deepEqual(reopened.body, saved.body, 'reopening returns the committed document')

  // CLI sees the same committed document and edits it through the same application.
  assert.deepEqual(cli('documents', 'show', id, '--json').body, saved.body, 'HTTP GET = CLI show')
  const listed = (await call('/api/v1/documents')).body.documents
  assert.deepEqual(cli('documents', 'list', '--json').body.documents, listed.map((item) => ({ ...item, open: false })))

  // A concurrent CLI save makes the HTTP session's later save a conflict, not a lost update.
  await write(`${doc}/transactions`, {
    request_id: 'edit-3',
    expected_revision: 4,
    commands: [{ op: 'rename', title: 'HTTP title' }],
  })
  const cliTwice = cli('documents', 'apply', id, '--expected-revision', '4', '--json', '--commands', repeatedKey)
  assert.deepEqual([cliTwice.status, cliTwice.body.code, 'command_index' in cliTwice.body], [1, 'invalid_request', false])
  assert.match(cliTwice.body.message, /duplicate key "s1"/)
  const cliEdit = cli('documents', 'apply', id, '--expected-revision', '4', '--json', '--commands',
    JSON.stringify([{ op: 'move_atoms', atoms: ['a1'], delta: { x: 5, y: 5 } }]))
  assert.equal(cliEdit.status, 0)
  assert.equal(cliEdit.body.document.revision, 5)
  assert.equal(cliEdit.body.session.unsaved_changes, false)
  const cliStale = cli('documents', 'undo', id, '--expected-revision', '4', '--json')
  assert.deepEqual([cliStale.status, cliStale.body.code], [1, 'revision_conflict'])
  const conflict = await write(`${doc}/save`, { request_id: 'save-2', expected_revision: 5 })
  assert.deepEqual([conflict.status, conflict.body.code], [409, 'save_conflict'])
  const unconfirmed = await write(`${doc}/close`, { request_id: 'close-unconfirmed', discard_unsaved: true })
  assert.deepEqual([unconfirmed.status, unconfirmed.body.code], [422, 'invalid_request'])
  const staleDiscard = await write(`${doc}/close`, { request_id: 'close-stale', discard_unsaved: true, expected_revision: 4 })
  assert.deepEqual([staleDiscard.status, staleDiscard.body.code], [409, 'revision_conflict'])
  const discarded = await write(`${doc}/close`, { request_id: 'close-2', discard_unsaved: true, expected_revision: 5 })
  assert.equal(discarded.body.discarded_changes, true)
  // The CLI's commit is reopened, but revision 5 already named the discarded HTTP edit in
  // this process, so the working copy continues at 6 and a client holding 5 is refused.
  const afterDiscard = (await call(doc)).body.document
  assert.deepEqual({ ...afterDiscard, revision: 5 }, cliEdit.body.document)
  assert.equal(afterDiscard.revision, 6)
  const staleAfterDiscard = await write(`${doc}/transactions`, {
    request_id: 'edit-stale',
    expected_revision: 5,
    commands: [{ op: 'rename', title: 'Stale' }],
  })
  assert.deepEqual([staleAfterDiscard.status, staleAfterDiscard.body.code], [409, 'revision_conflict'])

  // Restart recovery: unsaved work in a killed process is gone; the last commit survives.
  await write(`${doc}/transactions`, {
    request_id: 'edit-4',
    expected_revision: 6,
    commands: [{ op: 'delete_atoms', atoms: ['a1', 'a2', 'a3'] }],
  })
  writeFileSync(join(dataDir, 'documents', `.${id}.99999.0.tmp`), '{"partial":')
  await restartApi()
  const recovered = (await call(doc)).body
  assert.deepEqual(recovered.document, cliEdit.body.document)
  assert.equal(recovered.session.unsaved_changes, false)
  assert.equal(recovered.recovery, null, 'nothing was autosaved, so nothing is offered')

  // ── Cross-process save lock ────────────────────────────────────────────────────────────
  // Six CLI processes save different titles from the same base at once. Each pauses inside
  // the lock between its base check and commit, so without the lock several would commit.
  const raced = cli('documents', 'create', '--title', 'Race', '--json').body.document.id
  const racers = Array.from({ length: 6 }, (_, index) =>
    cliProcess(
      ['documents', 'apply', raced, '--expected-revision', '0', '--json', '--commands',
        JSON.stringify([{ op: 'rename', title: `Writer ${index}` }])],
      { [DELAY]: '300' },
    ).done,
  )
  const results = await Promise.all(racers)
  const winners = results.filter((result) => result.status === 0)
  assert.equal(winners.length, 1, `exactly one concurrent save commits: ${JSON.stringify(results.map((r) => r.body?.code ?? (r.body ? 'ok' : { status: r.status, signal: r.signal, stderr: r.stderr })))}`)
  for (const loser of results.filter((result) => result.status !== 0)) {
    assert.ok(['save_conflict', 'revision_conflict'].includes(loser.body.code), loser.body.code)
  }
  const raceFile = JSON.parse(readFileSync(join(dataDir, 'documents', `${raced}.json`), 'utf8'))
  assert.deepEqual([raceFile.document.revision, raceFile.document.title], [1, winners[0].body.document.title])

  // A process killed while holding the lock does not block later saves: the OS releases it.
  const holder = cliProcess(
    ['documents', 'apply', raced, '--expected-revision', '1', '--json', '--commands',
      JSON.stringify([{ op: 'rename', title: 'Killed before commit' }])],
    { [DELAY]: '20000' },
  )
  await sleep(1_500)
  const waiter = cliProcess(['documents', 'apply', raced, '--expected-revision', '1', '--json', '--commands',
    JSON.stringify([{ op: 'rename', title: 'After the crash' }])])
  await sleep(1_000)
  assert.equal(waiter.child.exitCode, null, 'the second save waits while the lock is held')
  holder.child.kill('SIGKILL')
  await holder.done
  const afterCrash = await waiter.done
  assert.equal(afterCrash.status, 0, JSON.stringify(afterCrash.body))
  assert.equal(afterCrash.body.document.title, 'After the crash')
  assert.ok(afterCrash.ms < 8_000, `released promptly (${afterCrash.ms} ms)`)

  // HTTP and the CLI share the lock: a CLI save from the same base waits for the API's
  // commit and is then refused as a conflict.
  await restartApi('SIGTERM', { [DELAY]: '1500' })
  const racedPath = `/api/v1/documents/${raced}`
  await write(`${racedPath}/transactions`, {
    request_id: 'race-http',
    expected_revision: 2,
    commands: [{ op: 'rename', title: 'From HTTP' }],
  })
  const httpSave = write(`${racedPath}/save`, { request_id: 'race-save', expected_revision: 3 })
  await sleep(300)
  const cliRace = cliProcess(['documents', 'apply', raced, '--expected-revision', '2', '--json', '--commands',
    JSON.stringify([{ op: 'rename', title: 'From CLI' }])])
  assert.equal((await httpSave).status, 200)
  const cliLost = await cliRace.done
  assert.deepEqual([cliLost.status, cliLost.body.code], [1, 'save_conflict'])
  assert.equal(cli('documents', 'show', raced, '--json').body.document.title, 'From HTTP')
  await restartApi('SIGTERM')

  // ── Recovery drafts ────────────────────────────────────────────────────────────────────
  const draftDoc = (await write('/api/v1/documents', { request_id: 'draft-create', title: 'Draft' })).body.document.id
  const draftPath = `/api/v1/documents/${draftDoc}`
  const draftFile = join(dataDir, 'drafts', `${draftDoc}.json`)
  const documentFile = join(dataDir, 'documents', `${draftDoc}.json`)
  await write(`${draftPath}/transactions`, { ...draw, request_id: 'draft-edit-1' })
  await write(`${draftPath}/transactions`, {
    request_id: 'draft-edit-2',
    expected_revision: 1,
    commands: [{ op: 'rename', title: 'Renamed' }],
  })
  const beforeCrash = (await write(`${draftPath}/undo`, { request_id: 'draft-undo', expected_revision: 2 })).body
  const autosaved = await write(`${draftPath}/autosave`, { request_id: 'auto-1' })
  assert.equal(autosaved.status, 200)
  assert.equal(autosaved.body.session.draft_revision, 3)
  assert.ok(existsSync(draftFile))
  const committedBefore = readFileSync(documentFile)
  const inUse = cli('documents', 'recover', draftDoc, '--json')
  assert.deepEqual([inUse.status, inUse.body.code], [1, 'draft_in_use'], 'the running API owns the draft')

  // Crash, restart, recover: content, history, identifiers, and revision are as autosaved.
  await restartApi()
  const offered = (await call(draftPath)).body
  assert.equal(offered.document.atoms.length, 0, 'storage still holds the empty commit')
  assert.deepEqual(
    { ...offered.recovery, saved_at_ms: 0 },
    { state: 'recoverable', revision: 3, base_revision: 0, saved_at_ms: 0, message: null },
  )
  const listedDraft = cli('documents', 'list', '--json').body.documents.find((item) => item.id === draftDoc)
  assert.equal(listedDraft.recovery.state, 'recoverable', 'the CLI sees the same abandoned draft')
  const recoveredDraft = await write(`${draftPath}/recover`, { request_id: 'recover-1' })
  assert.equal(recoveredDraft.status, 200)
  assert.deepEqual(recoveredDraft.body.document, beforeCrash.document)
  assert.deepEqual(recoveredDraft.body.session, { ...beforeCrash.session, draft_revision: 3 })
  assert.equal(recoveredDraft.body.recovery, null)
  const redoneDraft = await write(`${draftPath}/redo`, { request_id: 'draft-redo', expected_revision: 3 })
  assert.equal(redoneDraft.body.document.title, 'Renamed')
  const grown = await write(`${draftPath}/transactions`, {
    request_id: 'draft-edit-3',
    expected_revision: 4,
    commands: [{ op: 'add_atom', element: 'N', position: { x: 0, y: 40 } }],
  })
  assert.equal(grown.body.document.atoms.at(-1).id, 'a4', 'identifiers continue after the draft')
  await write(`${draftPath}/save`, { request_id: 'draft-save', expected_revision: 5 })
  assert.ok(!existsSync(draftFile), 'saving removes the draft')
  await restartApi()
  assert.equal((await call(draftPath)).body.recovery, null, 'a saved draft never comes back')

  // Discarding a draft after a crash: it is gone for good.
  await write(`${draftPath}/transactions`, {
    request_id: 'draft-edit-4',
    expected_revision: 5,
    commands: [{ op: 'delete_atoms', atoms: ['a4'] }],
  })
  await write(`${draftPath}/autosave`, { request_id: 'auto-2' })
  await restartApi()
  const dropped = await write(`${draftPath}/discard-draft`, { request_id: 'discard-1' })
  assert.deepEqual(dropped.body, { api_version: 'v1', id: draftDoc })
  assert.ok(!existsSync(draftFile))
  await restartApi()
  const afterDiscardDraft = (await call(draftPath)).body
  assert.equal(afterDiscardDraft.recovery, null)
  assert.equal(afterDiscardDraft.document.atoms.length, 4, 'the saved document is unchanged')

  // A draft whose base was saved over by another writer is offered only as a copy.
  await write(`${draftPath}/transactions`, {
    request_id: 'draft-edit-5',
    expected_revision: 5,
    commands: [{ op: 'rename', title: 'Unsaved in the API' }],
  })
  await write(`${draftPath}/autosave`, { request_id: 'auto-3' })
  const draftContent = (await call(draftPath)).body.document
  await stop(api.child, 'SIGKILL')
  const cliCommit = cli('documents', 'apply', draftDoc, '--expected-revision', '5', '--json', '--commands',
    JSON.stringify([{ op: 'rename', title: 'Saved by the CLI' }]))
  assert.equal(cliCommit.status, 0)
  await restartApi('SIGTERM')
  assert.equal((await call(draftPath)).body.recovery.state, 'outdated')
  const refusedRecovery = await write(`${draftPath}/recover`, { request_id: 'recover-2' })
  assert.deepEqual([refusedRecovery.status, refusedRecovery.body.code, refusedRecovery.body.current_revision], [409, 'draft_outdated', 6])
  const recoveredCopy = await write(`${draftPath}/recover-as-copy`, { request_id: 'recover-copy-1' })
  assert.equal(recoveredCopy.status, 201)
  assert.equal(recoveredCopy.body.document.title, 'Unsaved in the API (recovered)')
  assert.deepEqual(recoveredCopy.body.document.atoms, draftContent.atoms)
  assert.deepEqual(recoveredCopy.body.source, { id: draftDoc, closed: false, discarded_changes: false })
  assert.ok(!existsSync(draftFile))
  assert.equal((await call(draftPath)).body.document.title, 'Saved by the CLI')

  // An unreadable draft is reported and can only be discarded.
  writeFileSync(draftFile, '{"format": "chemifree.draft", "trunc')
  assert.equal((await call(draftPath)).body.recovery.state, 'unreadable')
  const unreadableRecover = await write(`${draftPath}/recover`, { request_id: 'recover-3' })
  assert.deepEqual([unreadableRecover.status, unreadableRecover.body.code], [500, 'document_unreadable'])
  assert.equal((await write(`${draftPath}/discard-draft`, { request_id: 'discard-2' })).status, 200)
  assert.ok(!existsSync(draftFile))

  // A failed autosave keeps the session's changes and the saved file; a retry succeeds. The
  // drafts directory is read-only before the edit, so the service cannot draft it on its own.
  const writable = process.getuid?.() !== 0 // Permissions do not stop root.
  const draftsDir = join(dataDir, 'drafts')
  if (writable) chmodSync(draftsDir, 0o500)
  await write(`${draftPath}/transactions`, {
    request_id: 'draft-edit-6',
    expected_revision: 6,
    commands: [{ op: 'rename', title: 'Autosave retry' }],
  })
  if (writable) {
    const failedAutosave = await write(`${draftPath}/autosave`, { request_id: 'auto-4' })
    chmodSync(draftsDir, 0o700)
    assert.deepEqual([failedAutosave.status, failedAutosave.body.code], [503, 'storage_failed'])
    assert.ok(!existsSync(draftFile))
    assert.equal((await call(draftPath)).body.session.unsaved_changes, true)
  }
  assert.equal((await write(`${draftPath}/autosave`, { request_id: 'auto-4' })).body.session.draft_revision, 7)

  // ── Save as copy ───────────────────────────────────────────────────────────────────────
  const originalBytes = readFileSync(documentFile)
  const copied = await write(`${draftPath}/save-as`, { request_id: 'copy-1', expected_revision: 7 })
  assert.equal(copied.status, 201)
  const copyId = copied.body.document.id
  assert.notEqual(copyId, draftDoc)
  assert.equal(copied.body.document.title, 'Autosave retry (copy)')
  assert.deepEqual(copied.body.source, { id: draftDoc, closed: true, discarded_changes: true })
  assert.deepEqual(readFileSync(documentFile), originalBytes, 'the original file is untouched')
  assert.ok(!existsSync(draftFile), "the source's draft went with its session")
  assert.deepEqual((await write(`${draftPath}/save-as`, { request_id: 'copy-1', expected_revision: 7 })).body, copied.body)
  const copyPath = `/api/v1/documents/${copyId}`
  await write(`${copyPath}/transactions`, {
    request_id: 'copy-edit',
    expected_revision: 0,
    commands: [{ op: 'delete_atoms', atoms: ['a1'] }],
  })
  await write(`${copyPath}/save`, { request_id: 'copy-save', expected_revision: 1 })
  assert.deepEqual(readFileSync(documentFile), originalBytes, 'editing the copy leaves the original alone')
  assert.equal((await call(draftPath)).body.document.title, 'Saved by the CLI')
  const cliCopy = cli('documents', 'save-as', draftDoc, '--expected-revision', '6', '--title', 'CLI copy', '--json')
  assert.equal(cliCopy.status, 0)
  assert.deepEqual(cliCopy.body.source, { id: draftDoc, closed: true, discarded_changes: false })
  assert.deepEqual((await call(`/api/v1/documents/${cliCopy.body.document.id}`)).body.document, cliCopy.body.document, 'HTTP = CLI')
  const staleCopy = cli('documents', 'save-as', draftDoc, '--expected-revision', '5', '--json')
  assert.deepEqual([staleCopy.status, staleCopy.body.code], [1, 'revision_conflict'])

  // A copy that cannot be written leaves nothing behind; the same request then succeeds.
  if (writable) {
    const documentsDir = join(dataDir, 'documents')
    const countBefore = (await call('/api/v1/documents')).body.documents.length
    const shown = (await call(draftPath)).body.document.revision
    chmodSync(documentsDir, 0o500)
    const failedCopy = await write(`${draftPath}/save-as`, { request_id: 'copy-2', expected_revision: shown })
    chmodSync(documentsDir, 0o700)
    assert.deepEqual([failedCopy.status, failedCopy.body.code], [503, 'storage_failed'])
    assert.equal((await call('/api/v1/documents')).body.documents.length, countBefore)
    const retriedCopy = await write(`${draftPath}/save-as`, { request_id: 'copy-2', expected_revision: shown })
    assert.equal(retriedCopy.status, 201)
    assert.equal((await call('/api/v1/documents')).body.documents.length, countBefore + 1)
  }

  // A copy whose source draft cannot be removed (a real permission error on drafts/): the copy
  // is committed, and the source session stays open with its unsaved changes and its draft,
  // which the reply reports as closed: false.
  if (writable) {
    const shown = (await call(draftPath)).body.document.revision
    await write(`${draftPath}/transactions`, {
      request_id: 'left-open-edit',
      expected_revision: shown,
      commands: [{ op: 'rename', title: 'Left open' }],
    })
    const drafted = (await write(`${draftPath}/autosave`, { request_id: 'left-open-auto' })).body
    const draftBytes = readFileSync(draftFile)
    const storedBytes = readFileSync(documentFile)
    const draftsDir = join(dataDir, 'drafts')
    chmodSync(draftsDir, 0o500)
    let leftOpen
    try {
      leftOpen = await write(`${draftPath}/save-as`, { request_id: 'left-open-copy', expected_revision: drafted.document.revision })
    } finally {
      chmodSync(draftsDir, 0o700)
    }
    assert.equal(leftOpen.status, 201)
    assert.deepEqual(leftOpen.body.source, { id: draftDoc, closed: false, discarded_changes: false })
    assert.equal(leftOpen.body.document.title, 'Left open (copy)')
    assert.deepEqual(readFileSync(draftFile), draftBytes, "the source's draft is kept")
    assert.deepEqual(readFileSync(documentFile), storedBytes, "the source's file is untouched")
    const source = (await call(draftPath)).body
    assert.deepEqual(
      [source.document.title, source.session.unsaved_changes, source.session.draft_revision, source.recovery],
      ['Left open', true, drafted.document.revision, null],
      'the source session keeps its changes and still owns its draft',
    )
    const copyNow = (await call(`/api/v1/documents/${leftOpen.body.document.id}`)).body
    assert.deepEqual(copyNow.document, leftOpen.body.document, 'the copy is committed and opens as returned')
    // With the permission back, saving the source commits it and removes its draft.
    await write(`${draftPath}/save`, { request_id: 'left-open-save', expected_revision: drafted.document.revision })
    assert.ok(!existsSync(draftFile))
  }

  // ── Storage feedback ──────────────────────────────────────────────────────────────────
  // The API's save of a document pauses inside its save lock, as a slow disk would. While it
  // does: asked about again, that save is reported as running at once; requests queued
  // behind it wait 3 s for the document's turn and fail as busy; a CLI save of the same
  // document waits 5 s for the lock and fails as busy. None of them changes anything, and
  // each is answered within the time budget. Afterwards, asking about the save again returns
  // its committed reply without writing again.
  await restartApi('SIGTERM', { [DELAY]: '15000' })
  const busyDoc = cli('documents', 'create', '--title', 'Busy', '--json').body.document.id
  const busyPath = `/api/v1/documents/${busyDoc}`
  const busyFile = join(dataDir, 'documents', `${busyDoc}.json`)
  const busyLock = join(dataDir, 'locks', `${busyDoc}.lock`)
  await write(`${busyPath}/transactions`, {
    request_id: 'busy-edit',
    expected_revision: 0,
    commands: [{ op: 'rename', title: 'Busy, edited' }],
  })
  const busyBytes = readFileSync(busyFile)
  const documentCount = () => readdirSync(join(dataDir, 'documents')).filter((name) => name.endsWith('.json')).length
  const busyDocuments = documentCount()
  /** Held once a non-blocking attempt to take the lock is refused. */
  const lockHeld = (path) => {
    try {
      execFileSync('flock', ['--nonblock', '--conflict-exit-code', '9', path, 'true'])
      return false
    } catch (error) {
      if (error.status === 9) return true
      throw error
    }
  }
  const timed = async (request) => {
    const started = performance.now()
    const result = await request
    return { ...result, ms: performance.now() - started }
  }
  const pausedSave = write(`${busyPath}/save`, { request_id: 'busy-save', expected_revision: 1 }, 30_000)
  while (!existsSync(busyLock) || !lockHeld(busyLock)) await sleep(20)
  const asked = await timed(write(`${busyPath}/save`, { request_id: 'busy-save', expected_revision: 1 }))
  assert.deepEqual([asked.status, asked.body.code], [409, 'request_in_progress'])
  assert.ok(asked.ms < 1_000, `answered at once (${Math.round(asked.ms)} ms)`)
  const cliSave = cliProcess(['documents', 'apply', busyDoc, '--expected-revision', '0', '--json', '--commands',
    JSON.stringify([{ op: 'rename', title: 'From the CLI' }])])
  const queued = await Promise.all([
    timed(write(`${busyPath}/save`, { request_id: 'busy-save-2', expected_revision: 1 }, 15_000)),
    timed(write(`${busyPath}/autosave`, { request_id: 'busy-autosave' }, 15_000)),
    timed(write(`${busyPath}/save-as`, { request_id: 'busy-copy', expected_revision: 1 }, 15_000)),
  ])
  for (const reply of queued) {
    assert.deepEqual([reply.status, reply.body.code, reply.body.reason], [503, 'storage_failed', 'busy'])
    assert.ok(reply.ms < 8_500, `answered within the time budget (${Math.round(reply.ms)} ms)`)
  }
  const cliBusy = await cliSave.done
  assert.deepEqual([cliBusy.status, cliBusy.body.code, cliBusy.body.reason], [1, 'storage_failed', 'busy'])
  assert.ok(cliBusy.ms < 8_500, `the CLI gave up within the time budget (${cliBusy.ms} ms)`)
  assert.ok(lockHeld(busyLock), 'the paused save was still running')
  assert.deepEqual(readFileSync(busyFile), busyBytes, 'nothing was committed meanwhile')
  assert.equal(documentCount(), busyDocuments, 'no copy was made')
  assert.ok(!existsSync(join(dataDir, 'drafts', `${busyDoc}.json`)), 'no draft was written')
  const busySaved = await pausedSave
  assert.deepEqual([busySaved.status, busySaved.body.session.unsaved_changes], [200, false])
  const committedAt = statSync(busyFile).mtimeMs
  const askedAgain = await timed(write(`${busyPath}/save`, { request_id: 'busy-save', expected_revision: 1 }))
  assert.deepEqual(askedAgain.body, busySaved.body, 'the committed reply')
  assert.ok(askedAgain.ms < 1_000, `replayed, not saved again (${Math.round(askedAgain.ms)} ms)`)
  assert.equal(statSync(busyFile).mtimeMs, committedAt, 'not written again')
  assert.equal(JSON.parse(readFileSync(busyFile, 'utf8')).document.title, 'Busy, edited')
  await restartApi('SIGTERM')

  // Read-only storage: the save fails with reason read_only and writes nothing. Permissions
  // do not stop root, so this step is skipped then.
  if (process.getuid?.() !== 0) {
    await write(`${busyPath}/transactions`, {
      request_id: 'read-only-edit',
      expected_revision: 1,
      commands: [{ op: 'rename', title: 'Read-only attempt' }],
    })
    const readOnlyBytes = readFileSync(busyFile)
    const documentsDir = join(dataDir, 'documents')
    chmodSync(documentsDir, 0o500)
    let readOnly
    try {
      readOnly = await write(`${busyPath}/save`, { request_id: 'read-only-save', expected_revision: 2 })
    } finally {
      chmodSync(documentsDir, 0o700)
    }
    assert.deepEqual([readOnly.status, readOnly.body.code, readOnly.body.reason], [503, 'storage_failed', 'read_only'])
    assert.deepEqual(readFileSync(busyFile), readOnlyBytes, 'nothing was written')
    assert.equal((await call(busyPath)).body.session.unsaved_changes, true, 'the session kept its changes')
    const writable = await write(`${busyPath}/save`, { request_id: 'read-only-save', expected_revision: 2 })
    assert.equal(writable.status, 200)
  }

  // At the 64-session limit, recovering a draft as a copy evicts the clean source session when
  // every other session holds unsaved work, and says so.
  const fullSource = (await write('/api/v1/documents', { request_id: 'full-source', title: 'Full table' })).body.document.id
  const fullPath = `/api/v1/documents/${fullSource}`
  await write(`${fullPath}/transactions`, {
    request_id: 'full-edit',
    expected_revision: 0,
    commands: [{ op: 'add_atom', element: 'S', position: { x: 0, y: 0 } }],
  })
  await write(`${fullPath}/autosave`, { request_id: 'full-auto' })
  await restartApi()
  const fullFile = join(dataDir, 'documents', `${fullSource}.json`)
  const fullBytes = readFileSync(fullFile)
  assert.equal((await call(fullPath)).body.recovery.state, 'recoverable')
  for (let index = 1; index < 64; index += 1) {
    const busy = (await write('/api/v1/documents', { request_id: `busy-${index}` })).body.document.id
    await write(`/api/v1/documents/${busy}/transactions`, {
      request_id: `busy-edit-${index}`,
      expected_revision: 0,
      commands: [{ op: 'rename', title: `Busy ${index}` }],
    })
  }
  const openBefore = (await call('/api/v1/documents')).body.documents.filter((document) => document.open)
  assert.equal(openBefore.length, 64)
  const fullCopy = await write(`${fullPath}/recover-as-copy`, { request_id: 'full-copy' })
  assert.equal(fullCopy.status, 201)
  assert.deepEqual(fullCopy.body.source, { id: fullSource, closed: true, discarded_changes: false })
  assert.deepEqual(fullCopy.body.document.atoms.map((atom) => atom.element), ['S'])
  const openAfter = (await call('/api/v1/documents')).body.documents.filter((document) => document.open)
  assert.deepEqual(
    [openAfter.length, openAfter.some((document) => document.id === fullSource), openAfter.some((document) => document.id === fullCopy.body.document.id)],
    [64, false, true],
  )
  assert.deepEqual(readFileSync(fullFile), fullBytes, "the source's commit is untouched")
  assert.ok(!existsSync(join(dataDir, 'drafts', `${fullSource}.json`)), 'the draft went into the copy')

  // ── Document list search and sort, rename, trash ───────────────────────────────────────
  const library = {}
  for (const [index, title] of ['Library alpha', 'Library beta', 'Library gamma', 'Other delta'].entries()) {
    const libraryCreated = await write('/api/v1/documents', { request_id: `lib-${index}`, title })
    assert.equal(libraryCreated.status, 201, JSON.stringify(libraryCreated.body))
    library[title] = libraryCreated.body.document.id
  }
  const titles = (body) => body.documents.map((item) => item.title)
  const searched = (await call('/api/v1/documents?query=LIBRARY&sort=title')).body
  assert.deepEqual(titles(searched), ['Library alpha', 'Library beta', 'Library gamma'], 'search ignores case')
  assert.deepEqual(
    titles(cli('documents', 'list', '--query', 'library', '--json').body),
    titles(searched),
    'HTTP search = CLI search',
  )
  assert.deepEqual([(await call('/api/v1/documents?sort=newest')).status], [422], 'unknown sort is refused')
  // Renaming commits at once; it is the newest save, so it sorts first by recency. Recency is
  // the stored file's modification time, a wall-clock time that a clock adjustment can move
  // backward, so the library is first made an hour old rather than relying on the gap between
  // writes.
  const anHourAgo = new Date(Date.now() - 3_600_000)
  for (const id of Object.values(library)) {
    utimesSync(join(dataDir, 'documents', `${id}.json`), anHourAgo, anHourAgo)
  }
  const renamed = await write(`/api/v1/documents/${library['Library beta']}/rename`, {
    request_id: 'rename-beta',
    expected_revision: 0,
    title: 'Library beta, renamed',
  })
  assert.deepEqual([renamed.status, renamed.body.document.revision, renamed.body.session.unsaved_changes], [200, 1, false])
  assert.equal(cli('documents', 'show', library['Library beta'], '--json').body.document.title, 'Library beta, renamed')
  const recent = (await call('/api/v1/documents?query=library&sort=recent')).body
  assert.equal(recent.documents[0].title, 'Library beta, renamed', 'most recently saved first')
  assert.deepEqual(
    titles(cli('documents', 'list', '--query', 'library', '--sort', 'recent', '--json').body),
    titles(recent),
    'HTTP order = CLI order',
  )

  // Deleting over HTTP: the CLI sees it in the trash, not among the documents.
  const alpha = library['Library alpha']
  await write(`/api/v1/documents/${alpha}/transactions`, {
    request_id: 'alpha-edit',
    expected_revision: 0,
    commands: [{ op: 'rename', title: 'Library alpha, edited' }],
  })
  const unsavedDelete = await write(`/api/v1/documents/${alpha}/delete`, { request_id: 'alpha-delete-0', expected_revision: 1 })
  assert.deepEqual([unsavedDelete.status, unsavedDelete.body.code], [409, 'unsaved_changes'])
  await write(`/api/v1/documents/${alpha}/save`, { request_id: 'alpha-save', expected_revision: 1 })
  const deleted = await write(`/api/v1/documents/${alpha}/delete`, { request_id: 'alpha-delete', expected_revision: 1 })
  assert.deepEqual(deleted.body, { api_version: 'v1', id: alpha, title: 'Library alpha, edited' })
  assert.ok(!existsSync(join(dataDir, 'documents', `${alpha}.json`)) && existsSync(join(dataDir, 'trash', `${alpha}.json`)))
  assert.ok(!cli('documents', 'list', '--json').body.documents.some((item) => item.id === alpha))
  const trashHttp = (await call('/api/v1/trash')).body
  assert.deepEqual(trashHttp, cli('documents', 'trash', '--json').body, 'HTTP trash = CLI trash')
  assert.deepEqual(trashHttp.documents.map((item) => [item.id, item.revision]), [[alpha, 1]])
  assert.equal((await call(`/api/v1/documents/${alpha}`)).status, 404)

  // Deleting with the CLI: the API's clean copy closes instead of showing it as stored, and
  // the API's unsaved copy of another document cannot re-create it by saving or drafting.
  const gamma = library['Library gamma']
  await call(`/api/v1/documents/${gamma}`)
  assert.equal(cli('documents', 'delete', gamma, '--expected-revision', '0', '--json').status, 0)
  assert.equal((await call(`/api/v1/documents/${gamma}`)).status, 404, 'the clean copy closed')
  const delta = library['Other delta']
  await write(`/api/v1/documents/${delta}/transactions`, {
    request_id: 'delta-edit',
    expected_revision: 0,
    commands: [{ op: 'rename', title: 'Unsaved in the API' }],
  })
  assert.equal(cli('documents', 'delete', delta, '--expected-revision', '0', '--json').status, 0,
    'without a draft, the CLI cannot see the API session')
  const staleSave = await write(`/api/v1/documents/${delta}/save`, { request_id: 'delta-save', expected_revision: 1 })
  assert.deepEqual([staleSave.status, staleSave.body.code], [409, 'save_conflict'])
  const staleDraft = await write(`/api/v1/documents/${delta}/autosave`, { request_id: 'delta-draft' })
  assert.deepEqual([staleDraft.status, staleDraft.body.code], [404, 'document_not_found'])
  assert.ok(!existsSync(join(dataDir, 'documents', `${delta}.json`)), 'not re-created')
  assert.ok(!existsSync(join(dataDir, 'drafts', `${delta}.json`)), 'no draft outlives it')
  const rescued = await write(`/api/v1/documents/${delta}/save-as`, { request_id: 'delta-copy', expected_revision: 1 })
  assert.equal(rescued.body.document.title, 'Unsaved in the API (copy)', 'the unsaved work can still be kept')

  // Drafts block a delete: the API's own (in use by a running process), then an abandoned one.
  const beta = library['Library beta']
  await write(`/api/v1/documents/${beta}/transactions`, {
    request_id: 'beta-edit',
    expected_revision: 1,
    commands: [{ op: 'rename', title: 'Library beta, drafted' }],
  })
  await write(`/api/v1/documents/${beta}/autosave`, { request_id: 'beta-draft' })
  const betaInUse = cli('documents', 'delete', beta, '--expected-revision', '1', '--json')
  assert.deepEqual([betaInUse.status, betaInUse.body.code], [1, 'draft_in_use'])
  await restartApi() // SIGKILL: the draft is abandoned.
  const pendingTrash = cli('documents', 'delete', beta, '--expected-revision', '1', '--json')
  assert.deepEqual([pendingTrash.status, pendingTrash.body.code], [1, 'recovery_pending'])
  assert.ok(existsSync(join(dataDir, 'documents', `${beta}.json`)), 'still stored with its draft')

  // A process killed in the middle of a delete leaves the document stored, not half-trashed.
  const crashing = (await write('/api/v1/documents', { request_id: 'crash-delete-create', title: 'Interrupted delete' })).body.document.id
  const killed = cliProcess(['documents', 'delete', crashing, '--expected-revision', '0', '--json'], { [DELAY]: '20000' })
  const stamp = join(dataDir, 'trash', `${crashing}.deleted`)
  while (!existsSync(stamp)) await sleep(20) // Inside the delete: the stamp precedes the move.
  killed.child.kill('SIGKILL')
  await killed.done
  assert.ok(existsSync(join(dataDir, 'documents', `${crashing}.json`)), 'still stored')
  assert.ok(!cli('documents', 'trash', '--json').body.documents.some((item) => item.id === crashing), 'not listed in the trash')
  assert.equal(cli('documents', 'delete', crashing, '--expected-revision', '0', '--json').status, 0, 'deletable afterwards')

  // Restoring brings the document back as it was trashed.
  const restored = await write(`/api/v1/trash/${alpha}/restore`, { request_id: 'alpha-restore' })
  assert.deepEqual(restored.body, { api_version: 'v1', id: alpha, title: 'Library alpha, edited' })
  assert.deepEqual((await write(`/api/v1/trash/${alpha}/restore`, { request_id: 'alpha-restore' })).body, restored.body, 'replayed')
  const again = await write(`/api/v1/trash/${alpha}/restore`, { request_id: 'alpha-restore-2' })
  assert.deepEqual([again.status, again.body.code], [404, 'document_not_found'])
  assert.equal(cli('documents', 'show', alpha, '--json').body.document.title, 'Library alpha, edited')
  assert.equal(cli('documents', 'restore', gamma, '--json').status, 0)
  assert.equal((await call(`/api/v1/documents/${gamma}`)).body.document.title, 'Library gamma')

  // ── Drafts written by the service, with no client asking ─────────────────────────────────
  /** Reads `path` until `done` holds for its session, or fails after `ms` with the last one. */
  async function untilSession(path, done, ms) {
    const deadline = performance.now() + ms
    for (;;) {
      const { session } = (await call(path)).body
      if (done(session)) return session
      if (performance.now() > deadline) assert.fail(`session did not reach the expected state: ${JSON.stringify(session)}`)
      await sleep(100)
    }
  }
  // Quiet period (2 s) plus the writer's tick, with room for a loaded machine.
  const DRAFTED_WITHIN = 6_000
  const unattended = (await write('/api/v1/documents', { request_id: 'unattended-create', title: 'Unattended' })).body
  const unattendedPath = `/api/v1/documents/${unattended.document.id}`
  const unattendedDraft = join(draftsDir, `${unattended.document.id}.json`)
  const unattendedEdited = (await write(`${unattendedPath}/transactions`, { ...draw, request_id: 'unattended-edit' })).body
  assert.ok(!existsSync(unattendedDraft), 'not drafted before the quiet period')
  await untilSession(unattendedPath, (session) => session.draft_revision === 1, DRAFTED_WITHIN)
  assert.ok(existsSync(unattendedDraft))
  await restartApi() // SIGKILL: the drafted work survives a crash.
  assert.equal((await call(unattendedPath)).body.recovery.state, 'recoverable')
  const unattendedRecovered = await write(`${unattendedPath}/recover`, { request_id: 'unattended-recover' })
  assert.deepEqual(unattendedRecovered.body.document, unattendedEdited.document, 'recovered exactly as edited')
  assert.equal(unattendedRecovered.body.session.undo_depth, 1)

  // A normal stop drafts unsaved work at once instead of losing the quiet period's worth.
  await write(`${unattendedPath}/transactions`, {
    request_id: 'unattended-rename',
    expected_revision: 1,
    commands: [{ op: 'rename', title: 'Drafted at stop' }],
  })
  await restartApi('SIGTERM')
  assert.equal((await call(unattendedPath)).body.recovery.revision, 2, 'the stop drafted the last edit')
  await write(`${unattendedPath}/recover`, { request_id: 'unattended-recover-2' })

  // Continuous edits cannot postpone a draft past the maximum wait (10 s).
  const editing = performance.now()
  let continuousRevision = 2
  let continuous
  for (;;) {
    continuous = (await write(`${unattendedPath}/transactions`, {
      request_id: `unattended-continuous-${continuousRevision}`,
      expected_revision: continuousRevision,
      commands: [{ op: 'rename', title: `Edit ${continuousRevision}` }],
    })).body.session
    continuousRevision += 1
    if (continuous.draft_revision !== 2) break
    assert.ok(performance.now() - editing < 13_000, 'drafted while edits continue')
    await sleep(400)
  }
  const waited = Math.round(performance.now() - editing)
  assert.ok(waited >= 9_500, `not drafted before the maximum wait (${waited} ms): ${JSON.stringify(continuous)}`)

  // A failed draft is retried on its own once storage recovers, and reported meanwhile.
  if (writable) {
    chmodSync(draftsDir, 0o500)
    let failing
    try {
      await write(`${unattendedPath}/transactions`, {
        request_id: 'unattended-blocked',
        expected_revision: continuousRevision,
        commands: [{ op: 'rename', title: 'Drafted after a failure' }],
      })
      failing = await untilSession(unattendedPath, (session) => session.draft_failure !== null, DRAFTED_WITHIN)
    } finally {
      chmodSync(draftsDir, 0o700)
    }
    assert.equal(failing.draft_failure.attempts, 1)
    assert.deepEqual(
      [failing.draft_failure.code, failing.draft_failure.reason],
      ['storage_failed', 'read_only'],
      'the record names the failure as the v1 error envelope does',
    )
    const retried = await untilSession(
      unattendedPath,
      (session) => session.draft_revision === continuousRevision + 1,
      DRAFTED_WITHIN,
    )
    assert.equal(retried.draft_failure, null, 'the retry clears the failure')
  }

  // Operation contract 1: the same requests get the same replies over HTTP and from one CLI
  // process (whose sessions last as long as it runs), apart from the generated document ID.
  {
    const operations = spawn(cliPath, ['operations'], { env, stdio: ['pipe', 'pipe', 'inherit'] })
    running.add(operations)
    const replies = []
    const waiting = []
    let pendingText = ''
    operations.stdout.on('data', (data) => {
      pendingText += data
      for (let end = pendingText.indexOf('\n'); end >= 0; end = pendingText.indexOf('\n')) {
        const line = pendingText.slice(0, end)
        pendingText = pendingText.slice(end + 1)
        const reply = JSON.parse(line)
        if (waiting.length > 0) waiting.shift()(reply)
        else replies.push(reply)
      }
    })
    /** Sends one line to the CLI and resolves with its reply. */
    const viaCli = (line) =>
      new Promise((resolveReply) => {
        waiting.push(resolveReply)
        operations.stdin.write(`${line}\n`)
      })
    const viaHttp = async (text, headers = auth) => {
      const response = await fetch(`${api.baseUrl}/api/v1/operations`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', ...headers },
        body: text,
        signal: AbortSignal.timeout(5_000),
      })
      assert.equal(response.headers.get('access-control-allow-origin'), null)
      return { status: response.status, body: await response.json() }
    }
    const request = (id, operation, parameters) =>
      JSON.stringify({ contract: 1, request_id: id, profile: 'preserve-semantics', operation, parameters })
    let httpDoc
    let cliDoc
    /** Sends `build(documentId)` both ways and checks that the replies agree. */
    const both = async (build, status = 200) => {
      const http = await viaHttp(build(httpDoc))
      const viaLine = await viaCli(build(cliDoc))
      assert.equal(http.status, status, JSON.stringify(http.body))
      const masked = (reply, id) => JSON.parse(JSON.stringify(reply).replaceAll(id ?? '\u0000', '<document>'))
      assert.deepEqual(masked(http.body, httpDoc), masked(viaLine, cliDoc), 'HTTP operation = CLI operation')
      return http.body
    }

    const catalog = await both(() => request('op-caps', 'capabilities.inspect', {}))
    assert.deepEqual(catalog.result, (await call('/api/v1/capabilities')).body.operation_catalog)
    const create = request('op-create', 'document.create', { title: 'Operations parity' })
    httpDoc = (await viaHttp(create)).body.result.document.id
    cliDoc = (await viaCli(create)).result.document.id
    const edit = (id) =>
      request('op-edit', 'document.transaction', {
        document_id: id,
        expected_revision: 0,
        commands: [{ op: 'add_atom', element: 'C', position: { x: 0, y: 0 } }],
      })
    assert.equal((await both(edit)).document_revision, 1)
    assert.equal((await both(edit)).document_revision, 1, 'a repeated request ID replays its reply')
    const mol = await both((id) => request('op-mol', 'document.export', { format: 'mol', document_id: id }))
    assert.equal(mol.outcome, 'partial')
    assert.match(mol.result.text, /\bV2000\n/)
    assert.ok(mol.loss_report.some((loss) => loss.feature === 'document_settings'))
    const stale = await both((id) => request('op-stale', 'history.undo', { document_id: id, expected_revision: 0 }))
    assert.deepEqual([stale.outcome, stale.diagnostics[0].code], ['error', 'revision_conflict'])
    const unsupported = await both(() => request('op-props', 'chem.properties', {}))
    assert.deepEqual([unsupported.outcome, unsupported.result], ['unsupported', null])
    await both(() => request('op-x', 'chem.teleport', {}), 400)
    const malformed = await both(() => '{"contract": 1,', 400)
    assert.deepEqual([malformed.envelope, malformed.diagnostics[0].code], ['protocol_error', 'malformed_request'])
    const saved = await both((id) => request('op-save', 'document.save', { document_id: id, expected_revision: 1 }))
    assert.equal(saved.result.session.unsaved_changes, false)
    await both((id) => request('op-close', 'document.close', { document_id: id }))

    // The CLI reads at most 1 MiB per request; the line ending, LF or CRLF, does not count.
    const LINE_LIMIT = 1024 * 1024
    /** A capabilities request padded with JSON whitespace to exactly `size` bytes. */
    const padded = (id, size) => {
      const text = request(id, 'capabilities.inspect', {})
      return `${text.slice(0, -1)}${' '.repeat(size - Buffer.byteLength(text))}}`
    }
    /** Writes `text` as is and resolves with the reply it gets. */
    const viaCliRaw = (text) =>
      new Promise((resolveReply) => {
        waiting.push(resolveReply)
        operations.stdin.write(text)
      })
    for (const ending of ['\n', '\r\n']) {
      const atLimit = await viaCliRaw(`${padded('op-limit', LINE_LIMIT)}${ending}`)
      assert.deepEqual([atLimit.outcome, atLimit.diagnostics], ['ok', []], `a 1 MiB request ending ${JSON.stringify(ending)}`)
    }
    const overLimit = await viaCliRaw(`${padded('op-over', LINE_LIMIT + 1)}\r\n`)
    assert.deepEqual([overLimit.envelope, overLimit.diagnostics[0].code], ['protocol_error', 'resource_limit'])
    const afterOver = await viaCli(request('op-after', 'capabilities.inspect', {}))
    assert.deepEqual([afterOver.request_id, afterOver.outcome], ['op-after', 'ok'], 'the next line still runs')
    operations.stdin.end()
    const [exitStatus] = await once(operations, 'exit')
    running.delete(operations)
    assert.equal(exitStatus, 0)
    assert.deepEqual(replies, [], 'one reply per request')
    assert.equal(cli('documents', 'show', cliDoc, '--json').body.document.atoms.length, 1, 'the CLI process saved its edit')

    // The HTTP adapter keeps the write guards, and answers what it cannot read in the envelope.
    const unguarded = await viaHttp(request('op-guard', 'document.create', {}), {})
    assert.deepEqual([unguarded.status, unguarded.body.code], [401, 'unauthorized'])
    const plain = await fetch(`${api.baseUrl}/api/v1/operations`, {
      method: 'POST',
      headers: { ...auth, 'content-type': 'text/plain' },
      body: request('op-plain', 'document.create', {}),
      signal: AbortSignal.timeout(5_000),
    })
    assert.equal(plain.status, 415)
    assert.deepEqual((await plain.json()).diagnostics[0].code, 'unsupported_media_type')
    const suffixed = await viaHttp(request('op-suffix', 'capabilities.inspect', {}), {
      ...auth,
      'content-type': 'application/merge-patch+json',
    })
    assert.deepEqual([suffixed.status, suffixed.body.outcome], [200, 'ok'], 'a +json type is JSON, as on the v1 routes')
    const oversized = await viaHttp(`${request('op-big', 'capabilities.inspect', {}).slice(0, -1)}${' '.repeat(256 * 1024)}}`)
    assert.deepEqual([oversized.status, oversized.body.diagnostics[0].code], [413, 'resource_limit'])
  }

  // ── Recorded states: a refused combination has the same reason over HTTP and the CLI ─────
  {
    const statesDoc = (await write('/api/v1/documents', { request_id: 'states-create', title: 'States' })).body.document.id
    const statesPath = `/api/v1/documents/${statesDoc}`
    const drawn = await write(`${statesPath}/transactions`, {
      request_id: 'states-draw',
      expected_revision: 0,
      commands: [
        { op: 'add_atom', element: 'C', position: { x: 0, y: 0 }, hydrogens: { mode: 'specified', count: 3 }, radical: 'doublet' },
        { op: 'add_atom', query: { kind: 'halogen' }, position: { x: 30, y: 0 }, bond_to: { atom: '$0', order: 'dative' } },
      ],
    })
    assert.equal(drawn.status, 200)
    // The CLI works on storage, so the drawing is saved first.
    assert.equal((await write(`${statesPath}/save`, { request_id: 'states-save', expected_revision: 1 })).status, 200)
    assert.deepEqual(cli('documents', 'show', statesDoc, '--json').body.document, drawn.body.document, 'HTTP = CLI view of states')
    for (const [name, command, reason] of [
      ['a charged query atom', { op: 'update_atom', atom: 'a2', charge: 1 }, 'query_atom_charge'],
      ['17 stated hydrogens', { op: 'update_atom', atom: 'a1', hydrogens: { mode: 'specified', count: 17 } }, 'hydrogen_count_out_of_range'],
      ['a query bond without its query', { op: 'update_bond', bond: 'b1', order: 'query' }, 'bond_query_missing'],
      ['attachment points out of order', { op: 'update_atom', atom: 'a1', attachment_points: [2, 1] }, 'attachment_points_invalid'],
      ['an element and a query', { op: 'update_atom', atom: 'a1', element: 'N', query: { kind: 'any' } }, 'atom_kind_conflict'],
    ]) {
      const commands = [{ op: 'update_atom', atom: 'a1', radical: 'triplet' }, command]
      const http = await write(`${statesPath}/transactions`, { request_id: `states-${reason}`, expected_revision: 1, commands })
      assert.deepEqual(
        [http.status, http.body.code, http.body.command_index, http.body.reason],
        [422, 'invalid_command', 1, reason],
        `${name} over HTTP`,
      )
      const viaCli = cli('documents', 'apply', statesDoc, '--expected-revision', '1', '--json', '--commands', JSON.stringify(commands))
      assert.deepEqual(
        [viaCli.status, viaCli.body.code, viaCli.body.command_index, viaCli.body.reason, viaCli.body.message],
        [1, 'invalid_command', 1, reason, http.body.message],
        `${name} from the CLI`,
      )
    }
    // A malformed value is a malformed body, without a reason, from either adapter.
    const malformed = [{ op: 'update_atom', atom: 'a1', radical: 'quartet' }]
    const badHttp = await write(`${statesPath}/transactions`, { request_id: 'states-malformed', expected_revision: 1, commands: malformed })
    assert.deepEqual([badHttp.status, badHttp.body.code, 'reason' in badHttp.body], [422, 'invalid_request', false])
    const badCli = cli('documents', 'apply', statesDoc, '--expected-revision', '1', '--json', '--commands', JSON.stringify(malformed))
    assert.deepEqual([badCli.body.code, 'reason' in badCli.body], ['invalid_request', false])
    assert.equal((await call(statesPath)).body.document.revision, 1, 'nothing changed')
  }

  // ── Document settings and preferences: the same rules, codes, and reasons over HTTP and the CLI
  // Preferences live in the data directory, so the CLI process and the API share them: a write
  // from one makes the other's stale write a revision conflict. A document copies its settings
  // when it is created and keeps them; settings are edited, reset, and imported as edits.
  {
    const put = (body) => call('/api/v1/preferences', { method: 'PUT', body, headers: auth })
    const shown = await call('/api/v1/preferences')
    assert.equal(shown.status, 200)
    assert.deepEqual(cli('preferences', 'show', '--json').body, shown.body, 'HTTP GET = CLI show of preferences')
    assert.deepEqual(
      [shown.body.revision, shown.body.computation.nmr.status, shown.body.computation.numeric_precision.status],
      [0, 'not_implemented', 'not_implemented'],
    )
    assert.equal(shown.body.computation.numeric_precision.followup, 'P19')
    const before = (await write('/api/v1/documents', { request_id: 'style-before', title: 'Before preferences' })).body.document
    assert.equal(before.settings.label_font, 'sans-serif')
    assert.equal((await write(`/api/v1/documents/${before.id}/save`, { request_id: 'style-before-save', expected_revision: 0 })).status, 200)
    const beforeBytes = readFileSync(join(dataDir, 'documents', `${before.id}.json`), 'utf8')

    // The CLI process writes first; the API's write from the same base is a conflict.
    const change = { ui: { theme: 'dark' }, new_documents: { label_font: 'monospace', bond_length_pt: 40 } }
    const viaCli = cli('preferences', 'set', '--expected-revision', '0', '--json', '--preferences', JSON.stringify(change))
    assert.deepEqual([viaCli.status, viaCli.body.revision], [0, 1])
    const stale = await put({ request_id: 'prefs-stale', expected_revision: 0, ...change })
    assert.deepEqual([stale.status, stale.body.code, stale.body.current_revision], [409, 'revision_conflict', 1])
    const staleCli = cli('preferences', 'set', '--expected-revision', '0', '--json', '--preferences', JSON.stringify(change))
    assert.deepEqual(
      [staleCli.status, staleCli.body.code, staleCli.body.current_revision, staleCli.body.message],
      [1, 'revision_conflict', 1, stale.body.message],
      'the same conflict from the CLI',
    )
    assert.ok(existsSync(join(dataDir, 'preferences.json')))

    // Computation preferences are refused alike, and nothing is stored.
    const computation = { ...change, computation: { nmr: { solvent: 'CDCl3' } } }
    const nmr = await put({ request_id: 'prefs-nmr', expected_revision: 1, ...computation })
    assert.deepEqual([nmr.status, nmr.body.code], [501, 'setting_not_implemented'])
    const nmrCli = cli('preferences', 'set', '--expected-revision', '1', '--json', '--preferences', JSON.stringify(computation))
    assert.deepEqual([nmrCli.status, nmrCli.body.code, nmrCli.body.message], [1, 'setting_not_implemented', nmr.body.message])
    const precision = { ...change, computation: { numeric_precision: null } }
    const precisionHttp = await put({ request_id: 'prefs-precision', expected_revision: 1, ...precision })
    assert.deepEqual([precisionHttp.status, precisionHttp.body.code], [501, 'setting_not_implemented'])
    assert.match(precisionHttp.body.message, /\(P19\)/)
    const precisionCli = cli('preferences', 'set', '--expected-revision', '1', '--json', '--preferences', JSON.stringify(precision))
    assert.deepEqual([precisionCli.status, precisionCli.body.code, precisionCli.body.message], [1, 'setting_not_implemented', precisionHttp.body.message])
    assert.equal((await call('/api/v1/preferences')).body.revision, 1, 'refused computation settings store nothing')
    // A preferred value out of range is refused with the setting's reason.
    const tooLarge = { ui: { theme: 'system' }, new_documents: { label_size_pt: 80 } }
    const large = await put({ request_id: 'prefs-large', expected_revision: 1, ...tooLarge })
    assert.deepEqual([large.status, large.body.code, large.body.reason], [422, 'invalid_request', 'label_size_out_of_range'])
    const largeCli = cli('preferences', 'set', '--expected-revision', '1', '--json', '--preferences', JSON.stringify(tooLarge))
    assert.deepEqual(
      [largeCli.status, largeCli.body.code, largeCli.body.reason, largeCli.body.message],
      [1, 'invalid_request', 'label_size_out_of_range', large.body.message],
    )
    assert.equal(cli('preferences', 'show', '--json').body.revision, 1, 'refusals store nothing')

    // New documents copy the preferences once; the document made before them keeps its own.
    const afterHttp = (await write('/api/v1/documents', { request_id: 'style-after', title: 'After preferences' })).body.document
    const afterCli = cli('documents', 'create', '--title', 'After preferences (CLI)', '--json').body.document
    for (const created of [afterHttp, afterCli]) {
      assert.deepEqual([created.settings.label_font, created.settings.bond_length_pt, created.settings.line_width_pt], ['monospace', 40, 1.2])
    }
    assert.equal(readFileSync(join(dataDir, 'documents', `${before.id}.json`), 'utf8'), beforeBytes, 'the earlier file is untouched')
    assert.equal(cli('documents', 'show', before.id, '--json').body.document.settings.label_font, 'sans-serif')
    assert.match(
      execFileSync(cliPath, ['documents', 'show', before.id], { env, encoding: 'utf8' }),
      /settings {2}bond length 30 pt · line width 1\.2 pt · labels sans-serif 12 pt/,
    )

    // update_settings refusals carry the same reason and message from either adapter.
    const cliDoc = afterCli.id
    for (const [name, command, reason] of [
      ['a bond length below 5 pt', { op: 'update_settings', bond_length_pt: 4.9 }, 'bond_length_out_of_range'],
      ['a line width of 0', { op: 'update_settings', line_width_pt: 0 }, 'line_width_out_of_range'],
      ['a label size over 72 pt', { op: 'update_settings', label_size_pt: 72.5 }, 'label_size_out_of_range'],
    ]) {
      const commands = [{ op: 'rename', title: 'Refused' }, command]
      const http = await write(`/api/v1/documents/${afterHttp.id}/transactions`, { request_id: `style-${reason}`, expected_revision: 0, commands })
      assert.deepEqual([http.status, http.body.code, http.body.command_index, http.body.reason], [422, 'invalid_command', 1, reason], `${name} over HTTP`)
      const refusedCli = cli('documents', 'apply', cliDoc, '--expected-revision', '0', '--json', '--commands', JSON.stringify(commands))
      assert.deepEqual(
        [refusedCli.status, refusedCli.body.code, refusedCli.body.command_index, refusedCli.body.reason, refusedCli.body.message],
        [1, 'invalid_command', 1, reason, http.body.message],
        `${name} from the CLI`,
      )
    }
    const badFont = [{ op: 'update_settings', label_font: 'cursive' }]
    const badFontHttp = await write(`/api/v1/documents/${afterHttp.id}/transactions`, { request_id: 'style-bad-font', expected_revision: 0, commands: badFont })
    assert.deepEqual([badFontHttp.status, badFontHttp.body.code, 'reason' in badFontHttp.body], [422, 'invalid_request', false])
    assert.equal(cli('documents', 'apply', cliDoc, '--expected-revision', '0', '--json', '--commands', JSON.stringify(badFont)).body.code, 'invalid_request')

    // Edited from the CLI, reset and imported over HTTP and from the CLI, each one undoable edit.
    const styled = cli('documents', 'apply', cliDoc, '--expected-revision', '0', '--json', '--commands',
      JSON.stringify([{ op: 'update_settings', carbon_labels: 'all', display_unit: 'mm' }]))
    assert.deepEqual([styled.status, styled.body.document.settings.carbon_labels], [0, 'all'])
    assert.deepEqual(cli('documents', 'settings', 'show', cliDoc, '--json').body, styled.body.document.settings)
    const imported = await write(`/api/v1/documents/${afterHttp.id}/settings/import`, { request_id: 'style-import', expected_revision: 0, source: cliDoc })
    assert.deepEqual([imported.status, imported.body.document.settings, imported.body.session.undo_depth], [200, styled.body.document.settings, 1])
    const resetCli = cli('documents', 'settings', 'reset', cliDoc, '--to', 'defaults', '--expected-revision', '1', '--json')
    assert.deepEqual([resetCli.status, resetCli.body.document.settings], [0, shown.body.new_document_settings])
    // The CLI reads stored documents, so the imported style is saved first.
    assert.equal((await write(`/api/v1/documents/${afterHttp.id}/save`, { request_id: 'style-after-save', expected_revision: 1 })).status, 200)
    const importCli = cli('documents', 'settings', 'import', cliDoc, '--source', afterHttp.id, '--expected-revision', '2', '--json')
    assert.deepEqual([importCli.status, importCli.body.document.revision, importCli.body.document.settings], [0, 3, styled.body.document.settings])
    const missing = await write(`/api/v1/documents/${afterHttp.id}/settings/import`, { request_id: 'style-import-missing', expected_revision: 1, source: 'doc-ffffffffffffffff' })
    const missingCli = cli('documents', 'settings', 'import', cliDoc, '--source', 'doc-ffffffffffffffff', '--expected-revision', '3', '--json')
    assert.deepEqual([missing.status, missing.body.code], [404, 'document_not_found'])
    assert.deepEqual([missingCli.body.code, missingCli.body.message], ['document_not_found', missing.body.message])
    const resetHttp = await write(`/api/v1/documents/${afterHttp.id}/settings/reset`, { request_id: 'style-reset', expected_revision: 1, to: 'preferences' })
    assert.deepEqual(resetHttp.body.document.settings, cli('preferences', 'show', '--json').body.new_document_settings)

    // New documents take every setting of a document, and the preferences reset, from either adapter.
    const fromDocument = await write('/api/v1/preferences/import', { request_id: 'prefs-import', expected_revision: 1, document: cliDoc })
    assert.deepEqual([fromDocument.status, fromDocument.body.revision, fromDocument.body.new_document_settings], [200, 2, styled.body.document.settings])
    const resetPrefs = cli('preferences', 'reset', '--expected-revision', '2', '--json')
    assert.deepEqual([resetPrefs.status, resetPrefs.body.revision, resetPrefs.body.new_documents, resetPrefs.body.ui.theme], [0, 3, {}, 'system'])
    assert.deepEqual((await call('/api/v1/preferences')).body, resetPrefs.body, 'the API reads what the CLI wrote')
  }

  // ── Version 1 files (before graph and scene were separated) ─────────────────────────────
  // A version 1 file placed in the data directory opens over HTTP and from the CLI as one
  // scene instance with its history; reading writes nothing. The first save that replaces it,
  // from either adapter, keeps its exact bytes under migrated/. A newer version is refused.
  {
    const fixtures = resolve(import.meta.dirname, '../crates/providers/tests/data')
    const fixtureId = 'doc-00000000000000f1'
    const placed = (name, documentId) => {
      const bytes = readFileSync(join(fixtures, name), 'utf8').replaceAll(fixtureId, documentId)
      const path = join(dataDir, 'documents', `${documentId}.json`)
      writeFileSync(path, bytes)
      return { path, bytes }
    }
    const kept = (documentId) => join(dataDir, 'migrated', `${documentId}.v1.json`)

    const viaHttp = placed('v1-document.json', 'doc-00000000000000a1')
    const opened = await call('/api/v1/documents/doc-00000000000000a1')
    assert.equal(opened.status, 200)
    assert.equal(opened.body.document.revision, 4)
    assert.deepEqual(opened.body.document.scene.instances, [
      { id: 's1', page: 'p1', molecule: 'm1', positions: [{ atom: 'a1', x: 0, y: 0 }, { atom: 'a2', x: 30, y: 10.5 }] },
    ])
    assert.deepEqual(opened.body.document.atoms.map((atom) => atom.position), [{ x: 0, y: 0 }, { x: 30, y: 10.5 }])
    assert.deepEqual([opened.body.session.undo_depth, opened.body.session.redo_depth], [2, 1])
    assert.deepEqual(cli('documents', 'show', 'doc-00000000000000a1', '--json').body, opened.body, 'HTTP GET = CLI show of a version 1 file')
    assert.equal(readFileSync(viaHttp.path, 'utf8'), viaHttp.bytes, 'opening wrote nothing')
    assert.equal(existsSync(join(dataDir, 'migrated')), false)

    const undone = await write('/api/v1/documents/doc-00000000000000a1/undo', { request_id: 'v1-undo', expected_revision: 4 })
    assert.deepEqual(undone.body.document.scene.instances[0].positions[1], { atom: 'a2', x: 30, y: 0 }, 'migrated history undoes')
    const savedV1 = await write('/api/v1/documents/doc-00000000000000a1/save', { request_id: 'v1-save', expected_revision: 5 })
    assert.equal(savedV1.status, 200)
    assert.equal(readFileSync(kept('doc-00000000000000a1'), 'utf8'), viaHttp.bytes, 'the first save kept the exact version 1 bytes')
    assert.equal(JSON.parse(readFileSync(viaHttp.path, 'utf8')).format_version, 5)
    assert.deepEqual(cli('documents', 'show', 'doc-00000000000000a1', '--json').body.document, savedV1.body.document)

    const viaCli = placed('v1-document.json', 'doc-00000000000000a2')
    const redone = cli('documents', 'redo', 'doc-00000000000000a2', '--expected-revision', '4', '--json')
    assert.equal(redone.status, 0)
    assert.equal(redone.body.document.title, 'Fixture edited')
    assert.equal(readFileSync(kept('doc-00000000000000a2'), 'utf8'), viaCli.bytes, 'a CLI commit keeps it too')
    const again = cli('documents', 'undo', 'doc-00000000000000a2', '--expected-revision', '5', '--json')
    assert.equal(again.status, 0)
    assert.equal(readFileSync(kept('doc-00000000000000a2'), 'utf8'), viaCli.bytes, 'a later save leaves the kept copy')
    const listedIds = cli('documents', 'list', '--json').body.documents.map((item) => item.id)
    assert.ok(!listedIds.some((listedId) => listedId.includes('.v1')), 'kept copies are not documents')

    // A version 2 file (before recorded states) opens with the default states and its
    // history; its first save keeps the exact bytes as a version 2 copy.
    const keptV2 = (documentId) => join(dataDir, 'migrated', `${documentId}.v2.json`)
    const v2 = placed('v2-document.json', 'doc-00000000000000b2')
    const openedV2 = await call('/api/v1/documents/doc-00000000000000b2')
    assert.equal(openedV2.status, 200)
    assert.deepEqual(
      [openedV2.body.document.revision, openedV2.body.session.undo_depth, openedV2.body.session.redo_depth],
      [6, 4, 1],
    )
    for (const atom of openedV2.body.document.atoms) {
      assert.deepEqual(
        [atom.kind, atom.query, atom.hydrogens, atom.radical, atom.attachment_points],
        ['element', null, { mode: 'inferred' }, 'none', []],
        `${atom.id} has the default states`,
      )
    }
    assert.deepEqual(openedV2.body.document.bonds.map((bond) => [bond.order, bond.query]), [['double', null]])
    assert.deepEqual(cli('documents', 'show', 'doc-00000000000000b2', '--json').body, openedV2.body, 'HTTP GET = CLI show of a version 2 file')
    assert.equal(readFileSync(v2.path, 'utf8'), v2.bytes, 'opening wrote nothing')
    const radical = await write('/api/v1/documents/doc-00000000000000b2/transactions', {
      request_id: 'v2-radical',
      expected_revision: 6,
      commands: [{ op: 'update_atom', atom: 'a1', radical: 'doublet' }],
    })
    assert.equal(radical.status, 200)
    const savedV2 = await write('/api/v1/documents/doc-00000000000000b2/save', { request_id: 'v2-save', expected_revision: 7 })
    assert.equal(savedV2.status, 200)
    assert.equal(readFileSync(keptV2('doc-00000000000000b2'), 'utf8'), v2.bytes, 'the first save kept the exact version 2 bytes')
    assert.equal(JSON.parse(readFileSync(v2.path, 'utf8')).format_version, 5)
    assert.equal(cli('documents', 'show', 'doc-00000000000000b2', '--json').body.document.atoms[0].radical, 'doublet')
    const undoneV2 = cli('documents', 'undo', 'doc-00000000000000b2', '--expected-revision', '7', '--json')
    assert.equal(undoneV2.body.document.atoms[0].radical, 'none', 'the edit undoes after saving in version 5')

    // Both entry adapters preserve an old draft's exact bytes before recovering it as a copy.
    const oldDraft = (documentId) => {
      placed('v2-document.json', documentId)
      const bytes = readFileSync(join(fixtures, 'v2-draft.json'), 'utf8').replaceAll(fixtureId, documentId)
      const path = join(dataDir, 'drafts', `${documentId}.json`)
      mkdirSync(join(dataDir, 'drafts'), { recursive: true })
      writeFileSync(path, bytes)
      return { path, bytes, kept: join(dataDir, 'migrated', `${documentId}.draft.v2.json`) }
    }
    const httpDraft = oldDraft('doc-00000000000000c1')
    const cliDraft = oldDraft('doc-00000000000000c2')
    const httpCopy = await write('/api/v1/documents/doc-00000000000000c1/recover-as-copy', { request_id: 'old-draft-copy' })
    const cliCopy = cli('documents', 'recover-as-copy', 'doc-00000000000000c2', '--json')
    assert.equal(httpCopy.status, 201)
    assert.equal(cliCopy.status, 0)
    assert.deepEqual(httpCopy.body.document.atoms, cliCopy.body.document.atoms)
    assert.deepEqual(httpCopy.body.document.scene, cliCopy.body.document.scene)
    for (const draft of [httpDraft, cliDraft]) {
      assert.equal(existsSync(draft.path), false)
      assert.equal(readFileSync(draft.kept, 'utf8'), draft.bytes)
    }

    // This independently generated v4 fixture uses the merged P11 tree and reference format.
    const v4Id = 'doc-00000000000000d4'
    const v4 = placed('v4-scene-document.json', v4Id)
    const openedV4 = await call(`/api/v1/documents/${v4Id}`)
    assert.equal(openedV4.status, 200)
    assert.deepEqual([openedV4.body.document.revision, openedV4.body.session.undo_depth], [2, 2])
    assert.deepEqual(openedV4.body.document.selection, { objects: ['s4'], atoms: ['a1'], bonds: [] })
    assert.deepEqual(openedV4.body.document.scene.references, [{ source: 's4', target: { kind: 'atom', id: 'a1' } }])
    assert.equal(openedV4.body.document.settings.label_font, 'sans-serif')
    assert.deepEqual(cli('documents', 'show', v4Id, '--json').body, openedV4.body)
    assert.equal(readFileSync(v4.path, 'utf8'), v4.bytes, 'reading the older document writes nothing')
    const styleV4 = await write(`/api/v1/documents/${v4Id}/transactions`, {
      request_id: 'v4-style', expected_revision: 2, commands: [{ op: 'update_settings', label_font: 'serif' }],
    })
    assert.equal(styleV4.status, 200)
    const savedV4 = await write(`/api/v1/documents/${v4Id}/save`, { request_id: 'v4-save', expected_revision: 3 })
    assert.equal(savedV4.status, 200)
    assert.equal(readFileSync(join(dataDir, 'migrated', `${v4Id}.v4.json`), 'utf8'), v4.bytes)
    assert.equal(JSON.parse(readFileSync(v4.path, 'utf8')).format_version, 5)
    const undoneV4 = cli('documents', 'undo', v4Id, '--expected-revision', '3', '--json')
    assert.equal(undoneV4.body.document.settings.label_font, 'sans-serif')
    assert.deepEqual(undoneV4.body.document.scene.references, openedV4.body.document.scene.references)

    const newer = placed('v4-document.json', 'doc-00000000000000a3')
    newer.bytes = newer.bytes.replace('"format_version": 4', '"format_version": 6')
    writeFileSync(newer.path, newer.bytes)
    const refused = await call('/api/v1/documents/doc-00000000000000a3')
    assert.deepEqual([refused.status, refused.body.code], [500, 'document_unreadable'])
    assert.match(refused.body.message, /version 6/)
    const unreadable = (await call('/api/v1/documents')).body.unreadable.find((item) => item.id === 'doc-00000000000000a3')
    assert.match(unreadable?.message ?? '', /version 6; this build reads versions 1, 2, 3, 4, and 5/)
    assert.equal(cli('documents', 'show', 'doc-00000000000000a3', '--json').body.code, 'document_unreadable')
    assert.equal(readFileSync(newer.path, 'utf8'), newer.bytes, 'a newer file is left untouched')
    assert.equal(existsSync(kept('doc-00000000000000a3')), false)
  }

  // The same scene transactions, selections and undo result through HTTP and the CLI.
  {
    const cliCreated = cli('documents', 'create', '--title', 'CLI scene', '--json')
    const httpCreated = await write('/api/v1/documents', { request_id: 'scene-create', title: 'HTTP scene' })
    assert.equal(cliCreated.status, 0)
    assert.equal(httpCreated.status, 201)
    const cliId = cliCreated.body.document.id
    const scenePath = `/api/v1/documents/${httpCreated.body.document.id}`
    const graphics = [
      { op: 'add_graphic', shape: 'line', points: [{ x: 0, y: 0 }, { x: 10, y: 0 }] },
      { op: 'add_graphic', shape: 'arrow', points: [{ x: 20, y: 0 }, { x: 30, y: 0 }] },
    ]
    const createdSelection = { objects: ['$0', '$1'], atoms: [], bonds: [] }
    const cliDrawn = cli('documents', 'apply', cliId, '--expected-revision', '0', '--json',
      '--commands', JSON.stringify(graphics), '--selection-after', JSON.stringify(createdSelection))
    const httpDrawn = await write(`${scenePath}/transactions`, {
      request_id: 'scene-draw', expected_revision: 0, commands: graphics, selection_after: createdSelection,
    })
    assert.equal(cliDrawn.status, 0)
    assert.equal(httpDrawn.status, 200)
    assert.deepEqual(cliDrawn.body.document.scene, httpDrawn.body.document.scene)
    assert.deepEqual(cliDrawn.body.document.selection, httpDrawn.body.document.selection)

    const group = [{ op: 'group', objects: ['s2', 's3'] }]
    const before = { objects: ['s2', 's3'], atoms: [], bonds: [] }
    const after = { objects: ['$0'], atoms: [], bonds: [] }
    const cliGrouped = cli('documents', 'apply', cliId, '--expected-revision', '1', '--json',
      '--commands', JSON.stringify(group), '--selection-before', JSON.stringify(before), '--selection-after', JSON.stringify(after))
    const httpGrouped = await write(`${scenePath}/transactions`, {
      request_id: 'scene-group', expected_revision: 1, commands: group, selection_before: before, selection_after: after,
    })
    assert.equal(cliGrouped.status, 0)
    assert.equal(httpGrouped.status, 200)
    assert.deepEqual(cliGrouped.body.document.scene, httpGrouped.body.document.scene)
    assert.deepEqual(cliGrouped.body.document.selection, httpGrouped.body.document.selection)

    const reference = [{ op: 'add_reference', source: 's2', target: { kind: 'object', id: 's3' } }]
    const cliLinked = cli('documents', 'apply', cliId, '--expected-revision', '2', '--json', '--commands', JSON.stringify(reference))
    const httpLinked = await write(`${scenePath}/transactions`, { request_id: 'scene-link', expected_revision: 2, commands: reference })
    assert.equal(cliLinked.status, 0)
    assert.equal(httpLinked.status, 200)
    assert.deepEqual(cliLinked.body.document.scene.references, httpLinked.body.document.scene.references)
    const deletion = [{ op: 'delete_objects', objects: ['s3'], mode: 'subtree' }]
    const cliRefused = cli('documents', 'apply', cliId, '--expected-revision', '3', '--json', '--commands', JSON.stringify(deletion))
    const httpRefused = await write(`${scenePath}/transactions`, { request_id: 'scene-delete', expected_revision: 3, commands: deletion })
    assert.deepEqual([cliRefused.status, cliRefused.body.code, cliRefused.body.reason], [1, 'invalid_command', 'reference_in_use'])
    assert.deepEqual([httpRefused.status, httpRefused.body.code, httpRefused.body.reason], [422, 'invalid_command', 'reference_in_use'])

    assert.equal(cli('documents', 'undo', cliId, '--expected-revision', '3', '--json').status, 0)
    assert.equal((await write(`${scenePath}/undo`, { request_id: 'scene-undo-link', expected_revision: 3 })).status, 200)
    const cliUndo = cli('documents', 'undo', cliId, '--expected-revision', '4', '--json')
    const httpUndo = await write(`${scenePath}/undo`, { request_id: 'scene-undo', expected_revision: 4 })
    assert.equal(cliUndo.status, 0)
    assert.equal(httpUndo.status, 200)
    assert.deepEqual(cliUndo.body.document.scene, httpUndo.body.document.scene)
    assert.deepEqual(cliUndo.body.document.selection, httpUndo.body.document.selection)
  }

  for (const [path, method, status, code] of [
    ['/api/v1/capabilities', 'POST', 405, 'method_not_allowed'],
    ['/api/v1/unknown', 'GET', 404, 'not_found'],
    ['/api/v2/health', 'GET', 404, 'not_found'],
  ]) {
    const response = await fetch(`${api.baseUrl}${path}`, { method, signal: AbortSignal.timeout(5_000) })
    assert.equal(response.status, status)
    const body = await response.json()
    assert.deepEqual([body.code, body.api_version], [code, 'v1'])
  }

  console.log(
    'Contract smoke passed: HTTP = CLI; document create/edit/undo/redo/save/reopen, write guards, ' +
      'revision and save conflicts, no revision reuse after discard, save failure, restart recovery, ' +
      'cross-process save lock (race and crash), recovery drafts (recover, discard, outdated copy, ' +
      'unreadable, autosave failure, copy at the session limit), save as copy (including a source draft that cannot be removed), ' +
      'storage feedback (lock waits, queued requests, lost replies, read-only storage), the document list (search, ' +
      'sort, rename, trash, restore, stale sessions, an interrupted delete), and drafts the service writes without a ' +
      'client (crash, stop, maximum wait, retry), operation contract 1 (HTTP = CLI, guards, protocol errors), and ' +
      'version 1 and 2 files (opened over HTTP and the CLI, migrated with history, the first save keeping the exact bytes, a newer version refused), ' +
      'old draft recovery preserving exact bytes through HTTP and the CLI, ' +
      'and refused combinations of recorded states (the same reason over HTTP and the CLI) verified.',
  )
} finally {
  for (const child of running) await stop(child)
  rmSync(dataDir, { recursive: true, force: true })
}
