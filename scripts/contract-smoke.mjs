import assert from 'node:assert/strict'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { once } from 'node:events'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
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
  return { child, baseUrl }
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

/** Starts a CLI process without waiting; `done` resolves with its result and duration. */
function cliProcess(args, extraEnv = {}) {
  const started = Date.now()
  const child = spawn(cliPath, args, { env: { ...env, ...extraEnv }, stdio: ['ignore', 'pipe', 'pipe'] })
  running.add(child)
  let stdout = ''
  child.stdout.on('data', (data) => (stdout += data))
  const done = once(child, 'exit').then(([status]) => {
    running.delete(child)
    return { status, body: stdout ? JSON.parse(stdout) : null, ms: Date.now() - started }
  })
  return { child, done }
}

const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms))
const DELAY = 'CHEMIFREE_TEST_COMMIT_DELAY_MS'

let api
try {
  api = await startApi()

  async function call(path, { method = 'GET', body, headers = {} } = {}) {
    const response = await fetch(`${api.baseUrl}${path}`, {
      method,
      headers: body === undefined ? headers : { 'content-type': 'application/json', ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(5_000),
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
    ['runtime_inspection', 'document_editing', 'persistence'],
  )
  assert.equal(new Set(capabilities.map((item) => item.id)).size, 8)

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
  const write = (path, body) => call(path, { method: 'POST', body, headers: auth })
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
  assert.deepEqual([failed.status, failed.body.code], [503, 'storage_failed'])
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
  assert.equal(winners.length, 1, `exactly one concurrent save commits: ${JSON.stringify(results.map((r) => r.body.code ?? 'ok'))}`)
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

  // A failed autosave keeps the session's changes and the saved file; a retry succeeds.
  await write(`${draftPath}/transactions`, {
    request_id: 'draft-edit-6',
    expected_revision: 6,
    commands: [{ op: 'rename', title: 'Autosave retry' }],
  })
  const writable = process.getuid?.() !== 0 // Permissions do not stop root.
  if (writable) {
    const draftsDir = join(dataDir, 'drafts')
    chmodSync(draftsDir, 0o500)
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
      'unreadable, autosave failure, copy at the session limit), and save as copy verified.',
  )
} finally {
  for (const child of running) await stop(child)
  rmSync(dataDir, { recursive: true, force: true })
}
