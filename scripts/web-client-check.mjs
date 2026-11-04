// Checks the web client's handling of requests whose outcome is unknown, against a scripted
// service instead of a browser, so that orderings a browser cannot produce on demand (a
// restart noticed by another request while one is being asked about) run every time. Also
// checks which runtime replies the client accepts.
import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { ConsentRequired, DocumentClient, outcomeOpen, isDocumentResponse, sameInspectionSnapshot, sameConversionSnapshot, conversionSnapshotMatcher, samePropertySnapshot } = await import('../apps/web/src/lib/document-contract.ts')
const { DUPLICATE_OFFSET, deletePlacement, duplicatePlacement, movePlacement, newAtomPlacement, placeNewAtoms, placementAt } =
  await import(
  '../apps/web/src/editor/placement.ts'
)
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')
const { mergeGroupSelection, selectionAfterGroup, selectionSnapshot } = await import('../apps/web/src/editor/scene-selection.ts')
const { readRuntime } = await import('../apps/web/src/lib/contract.ts')
const searchRequests = await import('../apps/web/src/lib/substructure-requests.ts')

const document = (id, revision = 0) => ({
  api_version: 'v1',
  document: { id, title: 'Scripted', revision, atoms: [], bonds: [] },
  session: { saved_revision: revision, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null },
  recovery: null,
})
const copy = (id) => ({ ...document(id), source: { id: 'doc-1', closed: true, discarded_changes: false } })
const json = (status, body) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
const error = (status, code) => json(status, { api_version: 'v1', code, message: code })

test('an inspection is stale when another graph replaces the same ID and revision', () => {
  const atom = { id: 'a1', element: 'C', charge: 0, isotope: null, radical: 'none', hydrogens: { mode: 'inferred' } }
  const carbon = { ...document('branch', 1).document, atoms: [atom] }
  assert.equal(sameInspectionSnapshot(carbon, structuredClone(carbon)), true)
  for (const changed of [
    { ...atom, element: 'O' }, { ...atom, charge: 1 }, { ...atom, isotope: 13 },
    { ...atom, radical: 'doublet' }, { ...atom, hydrogens: { mode: 'specified', count: 3 } },
  ]) {
    assert.equal(sameInspectionSnapshot({ ...carbon, atoms: [changed] }, carbon), false, 'same ID and revision do not establish graph identity')
  }
  assert.equal(sameInspectionSnapshot({ ...carbon, bonds: [{ id: 'b1', order: 'double' }] }, carbon), false)
  assert.equal(sameInspectionSnapshot({ ...carbon, revision: 2 }, carbon), false)
})
const lost = () => {
  throw new TypeError('fetch failed')
}

test('an unconfirmed CDXML creation is checked with the same request ID', async () => {
  const output = document('imported')
  const reply = { outcome: 'partial', diagnostics: [], loss_report: [], result: {
    document: output.document, session: output.session, recovery: null,
  } }
  const scripted = service({
    '/api/v1/operations': [lost, () => json(200, reply)],
    'GET /api/v1/documents/imported': [() => json(200, output)],
  })
  const client = new DocumentClient()
  const input = { source: '<CDXML/>', title: 'Imported', page_id: 1, fragment_id: 2 }
  await assert.rejects(client.createInterchange(input, 'a'.repeat(64)), { code: 'unreachable' })
  assert.equal((await client.createInterchange(input, 'a'.repeat(64))).document.id, 'imported')
  assert.equal(scripted.writes.length, 2)
  assert.equal(scripted.writes[0].requestId, scripted.writes[1].requestId)
})

test('reading a CDX or CDXML source asks for compact bytes', async () => {
  const choice = { page_id: 1, fragment_id: 2, page_path: 'page 1', fragment_path: 'page 1/fragment 2' }
  const reply = { outcome: 'ok', diagnostics: [], loss_report: [], result: { choices: [choice] } }
  const scripted = service({ '/api/v1/operations': [() => json(200, reply), () => json(200, reply)] })
  const client = new DocumentClient()
  assert.deepEqual(await client.readInterchange('VjCD0100AA==', 'cdx'), [choice])
  assert.deepEqual(await client.readInterchange('<CDXML/>', 'cdxml'), [choice])
  assert.deepEqual(scripted.writes.map((write) => write.parameters), [
    { mode: 'read', format: 'cdx', source: 'VjCD0100AA==', compact_bytes: true },
    { mode: 'read', format: 'cdxml', source: '<CDXML/>', compact_bytes: true },
  ])
})

/** What Vite's development proxy answers when the API connection drops before a reply. */
const proxyFailure = () => new Response('', { status: 502, headers: { 'content-type': 'text/plain' } })

/**
 * A scripted service. `answers` maps a request's path suffix (writes) or `GET <suffix>` (reads)
 * to the replies for its successive requests (a function is called with the request;
 * `lost` drops the reply). After `restart()`, the service issues a new token and refuses the
 * old one, as a new process would. `failSessions(n)` drops the next n session requests.
 */
function service(answers) {
  let token = 'token-1'
  let sessionFailures = 0
  const writes = []
  const reads = []
  const asked = new Map()
  const answer = (suffix) => {
    const count = asked.get(suffix) ?? 0
    asked.set(suffix, count + 1)
    const reply = answers[suffix]?.[count]
    assert.ok(reply, `no scripted answer for request ${count + 1} to ${suffix}`)
    return reply()
  }
  globalThis.fetch = async (path, init = {}) => {
    if (path === '/api/v1/session') {
      if (sessionFailures > 0) {
        sessionFailures -= 1
        lost()
      }
      return json(200, { header: 'x-chemifree-session', token })
    }
    if ((init.method ?? 'GET') === 'GET') {
      reads.push(path)
      const suffix = Object.keys(answers).find((key) => key.startsWith('GET ') && path.endsWith(key.slice(4)))
      assert.ok(suffix, `no scripted answer for GET ${path}`)
      return answer(suffix)
    }
    const sentWith = init.headers['x-chemifree-session']
    const body = JSON.parse(init.body)
    writes.push({ path, token: sentWith, requestId: body.request_id, parameters: body.parameters })
    if (sentWith !== token) return error(401, 'unauthorized')
    const suffix = Object.keys(answers).find((key) => !key.startsWith('GET ') && path.endsWith(key))
    assert.ok(suffix, `no scripted answer for ${path}`)
    return answer(suffix)
  }
  return {
    writes,
    reads,
    restart() {
      token = `token-${Number(token.slice(6)) + 1}`
    },
    failSessions(count) {
      sessionFailures = count
    },
  }
}

/** Another request of the page meets the restarted service, as an autosave or edit would. */
async function noticeRestart(client) {
  await assert.rejects(client.transact('doc-9', 0, []), { code: 'service_restarted' })
}

test('a create asked about again after another request saw a restart is not resent', async () => {
  let answered
  const firstAnswered = new Promise((resolve) => (answered = resolve))
  const scripted = service({
    '/api/v1/documents': [
      () => {
        answered()
        return error(409, 'request_in_progress')
      },
    ],
  })
  const client = new DocumentClient()
  const creating = client.create('Lost')
  await firstAnswered
  scripted.restart()
  await noticeRestart(client)
  await assert.rejects(creating, { code: 'create_unconfirmed' })
  const creates = scripted.writes.filter((write) => write.path === '/api/v1/documents')
  assert.deepEqual(creates.map((write) => write.token), ['token-1'], 'not resent to the restarted service')
})

test('a copy retried after another request saw a restart is not resent', async () => {
  const scripted = service({ '/save-as': [lost] })
  const client = new DocumentClient()
  await assert.rejects(client.saveAs('doc-1', 3, 'Branch'), { code: 'unreachable' })
  scripted.restart()
  await noticeRestart(client)
  await assert.rejects(client.saveAs('doc-1', 3, 'Branch'), { code: 'copy_unconfirmed' })
  const copies = scripted.writes.filter((write) => write.path.endsWith('/save-as'))
  assert.deepEqual(copies.map((write) => write.token), ['token-1'], 'not resent to the restarted service')
})

test('an unconfirmed save checked after another request saw a restart is not resent', async () => {
  const scripted = service({ '/save': [lost, lost] })
  const client = new DocumentClient()
  await assert.rejects(client.save('doc-1', 3), { code: 'save_unconfirmed' })
  scripted.restart()
  await noticeRestart(client)
  await assert.rejects(client.save('doc-1', 3), { code: 'service_restarted' })
  const saves = scripted.writes.filter((write) => write.path.endsWith('/save'))
  assert.deepEqual(saves.map((write) => write.token), ['token-1', 'token-1'], 'not resent to the restarted service')
})

test('a create in progress is asked about with the same request ID until it is answered', async () => {
  const scripted = service({
    '/api/v1/documents': [() => error(409, 'request_in_progress'), () => json(201, document('doc-2'))],
    // The answer to a repeated request may be the record of the first, so it is read again.
    'GET /api/v1/documents/doc-2': [() => json(200, document('doc-2'))],
  })
  const created = await new DocumentClient().create('Kept')
  assert.equal(created.document.id, 'doc-2')
  const ids = scripted.writes.map((write) => write.requestId)
  assert.equal(ids.length, 2)
  assert.equal(ids[1], ids[0], 'one request ID')
  assert.deepEqual(scripted.reads, ['/api/v1/documents/doc-2'])
})

test('a first attempt refused by an already restarted service is sent again', async () => {
  const scripted = service({
    '/transactions': [() => json(200, document('doc-9'))],
    '/save-as': [() => json(201, copy('doc-2'))],
  })
  const client = new DocumentClient()
  await client.transact('doc-9', 0, []) // The page holds the first service's token.
  scripted.restart()
  const copied = await client.saveAs('doc-1', 3, 'Branch')
  assert.equal(copied.document.id, 'doc-2')
  const copies = scripted.writes.filter((write) => write.path.endsWith('/save-as'))
  assert.deepEqual(copies.map((write) => write.token), ['token-1', 'token-2'], 'refused, then sent to the new service')
})

test('a save whose reply was lost is asked about again with the same request ID', async () => {
  const scripted = service({ '/save': [lost, () => json(200, document('doc-1', 3))] })
  const saved = await new DocumentClient().save('doc-1', 3)
  assert.equal(saved.session.saved_revision, 3)
  const ids = scripted.writes.map((write) => write.requestId)
  assert.equal(ids.length, 2)
  assert.equal(ids[1], ids[0], 'one request ID')
})

test("a create whose reply the proxy lost is not repeated after a restart", async () => {
  const scripted = service({ '/api/v1/documents': [proxyFailure] })
  const client = new DocumentClient()
  await assert.rejects(client.create(), { code: 'unreachable' })
  scripted.restart()
  await noticeRestart(client)
  await assert.rejects(client.create(), { code: 'create_unconfirmed' })
  const creates = scripted.writes.filter((write) => write.path === '/api/v1/documents')
  assert.deepEqual(creates.map((write) => write.token), ['token-1'], 'not sent again as a new create')
})

test('a save whose reply the proxy lost is asked about again with the same request ID', async () => {
  const scripted = service({ '/save': [proxyFailure, () => json(200, document('doc-1', 3))] })
  const saved = await new DocumentClient().save('doc-1', 3)
  assert.equal(saved.session.saved_revision, 3)
  const ids = scripted.writes.map((write) => write.requestId)
  assert.equal(ids.length, 2)
  assert.equal(ids[1], ids[0], 'one request ID')
})

test('a failed session request does not forget where an unknown create was sent', async () => {
  const scripted = service({ '/api/v1/documents': [lost] })
  const client = new DocumentClient()
  await assert.rejects(client.create(), { code: 'unreachable' })
  scripted.restart()
  await noticeRestart(client)
  scripted.failSessions(1)
  await assert.rejects(client.create(), { code: 'unreachable' })
  await assert.rejects(client.create(), { code: 'create_unconfirmed' })
  const creates = scripted.writes.filter((write) => write.path === '/api/v1/documents')
  assert.deepEqual(creates.map((write) => write.token), ['token-1'], 'not sent to the restarted service')
})

test('a create answered by replay shows the document as it is now', async () => {
  const now = { ...document('doc-2', 7), document: { ...document('doc-2', 7).document, title: 'Edited since' } }
  const scripted = service({
    '/api/v1/documents': [lost, () => json(201, document('doc-2', 0))],
    'GET /api/v1/documents/doc-2': [() => json(200, now)],
  })
  const client = new DocumentClient()
  await assert.rejects(client.create(), { code: 'unreachable' })
  const shown = await client.create()
  assert.deepEqual([shown.document.revision, shown.document.title], [7, 'Edited since'])
  assert.deepEqual(scripted.reads, ['/api/v1/documents/doc-2'])
})

test('a copy answered by replay shows the copy as it is now, with what became of the source', async () => {
  const now = { ...document('doc-2', 4) }
  const scripted = service({
    '/save-as': [lost, () => json(201, copy('doc-2'))],
    'GET /api/v1/documents/doc-2': [() => json(200, now)],
  })
  const client = new DocumentClient()
  await assert.rejects(client.saveAs('doc-1', 3), { code: 'unreachable' })
  const shown = await client.saveAs('doc-1', 3)
  assert.equal(shown.document.revision, 4)
  assert.deepEqual(shown.source, { id: 'doc-1', closed: true, discarded_changes: false })
  assert.equal(scripted.reads.length, 1)
})

test('a copy with an empty title and one without a title are different requests', async () => {
  const scripted = service({ '/save-as': [lost, () => json(201, copy('doc-2'))] })
  const client = new DocumentClient()
  await assert.rejects(client.saveAs('doc-1', 3, ''), { code: 'unreachable' })
  await client.saveAs('doc-1', 3)
  const ids = scripted.writes.map((write) => write.requestId)
  assert.notEqual(ids[1], ids[0], 'a different body gets its own request ID')
})

test('a replayed create whose re-read fails keeps its request ID', async () => {
  const scripted = service({
    '/api/v1/documents': [lost, () => json(201, document('doc-2', 0)), () => json(201, document('doc-2', 0))],
    'GET /api/v1/documents/doc-2': [() => error(503, 'resource_limit'), () => json(200, document('doc-2', 5))],
  })
  const client = new DocumentClient()
  await assert.rejects(client.create(), { code: 'unreachable' })
  await assert.rejects(client.create(), { code: 'resource_limit' })
  const shown = await client.create()
  assert.equal(shown.document.revision, 5)
  const ids = scripted.writes.map((write) => write.requestId)
  assert.ok(ids.every((id) => id === ids[0]), `the created document is asked about, not created again: ${ids.join(', ')}`)
})

test('a rename whose reply was lost is asked about again with the same request ID', async () => {
  const scripted = service({ '/rename': [lost, () => json(200, document('doc-1', 4))] })
  const renamed = await new DocumentClient().rename('doc-1', 3, 'Renamed')
  assert.equal(renamed.document.revision, 4)
  const ids = scripted.writes.map((write) => write.requestId)
  assert.equal(ids.length, 2)
  assert.equal(ids[1], ids[0], 'one request ID')
})

test('a delete still running is asked about until it is answered', async () => {
  const scripted = service({
    '/delete': [proxyFailure, () => error(409, 'request_in_progress'), () => json(200, { api_version: 'v1', id: 'doc-1', title: 'Gone' })],
  })
  const deleted = await new DocumentClient().delete('doc-1', 3)
  assert.equal(deleted.title, 'Gone')
  const ids = scripted.writes.map((write) => write.requestId)
  assert.equal(ids.length, 3)
  assert.ok(ids.every((id) => id === ids[0]), `one request ID: ${ids.join(', ')}`)
})

test('a restore the service refused is reported, not asked again', async () => {
  const scripted = service({ '/restore': [() => error(404, 'document_not_found')] })
  await assert.rejects(new DocumentClient().restore('doc-1'), { code: 'document_not_found' })
  assert.equal(scripted.writes.length, 1)
})

test('a restore whose reply was lost is not asked of a restarted service', async () => {
  // The service restarts while the reply to the first attempt is lost.
  const scripted = service({
    '/restore': [
      () => {
        scripted.restart()
        lost()
      },
    ],
  })
  await assert.rejects(new DocumentClient().restore('doc-1'), { code: 'service_restarted' })
  const restores = scripted.writes.filter((write) => write.path.endsWith('/restore'))
  assert.deepEqual(restores.map((write) => write.token), ['token-1', 'token-1'], 'not sent to the restarted service')
})

test("a session's draft failure is read when present and optional otherwise", async () => {
  const failing = document('doc-1', 2)
  failing.session = { ...failing.session, draft_failure: { attempts: 2, retry_in_ms: 4000, message: 'disk full' } }
  service({ 'GET /api/v1/documents/doc-1': [() => json(200, failing)], 'GET /api/v1/documents/doc-2': [() => json(200, document('doc-2'))] })
  const client = new DocumentClient()
  assert.deepEqual((await client.get('doc-1')).session.draft_failure, { attempts: 2, retry_in_ms: 4000, message: 'disk full' })
  assert.equal((await client.get('doc-2')).session.draft_failure, undefined, 'a service from before the field is still compatible')
})

test("a draft failure's error code and storage reason are read, and an unknown reason is refused", async () => {
  const failure = { attempts: 1, retry_in_ms: 2000, code: 'storage_failed', reason: 'read_only', message: 'denied' }
  const failing = document('doc-1', 2)
  failing.session = { ...failing.session, draft_failure: failure }
  const garbled = document('doc-2', 2)
  garbled.session = { ...garbled.session, draft_failure: { ...failure, reason: 'full' } }
  service({ 'GET /api/v1/documents/doc-1': [() => json(200, failing)], 'GET /api/v1/documents/doc-2': [() => json(200, garbled)] })
  const client = new DocumentClient()
  assert.deepEqual((await client.get('doc-1')).session.draft_failure, failure)
  await assert.rejects(client.get('doc-2'), { code: 'incompatible_response' })
})

test('a re-read of a document reports a service that restarted before the read instead of its stored copy', async () => {
  const scripted = service({
    '/autosave': [() => json(200, document('doc-1'))],
    'GET /api/v1/documents/doc-1': [() => json(200, document('doc-1'))],
  })
  const client = new DocumentClient()
  let restarts = 0
  client.onRestart(() => (restarts += 1))
  await client.autosave('doc-1') // the page's writes went to the first process
  scripted.restart()
  await assert.rejects(client.reread('doc-1'), { code: 'service_restarted' })
  assert.equal(restarts, 1, 'listeners hear of the restart')
})

test('a re-read of a document reports a service that restarted while the read was answered', async () => {
  const scripted = service({
    '/autosave': [() => json(200, document('doc-1'))],
    'GET /api/v1/documents/doc-1': [
      () => {
        scripted.restart()
        return json(200, document('doc-1'))
      },
    ],
  })
  const client = new DocumentClient()
  await client.autosave('doc-1')
  await assert.rejects(client.reread('doc-1'), { code: 'service_restarted' })
})

test('a re-read of a document from the same service returns it', async () => {
  service({
    '/autosave': [() => json(200, document('doc-1'))],
    'GET /api/v1/documents/doc-1': [() => json(200, document('doc-1', 3))],
  })
  const client = new DocumentClient()
  await client.autosave('doc-1')
  assert.equal((await client.reread('doc-1')).document.revision, 3)
})

test('an edit after a re-read found a restart meets that restart too, instead of the new service', async () => {
  const scripted = service({
    '/autosave': [() => json(200, document('doc-1'))],
    '/transactions': [() => json(200, document('doc-1', 1))],
    'GET /api/v1/documents/doc-1': [() => json(200, document('doc-1'))],
  })
  const client = new DocumentClient()
  await client.autosave('doc-1')
  scripted.restart()
  await assert.rejects(client.reread('doc-1'), { code: 'service_restarted' })
  await assert.rejects(client.transact('doc-1', 0, []), { code: 'service_restarted' })
  const edits = scripted.writes.filter((write) => write.path.endsWith('/transactions'))
  assert.deepEqual(edits.map((write) => write.token), ['token-1'], 'sent once, with the old token, and refused')
})

test('a re-read reports a restart since the document was opened, before the page ever wrote', async () => {
  const scripted = service({
    'GET /api/v1/documents/doc-1': [() => json(200, document('doc-1')), () => json(200, document('doc-1'))],
  })
  const client = new DocumentClient()
  await client.get('doc-1')
  scripted.restart()
  await assert.rejects(client.reread('doc-1'), { code: 'service_restarted' })
})

test('reading the document again after a re-read found a restart ties the page to the new service', async () => {
  const scripted = service({
    '/autosave': [() => json(200, document('doc-1')), () => json(200, document('doc-1'))],
    'GET /api/v1/documents/doc-1': [() => json(200, document('doc-1')), () => json(200, document('doc-1'))],
  })
  const client = new DocumentClient()
  await client.autosave('doc-1')
  scripted.restart()
  await assert.rejects(client.reread('doc-1'), { code: 'service_restarted' })
  await client.get('doc-1') // the page reloads the document from the new service
  await client.autosave('doc-1')
  const drafts = scripted.writes.filter((write) => write.path.endsWith('/autosave'))
  assert.deepEqual(drafts.map((write) => write.token), ['token-1', 'token-2'])
})

const health = { api_version: 'v1', service: 'chemifree', version: '0.1.0', stage: 'scaffold', status: 'ok' }
const capability = { id: 'document_editing', label: 'Editing', status: 'available', description: 'Drawings.' }
const catalog = () => ({
  implementation: 'chemifree',
  version: '0.1.0',
  contracts: [1],
  profiles: [{ id: 'preserve-semantics', execution: 'executable', note: 'Keeps meaning.' }],
  operations: [
    {
      id: 'document.transaction',
      representation: 'supported',
      execution: 'executable',
      note: 'Edits.',
      sub_operations: [
        { id: 'add_instance', representation: 'supported', execution: 'executable', note: 'A command.' },
        {
          id: 'scene_placement',
          representation: 'supported',
          execution: 'executable',
          note: 'A capability.',
          commands: ['add_instance'],
        },
        {
          id: 'radical',
          representation: 'not_supported',
          execution: 'not_implemented',
          followup: 'P10',
          note: 'No.',
          commands: [],
        },
      ],
    },
    { id: 'chem.properties', representation: 'not_supported', execution: 'not_implemented', followup: 'P19', note: 'No.' },
  ],
})
/** Answers the runtime reads with `capabilities` as the capabilities reply. */
function runtime(capabilities) {
  globalThis.fetch = async (path) => json(200, path === '/api/v1/health' ? health : capabilities)
  return readRuntime(new AbortController().signal)
}

test('external data status is shown separately from the engine and a failure does not disconnect runtime', async () => {
  const status = { api_version: 'v1', state: 'checked', reason: null, registrations: [{ registration_id: 'independent-fixture', state: 'unavailable', reason: 'data_unavailable' }], engine: 'not_implemented' }
  globalThis.fetch = async (path) => json(200, path === '/api/v1/health' ? health : path === '/api/v1/data' ? status : { api_version: 'v1', capabilities: [capability] })
  const snapshot = await readRuntime(new AbortController().signal)
  assert.deepEqual(snapshot.dataStatus, status)
  globalThis.fetch = async (path) => path === '/api/v1/data' ? json(503, {}) : json(200, path === '/api/v1/health' ? health : { api_version: 'v1', capabilities: [capability] })
  const disconnectedData = await readRuntime(new AbortController().signal)
  assert.equal(disconnectedData.dataStatus, null)
  assert.deepEqual(disconnectedData.capabilities, [capability])
})

test('extra data column counts are validated before they are displayed', async () => {
  const entry = { registration_id: 'independent-fixture', state: 'loaded', reason: null, retained_extra_columns: 2 }
  const status = { api_version: 'v1', state: 'checked', reason: null, registrations: [entry], engine: 'not_implemented' }
  const read = async (response) => {
    globalThis.fetch = async (path) => json(200, path === '/api/v1/health' ? health : path === '/api/v1/data' ? response : { api_version: 'v1', capabilities: [capability] })
    return readRuntime(new AbortController().signal)
  }
  assert.deepEqual((await read(status)).dataStatus, status)
  const oldEntry = { registration_id: entry.registration_id, state: entry.state, reason: entry.reason }
  const oldStatus = { ...status, registrations: [oldEntry] }
  assert.deepEqual((await read(oldStatus)).dataStatus, oldStatus)
  const zeroStatus = { ...status, registrations: [{ ...entry, retained_extra_columns: 0 }] }
  assert.deepEqual((await read(zeroStatus)).dataStatus, zeroStatus)
  for (const count of [-1, 1.5, '2', null, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal((await read({ ...status, registrations: [{ ...entry, retained_extra_columns: count }] })).dataStatus, null)
  }
  assert.equal((await read({ ...status, registrations: [{ ...entry, state: 'unavailable', reason: 'data_unavailable' }] })).dataStatus, null)
})

test('contradictory or duplicated data status is not presented as a valid registry', async () => {
  const entry = { registration_id: 'independent-fixture', state: 'loaded', reason: null }
  for (const value of [
    { api_version: 'v1', state: 'not_configured', reason: null, registrations: [entry], engine: 'not_implemented' },
    { api_version: 'v1', state: 'checked', reason: null, registrations: [entry, entry], engine: 'not_implemented' },
    { api_version: 'v1', state: 'checked', reason: null, registrations: [{ ...entry, state: 'unavailable' }], engine: 'not_implemented' },
  ]) {
    globalThis.fetch = async (path) => json(200, path === '/api/v1/health' ? health : path === '/api/v1/data' ? value : { api_version: 'v1', capabilities: [capability] })
    const snapshot = await readRuntime(new AbortController().signal)
    assert.equal(snapshot.dataStatus, null)
    assert.deepEqual(snapshot.capabilities, [capability])
  }
})

test('a capabilities reply without an operation catalog, from before it was added, is accepted', async () => {
  const snapshot = await runtime({ api_version: 'v1', capabilities: [capability] })
  assert.equal(snapshot.operationCatalog, null)
  assert.deepEqual(snapshot.capabilities, [capability])
})

test('an operation catalog is read with its sub-operations, follow-ups, and capability commands', async () => {
  const snapshot = await runtime({ api_version: 'v1', capabilities: [capability], operation_catalog: catalog() })
  assert.deepEqual(snapshot.operationCatalog, catalog())
})

test('an operation catalog from before capability rows named their commands is accepted', async () => {
  const older = catalog()
  for (const part of older.operations[0].sub_operations) delete part.commands
  const snapshot = await runtime({ api_version: 'v1', capabilities: [capability], operation_catalog: older })
  assert.deepEqual(snapshot.operationCatalog, older)
})

for (const [name, change] of [
  ['an unknown execution state', (value) => (value.operations[1].execution = 'planned')],
  ['an unknown representation', (value) => (value.operations[0].sub_operations[0].representation = 'partial')],
  ['a duplicate operation', (value) => value.operations.push(value.operations[1])],
  ['a non-string follow-up', (value) => (value.operations[1].followup = 19)],
  ['capability commands that are not a list', (value) => (value.operations[0].sub_operations[1].commands = 'add_instance')],
  ['a capability command that is not a name', (value) => (value.operations[0].sub_operations[1].commands = [7])],
  ['no contract version', (value) => (value.contracts = [])],
  ['a profile without its execution', (value) => delete value.profiles[0].execution],
]) {
  test(`an operation catalog with ${name} is refused`, async () => {
    const value = catalog()
    change(value)
    await assert.rejects(
      runtime({ api_version: 'v1', capabilities: [capability], operation_catalog: value }),
      /incompatible API v1 response/,
    )
  })
}

/** C1–O2 drawn in s1 and again in s2, 100 pt lower and 50 pt right. */
function drawnTwice() {
  const at = (atom, x, y) => ({ atom, x, y })
  return {
    api_version: 'v1',
    document: {
      id: 'doc-1',
      title: 'Placed',
      revision: 2,
      atoms: [
        { id: 'a1', element: 'C', charge: 0, isotope: null, position: { x: 0, y: 0 } },
        { id: 'a2', element: 'O', charge: 0, isotope: null, position: { x: 30, y: 0 } },
      ],
      bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single' }],
      molecule: { id: 'm1' },
      scene: {
        pages: [{ id: 'p1' }],
        instances: [
          { id: 's1', page: 'p1', molecule: 'm1', positions: [at('a1', 0, 0), at('a2', 30, 0)] },
          { id: 's2', page: 'p1', molecule: 'm1', positions: [at('a1', 50, 100), at('a2', 80, 100)] },
        ],
      },
    },
    session: { saved_revision: 0, unsaved_changes: true, undo_depth: 2, redo_depth: 0, draft_revision: null },
    recovery: null,
  }
}

test('a document reply with its graph and scene is accepted, and one from before them too', () => {
  assert.equal(isDocumentResponse(drawnTwice()), true)
  const older = drawnTwice()
  delete older.document.molecule
  delete older.document.scene
  assert.equal(isDocumentResponse(older), true, 'a service from before scene instances')
})

function groupedScene() {
  const value = drawnTwice()
  value.document.scene.roots = ['s1', 's4']
  value.document.scene.references = []
  value.document.scene.objects = [
    { kind: 'instance', id: 's1', parent: null, molecule: 'm1' },
    { kind: 'group', id: 's4', parent: null, children: ['s2', 's3'] },
    { kind: 'instance', id: 's2', parent: 's4', molecule: 'm1' },
    { kind: 'graphic', id: 's3', parent: 's4', shape: 'line', points: [{ x: 0, y: 0 }, { x: 4, y: 4 }] },
  ]
  value.document.selection = { objects: ['s4'], atoms: ['a1'], bonds: [] }
  return value
}

test('a scene reply validates containment and selected identifiers', () => {
  assert.equal(isDocumentResponse(groupedScene()), true)
  const invalid = groupedScene()
  invalid.document.scene.objects[1].children.push('s1')
  assert.equal(isDocumentResponse(invalid), false, 'an object may not occur under two parents')
  const stale = groupedScene()
  stale.document.selection.objects.push('s99')
  assert.equal(isDocumentResponse(stale), false, 'selection must not name a missing object')
  const partial = groupedScene()
  delete partial.document.scene.roots
  assert.equal(isDocumentResponse(partial), false, 'the tree is not partially present')
  const dangling = groupedScene()
  dangling.document.scene.references.push({ source: 's4', target: { kind: 'atom', id: 'a99' } })
  assert.equal(isDocumentResponse(dangling), false, 'references cannot target missing entities')
})

test('group selection merges unrelated changes made while the request is pending', () => {
  const before = groupedScene().document
  before.scene.roots = ['s1', 's2', 's3', 's6']
  before.scene.objects = [
    { kind: 'instance', id: 's1', parent: null, molecule: 'm1' },
    { kind: 'instance', id: 's2', parent: null, molecule: 'm1' },
    { kind: 'graphic', id: 's3', parent: null, shape: 'line', points: [{ x: 0, y: 0 }, { x: 4, y: 4 }] },
    { kind: 'graphic', id: 's6', parent: null, shape: 'line', points: [{ x: 5, y: 0 }, { x: 9, y: 4 }] },
  ]
  const pressed = new Set(['s1', 's2', 's3', 'a1'])
  const captured = selectionSnapshot(pressed, before)
  assert.deepEqual(selectionAfterGroup(captured, ['s2', 's3']), { objects: ['s1', '$0'], atoms: ['a1'], bonds: [] })
  pressed.delete('a1')
  pressed.delete('s1')
  pressed.add('s6')
  pressed.add('b1')
  const after = groupedScene().document
  after.scene.roots.push('s6')
  after.scene.objects.push({ kind: 'graphic', id: 's6', parent: null, shape: 'line', points: [{ x: 5, y: 0 }, { x: 9, y: 4 }] })
  after.selection.objects = ['s1', 's4']
  const merged = mergeGroupSelection(pressed, ['s2', 's3'], before, after)
  assert.deepEqual([...merged].sort(), ['b1', 's4', 's6'])
  assert.deepEqual(captured.atoms, ['a1'], 'the snapshot does not change with the live selection')
})

for (const [name, change] of [
  ['an atom without a position in an instance', (value) => value.document.scene.instances[1].positions.pop()],
  ['a position for an atom the graph lacks', (value) => (value.document.scene.instances[1].positions[1].atom = 'a9')],
  ['an atom placed twice in one instance', (value) => (value.document.scene.instances[1].positions[1].atom = 'a1')],
  ['a repeated instance', (value) => (value.document.scene.instances[1].id = 's1')],
  ['an instance on another page', (value) => (value.document.scene.instances[1].page = 'p2')],
  ['an instance of another molecule', (value) => (value.document.scene.instances[0].molecule = 'm2')],
  ['no instance', (value) => (value.document.scene.instances = [])],
  ['two pages', (value) => value.document.scene.pages.push({ id: 'p2' })],
  ['a scene without its molecule', (value) => delete value.document.molecule],
  ['a molecule without its scene', (value) => delete value.document.scene],
  ['a non-numeric coordinate', (value) => (value.document.scene.instances[0].positions[0].x = '0')],
  ['atom positions that are not the first instance\'s', (value) => (value.document.atoms[1].position = { x: 80, y: 100 })],
]) {
  test(`a document reply with ${name} is refused`, () => {
    const value = drawnTwice()
    change(value)
    assert.equal(isDocumentResponse(value), false)
  })
}

test('a new atom in one placement goes to the same place relative to its nearest atom in every placement', () => {
  const { document } = drawnTwice()
  // Clicked in s2 near a2 (80, 100): s1 holds a2 at (30, 0).
  assert.deepEqual(newAtomPlacement(document, 's2', { x: 90, y: 120 }), {
    positions: { s1: { x: 40, y: 20 }, s2: { x: 90, y: 120 } },
  })
  // Clicked in s1 near a1: s2 holds a1 50 right and 100 down.
  assert.deepEqual(newAtomPlacement(document, 's1', { x: -10, y: 5 }), {
    positions: { s1: { x: -10, y: 5 }, s2: { x: 40, y: 105 } },
  })
})

test('a new atom is placed by one position while there is one placement or no scene', () => {
  const one = drawnTwice().document
  one.scene.instances.pop()
  assert.deepEqual(newAtomPlacement(one, 's1', { x: 5, y: 5 }), { position: { x: 5, y: 5 } })
  const older = drawnTwice().document
  delete older.scene
  delete older.molecule
  assert.deepEqual(newAtomPlacement(older, null, { x: 5, y: 5 }), { position: { x: 5, y: 5 } })
})

test('a new atom in placements without atoms gets the clicked point in each', () => {
  const empty = drawnTwice().document
  empty.atoms = []
  empty.bonds = []
  for (const instance of empty.scene.instances) instance.positions = []
  assert.deepEqual(newAtomPlacement(empty, 's2', { x: 7, y: 8 }), {
    positions: { s1: { x: 7, y: 8 }, s2: { x: 7, y: 8 } },
  })
  assert.equal(placementAt(empty, { x: 7, y: 8 }), 's1', 'empty canvas belongs to the first placement')
})

test('a click on empty canvas belongs to the placement of the nearest atom', () => {
  const { document } = drawnTwice()
  assert.equal(placementAt(document, { x: 60, y: 90 }), 's2')
  assert.equal(placementAt(document, { x: 10, y: 20 }), 's1')
})

test('a queued gesture lapses when the placements it names have changed', () => {
  const { document } = drawnTwice()
  const placed = { op: 'add_atom', element: 'N', positions: { s1: { x: 0, y: 9 }, s2: { x: 50, y: 109 } } }
  const moved = { op: 'move_atoms', atoms: ['a1'], delta: { x: 1, y: 1 }, instance: 's2' }
  assert.deepEqual(stillApplicable([placed, moved], document), [placed, moved])
  const withoutS2 = drawnTwice().document
  withoutS2.scene.instances.pop()
  assert.equal(stillApplicable([placed], withoutS2), null, 'positions for an instance that is gone')
  assert.equal(stillApplicable([moved], withoutS2), null, 'a move in an instance that is gone')
  const withS3 = drawnTwice().document
  withS3.scene.instances.push({ ...withS3.scene.instances[1], id: 's3' })
  assert.equal(stillApplicable([placed], withS3), null, 'no position for a new instance')
  assert.equal(stillApplicable([{ op: 'add_atom', element: 'N', position: { x: 0, y: 0 } }], document), null)
})

/** a1 (0, 0) and a2 (100, 0) in s1; s2 drawn 80 pt lower, then a2 dragged 40 pt right in s2 only. */
function stretched() {
  const value = drawnTwice().document
  value.atoms[1].position = { x: 100, y: 0 }
  value.bonds = []
  const at = (atom, x, y) => ({ atom, x, y })
  value.scene.instances[0].positions = [at('a1', 0, 0), at('a2', 100, 0)]
  value.scene.instances[1].positions = [at('a1', 0, 80), at('a2', 140, 80)]
  return value
}

const vector = (from, to) => ({ x: to.x - from.x, y: to.y - from.y })

test('two new atoms bonded to each other keep their bond in every placement', () => {
  // A bond-tool click on empty canvas in s1: $0 at the click, $1 at the snapped end.
  const [first, second] = placeNewAtoms(stretched(), 's1', [
    { point: { x: 45, y: 30 } },
    { point: { x: 71, y: 15 }, bondTo: '$0' },
  ])
  assert.deepEqual(first.positions, { s1: { x: 45, y: 30 }, s2: { x: 45, y: 110 } }, 'the first follows its nearest atom, a1')
  for (const instance of ['s1', 's2']) {
    assert.deepEqual(vector(first.positions[instance], second.positions[instance]), { x: 26, y: -15 }, instance)
  }
})

test('a new atom bonded to an existing one keeps its bond in every placement, whatever atom is nearest', () => {
  // Nearer a1, bonded to a2: s2 moved a2 by (40, 80) but a1 by (0, 80).
  const [placed] = placeNewAtoms(stretched(), 's1', [{ point: { x: 30, y: 10 }, bondTo: 'a2' }])
  assert.deepEqual(placed.positions, { s1: { x: 30, y: 10 }, s2: { x: 70, y: 90 } })
})

test('duplicating a placement copies the last one, so repeated copies do not land on each other', () => {
  const document = stretched()
  assert.deepEqual(duplicatePlacement(document), { op: 'add_instance', source: 's2', offset: DUPLICATE_OFFSET })
  const older = drawnTwice().document
  delete older.scene
  delete older.molecule
  assert.equal(duplicatePlacement(older), null, 'a service without scene instances cannot duplicate')
})

test('moving or deleting a placement is built from the document it applies to', () => {
  const document = stretched()
  assert.deepEqual(movePlacement(document, 's2', { x: 10, y: 0 }), { op: 'move_instance', instance: 's2', offset: { x: 10, y: 0 } })
  assert.deepEqual(deletePlacement(document, 's2'), { op: 'delete_instance', instance: 's2' })
  // After a queued delete of s2, a press that still names it sends nothing.
  const deleted = stretched()
  deleted.scene.instances = deleted.scene.instances.filter((item) => item.id !== 's2')
  assert.equal(movePlacement(deleted, 's2', { x: 10, y: 0 }), null, 'a deleted placement is not moved')
  assert.equal(deletePlacement(deleted, 's2'), null, 'a deleted placement is not deleted again')
  assert.equal(deletePlacement(deleted, 's1'), null, 'the last placement is not deleted')
})


const { atomLabel, atomName, labelText, placed, queryLabel, showsLabel, textWidth } = await import(
  '../apps/web/src/editor/label.ts'
)

/**
 * The label's text is one chunk, so what precedes and follows the symbol is drawn against it
 * at natural widths (not from the edges of its reserved width), placed so that the symbol's
 * reserved width is centred on the atom.
 */
function assertFlows(label, name) {
  assert.equal(label.chunks.length, 1, `${name}: one chunk of text`)
  const [text] = label.chunks
  assert.equal(text.anchor, 'middle', name)
  const widths = text.runs.map((item) => textWidth(item.text, item.size))
  const sum = (values) => values.reduce((total, value) => total + value, 0)
  const at = text.runs.findIndex((item) => item.part === 'symbol')
  const centre = text.x - sum(widths) / 2 + sum(widths.slice(0, at)) + widths[at] / 2
  assert.ok(Math.abs(centre) < 1e-9, `${name}: the symbol's reserved width is centred on the atom (${centre})`)
}
const { bondName, bondRings, bondSides, bondStrokes, clipped, drawingStrokes, hitTest, leaningRight, placementFrame, viewBox } = await import(
  '../apps/web/src/editor/geometry.ts'
)
const { drawBond } = await import('../apps/web/src/editor/bond-gestures.ts')
const edits = await import('../apps/web/src/editor/atom-edits.ts')

/** An atom as a service with recorded states reports it; `states` overrides the defaults. */
const atomWith = (id, element, x, y, states = {}) => ({
  id,
  element,
  charge: 0,
  isotope: null,
  position: { x, y },
  kind: 'element',
  query: null,
  hydrogens: { mode: 'inferred' },
  radical: 'none',
  attachment_points: [],
  ...states,
})
const bondWith = (id, begin, end, order = 'single', query = null) => ({ id, begin, end, order, query, mark: null })

test('property results validate the analyzed snapshot and refuse incompatible or stale graphs', async () => {
  const shown = document('doc-properties', 1)
  shown.document.atoms = [atomWith('a1', 'C', 0, 0)]
  const result = {
    envelope: 'operation_result', contract: 1, outcome: 'ok', model_id: 'chemifree-properties-1',
    document_revision: 1, loss_report: [],
    result: {
      snapshot: { document: shown.document, session: shown.session, recovery: null },
      scope: 'molecule', property: 'formula', selection: null, display_decimals: 0,
      valence_model: 'chemifree-valence-3', arithmetic: 'binary64', data: null, valence_warnings: [],
      components: [{ atoms: ['a1'], formula: 'CH4', formal_charge: 0, mass: null, isotope_distribution: null, omitted_abundances: [], abundance_sums: [] }],
    },
  }
  const legacy = structuredClone(result)
  legacy.result.display_decimals = 2
  const invalid = structuredClone(result)
  invalid.result.components[0].atoms = ['a1', 'a1']
  service({ '/api/v1/operations': [() => json(200, result), () => json(200, legacy), () => json(200, invalid)] })
  const client = new DocumentClient()
  const inspection = await client.inspectProperties('doc-properties', 'molecule', 'formula', null, null)
  assert.equal(inspection.components[0].formula, 'CH4')
  assert.equal(inspection.display_decimals, 0)
  assert.equal((await client.inspectProperties('doc-properties', 'molecule', 'formula', null, null)).display_decimals, 2, 'earlier numeric formula replies remain readable')
  assert.equal(samePropertySnapshot(shown.document, inspection.snapshot), true)
  const replaced = { ...shown.document, atoms: [atomWith('a1', 'O', 0, 0)] }
  assert.equal(samePropertySnapshot(replaced, inspection.snapshot), false, 'same ID and revision cannot hide different atom content')
  assert.equal(samePropertySnapshot({ ...shown.document, scene: { changed: true } }, inspection.snapshot), false)
  await assert.rejects(client.inspectProperties('doc-properties', 'molecule', 'formula', null, null), { code: 'incompatible_response' })
})

test('aromaticity inspection checks the returned snapshot and its ring references', async () => {
  const shown = document('ring-doc', 1)
  shown.document.atoms = [atomWith('a1', 'C', 0, 0), atomWith('a2', 'C', 1, 0), atomWith('a3', 'C', 0, 1)]
  shown.document.bonds = [bondWith('b1', 'a1', 'a2'), bondWith('b2', 'a2', 'a3'), bondWith('b3', 'a3', 'a1')]
  const reply = { envelope: 'operation_result', contract: 1, outcome: 'ok', document_revision: 1,
    model_id: 'chemifree-aromaticity-2', result: { snapshot: { document: shown.document, session: shown.session, recovery: null },
      systems: [{ atoms: ['a1', 'a2', 'a3'], bonds: ['b1', 'b2', 'b3'], cycle_rank: 1, classification: 'non_aromatic', electrons: 3, reason: 'electron_count' }] } }
  const invalid = structuredClone(reply)
  invalid.result.systems[0].bonds[0] = 'b99'
  service({ '/api/v1/operations': [() => json(200, reply), () => json(200, invalid)] })
  const client = new DocumentClient()
  const result = await client.inspectAromaticity('ring-doc')
  assert.equal(result.systems[0].classification, 'non_aromatic')
  assert.equal(sameInspectionSnapshot(shown.document, result.snapshot), true)
  await assert.rejects(client.inspectAromaticity('ring-doc'), { code: 'analysis_unavailable' })
})

test('aromaticity conversion exposes a model refusal rather than a document', async () => {
  service({ '/api/v1/operations': [() => json(200, { outcome: 'error', diagnostics: [{ code: 'invalid_command', message: 'Conversion refused', details: { reason: 'aromatic_bond_outside_ring' } }] })] })
  const selection = { atoms: [], bonds: [], objects: [] }
  await assert.rejects(new DocumentClient().convertAromaticity('ring-doc', 1, 'kekule', selection),
    { code: 'invalid_command', stateReason: 'aromatic_bond_outside_ring' })
})

test('identity comparison validates both snapshots, symmetry and folded atom IDs', async () => {
  const source = document('identity-source', 2)
  source.document.atoms = [atomWith('a1', 'C', 0, 0), atomWith('a2', 'H', 1, 0)]
  source.document.bonds = [bondWith('b1', 'a1', 'a2')]
  const target = document('identity-target', 3)
  target.document.atoms = [atomWith('a1', 'C', 0, 0)]
  const identity = { code: 'bounded-code', numbering: ['a1'], symmetry: [['a1'], ['a2']], folded_hydrogens: [{ hydrogen: 'a2', attached_to: 'a1' }] }
  const targetIdentity = { code: 'bounded-code', numbering: ['a1'], symmetry: [['a1']], folded_hydrogens: [] }
  const reply = { envelope: 'operation_result', contract: 1, outcome: 'ok', document_revision: 2,
    model_id: 'chemifree-chemical-graph-2', result: {
      snapshot: { document: source.document, session: source.session, recovery: null }, identity,
      comparison: { snapshot: { document: target.document, session: target.session, recovery: { state: 'recoverable', revision: 4, base_revision: 3, saved_at_ms: 0, message: null } }, identity: targetIdentity, equivalent: true },
    } }
  const badOrbit = structuredClone(reply)
  badOrbit.result.identity.symmetry = [['a1', 'a1']]
  const badTarget = structuredClone(reply)
  badTarget.result.comparison.snapshot.document.id = 'different-target'
  const badEquality = structuredClone(reply)
  badEquality.result.comparison.equivalent = false
  const unsupported = { outcome: 'unsupported', model_id: 'chemifree-chemical-graph-2', result: null,
    diagnostics: [{ message: 'Graph identity was not determined under this model.', details: {
      side: 'target', reason: 'element_or_charge', snapshot: { document: target.document, session: target.session, recovery: null },
    } }] }
  const wrongFailure = structuredClone(unsupported)
  wrongFailure.diagnostics[0].details.snapshot.document.id = 'wrong-target'
  service({ '/api/v1/operations': [() => json(200, reply), () => json(200, badOrbit), () => json(200, badTarget), () => json(200, badEquality), () => json(200, wrongFailure), () => json(200, unsupported)] })
  const client = new DocumentClient()
  const result = await client.inspectIdentity('identity-source', 'chemical', 'identity-target')
  assert.equal(result.comparison.equivalent, true)
  assert.equal(result.comparison.snapshot.revision, 3)
  assert.equal(result.comparison.recovery.state, 'recoverable')
  assert.deepEqual(result.identity.folded_hydrogens, [{ hydrogen: 'a2', attached_to: 'a1' }])
  for (let i = 0; i < 3; i++) {
    await assert.rejects(client.inspectIdentity('identity-source', 'chemical', 'identity-target'), { code: 'incompatible_response' })
  }
  await assert.rejects(client.inspectIdentity('identity-source', 'chemical', 'identity-target'), { code: 'incompatible_response' })
  await assert.rejects(client.inspectIdentity('identity-source', 'chemical', 'identity-target'), {
    code: 'identity_unavailable', message: 'Graph identity was not determined under this model. Comparison snapshot identity-target, revision 3: element or charge. Inspect again to include later edits.',
  })
})

/** A query C-O and a target C-C-O whose ids overlap, and the service's reply for them. */
function substructureReply() {
  const query = document('match-query', 2)
  query.document.atoms = [atomWith('a1', 'C', 0, 0), atomWith('a2', 'O', 30, 0)]
  query.document.bonds = [bondWith('b1', 'a1', 'a2')]
  const target = document('match-target', 5)
  target.document.atoms = [atomWith('a1', 'C', 0, 0), atomWith('a2', 'C', 30, 0), atomWith('a3', 'O', 60, 0)]
  target.document.bonds = [bondWith('b1', 'a1', 'a2'), bondWith('b2', 'a2', 'a3')]
  const view = (shown) => ({ document: shown.document, session: shown.session, recovery: null })
  const reply = { envelope: 'operation_result', contract: 1, outcome: 'ok', document_revision: 2, model_id: 'chemifree-substructure-1',
    diagnostics: [], loss_report: [], result: {
      query: view(query), target: view(target), aromaticity_model: 'chemifree-aromaticity-2',
      limits: { query_atoms: 64, search_steps: 2000000, scanned_candidates: 20000000, mappings: 1000 }, steps: 4,
      mappings: [{ atoms: [{ query: 'a1', target: 'a2' }, { query: 'a2', target: 'a3' }], bonds: [{ query: 'b1', target: 'b2' }] }],
    } }
  return { query, target, view, reply }
}

test('a substructure search is accepted only with mappings that fit the two snapshots', async () => {
  const { reply } = substructureReply()
  const mutate = (change) => {
    const copy = structuredClone(reply)
    change(copy)
    return copy
  }
  const bad = [
    mutate((r) => { r.result.mappings[0].atoms[1].target = 'a2' }), // not injective
    mutate((r) => { r.result.mappings[0].atoms[1].target = 'a9' }), // a target atom the snapshot lacks
    mutate((r) => { r.result.mappings[0].atoms.pop() }), // a query atom without a pair
    mutate((r) => { r.result.mappings[0].bonds[0].target = 'b1' }), // the bond is not between the images
    mutate((r) => { r.result.mappings[0].bonds = [] }), // a query bond without a pair
    mutate((r) => { r.result.mappings.push(r.result.mappings[0]); r.result.limits.mappings = 1 }), // over the stated limit
    mutate((r) => { r.document_revision = 3 }), // the revision is not the query snapshot's
    mutate((r) => { r.model_id = 'chemifree-substructure-0' }),
    mutate((r) => { r.loss_report = [{ reason: 'x' }] }),
    mutate((r) => { r.result.query.document.id = 'someone-else' }),
  ]
  service({ '/api/v1/operations': [() => json(200, reply), ...bad.map((body) => () => json(200, body))] })
  const client = new DocumentClient()
  const found = await client.searchSubstructure('match-query', 'match-target')
  assert.equal(found.model, 'chemifree-substructure-1')
  assert.deepEqual(found.mappings[0].atoms, [{ query: 'a1', target: 'a2' }, { query: 'a2', target: 'a3' }])
  assert.equal(found.target.revision, 5)
  for (const [index] of bad.entries()) {
    await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { code: 'incompatible_response' }, `case ${index}`)
  }
})

test('a substructure search of a drawing in itself needs one snapshot', async () => {
  const { reply, query, view } = substructureReply()
  const self = structuredClone(reply)
  self.result.target = view(query)
  self.result.mappings = [{ atoms: [{ query: 'a1', target: 'a1' }, { query: 'a2', target: 'a2' }], bonds: [{ query: 'b1', target: 'b1' }] }]
  const differing = structuredClone(self)
  differing.result.target.document = { ...differing.result.target.document, revision: 3 }
  service({ '/api/v1/operations': [() => json(200, self), () => json(200, differing)] })
  const client = new DocumentClient()
  assert.equal((await client.searchSubstructure('match-query', 'match-query')).mappings.length, 1)
  await assert.rejects(client.searchSubstructure('match-query', 'match-query'), { code: 'incompatible_response' })
})

test('a substructure refusal carries both snapshots, and any other error stays an ApiError', async () => {
  const { reply, query, target, view } = substructureReply()
  const refusal = { ...structuredClone(reply), outcome: 'unsupported', result: null, diagnostics: [{
    code: 'match_unsupported', message: 'The matcher cannot read this atom or bond, and the answer depends on it.',
    details: { query_snapshot: view(query), target_snapshot: view(target), side: 'target', reason: 'fused_partially_aromatic', item: 'bond', id: 'b2' },
  }] }
  const limit = structuredClone(refusal)
  limit.outcome = 'error'
  limit.diagnostics[0] = { code: 'match_limit', message: 'The search reached a limit before it could give a complete answer.',
    details: { query_snapshot: view(query), target_snapshot: view(target), limit_kind: 'search_steps', limit: 2000000 } }
  const wrongOutcome = structuredClone(refusal)
  wrongOutcome.outcome = 'error'
  const missingSnapshot = structuredClone(refusal)
  delete missingSnapshot.diagnostics[0].details.target_snapshot
  const notFound = { envelope: 'operation_result', outcome: 'error', result: null, diagnostics: [{ code: 'document_not_found', message: 'No such document.', details: {} }] }
  service({ '/api/v1/operations': [() => json(200, refusal), () => json(200, limit), () => json(200, wrongOutcome), () => json(200, missingSnapshot), () => json(200, notFound)] })
  const client = new DocumentClient()
  await assert.rejects(client.searchSubstructure('match-query', 'match-target'), (failure) => {
    assert.equal(failure.name, 'SubstructureRefusal')
    assert.equal(failure.code, 'match_unsupported')
    assert.equal(failure.message, 'The matcher cannot read this atom or bond, and the answer depends on it. The target drawing, bond b2: fused partially aromatic.')
    assert.equal(failure.query.revision, 2)
    assert.equal(failure.target.revision, 5)
    return true
  })
  await assert.rejects(client.searchSubstructure('match-query', 'match-target'), (failure) => {
    assert.equal(failure.code, 'match_limit')
    assert.match(failure.message, /Limit: search steps \(2000000\)\./)
    return true
  })
  await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { code: 'incompatible_response' })
  await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { code: 'incompatible_response' })
  await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { name: 'ApiError', code: 'document_not_found' })
})

test('a failure code that is an object property name is an ordinary API error, not a refusal', async () => {
  const { query, target, view } = substructureReply()
  const named = (code) => ({ envelope: 'operation_result', outcome: 'error', result: null, model_id: null, document_revision: null,
    diagnostics: [{ code, message: `Failed as ${code}.`, details: { query_snapshot: view(query), target_snapshot: view(target) } }] })
  service({ '/api/v1/operations': ['constructor', 'toString', '__proto__', 'hasOwnProperty'].map((code) => () => json(200, named(code))) })
  const client = new DocumentClient()
  for (const code of ['constructor', 'toString', '__proto__', 'hasOwnProperty']) {
    await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { name: 'ApiError', code, message: `Failed as ${code}.` })
  }
})

test('a search that does not end for one document does not stop another document from searching', () => {
  const { createSearches, beginSearch, isLatest, isRunning, startRunning, finishRunning, leaveAll } = searchRequests
  const searches = createSearches()
  let running = new Map()
  const tokenA = beginSearch(searches, 'doc-a')
  running = startRunning(running, 'doc-a', tokenA)
  // Document B is searched while A's request is still out.
  assert.equal(isRunning(running, 'doc-b'), false, 'A running does not make B busy')
  const tokenB = beginSearch(searches, 'doc-b')
  running = startRunning(running, 'doc-b', tokenB)
  assert.equal(isRunning(running, 'doc-a') && isRunning(running, 'doc-b'), true)
  // B ends first; A's late outcome may still be stored under A.
  running = finishRunning(running, 'doc-b', tokenB)
  assert.deepEqual([isRunning(running, 'doc-a'), isRunning(running, 'doc-b')], [true, false])
  assert.equal(isLatest(searches, 'doc-a', tokenA), true, 'a late result is stored under its own document')
  running = finishRunning(running, 'doc-a', tokenA)
  assert.equal(running.size, 0)
  // A newer search of the same document replaces the older: the older neither stores nor clears.
  const first = beginSearch(searches, 'doc-a')
  running = startRunning(running, 'doc-a', first)
  const second = beginSearch(searches, 'doc-a')
  running = startRunning(running, 'doc-a', second)
  assert.equal(isLatest(searches, 'doc-a', first), false)
  assert.equal(finishRunning(running, 'doc-a', first), running, 'an older search cannot end the newer one')
  assert.equal(isRunning(finishRunning(running, 'doc-a', first), 'doc-a'), true)
  // Leaving drops every outcome that was still to come, and no count can go negative.
  leaveAll(searches)
  assert.equal(isLatest(searches, 'doc-a', second), false)
  assert.equal(finishRunning(new Map(), 'doc-a', second).size, 0)
})

test('the other drawing of a result is judged changed by the document list, and only when the list says so', () => {
  const { otherDrawingChanged } = searchRequests
  const searched = { id: 'doc-b', title: 'B', revision: 3, atoms: [{ id: 'a1' }, { id: 'a2' }], bonds: [{ id: 'b1' }] }
  const summary = (changes = {}) => ({ id: 'doc-b', title: 'B', revision: 3, atom_count: 2, bond_count: 1, open: false, unsaved_changes: false, recovery: null, saved_at_ms: null, ...changes })
  assert.equal(otherDrawingChanged(searched, [summary()]), false, 'the list still holds the searched snapshot')
  assert.equal(otherDrawingChanged(searched, [summary({ revision: 4 })]), true, 'a later revision')
  assert.equal(otherDrawingChanged(searched, [summary({ revision: 2 })]), true, 'an earlier revision is also not the searched drawing')
  assert.equal(otherDrawingChanged(searched, [summary({ atom_count: 3 })]), true, 'an atom added at the same revision (a restart reused it)')
  assert.equal(otherDrawingChanged(searched, [summary({ bond_count: 0 })]), true, 'a bond removed')
  assert.equal(otherDrawingChanged(searched, []), false, 'a drawing the list does not hold is not judged (a filter looks the same)')
  assert.equal(otherDrawingChanged(searched, [summary({ id: 'doc-c', revision: 9 })]), false, 'another document changing says nothing')
  assert.equal(otherDrawingChanged(null, [summary({ revision: 9 })]), false, 'a drawing searched in itself is judged by its own snapshot')
})

test('a substructure search after a service restart is asked about, not answered from the old token', async () => {
  const { reply } = substructureReply()
  const scripted = service({ '/api/v1/operations': [() => json(200, reply)] })
  const client = new DocumentClient()
  await client.searchSubstructure('match-query', 'match-target')
  scripted.restart()
  await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { code: 'service_restarted' })
})

/** C1–N2 with every recorded state somewhere, as a service with recorded states replies. */
function recorded() {
  return {
    api_version: 'v1',
    document: {
      id: 'doc-1',
      title: 'States',
      revision: 3,
      atoms: [
        atomWith('a1', 'C', 0, 0, { hydrogens: { mode: 'specified', count: 3 }, radical: 'doublet', attachment_points: [1, 3] }),
        atomWith('a2', '![N,O]', 30, 0, {
          kind: 'query',
          query: { kind: 'list', elements: ['N', 'O'], negated: true },
          hydrogens: { mode: 'unknown' },
        }),
      ],
      bonds: [bondWith('b1', 'a1', 'a2', 'query', { kind: 'list', orders: ['single', 'aromatic'] })],
    },
    session: { saved_revision: 0, unsaved_changes: true, undo_depth: 3, redo_depth: 0, draft_revision: null },
    recovery: null,
  }
}

test('a document reply with recorded states is accepted, and older replies have the defaults filled in', async () => {
  assert.equal(isDocumentResponse(recorded()), true)
  const older = {
    ...document('doc-1', 1),
    document: {
      id: 'doc-1',
      title: 'Older',
      revision: 1,
      atoms: [{ id: 'a1', element: 'C', charge: 0, isotope: null, position: { x: 0, y: 0 } }],
      bonds: [],
    },
  }
  service({ '/transactions': [() => json(200, older)] })
  const reply = await new DocumentClient().transact('doc-1', 0, [])
  assert.deepEqual(reply.document.atoms[0], atomWith('a1', 'C', 0, 0))
})

test('a bond from before recorded states gets a null query', async () => {
  const older = {
    ...document('doc-1', 1),
    document: {
      id: 'doc-1',
      title: 'Older',
      revision: 1,
      atoms: [
        { id: 'a1', element: 'C', charge: 0, isotope: null, position: { x: 0, y: 0 } },
        { id: 'a2', element: 'C', charge: 0, isotope: null, position: { x: 30, y: 0 } },
      ],
      bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'double' }],
    },
  }
  service({ '/transactions': [() => json(200, older)] })
  const reply = await new DocumentClient().transact('doc-1', 0, [])
  assert.deepEqual(reply.document.bonds[0], bondWith('b1', 'a1', 'a2', 'double'))
})

for (const [name, change] of [
  ['an atom without some of the state fields', (value) => delete value.document.atoms[0].radical],
  ['one atom with the state fields and one without', (value) => {
    for (const field of ['kind', 'query', 'hydrogens', 'radical', 'attachment_points']) delete value.document.atoms[1][field]
  }],
  ['atoms with state fields and a bond without its query', (value) => delete value.document.bonds[0].query],
  ['atoms without state fields and a bond with its query', (value) => {
    for (const atom of value.document.atoms) {
      for (const field of ['kind', 'query', 'hydrogens', 'radical', 'attachment_points']) delete atom[field]
    }
  }],
  ['a stated hydrogen count above 16', (value) => (value.document.atoms[0].hydrogens = { mode: 'specified', count: 17 })],
  ['an inferred hydrogen state with a count', (value) => (value.document.atoms[0].hydrogens = { mode: 'inferred', count: 2 })],
  ['a stated hydrogen state without a count', (value) => (value.document.atoms[0].hydrogens = { mode: 'specified' })],
  ['an unknown radical', (value) => (value.document.atoms[0].radical = 'quartet')],
  ['attachment points out of order', (value) => (value.document.atoms[0].attachment_points = [3, 1])],
  ['an attachment point above 4', (value) => (value.document.atoms[0].attachment_points = [5])],
  ['a charged query atom', (value) => (value.document.atoms[1].charge = 1)],
  ['a query atom with an isotope', (value) => (value.document.atoms[1].isotope = 13)],
  ['a query atom without its query', (value) => (value.document.atoms[1].query = null)],
  ['an element atom with a query', (value) => (value.document.atoms[0].query = { kind: 'any' })],
  ['an empty query list', (value) => (value.document.atoms[1].query.elements = [])],
  ['a query list that repeats an element', (value) => (value.document.atoms[1].query.elements = ['N', 'N'])],
  ['a query bond without its query', (value) => (value.document.bonds[0].query = null)],
  ['a single bond with a query', (value) => (value.document.bonds[0].order = 'single')],
  ['a query bond list naming a dative bond', (value) => (value.document.bonds[0].query.orders = ['dative'])],
  ['a bond kind added with recorded states, without its query field', (value) => {
    for (const atom of value.document.atoms) {
      for (const field of ['kind', 'query', 'hydrogens', 'radical', 'attachment_points']) delete atom[field]
    }
    value.document.atoms[1].element = 'N'
    value.document.atoms[1].charge = 0
    value.document.bonds[0] = { id: 'b1', begin: 'a1', end: 'a2', order: 'aromatic' }
  }],
]) {
  test(`a document reply with ${name} is refused`, () => {
    const value = recorded()
    change(value)
    assert.equal(isDocumentResponse(value), false)
  })
}

test("a refused combination's reason is read from the error envelope", async () => {
  service({
    '/transactions': [
      () =>
        json(422, { api_version: 'v1', code: 'invalid_command', message: 'a query atom carries no charge', command_index: 1, reason: 'query_atom_charge' }),
    ],
  })
  await assert.rejects(new DocumentClient().transact('doc-1', 0, []), {
    code: 'invalid_command',
    commandIndex: 1,
    stateReason: 'query_atom_charge',
  })
})

test('a bonded carbon is labelled when it records a state, and inferred hydrogens add nothing', () => {
  assert.equal(showsLabel(atomWith('a1', 'C', 0, 0), 2), false, 'a bare vertex')
  assert.equal(atomLabel(atomWith('a1', 'C', 0, 0), 2), null)
  for (const states of [
    { hydrogens: { mode: 'specified', count: 0 } },
    { hydrogens: { mode: 'unknown' } },
    { radical: 'triplet' },
    { attachment_points: [2] },
  ]) {
    assert.equal(showsLabel(atomWith('a1', 'C', 0, 0, states), 2), true, JSON.stringify(states))
  }
  const nitrogen = atomLabel(atomWith('a1', 'N', 0, 0), 1)
  assert.deepEqual(labelText(nitrogen), ['N'], 'no hydrogen count is shown for inferred hydrogens')
})

test('stated and unknown hydrogens follow the symbol with their count', () => {
  const texts = (hydrogens) => labelText(atomLabel(atomWith('a1', 'N', 0, 0, { hydrogens }), 1))
  assert.deepEqual(texts({ mode: 'specified', count: 3 }), ['N', 'H', '3'])
  assert.deepEqual(texts({ mode: 'specified', count: 1 }), ['N', 'H'])
  assert.deepEqual(texts({ mode: 'specified', count: 0 }), ['N', 'H', '0'])
  assert.deepEqual(texts({ mode: 'unknown' }), ['N', 'H', '?'])
})

test('stated hydrogens go left of the symbol when the bonds lean right, clear of the bond', () => {
  const methyl = atomWith('a1', 'C', 0, 0, { hydrogens: { mode: 'specified', count: 3 }, isotope: 13 })
  const label = atomLabel(methyl, 1, true)
  assert.deepEqual(labelText(label), ['H', '3', '13', 'C'])
  assertFlows(label, 'H3 13C')
  assert.ok(label.chunks[0].box.x < -textWidth('C', 12) / 2 - textWidth('H', 12), 'the hydrogens are left of the symbol')
  const view = {
    id: 'doc-1',
    title: 'Lean',
    revision: 1,
    atoms: [atomWith('a1', 'C', 0, 0), atomWith('a2', 'C', 26, -15), atomWith('a3', 'C', 26, 15), atomWith('a4', 'C', 26, 45)],
    bonds: [bondWith('b1', 'a1', 'a2'), bondWith('b2', 'a2', 'a3'), bondWith('b3', 'a3', 'a4')],
  }
  assert.deepEqual([...leaningRight(view)].sort(), ['a1'], 'a2 leans left, a3 is between, a4 bonds straight up')
  // Drawn as the canvas does, the bond from a1 leaves its label with room to spare.
  const [from, to] = clipped({ x: 0, y: 0 }, { x: 26, y: -15 }, label.box, null)
  assert.ok(to.x - from.x > 10, `${from.x} → ${to.x}`)
})

/** Whether `inner` lies within `outer`, beyond floating-point rounding (1e-9 pt). */
const within = (inner, outer) =>
  inner.x >= outer.x - 1e-9 &&
  inner.y >= outer.y - 1e-9 &&
  inner.x + inner.width <= outer.x + outer.width + 1e-9 &&
  inner.y + inner.height <= outer.y + outer.height + 1e-9

test("an atom's label box encloses its text, radical dots, and attachment points", () => {
  const plain = atomLabel(atomWith('a1', 'O', 0, 0, { isotope: 18, charge: -1, hydrogens: { mode: 'specified', count: 2 } }), 1)
  for (const radical of ['singlet', 'doublet', 'triplet', 'unknown']) {
    const label = atomLabel(
      atomWith('a1', 'O', 0, 0, { isotope: 18, charge: -1, hydrogens: { mode: 'specified', count: 2 }, radical, attachment_points: [1, 2, 3, 4] }),
      1,
    )
    for (const dot of label.dots) {
      assert.ok(within({ x: dot.dx - 1.4, y: dot.dy - 1.4, width: 2.8, height: 2.8 }, label.box), `${radical} dot inside`)
    }
    for (const chunk of [...label.chunks, ...label.marks]) {
      assert.ok(within(chunk.box, label.box), `${radical}: ${chunk.role} inside`)
    }
    assert.ok(label.box.y < plain.box.y, `${radical}: the dots raise the top`)
    assert.ok(label.box.y + label.box.height > plain.box.y + plain.box.height, 'the attachment mark lowers the bottom')
  }
})

test('a long query list is shortened on the canvas and named in full', () => {
  const query = { kind: 'list', elements: ['N', 'O', 'S', 'P', 'Se'], negated: true }
  assert.equal(queryLabel(query), '![N,O,S,…]')
  assert.equal(queryLabel({ kind: 'list', elements: ['N', 'O', 'S'], negated: false }), '[N,O,S]')
  const label = atomLabel(atomWith('a1', '![N,O,S,P,Se]', 0, 0, { kind: 'query', query }), 0)
  assert.deepEqual(labelText(label), ['![N,O,S,…]'])
  assert.match(atomName(atomWith('a1', '![N,O,S,P,Se]', 0, 0, { kind: 'query', query })), /any atom except N, O, S, P, Se/)
})

test('a bond stops 2 pt outside both label boxes', () => {
  const begin = atomLabel(atomWith('a1', 'N', 0, 0, { hydrogens: { mode: 'specified', count: 2 } }), 1).box
  const end = atomLabel(atomWith('a2', 'O', 40, 0), 1).box
  const placedEnd = { ...end, x: end.x + 40 }
  const [from, to] = clipped({ x: 0, y: 0 }, { x: 40, y: 0 }, begin, placedEnd)
  assert.equal(from.x, begin.x + begin.width + 2)
  assert.equal(to.x, placedEnd.x - 2)
})

test('a squeezed bond draws only in the room between labels, never reversed or across them', () => {
  const box = atomLabel(atomWith('a1', 'Cl', 0, 0, { hydrogens: { mode: 'specified', count: 2 } }), 1).box
  assert.equal(clipped({ x: 0, y: 0 }, { x: 12, y: 0 }, box, { ...box, x: box.x + 12 }), null)
  const first = { x: -1, y: -1, width: 4, height: 2 }
  const second = { x: 9, y: -1, width: 4, height: 2 }
  const [from, to] = clipped({ x: 0, y: 0 }, { x: 12, y: 0 }, first, second)
  assert.deepEqual([from, to], [{ x: 5, y: 0 }, { x: 7, y: 0 }], 'a narrow gap keeps its available stroke')
})

test('every bond kind is drawn differently and named', () => {
  const at = (x) => ({ x, y: 0 })
  const shapes = (order, query = null) =>
    bondStrokes(bondWith('b1', 'a1', 'a2', order, query), at(0), at(30), null, null).map((stroke) =>
      stroke.shape === 'line' ? `line:${stroke.pattern}` : 'polyline',
    )
  assert.deepEqual(shapes('single'), ['line:solid'])
  assert.deepEqual(shapes('double'), ['line:solid', 'line:solid'])
  assert.deepEqual(shapes('aromatic'), ['line:solid', 'line:dashed'])
  assert.deepEqual(shapes('dative'), ['line:solid', 'polyline'])
  assert.deepEqual(shapes('query', { kind: 'any' }), ['line:dotted'])
  assert.deepEqual(shapes('unknown'), ['polyline'])
  const [, head] = bondStrokes(bondWith('b1', 'a1', 'a2', 'dative'), at(0), at(30), null, null)
  assert.deepEqual(head.points[1], at(30), 'the arrow points at the end atom')
  assert.ok(head.points[0].x < 30 && head.points[2].x < 30)
  assert.equal(bondName(bondWith('b1', 'a1', 'a2', 'dative')), 'Bond b1: dative bond from a1 to a2')
  assert.equal(
    bondName(bondWith('b1', 'a1', 'a2', 'query', { kind: 'list', orders: ['single', 'aromatic'] })),
    'Bond b1: query bond, single or aromatic, between a1 and a2',
  )
})

test('the view box grows by each label box', () => {
  const query = { kind: 'list', elements: ['N', 'O', 'S', 'P'], negated: true }
  const label = atomLabel(atomWith('a1', '![N,O,S,P]', 30, 100, { kind: 'query', query, radical: 'doublet' }), 0)
  const placedBox = { ...label.box, x: label.box.x + 30, y: label.box.y + 100 }
  assert.ok(placedBox.x < 0, 'the label reaches past the frame')
  const box = viewBox([{ x: 30, y: 100 }], [placedBox])
  assert.ok(within(placedBox, box), JSON.stringify({ placedBox, box }))
  assert.equal(viewBox([{ x: 30, y: 100 }]).x, 0, 'without labels the frame starts at 0')
})

test('a labelled atom is hit anywhere in its label box', () => {
  const nitrogen = atomWith('a1', 'N', 100, 100, { hydrogens: { mode: 'specified', count: 3 } })
  const view = { id: 'doc-1', title: 'Hit', revision: 1, atoms: [nitrogen], bonds: [] }
  const labels = new Map([['a1', atomLabel(nitrogen, 0)]])
  const onTheCount = { x: 115, y: 102 }
  assert.deepEqual(hitTest(view, onTheCount, new Set(), undefined, labels), { kind: 'atom', id: 'a1' })
  assert.equal(hitTest(view, onTheCount), null, 'beyond the vertex radius without the label')
  assert.equal(hitTest(view, { x: 125, y: 100 }, new Set(), undefined, labels), null, 'outside the box')
})

/** A document with one atom (and one bond to a second) for the inspector's edits. */
const editable = (states = {}, bond = bondWith('b1', 'a1', 'a2')) => ({
  id: 'doc-1',
  title: 'Edits',
  revision: 1,
  atoms: [atomWith('a1', 'C', 0, 0, states), atomWith('a2', 'C', 30, 0)],
  bonds: [bond],
})

test('setting a query clears the charge and isotope the latest document holds', () => {
  assert.deepEqual(edits.setQuery('a1', { kind: 'halogen' })(editable({ charge: 1, isotope: 13 })), [
    { op: 'update_atom', atom: 'a1', query: { kind: 'halogen' }, charge: 0, isotope: null },
  ])
  assert.deepEqual(edits.setQuery('a1', { kind: 'halogen' })(editable()), [
    { op: 'update_atom', atom: 'a1', query: { kind: 'halogen' } },
  ])
  const halogen = { kind: 'query', element: 'X', query: { kind: 'halogen' } }
  assert.equal(edits.setQuery('a1', { kind: 'halogen' })(editable(halogen)), null, 'already set')
  assert.equal(edits.setQuery('a9', { kind: 'any' })(editable()), null, 'gone')
})

test('a charge or isotope is never sent for a query atom, nor a charge past ±8', () => {
  const query = { kind: 'query', element: '*', query: { kind: 'any' } }
  assert.equal(edits.stepCharge('a1', 1)(editable(query)), null)
  assert.equal(edits.setIsotope('a1', 13)(editable(query)), null)
  assert.equal(edits.stepCharge('a1', 1)(editable({ charge: 8 })), null)
  assert.equal(edits.stepCharge('a1', -1)(editable({ charge: -8 })), null)
  assert.deepEqual(edits.stepCharge('a1', 1)(editable({ charge: 2 })), [{ op: 'update_atom', atom: 'a1', charge: 3 }])
  assert.deepEqual(edits.setIsotope('a1', null)(editable({ isotope: 13 })), [{ op: 'update_atom', atom: 'a1', isotope: null }])
})

/** Applies an inspector edit's update_atom commands to a document, as the service would. */
function applied(document, commands) {
  if (commands === null) return document
  const atoms = document.atoms.map((atom) => {
    const command = commands.find((candidate) => candidate.atom === atom.id)
    if (!command) return atom
    const { op: _op, atom: _id, element, query, ...rest } = command
    const kind = query ? { kind: 'query', query, element: queryLabel(query) } : element ? { kind: 'element', query: null, element } : {}
    return { ...atom, ...rest, ...kind }
  })
  return { ...document, atoms }
}

for (const [name, order] of [
  ['a charge press then a query press', [edits.stepCharge('a1', 1), edits.setQuery('a1', { kind: 'any' })]],
  ['a query press then a charge press', [edits.setQuery('a1', { kind: 'any' }), edits.stepCharge('a1', 1)]],
  ['an isotope then a query', [edits.setIsotope('a1', 13), edits.setQuery('a1', { kind: 'any' })]],
]) {
  test(`${name}, each built from the latest document, leave a query atom without charge or isotope`, () => {
    let latest = editable()
    for (const edit of order) latest = applied(latest, edit(latest))
    const [atom] = latest.atoms
    assert.deepEqual([atom.kind, atom.charge, atom.isotope], ['query', 0, null])
  })
}

test('hydrogen edits state a count and step it within 0 to 16', () => {
  assert.deepEqual(edits.setHydrogenMode('a1', 'specified')(editable()), [
    { op: 'update_atom', atom: 'a1', hydrogens: { mode: 'specified', count: 1 } },
  ])
  assert.deepEqual(edits.setHydrogenMode('a1', 'unknown')(editable()), [
    { op: 'update_atom', atom: 'a1', hydrogens: { mode: 'unknown' } },
  ])
  assert.equal(edits.setHydrogenMode('a1', 'inferred')(editable()), null, 'already inferred')
  assert.equal(edits.stepHydrogens('a1', 1)(editable()), null, 'no count to step')
  assert.equal(edits.stepHydrogens('a1', 1)(editable({ hydrogens: { mode: 'specified', count: 16 } })), null)
  assert.equal(edits.stepHydrogens('a1', -1)(editable({ hydrogens: { mode: 'specified', count: 0 } })), null)
  assert.deepEqual(edits.stepHydrogens('a1', 1)(editable({ hydrogens: { mode: 'specified', count: 15 } })), [
    { op: 'update_atom', atom: 'a1', hydrogens: { mode: 'specified', count: 16 } },
  ])
})

test('radical and attachment point edits keep the points ascending', () => {
  assert.deepEqual(edits.setRadical('a1', 'doublet')(editable()), [{ op: 'update_atom', atom: 'a1', radical: 'doublet' }])
  assert.equal(edits.setRadical('a1', 'none')(editable()), null)
  assert.deepEqual(edits.toggleAttachmentPoint('a1', 2)(editable({ attachment_points: [1, 3] })), [
    { op: 'update_atom', atom: 'a1', attachment_points: [1, 2, 3] },
  ])
  assert.deepEqual(edits.toggleAttachmentPoint('a1', 3)(editable({ attachment_points: [1, 3] })), [
    { op: 'update_atom', atom: 'a1', attachment_points: [1] },
  ])
  assert.equal(edits.toggleAttachmentPoint('a1', 5)(editable()), null)
})

test('bond kind edits send a query only with order query', () => {
  assert.deepEqual(edits.setBondKind('b1', { order: 'aromatic' })(editable()), [{ op: 'update_bond', bond: 'b1', order: 'aromatic' }])
  assert.equal(edits.setBondKind('b1', { order: 'single' })(editable()), null, 'already single')
  assert.deepEqual(edits.toggleQueryBondOrder('b1', 'aromatic')(editable()), [
    { op: 'update_bond', bond: 'b1', order: 'query', query: { kind: 'list', orders: ['aromatic'] } },
  ])
  const listed = bondWith('b1', 'a1', 'a2', 'query', { kind: 'list', orders: ['aromatic'] })
  assert.deepEqual(edits.toggleQueryBondOrder('b1', 'single')(editable({}, listed)), [
    { op: 'update_bond', bond: 'b1', order: 'query', query: { kind: 'list', orders: ['single', 'aromatic'] } },
  ])
  assert.deepEqual(edits.toggleQueryBondOrder('b1', 'aromatic')(editable({}, listed)), [
    { op: 'update_bond', bond: 'b1', order: 'query', query: { kind: 'any' } },
  ])
})

test('a typed query list names 1 to 10 distinct symbols', () => {
  assert.deepEqual(edits.parseQueryList('N, O,S'), ['N', 'O', 'S'])
  assert.equal(edits.parseQueryList(''), null)
  assert.equal(edits.parseQueryList('N, N'), null)
  assert.equal(edits.parseQueryList('n'), null)
  assert.equal(edits.parseQueryList('H He Li Be B C N O F Ne Na'), null)
})

// Natural advances of the bundled font (Geist, weight 500) at 12 pt, measured in Chromium:
// C 0.71, l 0.28, I 0.28, W 0.97 of the font size.
test("a symbol's reserved width holds its natural width without being far wider", () => {
  for (const [symbol, natural] of [
    ['I', 3.36],
    ['Cl', 11.88],
    ['W', 11.64],
  ]) {
    const [chunk] = atomLabel(atomWith('a1', symbol, 0, 0), 0).chunks
    assert.ok(chunk.box.width >= natural && chunk.box.width <= natural + 2, `${symbol}: ${chunk.box.width} for ${natural}`)
    assert.deepEqual([chunk.anchor, chunk.x], ['middle', 0], `${symbol} is centred on the atom`)
  }
  assertFlows(atomLabel(atomWith('a1', 'Cl', 0, 0, { hydrogens: { mode: 'specified', count: 2 }, charge: -1 }), 1), 'ClH2−')
  const list = { kind: 'query', element: '![N,O,S,P]', query: { kind: 'list', elements: ['N', 'O', 'S', 'P'], negated: true } }
  const hydrogens = { mode: 'specified', count: 2 }
  assertFlows(atomLabel(atomWith('a1', '![N,O,S,P]', 0, 0, { ...list, hydrogens }), 1), '![N,O,S,…]H2')
  assertFlows(atomLabel(atomWith('a1', '![N,O,S,P]', 0, 0, { ...list, hydrogens }), 1, true), 'H2![N,O,S,…]')
})

for (const element of ['O', 'N']) {
  test(`${element} is hit within 9 pt of its centre, where its label box is narrower`, () => {
    const atom = atomWith('a1', element, 100, 100)
    const view = { id: 'doc-1', title: 'Hit', revision: 1, atoms: [atom], bonds: [] }
    const labels = new Map([['a1', atomLabel(atom, 1)]])
    for (const dx of [-8, -6, 6, 8]) {
      assert.deepEqual(hitTest(view, { x: 100 + dx, y: 100 }, new Set(), undefined, labels), { kind: 'atom', id: 'a1' }, `${dx}`)
    }
    assert.equal(hitTest(view, { x: 110, y: 100 }, new Set(), undefined, labels), null, 'beyond both')
  })
}

test('the view box holds every placement frame and its ID', () => {
  const query = { kind: 'list', elements: ['N', 'O', 'S', 'P'], negated: true }
  const atoms = [
    atomWith('a1', '![N,O,S,…]', 380, 100, { kind: 'query', query, hydrogens: { mode: 'specified', count: 16 } }),
    atomWith('a2', 'N', 200, 230, { attachment_points: [1, 2] }),
    atomWith('a3', 'N', 200, 2),
  ]
  const boxes = atoms.map((atom) => placed(atomLabel(atom, 0).box, atom.position))
  const frame = placementFrame(
    atoms.map((atom) => atom.position),
    boxes,
    's2',
  )
  for (const box of boxes) {
    const padded = { x: box.x - 14, y: box.y - 14, width: box.width + 28, height: box.height + 28 }
    assert.ok(within(padded, frame.rect), 'the frame keeps 14 pt around every label')
  }
  assert.ok(within(frame.rect, frame.box), 'the frame is inside its box')
  // The ID is 8 pt text centred on idAt, measured at most 0.655 of its size above its middle.
  assert.ok(frame.idAt.y < frame.rect.y && frame.box.y <= frame.idAt.y - 0.655 * 8, 'the ID sits above the frame, inside its box')
  const box = viewBox(
    atoms.map((atom) => atom.position),
    [...boxes, frame.box],
  )
  assert.ok(within(frame.box, box), JSON.stringify({ frame: frame.box, box }))
})

const drawn = editable()
const unbonded = { ...editable(), bonds: [] }
for (const [name, document, from, to, order, expected] of [
  ['between two atoms', unbonded, { atom: 'a1' }, { atom: 'a2' }, 'dative', [{ op: 'add_bond', begin: 'a1', end: 'a2', order: 'dative' }]],
  [
    'over a bond drawn the other way',
    drawn,
    { atom: 'a2' },
    { atom: 'a1' },
    'dative',
    [
      { op: 'delete_bonds', bonds: ['b1'] },
      { op: 'add_bond', begin: 'a2', end: 'a1', order: 'dative' },
    ],
  ],
  ['over a bond drawn the same way', drawn, { atom: 'a1' }, { atom: 'a2' }, 'dative', [{ op: 'update_bond', bond: 'b1', order: 'dative' }]],
  ['as a double bond either way', drawn, { atom: 'a2' }, { atom: 'a1' }, 'double', [{ op: 'update_bond', bond: 'b1', order: 'double' }]],
  [
    'from empty space onto an atom',
    drawn,
    { point: { x: 0, y: 40 } },
    { atom: 'a1' },
    'dative',
    [
      { op: 'add_atom', element: 'C', position: { x: 0, y: 40 } },
      { op: 'add_bond', begin: '$0', end: 'a1', order: 'dative' },
    ],
  ],
  [
    'from an atom into empty space',
    drawn,
    { atom: 'a1' },
    { point: { x: 0, y: 40 } },
    'dative',
    [{ op: 'add_atom', element: 'C', position: { x: 0, y: 40 }, bond_to: { atom: 'a1', order: 'dative' } }],
  ],
  [
    'from empty space into empty space',
    drawn,
    { point: { x: 0, y: 40 } },
    { point: { x: 26, y: 55 } },
    'dative',
    [
      { op: 'add_atom', element: 'C', position: { x: 0, y: 40 } },
      { op: 'add_atom', element: 'C', position: { x: 26, y: 55 }, bond_to: { atom: '$0', order: 'dative' } },
    ],
  ],
  ['from an atom onto itself', drawn, { atom: 'a1' }, { atom: 'a1' }, 'dative', []],
]) {
  test(`a bond drawn ${name} starts where the drag starts`, () => {
    assert.deepEqual(drawBond(document, null, from, to, order), expected)
  })
}

test('a bond drawn from empty space onto an atom is placed relative to that atom in every placement', () => {
  const { document } = drawnTwice()
  assert.deepEqual(drawBond(document, 's1', { point: { x: 30, y: 40 } }, { atom: 'a2' }, 'dative'), [
    { op: 'add_atom', element: 'C', positions: { s1: { x: 30, y: 40 }, s2: { x: 80, y: 140 } } },
    { op: 'add_bond', begin: '$0', end: 'a2', order: 'dative' },
  ])
})

test('a typed query list keeps the negation the latest document holds, and the negation keeps the list', () => {
  const list = (elements, negated) => ({ kind: 'query', element: 'L', query: { kind: 'list', elements, negated } })
  assert.deepEqual(edits.setQueryElements('a1', ['N', 'O', 'S'])(editable(list(['N', 'O'], true))), [
    { op: 'update_atom', atom: 'a1', query: { kind: 'list', elements: ['N', 'O', 'S'], negated: true } },
  ])
  assert.deepEqual(edits.setQueryElements('a1', ['N'])(editable({ charge: 1 })), [
    { op: 'update_atom', atom: 'a1', query: { kind: 'list', elements: ['N'], negated: false }, charge: 0 },
  ])
  assert.deepEqual(edits.setQueryNegated('a1', true)(editable(list(['N', 'O', 'S'], false))), [
    { op: 'update_atom', atom: 'a1', query: { kind: 'list', elements: ['N', 'O', 'S'], negated: true } },
  ])
  assert.equal(edits.setQueryNegated('a1', true)(editable(list(['N'], true))), null, 'already negated')
  assert.equal(edits.setQueryNegated('a1', true)(editable()), null, 'not a list')
})

for (const [name, order] of [
  ['a typed list then the negation', [edits.setQueryElements('a1', ['N', 'O', 'S']), edits.setQueryNegated('a1', true)]],
  ['the negation then a typed list', [edits.setQueryNegated('a1', true), edits.setQueryElements('a1', ['N', 'O', 'S'])]],
]) {
  test(`${name}, each built from the latest document, keep both`, () => {
    let latest = editable({ kind: 'query', element: '[N,O]', query: { kind: 'list', elements: ['N', 'O'], negated: false } })
    for (const edit of order) latest = applied(latest, edit(latest))
    assert.deepEqual(latest.atoms[0].query, { kind: 'list', elements: ['N', 'O', 'S'], negated: true })
  })
}

/** Whether segments p–q and r–s cross. */
function crosses(p, q, r, s) {
  const turn = (a, b, c) => Math.sign((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x))
  return turn(p, q, r) * turn(p, q, s) < 0 && turn(r, s, p) * turn(r, s, q) < 0
}

test('aromatic dashed lines lie inside a ring drawn in mixed directions and do not cross at its vertices', () => {
  const centre = { x: 100, y: 100 }
  const atoms = Array.from({ length: 6 }, (_, index) => {
    const angle = (index * Math.PI) / 3
    return atomWith(`a${index + 1}`, 'C', centre.x + 30 * Math.cos(angle), centre.y + 30 * Math.sin(angle))
  })
  const ends = [
    ['a1', 'a2'],
    ['a3', 'a2'],
    ['a3', 'a4'],
    ['a5', 'a4'],
    ['a5', 'a6'],
    ['a1', 'a6'],
  ]
  const bonds = ends.map(([begin, end], index) => bondWith(`b${index + 1}`, begin, end, 'aromatic'))
  const view = { id: 'doc-1', title: 'Ring', revision: 1, atoms, bonds }
  const sides = bondSides(view)
  const at = (id) => atoms.find((atom) => atom.id === id).position
  const dashed = bonds.map((bond) => {
    const [, line] = bondStrokes(bond, at(bond.begin), at(bond.end), null, null, sides.get(bond.id))
    assert.equal(line.pattern, 'dashed')
    const middle = { x: (line.x1 + line.x2) / 2, y: (line.y1 + line.y2) / 2 }
    const bondMiddle = { x: (at(bond.begin).x + at(bond.end).x) / 2, y: (at(bond.begin).y + at(bond.end).y) / 2 }
    const toCentre = (point) => Math.hypot(point.x - centre.x, point.y - centre.y)
    assert.ok(toCentre(middle) < toCentre(bondMiddle), `${bond.id}'s dashed line is inside the ring`)
    return [
      { x: line.x1, y: line.y1 },
      { x: line.x2, y: line.y2 },
    ]
  })
  dashed.forEach((line, index) => {
    const next = dashed[(index + 1) % dashed.length]
    assert.ok(!crosses(line[0], line[1], next[0], next[1]), `b${index + 1} and the next do not cross`)
  })
})

/** Whether `bond`'s dashed line, drawn on the side bondSides gives it, is nearer `centre` than the bond. */
function dashedToward(view, bond, centre, sides = bondSides(view)) {
  const at = (id) => view.atoms.find((atom) => atom.id === id).position
  const [, line] = bondStrokes(bond, at(bond.begin), at(bond.end), null, null, sides.get(bond.id))
  const middle = { x: (line.x1 + line.x2) / 2, y: (line.y1 + line.y2) / 2 }
  const bondMiddle = { x: (at(bond.begin).x + at(bond.end).x) / 2, y: (at(bond.begin).y + at(bond.end).y) / 2 }
  const toCentre = (point) => Math.hypot(point.x - centre.x, point.y - centre.y)
  return toCentre(middle) < toCentre(bondMiddle)
}

/** Hexagon vertices around `centre`, the first at `start` degrees. */
const hexagon = (centre, start) =>
  Array.from({ length: 6 }, (_, index) => {
    const angle = ((start + index * 60) * Math.PI) / 180
    return { x: centre.x + 30 * Math.cos(angle), y: centre.y + 30 * Math.sin(angle) }
  })

const reversedBonds = (bonds) => bonds.map((bond) => ({ ...bond, begin: bond.end, end: bond.begin }))

/** A benzene-like ring around (100, 100), substituted on r1 and r2, its ring bonds drawn either way. */
function orthoRing(reversed) {
  const centre = { x: 100, y: 100 }
  const ring = hexagon(centre, 0).map((point, index) => atomWith(`r${index + 1}`, 'C', point.x, point.y))
  // Substituents on r1 and r2, pointing away from the ring: their sides cancel out.
  const outside = hexagon(centre, 0)
    .slice(0, 2)
    .map((point, index) => atomWith(`s${index + 1}`, 'C', 2 * point.x - centre.x, 2 * point.y - centre.y))
  const ringBonds = ring.map((atom, index) => bondWith(`b${index + 1}`, atom.id, ring[(index + 1) % 6].id, 'aromatic'))
  const bonds = [...(reversed ? reversedBonds(ringBonds) : ringBonds), bondWith('x1', 'r1', 's1'), bondWith('x2', 'r2', 's2')]
  return { centre, view: { id: 'doc-1', title: 'Ortho', revision: 1, atoms: [...ring, ...outside], bonds } }
}

for (const reversed of [false, true]) {
  const drawn = reversed ? 'drawn end to begin' : 'drawn begin to end'

  test(`an aromatic ring bond between two substituted atoms has its dashed line inside the ring (${drawn})`, () => {
    const { centre, view } = orthoRing(reversed)
    for (const bond of view.bonds.filter((item) => item.order === 'aromatic')) {
      assert.ok(dashedToward(view, bond, centre), `${bond.id}'s dashed line is inside the ring`)
    }
  })

  test(`aromatic bonds of fused rings have their dashed lines inside a ring (${drawn})`, () => {
    // Two hexagons sharing the edge a1–a2, like naphthalene.
    const first = { x: 100, y: 100 }
    const second = { x: 100 + 60 * Math.cos(Math.PI / 6), y: 100 }
    const [a1, a2, a3, a4, a5, a6] = hexagon(first, -30)
    const [, , b3, b4, b5, b6] = hexagon(second, 150)
    const points = { a1, a2, a3, a4, a5, a6, a7: b6, a8: b5, a9: b4, a10: b3 }
    const atoms = Object.entries(points).map(([id, point]) => atomWith(id, 'C', point.x, point.y))
    const rings = {
      first: ['a1', 'a2', 'a3', 'a4', 'a5', 'a6'],
      second: ['a2', 'a1', 'a7', 'a8', 'a9', 'a10'],
    }
    const forward = [
      ...rings.first.map((id, index) => [id, rings.first[(index + 1) % 6]]),
      ...rings.second.slice(1).map((id, index) => [id, rings.second[(index + 2) % 6]]),
    ].map(([begin, end], index) => bondWith(`b${index + 1}`, begin, end, 'aromatic'))
    const bonds = reversed ? reversedBonds(forward) : forward
    const view = { id: 'doc-1', title: 'Fused', revision: 1, atoms, bonds }
    for (const bond of bonds) {
      const inFirst = rings.first.includes(bond.begin) && rings.first.includes(bond.end)
      const inSecond = rings.second.includes(bond.begin) && rings.second.includes(bond.end)
      // The fused bond's dashed line may go into either ring.
      const inside = (inFirst && dashedToward(view, bond, first)) || (inSecond && dashedToward(view, bond, second))
      assert.ok(inside, `${bond.id} (${bond.begin}–${bond.end})'s dashed line is inside its ring`)
    }
  })
}

test('the rings bondSides follows are found once per graph and passed in, not searched for each placement', () => {
  const { centre, view } = orthoRing(true)
  const found = bondRings(view)
  assert.deepEqual([...found.keys()].sort(), ['b1', 'b2', 'b3', 'b4', 'b5', 'b6'], 'the substituent bonds are in no ring')
  assert.deepEqual([...found.get('b1')].sort(), ['r1', 'r2', 'r3', 'r4', 'r5', 'r6'])
  const b1 = view.bonds[0]
  // Another placement moves the atoms but keeps the graph: the rings found once still apply.
  const shift = (atom) => ({ ...atom, position: { x: atom.position.x + 300, y: atom.position.y } })
  const moved = { ...view, atoms: view.atoms.map(shift) }
  assert.ok(dashedToward(moved, b1, { x: centre.x + 300, y: centre.y }, bondSides(moved, undefined, found)))
  // bondSides uses only the rings it is given: told of none, the reversed bond between the two
  // substituted atoms follows its neighbours, whose sides cancel out, and its line goes outside.
  assert.ok(!dashedToward(view, b1, centre, bondSides(view, undefined, new Map())))
  assert.equal(bondRings({ ...view, bonds: view.bonds.slice(1) }).size, 0, 'a chain has no rings')
})

test('atoms alike share one label layout, whatever their identifiers and positions', () => {
  const first = atomLabel(atomWith('a1', 'N', 0, 0, { radical: 'doublet' }), 1)
  assert.equal(atomLabel(atomWith('a7', 'N', 300, 40, { radical: 'doublet' }), 2), first)
  assert.notEqual(atomLabel(atomWith('a1', 'N', 0, 0, { radical: 'triplet' }), 1), first)
})

test('accessible names say which placement they are in when there are several', () => {
  const oxygen = atomWith('a1', 'O', 0, 0)
  assert.equal(atomName(oxygen), 'Atom a1: O')
  assert.equal(atomName(oxygen, 's2'), 'Atom a1 in s2: O')
  assert.equal(bondName(bondWith('b1', 'a1', 'a2')), 'Bond b1: single bond between a1 and a2')
  assert.equal(bondName(bondWith('b1', 'a1', 'a2'), 's2'), 'Bond b1 in s2: single bond between a1 and a2')
})

test('a short bond can be hit between both its bare or labelled atoms', () => {
  for (const elements of [['C', 'C'], ['N', 'O']]) {
    const atoms = [atomWith('a1', elements[0], 0, 0), atomWith('a2', elements[1], 5, 0)]
    const view = { id: 'doc-short', title: 'Short', revision: 1, atoms, bonds: [bondWith('b1', 'a1', 'a2')] }
    const labels = new Map(atoms.map((atom) => [atom.id, atomLabel(atom, 1)]))
    assert.deepEqual(hitTest(view, { x: 2.5, y: 0 }, new Set(), undefined, labels), { kind: 'bond', id: 'b1' }, elements.join('–'))
    assert.deepEqual(hitTest(view, { x: 0, y: 0 }, new Set(), undefined, labels), { kind: 'atom', id: 'a1' })
    assert.deepEqual(hitTest(view, { x: 5, y: 0 }, new Set(), undefined, labels), { kind: 'atom', id: 'a2' })
  }
})

test('a wide bond leaves room for its round caps outside both label boxes', () => {
  const begin = { x: 0, y: 0 }
  const end = { x: 100, y: 0 }
  const left = { x: -4, y: -4, width: 8, height: 8 }
  const right = { x: 96, y: -4, width: 8, height: 8 }
  const [stroke] = bondStrokes(bondWith('b1', 'a1', 'a2'), begin, end, left, right, 1, 1, 10)
  assert.equal(stroke.shape, 'line')
  assert.ok(stroke.x1 - (left.x + left.width) >= 6.4 - 0.01, 'the left round cap clears its label')
  assert.ok(right.x - stroke.x2 >= 6.4 - 0.01, 'the right round cap clears its label')
})

test('a wide short bond cannot draw ink across labels when no room remains', () => {
  const begin = atomLabel(atomWith('a1', 'N', 0, 0), 1).box
  const end = atomLabel(atomWith('a2', 'O', 5, 0), 1).box
  const placedEnd = { ...end, x: end.x + 5 }
  for (const order of ['single', 'double', 'aromatic', 'dative', 'query', 'unknown']) {
    const query = order === 'query' ? { kind: 'any' } : null
    const strokes = bondStrokes(bondWith('b1', 'a1', 'a2', order, query), { x: 0, y: 0 }, { x: 5, y: 0 }, begin, placedEnd, 1, 1, 10)
    assert.deepEqual(strokes, [], `${order} leaves overlapping labels clear`)
  }
})

test('wide bond ink and its selection highlight stay inside the view and placement frame', async () => {
  const { bondSpacing, strokeReach } = await import('../apps/web/src/editor/geometry.ts')
  const positions = [{ x: -100, y: 0 }, { x: -95, y: 0 }]
  const reach = strokeReach(10, bondSpacing(10)) + 3
  const view = viewBox(positions, [], reach)
  const frame = placementFrame(positions, [], 's1', reach)
  assert.ok(view.x <= positions[0].x - reach, 'the view holds the leftmost highlight')
  assert.ok(view.y <= -reach, 'the view holds the upper highlight')
  assert.ok(frame.rect.x <= positions[0].x - reach, 'the placement frame holds the highlight')
  assert.ok(frame.rect.y <= -reach, 'the placement frame holds the upper highlight')
})

const { DEFAULT_SETTINGS, isPreferencesResponse, settingsOf } = await import('../apps/web/src/lib/document-contract.ts')
const settingsEdits = await import('../apps/web/src/editor/settings.ts')
const { DEFAULT_LABEL_STYLE, labelStyleOf } = await import('../apps/web/src/editor/label.ts')
const { bondSpacing, freeNeighborPosition, snappedEndpoint } = await import('../apps/web/src/editor/geometry.ts')

/** A document reply carrying `settings` (left out when undefined, as an older service does). */
function styled(settings) {
  const reply = document('doc-1')
  if (settings !== undefined) reply.document.settings = settings
  return reply
}

test('document settings are checked at the boundary and an older reply has none', () => {
  assert.ok(isDocumentResponse(styled(DEFAULT_SETTINGS)))
  assert.ok(isDocumentResponse(styled(undefined)), 'a service from before settings')
  assert.equal(settingsOf(styled(undefined).document), DEFAULT_SETTINGS, 'and draws with the defaults')
  const custom = { ...DEFAULT_SETTINGS, bond_length_pt: 42.5, label_font: 'monospace', display_unit: 'mm' }
  assert.deepEqual(settingsOf(styled(custom).document), custom)
  for (const bad of [
    { ...DEFAULT_SETTINGS, bond_length_pt: 4.9 },
    { ...DEFAULT_SETTINGS, line_width_pt: 10.5 },
    { ...DEFAULT_SETTINGS, label_size_pt: Number.NaN },
    { ...DEFAULT_SETTINGS, label_font: 'cursive' },
    { ...DEFAULT_SETTINGS, carbon_labels: 'none' },
    { ...DEFAULT_SETTINGS, display_unit: 'px' },
    { ...DEFAULT_SETTINGS, extra: 1 },
    (({ display_unit: _, ...rest }) => rest)(DEFAULT_SETTINGS),
    null,
  ]) {
    assert.equal(isDocumentResponse(styled(bad)), false, JSON.stringify(bad))
  }
})

/** Preferences as the service reports them; `overrides` replaces top-level fields. */
function preferences(overrides = {}) {
  const computation = (followup) => ({ status: 'not_implemented', owner: 'preferences', followup, note: 'Not yet.' })
  return {
    api_version: 'v1',
    revision: 2,
    ui: { theme: 'dark' },
    new_documents: { bond_length_pt: 40 },
    new_document_settings: { ...DEFAULT_SETTINGS, bond_length_pt: 40 },
    computation: { nmr: computation('P66'), numeric_precision: computation('P19') },
    ...overrides,
  }
}

test('a preferences reply is checked whole, computation included', () => {
  assert.ok(isPreferencesResponse(preferences()))
  const withMass = (decimals) => preferences({ computation: {
    ...preferences().computation,
    mass_precision: { status: 'available', owner: 'preferences', decimals },
  } })
  assert.ok(isPreferencesResponse(withMass(0)))
  assert.ok(isPreferencesResponse(withMass(8)))
  assert.equal(isPreferencesResponse(withMass(9)), false)
  assert.equal(isPreferencesResponse(withMass(1.5)), false)
  for (const [name, bad] of [
    ['an unknown theme', preferences({ ui: { theme: 'sepia' } })],
    ['a negative revision', preferences({ revision: -1 })],
    ['a stored value out of range', preferences({ new_documents: { label_size_pt: 100 } })],
    ['an unknown stored field', preferences({ new_documents: { color: 'red' } })],
    ['partial new-document settings', preferences({ new_document_settings: { bond_length_pt: 40 } })],
    ['computation claimed to run', preferences({ computation: { ...preferences().computation, nmr: { ...preferences().computation.nmr, status: 'implemented' } } })],
    ['computation left out', preferences({ computation: undefined })],
    ['another API version', preferences({ api_version: 'v2' })],
  ]) {
    assert.equal(isPreferencesResponse(bad), false, name)
  }
})

test('the client sends preference writes with their revision and refuses an incompatible reply', async () => {
  const sent = []
  const scripted = service({
    'GET /api/v1/preferences': [() => json(200, preferences())],
    '/api/v1/preferences': [() => json(200, preferences({ revision: 3 })), () => json(200, { api_version: 'v1' })],
    '/api/v1/preferences/reset': [() => json(200, preferences({ revision: 4, new_documents: {} }))],
    '/api/v1/preferences/import': [() => json(200, preferences({ revision: 5 }))],
    '/settings/reset': [() => json(200, styled(DEFAULT_SETTINGS))],
    '/settings/import': [() => json(200, styled(DEFAULT_SETTINGS))],
  })
  const fetchScripted = globalThis.fetch
  globalThis.fetch = (path, init = {}) => {
    if (init.body) sent.push({ path, method: init.method, body: JSON.parse(init.body) })
    return fetchScripted(path, init)
  }
  const client = new DocumentClient()
  assert.equal((await client.preferences()).revision, 2)
  const change = { ui: { theme: 'light' }, new_documents: { label_font: 'serif' } }
  assert.equal((await client.setPreferences(2, change)).revision, 3)
  await assert.rejects(client.setPreferences(3, change), (failure) => failure.code === 'incompatible_response')
  await client.resetPreferences(3)
  await client.preferencesFromDocument(4, 'doc-2')
  await client.resetSettings('doc-1', 7, 'preferences')
  await client.importSettings('doc-1', 8, 'doc-2')
  assert.deepEqual(
    sent.map(({ path, method, body: { request_id: _, ...body } }) => [method, path, body]),
    [
      ['PUT', '/api/v1/preferences', { expected_revision: 2, ...change }],
      ['PUT', '/api/v1/preferences', { expected_revision: 3, ...change }],
      ['POST', '/api/v1/preferences/reset', { expected_revision: 3 }],
      ['POST', '/api/v1/preferences/import', { expected_revision: 4, document: 'doc-2' }],
      ['POST', '/api/v1/documents/doc-1/settings/reset', { expected_revision: 7, to: 'preferences' }],
      ['POST', '/api/v1/documents/doc-1/settings/import', { expected_revision: 8, source: 'doc-2' }],
    ],
  )
  assert.equal(scripted.writes.length, 6)
})

test('a refused setting carries its reason whether it was a command or a preference', async () => {
  service({
    '/transactions': [
      () => json(422, { api_version: 'v1', code: 'invalid_command', message: 'm', command_index: 0, reason: 'bond_length_out_of_range' }),
    ],
    '/api/v1/preferences': [() => json(422, { api_version: 'v1', code: 'invalid_request', message: 'm', reason: 'label_size_out_of_range' })],
  })
  const client = new DocumentClient()
  await assert.rejects(client.transact('doc-1', 0, [{ op: 'update_settings', bond_length_pt: 1 }]), (failure) =>
    failure.stateReason === 'bond_length_out_of_range',
  )
  await assert.rejects(client.setPreferences(0, { ui: { theme: 'system' }, new_documents: { label_size_pt: 100 } }), (failure) =>
    failure.code === 'invalid_request' && failure.stateReason === 'label_size_out_of_range',
  )
})

test('lengths are stored in pt and shown in the display unit', () => {
  const { formatLength, fromDisplay, toDisplay } = settingsEdits
  assert.equal(toDisplay(72, 'in'), 1)
  assert.equal(toDisplay(72, 'mm'), 25.4)
  assert.equal(toDisplay(72, 'cm'), 2.54)
  assert.equal(toDisplay(30, 'pt'), 30)
  assert.equal(formatLength(30, 'mm'), '10.58 mm')
  assert.equal(formatLength(1.2, 'in'), '0.017 in')
  // Exact halves in the unit round up, whatever error the conversion leaves: 45 pt is 15.875 mm,
  // 9 pt is 0.125 in.
  assert.equal(toDisplay(45, 'mm'), 15.88)
  assert.equal(toDisplay(9, 'in'), 0.125)
  assert.equal(toDisplay(36.9, 'pt'), 36.9)
  assert.equal(fromDisplay(25.4, 'mm'), 72)
  assert.equal(fromDisplay(1, 'cm'), 28.3465)
  // A shown value is rounded to the unit's decimals, so it converts back within half of its last
  // digit: 0.0005 cm is 0.0142 pt.
  const back = fromDisplay(toDisplay(42.5, 'cm'), 'cm')
  assert.ok(Math.abs(back - 42.5) <= 0.0142, `${back}`)
})

test('a typed length is read in the display unit, its shown bounds are accepted, and the shown value sends nothing', () => {
  const { typedSetting } = settingsEdits
  const set = (pt) => ({ kind: 'set', pt })
  // The bounds as shown in each unit are the bounds.
  assert.deepEqual(typedSetting('bond_length_pt', '1.76', 30, 'mm'), set(5))
  assert.deepEqual(typedSetting('bond_length_pt', '70.56', 30, 'mm'), set(200))
  assert.deepEqual(typedSetting('line_width_pt', '3.53', 1.2, 'mm'), set(10))
  assert.deepEqual(typedSetting('line_width_pt', '0.001', 1.2, 'in'), set(0.1))
  assert.deepEqual(typedSetting('line_width_pt', '0.139', 1.2, 'in'), set(10))
  assert.deepEqual(typedSetting('label_size_pt', '72', 12, 'pt'), set(72))
  // Beyond the rounding of a bound, a value is out of range.
  assert.deepEqual(typedSetting('bond_length_pt', '1.75', 30, 'mm'), { kind: 'invalid' })
  assert.deepEqual(typedSetting('bond_length_pt', '70.57', 30, 'mm'), { kind: 'invalid' })
  for (const text of ['', ' ', 'abc', '1e999', 'NaN']) {
    assert.deepEqual(typedSetting('bond_length_pt', text, 30, 'mm'), { kind: 'invalid' }, JSON.stringify(text))
  }
  // The value shown, however it is written, is no change: 30 pt is shown as 10.58 mm.
  for (const text of ['10.58', '10.580', ' 10.58 ', '1.058e1']) {
    assert.deepEqual(typedSetting('bond_length_pt', text, 30, 'mm'), { kind: 'unchanged' }, text)
  }
  assert.deepEqual(typedSetting('bond_length_pt', '20', 30, 'mm'), set(56.6929))
  assert.deepEqual(typedSetting('bond_length_pt', '45', 30, 'pt'), set(45))
})

/** The document `settings` applies to, as the editor builds edits from the latest one. */
const withSettings = (settings) => ({ ...styled(settings).document, settings })

test('settings edits are built from the latest document and skip what no longer applies', () => {
  const { setSettings, stepSetting } = settingsEdits
  const latest = withSettings({ ...DEFAULT_SETTINGS, bond_length_pt: 40 })
  assert.deepEqual(stepSetting('bond_length_pt', 1)(latest), [{ op: 'update_settings', bond_length_pt: 41 }])
  assert.deepEqual(stepSetting('line_width_pt', -1)(latest), [{ op: 'update_settings', line_width_pt: 1.1 }])
  assert.deepEqual(
    stepSetting('line_width_pt', 1)(withSettings({ ...DEFAULT_SETTINGS, line_width_pt: 0.3 })),
    [{ op: 'update_settings', line_width_pt: 0.4 }],
    'steps do not accumulate floating-point error',
  )
  assert.equal(stepSetting('label_size_pt', 1)(withSettings({ ...DEFAULT_SETTINGS, label_size_pt: 72 })), null, 'at the limit')
  assert.equal(stepSetting('bond_length_pt', -1)(withSettings({ ...DEFAULT_SETTINGS, bond_length_pt: 5.5 })), null)
  assert.deepEqual(setSettings({ bond_length_pt: 40, label_font: 'serif' })(latest), [{ op: 'update_settings', label_font: 'serif' }])
  assert.equal(setSettings({ bond_length_pt: 40 })(latest), null, 'already so')
  assert.equal(setSettings({ bond_length_pt: 300 })(latest), null, 'out of range')
  assert.equal(setSettings({ label_font: 'serif' })(styled(undefined).document), null, 'a service without settings')
})

test('labels scale with the label size and take the label font', () => {
  const atom = atomWith('a1', 'O', 0, 0, { isotope: 18, charge: -1, hydrogens: { mode: 'specified', count: 2 }, radical: 'doublet', attachment_points: [1] })
  const base = atomLabel(atom, 1)
  assert.equal(atomLabel(atom, 1, false, DEFAULT_LABEL_STYLE), base, 'the default style is the built-in layout')
  const twice = atomLabel(atom, 1, false, { ...DEFAULT_LABEL_STYLE, size: 24 })
  const scaled = (box) => ({ x: box.x * 2, y: box.y * 2, width: box.width * 2, height: box.height * 2 })
  const close = (a, b) => Object.keys(a).every((key) => Math.abs(a[key] - b[key]) < 1e-9)
  assert.ok(close(twice.box, scaled(base.box)), JSON.stringify({ twice: twice.box, base: base.box }))
  assert.deepEqual(twice.chunks[0].runs.map((run) => run.size), base.chunks[0].runs.map((run) => run.size * 2))
  assert.deepEqual(twice.dots.map((dot) => dot.radius), base.dots.map((dot) => dot.radius * 2))
  const mono = atomLabel(atomWith('a1', 'W', 0, 0), 0, false, { ...DEFAULT_LABEL_STYLE, font: 'monospace' })
  assert.equal(mono.font, 'monospace')
  assert.ok(mono.box.width < atomLabel(atomWith('a1', 'W', 0, 0), 0).box.width, 'a monospace W is narrower than Geist’s')
  const serif = atomLabel(atomWith('a1', 'W', 0, 0), 0, false, { ...DEFAULT_LABEL_STYLE, font: 'serif' })
  assert.ok(serif.box.width > mono.box.width, 'each font reserves its own widths')
  assert.deepEqual(
    labelStyleOf({ ...DEFAULT_SETTINGS, label_font: 'serif', label_size_pt: 9, carbon_labels: 'all' }),
    { font: 'serif', size: 9, carbons: 'all' },
  )
})

test('carbon labels follow the document setting', () => {
  const vertex = atomWith('a1', 'C', 0, 0)
  assert.equal(showsLabel(vertex, 2), false)
  assert.equal(showsLabel(vertex, 2, 'all'), true)
  assert.equal(atomLabel(vertex, 2), null)
  assert.deepEqual(labelText(atomLabel(vertex, 2, false, { ...DEFAULT_LABEL_STYLE, carbons: 'all' })), ['C'])
})

test('new bonds take the document bond length, and wider lines spread multiple bonds', () => {
  assert.deepEqual(snappedEndpoint({ x: 0, y: 0 }, { x: 10, y: 0.5 }, 45), { x: 45, y: 0 })
  assert.deepEqual(snappedEndpoint({ x: 0, y: 0 }, { x: 10, y: 0.5 }), { x: 30, y: 0 }, 'the built-in default')
  const view = { id: 'doc-1', title: 'B', revision: 0, atoms: [atomWith('a1', 'C', 0, 0)], bonds: [] }
  const free = freeNeighborPosition(view, 'a1', 50)
  assert.ok(Math.abs(Math.hypot(free.x, free.y) - 50) < 0.01, JSON.stringify(free))
  assert.equal(bondSpacing(1.2), 1)
  assert.equal(bondSpacing(0.5), 1, 'thin lines keep the built-in spacing')
  assert.equal(bondSpacing(3.6), 3)
  const at = (x) => ({ x, y: 0 })
  const offsets = (spacing) =>
    bondStrokes(bondWith('b1', 'a1', 'a2', 'double'), at(0), at(30), null, null, 1, spacing).map((stroke) => stroke.y1)
  assert.deepEqual(offsets(1), [-2.2, 2.2])
  assert.deepEqual(offsets(3), [-6.6, 6.6])
})

test('new-document preferences are built from the latest preferences and store what differs from the defaults', () => {
  const { preferenceChange, preferenceStep } = settingsEdits
  const latest = preferences()
  assert.deepEqual(preferenceChange(latest, { label_font: 'serif' }), {
    ui: { theme: 'dark' },
    new_documents: { bond_length_pt: 40, label_font: 'serif' },
  })
  assert.deepEqual(preferenceChange(latest, { bond_length_pt: 30 }), { ui: { theme: 'dark' }, new_documents: {} }, 'back to the default')
  assert.equal(preferenceChange(latest, { bond_length_pt: 40 }), null, 'already so')
  assert.equal(preferenceChange(latest, { line_width_pt: 0 }), null, 'out of range')
  assert.deepEqual(preferenceStep(latest, 'bond_length_pt', 1), { ui: { theme: 'dark' }, new_documents: { bond_length_pt: 41 } })
  const atLimit = preferences({ new_document_settings: { ...DEFAULT_SETTINGS, label_size_pt: 4 } })
  assert.equal(preferenceStep(atLimit, 'label_size_pt', -1), null)
})


test('an unanswered import reuses the request ID against the same service', async () => {
  const scripted = service({ '/api/v1/operations': [lost, () => json(200, {
    outcome: 'partial', result: document('doc-1', 1), diagnostics: [], loss_report: [],
  })] })
  const client = new DocumentClient()
  const input = { documentId: 'doc-1', revision: 0, format: 'mol', source: 'source', record: 1, digest: 'a'.repeat(64) }
  await assert.rejects(client.importCt(input), { code: 'unreachable' })
  const result = await client.importCt(input)
  assert.equal(result.document.revision, 1)
  assert.equal(scripted.writes.length, 2)
  assert.equal(scripted.writes[0].requestId, scripted.writes[1].requestId)
})

test('conversion failures retain their record and line context', async () => {
  service({ '/api/v1/operations': [() => json(200, { outcome: 'error', result: null,
    diagnostics: [{ code: 'data_line_too_long', message: 'Too long.', details: { record: 2, line: 13 } }], loss_report: [] })] })
  const client = new DocumentClient()
  await assert.rejects(client.previewCt('doc-1', 0, 'sdf', 'source', 1), {
    code: 'data_line_too_long', message: 'Too long. Record 2. Line 13.',
  })
})


test('an import preview exposes the actual graph even when a revision is reused', async () => {
  const actual = document('doc-1', 1)
  actual.document.atoms = [{ id: 'a1', element: 'O', charge: 0, isotope: null, position: { x: 0, y: 0 } }]
  service({ '/api/v1/operations': [() => json(200, { outcome: 'partial', diagnostics: [], loss_report: [],
    result: { selected_record: 1, preview_digest: 'a'.repeat(64), records: [{ title: '', atoms: [], bonds: [], fields: [] }],
      snapshot: actual, document: actual.document } })] })
  const client = new DocumentClient()
  const preview = await client.previewCt('doc-1', 1, 'mol', 'source', 1)
  const shown = structuredClone(preview.snapshot)
  shown.atoms[0].element = 'C'
  assert.equal(shown.id, preview.snapshot.id)
  assert.equal(shown.revision, preview.snapshot.revision)
  assert.equal(sameConversionSnapshot(shown, preview.snapshot), false, 'same ID and revision must not authorize a different graph')
})

test('SMILES refusals say where in characters and bytes, and a consent refusal carries what to accept', async () => {
  const refusal = (code, message, details) => () => json(200, { outcome: 'error', result: null, loss_report: [],
    diagnostics: [{ code, message, details }] })
  service({ '/api/v1/operations': [
    refusal('unexpected_character', 'This character cannot appear here.',
      { class: 'syntax', offset: 1, length: 2, character_offset: 1, character_length: 1 }),
    refusal('aromatic_system_unsupported', 'Outside the model.',
      { class: 'unsupported', offset: 0, length: 1, character_offset: 0, character_length: 1, reason: 'not_conjugated' }),
    refusal('empty_input', 'The SMILES text is empty.',
      { class: 'syntax', offset: 0, length: 0, character_offset: 0, character_length: 0 }),
    refusal('loss_consent_required', 'Accept the losses.', {
      features: ['radical_state'],
      losses: [{ feature: 'radical_state', source_path: 'source[0..=3]', disposition: 'dropped', reason: 'Unknown.', semantic_impact: 'chemistry' }],
    }),
    refusal('loss_consent_required', 'Accept the assumptions.', {
      features: ['hydrogen_assumption'], loss_digest: 'f'.repeat(64),
      losses: [{ feature: 'hydrogen_assumption', source_path: 'atoms[a1]', disposition: 'unknown', reason: 'No model.', semantic_impact: 'chemistry' }],
    }),
  ] })
  const client = new DocumentClient()
  await assert.rejects(client.previewSmiles('doc-1', 0, 'C\u00e9O'), {
    code: 'unexpected_character',
    message: 'This character cannot appear here. Problem at character 2 (bytes 1 to 2).',
  })
  await assert.rejects(client.previewSmiles('doc-1', 0, 'c1cccc1'), {
    code: 'aromatic_system_unsupported',
    message: 'Outside the model. Problem at character 1 (byte 0). Reason: not_conjugated.',
  })
  await assert.rejects(client.previewSmiles('doc-1', 0, ''), {
    message: 'The SMILES text is empty. Problem at the position before character 1 (byte 0).',
  })
  const commit = { documentId: 'doc-1', revision: 0, source: '[CH2]', digest: 'a'.repeat(64), acceptLoss: false }
  const imported = await client.importSmiles(commit).catch((failure) => failure)
  assert.ok(imported instanceof ConsentRequired, 'an import refusal that asks for consent is a ConsentRequired')
  assert.deepEqual(imported.features, ['radical_state'])
  assert.equal(imported.losses[0].feature, 'radical_state')
  assert.equal(imported.digest, undefined)
  const written = await client.exportSmiles('doc-1', 'recorded').catch((failure) => failure)
  assert.ok(written instanceof ConsentRequired)
  assert.equal(written.digest, 'f'.repeat(64), 'an export refusal carries the digest to accept')
})

test('a SMILES preview of a different drawing or revision is refused', async () => {
  const other = document('doc-2', 0)
  service({ '/api/v1/operations': [() => json(200, { outcome: 'partial', diagnostics: [], loss_report: [], model_id: 'chemifree-smiles-1',
    result: { atoms: [], bonds: [], ignored_text: null, preview_digest: 'a'.repeat(64), snapshot: other, document: other.document,
      valence_model: 'v', aromaticity_model: 'a' } })] })
  const client = new DocumentClient()
  await assert.rejects(client.previewSmiles('doc-1', 0, 'C'), { code: 'incompatible_response' })
})

test('a conversion snapshot is the same drawing whatever its key order or absent fields', () => {
  const first = document('doc-1', 1).document
  const reordered = Object.fromEntries(Object.entries(first).reverse())
  reordered.atoms = first.atoms.map((atom) => Object.fromEntries(Object.entries(atom).reverse()))
  assert.notEqual(JSON.stringify(reordered), JSON.stringify(first), 'the fixture really differs as text')
  assert.equal(sameConversionSnapshot(reordered, first), true)
  assert.equal(sameConversionSnapshot({ ...first, note: undefined }, first), true, 'a key holding undefined is absent')
  const changed = structuredClone(first)
  changed.atoms = [...changed.atoms, { id: 'a9', element: 'C', charge: 0, isotope: null, position: { x: 0, y: 0 } }]
  assert.equal(sameConversionSnapshot(changed, first), false)
  const moved = structuredClone(first)
  moved.title = 'Other'
  assert.equal(sameConversionSnapshot(moved, first), false)
  assert.equal(sameConversionSnapshot({ ...first, atoms: [...first.atoms].reverse(), bonds: first.bonds }, { ...first, atoms: [{ id: 'a2' }, { id: 'a1' }], bonds: first.bonds }), false, 'array order matters')
})

test('a conversion snapshot compares as JSON values: undefined is absent, null is not', () => {
  const first = document('doc-1', 1).document
  // JSON drops a key holding undefined, so it equals an absent key, in either direction.
  assert.equal(sameConversionSnapshot({ ...first, note: undefined }, first), true)
  assert.equal(sameConversionSnapshot(first, { ...first, note: undefined }), true)
  assert.equal(sameConversionSnapshot({ a: { b: undefined } }, { a: {} }), true)
  // null is a value: it differs from an absent key and from undefined.
  assert.equal(sameConversionSnapshot({ ...first, note: null }, first), false)
  assert.equal(sameConversionSnapshot(first, { ...first, note: null }), false)
  assert.equal(sameConversionSnapshot({ ...first, note: null }, { ...first, note: undefined }), false)
  assert.equal(sameConversionSnapshot({ ...first, note: null }, { ...first, note: null }), true)
  // In an array JSON turns undefined into null, so the position is kept.
  assert.equal(sameConversionSnapshot({ list: [undefined, 1] }, { list: [null, 1] }), true)
  assert.equal(sameConversionSnapshot({ list: [undefined, 1] }, { list: [1] }), false)
  assert.equal(sameConversionSnapshot({ list: [] }, { list: {} }), false)
  // A number that JSON cannot hold is null there.
  assert.equal(sameConversionSnapshot({ x: Number.NaN }, { x: null }), true)
  assert.equal(sameConversionSnapshot({ x: Number.NaN }, { x: 0 }), false)
  // Empty is not absent: an empty object or array is a value.
  assert.equal(sameConversionSnapshot({ ...first, note: {} }, first), false)
})

test('a conversion snapshot comparison stops at a bounded depth and calls deeper values different', () => {
  const nested = (levels, leaf) => {
    let value = leaf
    for (let level = 0; level < levels; level += 1) value = { next: value }
    return value
  }
  // 64 levels of objects put the leaf at depth 64, which is still compared.
  assert.equal(sameConversionSnapshot(nested(64, 1), nested(64, 1)), true)
  assert.equal(sameConversionSnapshot(nested(64, 1), nested(64, 2)), false)
  // One level more is past the bound: different, even when every value is the same.
  assert.equal(sameConversionSnapshot(nested(65, 1), nested(65, 1)), false)
  const shared = nested(65, 1)
  assert.equal(sameConversionSnapshot({ shared }, { shared }), true, 'one object is equal to itself without being walked')
  // Arrays count as levels, and a cyclic value ends instead of looping.
  const deepArray = (levels) => Array.from({ length: levels }).reduce((value) => [value], 1)
  assert.equal(sameConversionSnapshot({ a: deepArray(62) }, { a: deepArray(62) }), true)
  assert.equal(sameConversionSnapshot({ a: deepArray(70) }, { a: deepArray(70) }), false)
  const loop = () => {
    const cyclic = { id: 'a' }
    cyclic.self = cyclic
    return cyclic
  }
  assert.equal(sameConversionSnapshot(loop(), loop()), false)
})

test('a snapshot matcher compares each distinct snapshot with the drawing once', () => {
  const shown = document('doc-1', 1).document
  let reads = 0
  const counted = (value) => new Proxy(value, { ownKeys(target) { reads += 1; return Reflect.ownKeys(target) } })
  const same = counted(structuredClone(shown))
  const changed = counted({ ...structuredClone(shown), title: 'Other' })
  const matches = conversionSnapshotMatcher(shown)
  assert.equal(matches(same), true)
  const afterFirst = reads
  assert.ok(afterFirst > 0, 'the comparison really read the snapshot')
  assert.equal(matches(same), true)
  assert.equal(matches(same), true)
  assert.equal(reads, afterFirst, 'asking again about the same snapshot does not compare it again')
  assert.equal(matches(changed), false)
  const afterSecond = reads
  assert.equal(matches(changed), false)
  assert.equal(reads, afterSecond)
  // A new drawing starts over: the answer was about the old one.
  const other = conversionSnapshotMatcher({ ...shown, title: 'Other' })
  assert.equal(other(same), false)
  assert.equal(other(changed), true)
})

test('an unanswered SMILES import is an open outcome and is retried with the same request ID', async () => {
  const scripted = service({ '/api/v1/operations': [lost, () => json(200, {
    outcome: 'partial', result: document('doc-1', 1), diagnostics: [], loss_report: [], model_id: 'chemifree-smiles-1',
  })] })
  const client = new DocumentClient()
  const input = { documentId: 'doc-1', revision: 0, source: 'CCO', digest: 'a'.repeat(64), acceptLoss: false }
  const failure = await client.importSmiles(input).catch((error) => error)
  assert.equal(failure.code, 'unreachable')
  assert.equal(outcomeOpen(failure), true, 'a lost reply leaves the outcome open')
  const result = await client.importSmiles(input)
  assert.equal(result.document.revision, 1)
  assert.equal(scripted.writes.length, 2)
  assert.equal(scripted.writes[0].requestId, scripted.writes[1].requestId, 'the retry is the same request')
  // A refusal is definite: the next attempt is a new request.
  const refused = service({ '/api/v1/operations': [() => json(200, { outcome: 'error', result: null, loss_report: [],
    diagnostics: [{ code: 'preview_changed', message: 'Changed.', details: {} }] }), () => json(200, {
    outcome: 'partial', result: document('doc-1', 1), diagnostics: [], loss_report: [], model_id: 'chemifree-smiles-1' })] })
  const second = new DocumentClient()
  assert.equal(outcomeOpen(await second.importSmiles(input).catch((error) => error)), false)
  await second.importSmiles(input)
  assert.notEqual(refused.writes[0].requestId, refused.writes[1].requestId)
})


// ---------------------------------------------------------------------------------------------
// Stereo: marks on bonds and stated configurations, as the service sends them. Representation
// only; the editor draws none of it.
// ---------------------------------------------------------------------------------------------

/** a1 bonded to a2, a3, a4 (a center) and an alkene a5=a6 with a7 on a5 and a8 on a6. */
function stereoDrawing(id = 'doc-stereo', revision = 4) {
  const value = document(id, revision)
  value.document.atoms = Array.from({ length: 8 }, (_, index) => atomWith(`a${index + 1}`, 'C', index * 30, 0))
  value.document.bonds = [
    { ...bondWith('b1', 'a1', 'a2'), mark: { kind: 'wedge', narrow: 'end' } },
    { ...bondWith('b2', 'a1', 'a3'), mark: { kind: 'hash', narrow: 'begin' } },
    { ...bondWith('b3', 'a1', 'a4'), mark: { kind: 'wavy' } },
    { ...bondWith('b4', 'a5', 'a6', 'double'), mark: { kind: 'crossed' } },
    { ...bondWith('b5', 'a5', 'a7'), mark: null },
    { ...bondWith('b6', 'a6', 'a8'), mark: null },
  ]
  value.document.stereo = {
    atoms: [{ atom: 'a1', configuration: { kind: 'tetrahedral', neighbors: ['a2', 'a3', 'a4', 'implicit_hydrogen'], turn: 'anticlockwise' } }],
    bonds: [{ bond: 'b4', configuration: { kind: 'relative', begin_neighbor: 'a7', end_neighbor: 'a8', relation: 'opposite_sides' } }],
  }
  return value
}

test('a reply holding marks and stated configurations is accepted as sent', () => {
  assert.equal(isDocumentResponse(stereoDrawing()), true)
  const unknown = stereoDrawing()
  unknown.document.stereo = {
    atoms: [{ atom: 'a1', configuration: { kind: 'unknown', neighbors: ['a2', 'a3', 'a4', 'lone_pair'] } }],
    bonds: [{ bond: 'b4', configuration: { kind: 'unknown' } }],
  }
  assert.equal(isDocumentResponse(unknown), true, 'unknown is an explicit record, with a lone pair as a slot')
})

const { sameContent } = await import('../apps/web/src/hooks/use-document-editor.ts')

test('a reopened copy counts as the same content only when its stereo configurations match', () => {
  const shown = stereoDrawing().document
  assert.equal(sameContent(structuredClone(shown), shown), true)
  const lostAll = { ...structuredClone(shown), stereo: { atoms: [], bonds: [] } }
  assert.equal(sameContent(lostAll, shown), false, 'both configurations missing from the reopened copy')
  const omitted = structuredClone(shown)
  delete omitted.stereo
  assert.equal(sameContent(omitted, shown), false, 'an omitted table is the empty one')
  const lostAtom = structuredClone(shown)
  lostAtom.stereo.atoms = []
  assert.equal(sameContent(lostAtom, shown), false, 'the atom configuration is missing')
  const lostBond = structuredClone(shown)
  lostBond.stereo.bonds = []
  assert.equal(sameContent(lostBond, shown), false, 'the bond configuration is missing')
  const turned = structuredClone(shown)
  turned.stereo.atoms[0].configuration.turn = 'clockwise'
  assert.equal(sameContent(turned, shown), false, 'a different stated turn')
  const bare = { ...structuredClone(shown), stereo: { atoms: [], bonds: [] } }
  const plain = structuredClone(bare)
  delete plain.stereo
  assert.equal(sameContent(plain, bare), true, 'an omitted table equals an empty one')
})

test('an older reply with no marks and no stereo table is accepted and filled in', async () => {
  const older = stereoDrawing()
  delete older.document.stereo
  older.document.bonds = older.document.bonds.map(({ mark, ...bond }) => bond)
  assert.equal(isDocumentResponse(older), true)
  service({ '/transactions': [() => json(200, older)] })
  const reply = await new DocumentClient().transact('doc-stereo', 0, [])
  assert.deepEqual(reply.document.bonds.map((bond) => bond.mark), [null, null, null, null, null, null])
  assert.deepEqual(reply.document.stereo, { atoms: [], bonds: [], groups: [] })
  assert.deepEqual(reply.notes, [], 'an older service reports no notes')
})

const clearedNote = (path = 'atoms[a1]') => ({
  feature: 'stereo_configuration', source_path: path, disposition: 'dropped', semantic_impact: 'stereo',
  reason: 'The stereo configuration of atom a1 was cleared because its bonded neighbors changed. Undoing this edit, while it is still in the history, brings it back.',
})

test('the notes a transaction reply carries are read, and a malformed note is refused', async () => {
  const withNotes = stereoDrawing()
  withNotes.notes = [clearedNote(), clearedNote('bonds[b4]')]
  assert.equal(isDocumentResponse(withNotes), true)
  service({ '/transactions': [() => json(200, withNotes)] })
  const reply = await new DocumentClient().transact('doc-stereo', 0, [{ op: 'delete_atoms', atoms: ['a2'] }])
  assert.deepEqual(reply.notes.map((note) => note.source_path), ['atoms[a1]', 'bonds[b4]'])
  const refused = (change) => {
    const value = stereoDrawing()
    value.notes = [clearedNote()]
    change(value.notes[0])
    return isDocumentResponse(value)
  }
  const cases = {
    'a note about another feature': (n) => { n.feature = 'hydrogen_state' },
    'a note that is not a drop': (n) => { n.disposition = 'normalized' },
    'a note with another impact': (n) => { n.semantic_impact = 'chemistry' },
    'a note with no reason': (n) => { delete n.reason },
    'a note with a field the contract does not have': (n) => { n.consent = 'required' },
  }
  for (const [why, change] of Object.entries(cases)) assert.equal(refused(change), false, why)
  const notAList = stereoDrawing()
  notAList.notes = clearedNote()
  assert.equal(isDocumentResponse(notAList), false, 'notes is a list')
})

const groupRow = (id, kind, stored, members) => {
  const number = kind === 'absolute' ? null : (stored ?? 0)
  return { id, kind, stored_number: stored, number, label: number === null ? 'abs' : `${kind}${number}`, members }
}

// A drawing with two stored tetrahedral centers, a1 and a5, and a snapshot's group list.
function groupedDrawing(groups) {
  const value = stereoDrawing()
  value.document.bonds = [
    ...value.document.bonds.slice(0, 3),
    bondWith('b4', 'a5', 'a6'), bondWith('b5', 'a5', 'a7'), bondWith('b6', 'a5', 'a8'),
  ].map((bond) => ({ ...bond, mark: null }))
  value.document.stereo = {
    atoms: [
      { atom: 'a1', configuration: { kind: 'tetrahedral', neighbors: ['a2', 'a3', 'a4', 'implicit_hydrogen'], turn: 'anticlockwise' } },
      { atom: 'a5', configuration: { kind: 'tetrahedral', neighbors: ['a6', 'a7', 'a8', 'implicit_hydrogen'], turn: 'clockwise' } },
    ],
    bonds: [],
    groups,
  }
  return value
}

test('a snapshot with enhanced stereo groups is accepted and carried as sent', async () => {
  assert.equal(isDocumentResponse(groupedDrawing([])), true, 'an empty list is the always-present form')
  const reply = groupedDrawing([groupRow('e1', 'or', 2, ['a1']), groupRow('e3', 'and', null, ['a5'])])
  assert.equal(isDocumentResponse(reply), true, 'a missing number reads as 0, label and0')
  const odd = groupedDrawing([groupRow('e1', 'or', -3, ['a1']), groupRow('e2', 'or', 0, ['a5'])])
  assert.equal(isDocumentResponse(odd), true, 'negative and zero numbers are kept as stated')
  const bounds = groupedDrawing([groupRow('e1', 'or', 2147483647, ['a1']), groupRow('e2', 'or', -2147483648, ['a5'])])
  assert.equal(isDocumentResponse(bounds), true, 'the 32-bit bounds')
  assert.equal(isDocumentResponse(groupedDrawing([groupRow('e1', 'absolute', null, ['a1', 'a5'])])), true)
  service({ '/transactions': [() => json(200, reply)] })
  const read = await new DocumentClient().transact('doc-stereo', 0, [])
  assert.deepEqual(read.document.stereo.groups, reply.document.stereo.groups, 'the list is carried to the page')
})

test('a snapshot whose enhanced stereo groups are inconsistent is refused as a whole', () => {
  const refused = (why, groups) => assert.equal(isDocumentResponse(groupedDrawing(groups)), false, why)
  const good = () => [groupRow('e1', 'or', 2, ['a1']), groupRow('e2', 'and', null, ['a5'])]
  assert.equal(isDocumentResponse(groupedDrawing(good())), true)
  const mutate = (change) => { const groups = good(); change(groups); return groups }
  refused('not a list', 'e1')
  refused('ids not ascending', [groupRow('e2', 'or', 1, ['a1']), groupRow('e1', 'or', 2, ['a5'])])
  refused('a repeated id', [groupRow('e1', 'or', 1, ['a1']), groupRow('e1', 'and', 1, ['a5'])])
  refused('not a group id', mutate((g) => { g[0].id = 'a1' }))
  refused('a leading zero in the id', mutate((g) => { g[0].id = 'e01' }))
  refused('the short label is not a kind', mutate((g) => { g[0].kind = 'abs' }))
  refused('an unknown kind', mutate((g) => { g[0].kind = 'racemic' }))
  refused('a fractional number', mutate((g) => { g[0].stored_number = 1.5; g[0].number = 1.5; g[0].label = 'or1.5' }))
  refused('a number beyond 32 bits', mutate((g) => { g[0].stored_number = 2147483648; g[0].number = 2147483648; g[0].label = 'or2147483648' }))
  refused('a numeric string', mutate((g) => { g[0].stored_number = '2' }))
  refused('an interpreted number that is not the stored one', mutate((g) => { g[0].number = 3 }))
  refused('an omitted number that does not read as 0', mutate((g) => { g[1].number = null }))
  refused('an absolute group with a stored number', [groupRow('e1', 'absolute', 1, ['a1'])])
  refused('an absolute group that still has a number', mutate((g) => { g[0].kind = 'absolute'; g[0].stored_number = null }))
  refused('a label that is not the derived one', mutate((g) => { g[0].label = 'OR 2' }))
  refused('a missing field', mutate((g) => { delete g[0].label }))
  refused('a field the contract does not have', mutate((g) => { g[0].ratio = '1:1' }))
  refused('no members', mutate((g) => { g[0].members = [] }))
  refused('members not ascending', [groupRow('e1', 'or', 1, ['a5', 'a1'])])
  refused('a repeated member', [groupRow('e1', 'or', 1, ['a1', 'a1'])])
  refused('a member in two groups', [groupRow('e1', 'or', 1, ['a1', 'a5']), groupRow('e2', 'and', null, ['a5'])])
  refused('a member that is not an atom', mutate((g) => { g[0].members = ['a99'] }))
  refused('a member without a stored configuration', mutate((g) => { g[0].members = ['a2'] }))
  const unknownCenter = groupedDrawing(good())
  unknownCenter.document.stereo.atoms[1].configuration = { kind: 'unknown', neighbors: ['a6', 'a7', 'a8', 'implicit_hydrogen'] }
  assert.equal(isDocumentResponse(unknownCenter), false, 'a member whose configuration is unknown')
  refused('a bond as a member', mutate((g) => { g[0].members = ['b1'] }))
  refused('two absolute groups', [groupRow('e1', 'absolute', null, ['a1']), groupRow('e2', 'absolute', null, ['a5'])])
  refused('one kind and number twice', [groupRow('e1', 'or', 1, ['a1']), groupRow('e2', 'or', 1, ['a5'])])
  refused('an omitted number and a 0 of one kind', [groupRow('e1', 'or', null, ['a1']), groupRow('e2', 'or', 0, ['a5'])])
  // Other validators keep their strictness: an unknown key beside the lists is still refused.
  const extra = groupedDrawing(good())
  extra.document.stereo.ratio = 1
  assert.equal(isDocumentResponse(extra), false)
  const omitted = groupedDrawing(good())
  delete omitted.document.stereo.groups
  assert.equal(isDocumentResponse(omitted), true, 'an older service sends no list')
})

test('a snapshot from an older service, with no group list, is the same table as one with an empty list', async () => {
  const current = groupedDrawing([]).document
  const older = structuredClone(current)
  delete older.stereo.groups
  assert.equal(sameContent(older, current), true, 'queued edits are retried across an upgrade')
  assert.equal(sameInspectionSnapshot(older, current), true, 'an inspection of the older snapshot is not stale')
  assert.equal(sameInspectionSnapshot(current, older), true)
  const bare = { ...structuredClone(current), stereo: { atoms: [], bonds: [], groups: [] } }
  const none = structuredClone(bare)
  delete none.stereo
  assert.equal(sameContent(none, bare), true, 'no table and an empty one with an empty list')
  const grouped = groupedDrawing([groupRow('e1', 'or', 1, ['a1'])]).document
  assert.equal(sameContent(older, grouped), false, 'a real group is still a difference')
  const reply = groupedDrawing([])
  delete reply.document.stereo.groups
  service({ '/transactions': [() => json(200, reply)] })
  const read = await new DocumentClient().transact('doc-stereo', 0, [])
  assert.deepEqual(read.document.stereo.groups, [], 'an omitted list is read as no groups')
})

test('every group set the domain accepts is accepted', () => {
  // Derived from the domain rules (crates/domain/src/stereo_group.rs, `validate`, which also
  // runs when a document is restored): non-empty groups, members stored tetrahedral centers in
  // one group each, at most one absolute group, and one group for each kind and interpreted
  // number among `and` and `or`. Nothing else is required of a group set.
  const accepted = {
    'no groups': [],
    'a singleton group': [groupRow('e1', 'or', 1, ['a1'])],
    'one group of all the centers': [groupRow('e1', 'and', 1, ['a1', 'a5'])],
    'absolute with and and or of the same number': [groupRow('e1', 'absolute', null, ['a1']), groupRow('e2', 'and', null, ['a5'])],
    'and and or both reading 0': [groupRow('e1', 'and', null, ['a1']), groupRow('e2', 'or', 0, ['a5'])],
    'the same number of different kinds': [groupRow('e1', 'and', 3, ['a1']), groupRow('e2', 'or', 3, ['a5'])],
    'negative and zero numbers of one kind': [groupRow('e1', 'or', -1, ['a1']), groupRow('e2', 'or', 0, ['a5'])],
    'ids that skip, later than the first': [groupRow('e4', 'or', 1, ['a1']), groupRow('e17', 'or', 2, ['a5'])],
  }
  for (const [why, groups] of Object.entries(accepted)) assert.equal(isDocumentResponse(groupedDrawing(groups)), true, why)
  // Members order numerically (a10 after a2), as the service lists atoms.
  const wide = groupedDrawing([groupRow('e1', 'or', 1, ['a2', 'a10'])])
  const stereo = wide.document.stereo
  stereo.atoms = [
    { atom: 'a2', configuration: { kind: 'tetrahedral', neighbors: ['a3', 'a4', 'a5', 'implicit_hydrogen'], turn: 'clockwise' } },
    { atom: 'a10', configuration: { kind: 'tetrahedral', neighbors: ['a11', 'a12', 'a13', 'implicit_hydrogen'], turn: 'clockwise' } },
  ]
  wide.document.atoms = Array.from({ length: 13 }, (_, i) => ({ ...wide.document.atoms[0], id: `a${i + 1}` }))
  wide.document.bonds = [['b1', 'a2', 'a3'], ['b2', 'a2', 'a4'], ['b3', 'a2', 'a5'], ['b4', 'a10', 'a11'], ['b5', 'a10', 'a12'], ['b6', 'a10', 'a13']]
    .map(([id, begin, end]) => ({ ...bondWith(id, begin, end), mark: null }))
  assert.equal(isDocumentResponse(wide), true, 'a10 follows a2')
})

test('notes about enhanced stereo groups are read: members that left, groups that went, relations that changed', () => {
  const note = (feature, source_path, disposition) => ({ feature, source_path, disposition, semantic_impact: 'stereo', reason: 'Stereo group e1 changed.' })
  const withNotes = (notes) => ({ ...stereoDrawing(), notes })
  const good = [
    note('stereo_configuration', 'atoms[a1]', 'dropped'),
    note('stereo_group_member', 'atoms[a1]', 'dropped'),
    note('stereo_group', 'groups[e2]', 'dropped'),
    note('stereo_group_relation', 'groups[e1]', 'dropped'),
    note('stereo_group_relation', 'groups[e1]', 'normalized'),
  ]
  assert.equal(isDocumentResponse(withNotes(good)), true)
  assert.equal(isDocumentResponse(withNotes([note('stereo_group_member', 'atoms[a1]', 'normalized')])), false, 'only a relation can be normalized')
  assert.equal(isDocumentResponse(withNotes([note('stereo_configuration', 'atoms[a1]', 'normalized')])), false)
  assert.equal(isDocumentResponse(withNotes([note('stereo_group_ratio', 'groups[e1]', 'dropped')])), false, 'an unknown feature')
  const refused = (feature, path, disposition) => assert.equal(isDocumentResponse(withNotes([note(feature, path, disposition)])), false, `${feature} ${path} ${disposition}`)
  assert.equal(isDocumentResponse(withNotes([note('stereo_configuration', 'bonds[b4]', 'dropped')])), true)
  refused('stereo_configuration', 'groups[e1]', 'dropped')
  refused('stereo_group_member', 'groups[e1]', 'dropped')
  refused('stereo_group_member', 'bonds[b4]', 'dropped')
  refused('stereo_group', 'atoms[a1]', 'dropped')
  refused('stereo_group', 'groups[e2]', 'normalized')
  refused('stereo_group_relation', 'atoms[a1]', 'dropped')
  refused('stereo_group_relation', 'groups[a1]', 'normalized')
  refused('stereo_group_relation', 'groups[e1', 'dropped')
  refused('stereo_group_relation', 'groups[e1] ', 'dropped')
  refused('stereo_group_relation', '', 'dropped')
  refused('stereo_group_relation', 'groups[e1]', 'preserved')
})

test('the stereo commands lapse with the bond or atom they name, and words and $N are not ids', () => {
  const drawing = stereoDrawing().document
  const center = { op: 'set_atom_configuration', atom: 'a1', configuration: { kind: 'tetrahedral', neighbors: ['a2', 'a3', 'a4', 'implicit_hydrogen'], turn: 'clockwise' } }
  const alkene = { op: 'set_bond_configuration', bond: 'b4', configuration: { kind: 'relative', begin_neighbor: 'a7', end_neighbor: 'implicit_hydrogen', relation: 'same_side' } }
  const mark = { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'begin' } }
  assert.deepEqual(stillApplicable([center, alkene, mark], drawing), [center, alkene, mark])
  const fresh = [
    { op: 'add_atom', element: 'N', position: { x: 0, y: 0 }, bond_to: { atom: 'a1', order: 'single' } },
    { op: 'set_bond_mark', bond: '$0', mark: null },
    { op: 'set_atom_configuration', atom: 'a1', configuration: { kind: 'unknown', neighbors: ['a2', 'a3', 'a4', '$0'] } },
  ]
  assert.deepEqual(stillApplicable(fresh, drawing), fresh, '$N names what an earlier command created')
  const without = (id) => ({ ...drawing, atoms: drawing.atoms.filter((atom) => atom.id !== id), bonds: drawing.bonds.filter((bond) => bond.id !== id) })
  assert.equal(stillApplicable([mark], without('b1')), null, 'the marked bond is gone')
  assert.equal(stillApplicable([center], without('a3')), null, 'a neighbor named by the center is gone')
  assert.equal(stillApplicable([center], without('a1')), null, 'the center is gone')
  assert.equal(stillApplicable([alkene], without('a7')), null, 'a neighbor of the double bond is gone')
  assert.equal(stillApplicable([{ op: 'set_bond_configuration', bond: 'b4', configuration: null }], without('b4')), null)
  // Deriving from the drawing names atoms, bonds and an instance, each of which must still exist.
  const placed = { ...drawing, scene: { instances: [{ id: 's1', positions: [] }] } }
  const derive = { op: 'set_configuration_from_drawing', atoms: ['a1'], bonds: ['b4'], instance: 's1' }
  assert.deepEqual(stillApplicable([derive], placed), [derive])
  assert.deepEqual(stillApplicable([{ op: 'set_configuration_from_drawing' }], drawing), [{ op: 'set_configuration_from_drawing' }], 'no list names nothing')
  assert.equal(stillApplicable([derive], { ...without('a1'), scene: placed.scene }), null, 'the named center is gone')
  assert.equal(stillApplicable([derive], { ...without('b4'), scene: placed.scene }), null, 'the named double bond is gone')
  assert.equal(stillApplicable([derive], drawing), null, 'the named instance is gone')
  const created = [
    { op: 'add_atom', element: 'N', position: { x: 0, y: 0 }, bond_to: { atom: 'a1', order: 'single' } },
    { op: 'set_configuration_from_drawing', atoms: ['a1', '$0'], bonds: ['$0'] },
  ]
  assert.deepEqual(stillApplicable(created, drawing), created, '$N names what an earlier command created')
})

test('a malformed mark or stereo table is refused, and so is a kind the client does not know yet', () => {
  const refused = (change) => {
    const value = stereoDrawing()
    change(value.document)
    return isDocumentResponse(value)
  }
  const cases = {
    'a mark kind the client does not know (a later "unknown" or "hollow")': (d) => { d.bonds[0].mark = { kind: 'hollow', narrow: 'end' } },
    'a wedge with no narrow end': (d) => { d.bonds[0].mark = { kind: 'wedge' } },
    'a narrow end that is neither end': (d) => { d.bonds[0].mark = { kind: 'wedge', narrow: 'middle' } },
    'a wavy mark with a narrow end': (d) => { d.bonds[2].mark = { kind: 'wavy', narrow: 'end' } },
    'a wedge on a double bond': (d) => { d.bonds[3].mark = { kind: 'wedge', narrow: 'end' } },
    'a crossed mark on a single bond': (d) => { d.bonds[4].mark = { kind: 'crossed' } },
    'some bonds with a mark field and some without': (d) => { delete d.bonds[5].mark },
    'a record for an atom the drawing lacks': (d) => { d.stereo.atoms[0].atom = 'a9' },
    'two records for one center': (d) => { d.stereo.atoms.push(structuredClone(d.stereo.atoms[0])) },
    'a center whose neighbors are not its bonded atoms': (d) => { d.stereo.atoms[0].configuration.neighbors = ['a2', 'a3', 'a5', 'implicit_hydrogen'] },
    'a center naming a neighbor twice': (d) => { d.stereo.atoms[0].configuration.neighbors = ['a2', 'a3', 'a3', 'implicit_hydrogen'] },
    'a center with a slot word the client does not know': (d) => { d.stereo.atoms[0].configuration.neighbors[3] = 'unspecified' },
    'a center with three neighbors and no fourth slot': (d) => { d.stereo.atoms[0].configuration.neighbors = ['a2', 'a3', 'a4', 'a4'] },
    'a turn that is neither word': (d) => { d.stereo.atoms[0].configuration.turn = 'positive' },
    'a configuration kind the client does not know': (d) => { d.stereo.atoms[0].configuration.kind = 'square_planar' },
    'an unknown center that names a turn': (d) => { d.stereo.atoms[0].configuration = { kind: 'unknown', neighbors: ['a2', 'a3', 'a4', 'implicit_hydrogen'], turn: 'clockwise' } },
    'a double-bond record for a single bond': (d) => { d.stereo.bonds[0].bond = 'b5' },
    'a relation that is not a word of the contract (the public spelling)': (d) => { d.stereo.bonds[0].configuration.relation = 'opposite_side' },
    'a reference that is the partner': (d) => { d.stereo.bonds[0].configuration.begin_neighbor = 'a6' },
    'a reference bonded to the other end': (d) => { d.stereo.bonds[0].configuration.begin_neighbor = 'a8' },
    'a field the contract does not have': (d) => { d.stereo.extra = [] },
  }
  for (const [why, change] of Object.entries(cases)) assert.equal(refused(change), false, why)
})

test('a marked bond is named with its mark and the atom at its narrow end', () => {
  const marked = (mark) => ({ ...bondWith('b1', 'a1', 'a2'), mark })
  assert.equal(bondName(marked(null)), 'Bond b1: single bond between a1 and a2')
  assert.equal(bondName(marked({ kind: 'wedge', narrow: 'begin' })), 'Bond b1: single bond between a1 and a2, wedge, narrow end at a1')
  assert.equal(bondName(marked({ kind: 'hash', narrow: 'end' })), 'Bond b1: single bond between a1 and a2, hashed wedge, narrow end at a2')
  assert.equal(bondName(marked({ kind: 'wavy' })), 'Bond b1: single bond between a1 and a2, wavy')
  assert.equal(bondName({ ...bondWith('b1', 'a1', 'a2', 'double'), mark: { kind: 'crossed' } }, 's2'), 'Bond b1 in s2: double bond between a1 and a2, crossed')
})

test('an inspection is stale when only the stereo changed under the same graph and revision', () => {
  const shown = stereoDrawing().document
  assert.equal(sameInspectionSnapshot(shown, structuredClone(shown)), true)
  const mirrored = structuredClone(shown)
  mirrored.stereo.atoms[0].configuration.turn = 'clockwise'
  assert.equal(sameInspectionSnapshot(shown, mirrored), false)
  assert.equal(samePropertySnapshot(shown, mirrored), false)
  const cleared = structuredClone(shown)
  cleared.stereo = { atoms: [], bonds: [] }
  assert.equal(sameInspectionSnapshot(shown, cleared), false)
})

const stereoLoss = (path) => ({
  feature: 'stereo', source_path: path, disposition: 'unknown', semantic_impact: 'stereo',
  reason: 'The drawing holds stereo marks or stated configurations that this comparison does not read.',
})

/** A stereo-bearing source and a plain one-atom target, and the identity reply for them. */
function stereoIdentityReply() {
  const source = document('identity-source', 2)
  source.document.atoms = ['a1', 'a2', 'a3', 'a4'].map((id, index) => atomWith(id, 'C', index * 30, 0))
  source.document.bonds = [
    { ...bondWith('b1', 'a1', 'a2'), mark: { kind: 'wedge', narrow: 'begin' } },
    { ...bondWith('b2', 'a1', 'a3'), mark: null },
    { ...bondWith('b3', 'a1', 'a4'), mark: null },
  ]
  source.document.stereo = { atoms: [], bonds: [] }
  const target = document('identity-target', 3)
  target.document.atoms = [atomWith('a1', 'C', 0, 0)]
  const identity = { code: 'code', numbering: ['a1', 'a2', 'a3', 'a4'], symmetry: [['a1'], ['a2'], ['a3'], ['a4']], folded_hydrogens: [] }
  const targetIdentity = { code: 'other', numbering: ['a1'], symmetry: [['a1']], folded_hydrogens: [] }
  return { envelope: 'operation_result', contract: 1, outcome: 'partial', document_revision: 2, model_id: 'chemifree-recorded-graph-1',
    diagnostics: [], loss_report: [stereoLoss('snapshot')], result: {
      snapshot: { document: source.document, session: source.session, recovery: null }, identity,
      comparison: { snapshot: { document: target.document, session: target.session, recovery: null }, identity: targetIdentity, equivalent: false },
      stereo: { compared: false, source_present: true, other_present: false },
    } }
}

test('an identity answer that ignored stereo is accepted only as partial, with the flags and the loss', async () => {
  const reply = stereoIdentityReply()
  const mutate = (change) => {
    const copy = structuredClone(reply)
    change(copy)
    return copy
  }
  const bad = {
    'ok with a stereo-bearing drawing': mutate((r) => { r.outcome = 'ok'; r.loss_report = [] }),
    'partial with no loss': mutate((r) => { r.loss_report = [] }),
    'partial for a drawing with no stereo': mutate((r) => { r.result.comparison.snapshot.document.id = 'identity-target'; r.result.stereo.source_present = false }),
    'no flags from a service that holds stereo': mutate((r) => { delete r.result.stereo }),
    'a flag that the snapshot contradicts': mutate((r) => { r.result.stereo.other_present = true; r.loss_report.push(stereoLoss('comparison.snapshot')) }),
    'compared claimed true': mutate((r) => { r.result.stereo.compared = true }),
    'the loss for the wrong side': mutate((r) => { r.loss_report = [stereoLoss('comparison.snapshot')] }),
    'a second loss nobody asked for': mutate((r) => { r.loss_report.push(stereoLoss('comparison.snapshot')) }),
    'a loss that says dropped': mutate((r) => { r.loss_report[0].disposition = 'dropped' }),
    'an unknown field in the flags': mutate((r) => { r.result.stereo.target_present = false }),
  }
  service({ '/api/v1/operations': [() => json(200, reply), ...Object.values(bad).map((body) => () => json(200, body))] })
  const client = new DocumentClient()
  const result = await client.inspectIdentity('identity-source', 'recorded', 'identity-target')
  assert.deepEqual(result.stereo, { sourcePresent: true, otherPresent: false })
  assert.equal(result.comparison.equivalent, false)
  for (const why of Object.keys(bad)) {
    await assert.rejects(client.inspectIdentity('identity-source', 'recorded', 'identity-target'), { code: 'incompatible_response' }, why)
  }
})

test('an identity answer for drawings without stereo stays ok, with or without the flags', async () => {
  const reply = stereoIdentityReply()
  reply.result.snapshot.document.bonds = reply.result.snapshot.document.bonds.map((bond) => ({ ...bond, mark: null }))
  reply.outcome = 'ok'
  reply.loss_report = []
  reply.result.stereo = { compared: false, source_present: false, other_present: false }
  const older = structuredClone(reply)
  delete older.result.stereo
  const alone = structuredClone(reply)
  alone.result.comparison = null
  alone.result.stereo.other_present = null
  service({ '/api/v1/operations': [() => json(200, reply), () => json(200, older), () => json(200, alone)] })
  const client = new DocumentClient()
  assert.deepEqual((await client.inspectIdentity('identity-source', 'recorded', 'identity-target')).stereo, { sourcePresent: false, otherPresent: false })
  assert.deepEqual((await client.inspectIdentity('identity-source', 'recorded', 'identity-target')).stereo, { sourcePresent: false, otherPresent: false })
  assert.deepEqual((await client.inspectIdentity('identity-source', 'recorded', null)).stereo, { sourcePresent: false, otherPresent: null })
})

test('a substructure search that ignored stereo is accepted only as partial, with the flags and the loss', async () => {
  const { reply, query } = substructureReply()
  query.document.bonds = [{ ...query.document.bonds[0], mark: { kind: 'hash', narrow: 'end' } }]
  const marked = structuredClone(reply)
  marked.result.query.document = query.document
  marked.outcome = 'partial'
  marked.loss_report = [stereoLoss('query')]
  marked.result.stereo = { compared: false, query_present: true, target_present: false }
  const mutate = (change) => {
    const copy = structuredClone(marked)
    change(copy)
    return copy
  }
  const bad = {
    'ok with a stereo-bearing query': mutate((r) => { r.outcome = 'ok'; r.loss_report = [] }),
    'partial with no loss': mutate((r) => { r.loss_report = [] }),
    'no flags from a service that holds stereo': mutate((r) => { delete r.result.stereo }),
    'a flag the snapshot contradicts': mutate((r) => { r.result.stereo.target_present = true }),
    'the loss for the wrong side': mutate((r) => { r.loss_report = [stereoLoss('target')] }),
    'compared claimed true': mutate((r) => { r.result.stereo.compared = true }),
  }
  service({ '/api/v1/operations': [() => json(200, marked), ...Object.values(bad).map((body) => () => json(200, body))] })
  const client = new DocumentClient()
  const found = await client.searchSubstructure('match-query', 'match-target')
  assert.deepEqual(found.stereo, { queryPresent: true, targetPresent: false })
  assert.equal(found.mappings.length, 1)
  for (const why of Object.keys(bad)) {
    await assert.rejects(client.searchSubstructure('match-query', 'match-target'), { code: 'incompatible_response' }, why)
  }
  // Drawings without stereo stay ok, and the model property is still stated.
  const plain = substructureReply().reply
  plain.result.stereo = { compared: false, query_present: false, target_present: false }
  service({ '/api/v1/operations': [() => json(200, plain)] })
  assert.deepEqual((await new DocumentClient().searchSubstructure('match-query', 'match-target')).stereo, { queryPresent: false, targetPresent: false })
})

// ---------------------------------------------------------------------------------------------
// Stereo marks drawn by the editor: wedge, hashed wedge, wavy and crossed strokes, the commands
// of a mark gesture, and what it says. Expected values are written out, not recomputed.
// ---------------------------------------------------------------------------------------------

const { markStrokes, wavePath, markFits, strokeReach, MARK_STYLE, distance } = await import('../apps/web/src/editor/geometry.ts')
const { clickBond, staleStereoClears } = await import('../apps/web/src/editor/bond-gestures.ts')
const { buildMark, cycleMark, inspectorWhere, markBondGesture, sendMark, setMark } = await import('../apps/web/src/editor/mark-gestures.ts')
const { describeStereoChange } = await import('../apps/web/src/editor/stereo-status.ts')

const pt = (x, y) => ({ x, y })
const strokesOf = (mark, begin, end, boxes = [null, null], spacing = 1, lineWidth = 1.2, order = 'single') =>
  bondStrokes({ ...bondWith('b1', 'a1', 'a2', order), mark }, begin, end, boxes[0], boxes[1], 1, spacing, lineWidth)
const sortedPoints = (points) => points.map(({ x, y }) => [x, y]).sort((p, q) => p[0] - q[0] || p[1] - q[1])

test('a wedge is a polygon from a point at its narrow end to 4 pt at the other, whichever end is narrow', () => {
  assert.deepEqual(strokesOf({ kind: 'wedge', narrow: 'begin' }, pt(0, 0), pt(30, 0)), [
    { shape: 'polygon', points: [pt(0, 0), pt(30, -2), pt(30, 2)] },
  ])
  assert.deepEqual(strokesOf({ kind: 'wedge', narrow: 'end' }, pt(0, 0), pt(30, 0)), [
    { shape: 'polygon', points: [pt(0, 2), pt(0, -2), pt(30, 0)] },
  ])
})

test('a wedge is 4 pt wide at its wide end whatever the bond length: the width is a setting in points', () => {
  for (const length of [15, 30, 60, 200]) {
    const [stroke] = strokesOf({ kind: 'wedge', narrow: 'begin' }, pt(0, 0), pt(length, 0))
    const wide = stroke.points.filter((p) => p.x === length)
    assert.equal(wide.length, 2, `${length} pt`)
    assert.equal(distance(wide[0], wide[1]), 4, `${length} pt`)
  }
  assert.equal(MARK_STYLE.wedgeWidth, 4)
  assert.equal(MARK_STYLE.hashSpacing, 2.7)
})

test('a wedge is never thinner than its line', () => {
  const [thick] = strokesOf({ kind: 'wedge', narrow: 'begin' }, pt(0, 0), pt(30, 0), [null, null], bondSpacing(10), 10)
  assert.deepEqual(thick.points, [pt(0, 0), pt(30, -5), pt(30, 5)])
})

test('a label cuts a wedge without changing its slope', () => {
  // The box grown by the stroke gap (1.4 + 0.6) ends at x = 8, where the half width is 2 * 8 / 30.
  const near = { x: -6, y: -4, width: 12, height: 8 }
  assert.deepEqual(strokesOf({ kind: 'wedge', narrow: 'begin' }, pt(0, 0), pt(30, 0), [near, null]), [
    { shape: 'polygon', points: [pt(8, 0.53), pt(8, -0.53), pt(30, -2), pt(30, 2)] },
  ])
  // Cut at x = 22, where the full wedge is 2 * 22 / 30 = 1.47 wide on each side.
  const far = { x: 24, y: -4, width: 12, height: 8 }
  assert.deepEqual(strokesOf({ kind: 'wedge', narrow: 'begin' }, pt(0, 0), pt(30, 0), [null, far]), [
    { shape: 'polygon', points: [pt(0, 0), pt(22, -1.47), pt(22, 1.47)] },
  ])
})

test('a mark has no ink where labels leave no room or the bond has no length', () => {
  const box = { x: -20, y: -5, width: 70, height: 10 }
  for (const mark of [{ kind: 'wedge', narrow: 'begin' }, { kind: 'hash', narrow: 'end' }, { kind: 'wavy' }]) {
    assert.deepEqual(strokesOf(mark, pt(0, 0), pt(30, 0), [box, null]), [], mark.kind)
    assert.deepEqual(strokesOf(mark, pt(5, 5), pt(5, 5)), [], `${mark.kind} of no length`)
  }
  assert.deepEqual(strokesOf({ kind: 'crossed' }, pt(0, 0), pt(0, 0), [null, null], 1, 1.2, 'double'), [])
})

test('swapping a bond end for end together with its narrow end gives the same picture', () => {
  const begin = pt(10, 60)
  const end = pt(35.98, 45)
  const points = (stroke) => (stroke.shape === 'polygon' ? stroke.points : stroke.segments.flat())
  for (const kind of ['wedge', 'hash']) {
    const forward = markStrokes({ kind, narrow: 'begin' }, begin, end, null, null, 1, 1.2)[0]
    const swapped = markStrokes({ kind, narrow: 'end' }, end, begin, null, null, 1, 1.2)[0]
    assert.deepEqual(sortedPoints(points(swapped)), sortedPoints(points(forward)), kind)
  }
})

test('a hashed wedge has a stripe every 2.7 pt from its narrow end, longer toward the wide end', () => {
  const [begin] = strokesOf({ kind: 'hash', narrow: 'begin' }, pt(0, 0), pt(30, 0))
  assert.equal(begin.shape, 'stripes')
  assert.equal(begin.segments.length, 11)
  assert.deepEqual(begin.segments[0], [pt(1.35, 0.6), pt(1.35, -0.6)], 'no shorter than a line is wide')
  assert.deepEqual(begin.segments[10], [pt(28.35, 1.89), pt(28.35, -1.89)])
  const [end] = strokesOf({ kind: 'hash', narrow: 'end' }, pt(0, 0), pt(30, 0))
  assert.deepEqual(end.segments[0], [pt(28.65, 0.6), pt(28.65, -0.6)])
  assert.deepEqual(end.segments[10], [pt(1.65, 1.89), pt(1.65, -1.89)])
})

test('a label removes stripes without moving the others, and a short piece keeps one stripe', () => {
  const whole = strokesOf({ kind: 'hash', narrow: 'begin' }, pt(0, 0), pt(30, 0))[0].segments
  // The box grown by the gap ends at x = 22: the stripes at 1.35 + 2.7 k up to 20.25 stay.
  const cut = strokesOf({ kind: 'hash', narrow: 'begin' }, pt(0, 0), pt(30, 0), [null, { x: 24, y: -4, width: 12, height: 8 }])[0].segments
  assert.deepEqual(cut, whole.slice(0, 8))
  assert.equal(strokesOf({ kind: 'hash', narrow: 'begin' }, pt(0, 0), pt(1, 0))[0].segments.length, 1)
  // The taper still runs from 0 to 2 over the whole 1 pt, so the stripe half way is 1 long on each side.
  assert.deepEqual(strokesOf({ kind: 'hash', narrow: 'begin' }, pt(0, 0), pt(1, 0))[0].segments[0], [pt(0.5, 1), pt(0.5, -1)])
})

test('a very long hashed wedge or wave has a bounded number of parts', () => {
  const [hashed] = strokesOf({ kind: 'hash', narrow: 'begin' }, pt(0, 0), pt(100000, 0))
  assert.ok(hashed.segments.length <= MARK_STYLE.maxParts && hashed.segments.length >= MARK_STYLE.maxParts - 1, `${hashed.segments.length}`)
  const [wave] = strokesOf({ kind: 'wavy' }, pt(0, 0), pt(100000, 0))
  assert.equal(wave.halfWaves, MARK_STYLE.maxParts)
})

test('a wavy bond is a wave on its axis, with arcs bulging to alternate sides', () => {
  const [wave] = strokesOf({ kind: 'wavy' }, pt(0, 0), pt(30, 0))
  assert.deepEqual(wave, { shape: 'wave', from: pt(0, 0), to: pt(30, 0), halfWaves: 8, amplitude: 1.8 })
  assert.equal(
    wavePath(wave),
    'M 0 0 Q 1.88 3.6 3.75 0 T 7.5 0 T 11.25 0 T 15 0 T 18.75 0 T 22.5 0 T 26.25 0 T 30 0',
  )
  assert.equal(strokesOf({ kind: 'wavy' }, pt(0, 0), pt(5, 0))[0].halfWaves, 2, 'at least two arcs')
})

test('a crossed double bond is two lines crossing between the places of its two lines', () => {
  assert.deepEqual(strokesOf({ kind: 'crossed' }, pt(0, 0), pt(30, 0), [null, null], 1, 1.2, 'double'), [
    { shape: 'line', x1: 0, y1: 2.2, x2: 30, y2: -2.2, pattern: 'solid' },
    { shape: 'line', x1: 0, y1: -2.2, x2: 30, y2: 2.2, pattern: 'solid' },
  ])
})

test('a marked bond is drawn as its mark only where the mark fits the bond order', () => {
  assert.equal(markFits('wedge', 'single'), true)
  assert.equal(markFits('wavy', 'double'), false)
  assert.equal(markFits('crossed', 'double'), true)
  assert.equal(markFits('crossed', 'single'), false)
  // A wedge on a double bond cannot come from the service; the picture stays the double bond.
  assert.deepEqual(
    strokesOf({ kind: 'wedge', narrow: 'begin' }, pt(0, 0), pt(30, 0), [null, null], 1, 1.2, 'double').map((s) => s.shape),
    ['line', 'line'],
  )
})

test('the ink of every mark stays within the reach the view box and placement frames allow', () => {
  for (const lineWidth of [0.1, 1.2, 3, 10]) {
    const spacing = bondSpacing(lineWidth)
    const room = strokeReach(lineWidth, spacing)
    const across = (stroke) => {
      switch (stroke.shape) {
        case 'polygon': return Math.max(...stroke.points.map((p) => Math.abs(p.y)))
        case 'stripes': return Math.max(...stroke.segments.flat().map((p) => Math.abs(p.y)))
        case 'wave': return stroke.amplitude + lineWidth / 2
        case 'line': return Math.max(Math.abs(stroke.y1), Math.abs(stroke.y2)) + lineWidth / 2
        default: throw new Error(stroke.shape)
      }
    }
    for (const [mark, order] of [[{ kind: 'wedge', narrow: 'begin' }, 'single'], [{ kind: 'hash', narrow: 'end' }, 'single'], [{ kind: 'wavy' }, 'single'], [{ kind: 'crossed' }, 'double']]) {
      for (const stroke of strokesOf(mark, pt(0, 0), pt(30, 0), [null, null], spacing, lineWidth, order)) {
        assert.ok(across(stroke) <= room, `${mark.kind} at line width ${lineWidth}: ${across(stroke)} > ${room}`)
      }
    }
  }
})

test('a marked bond is found where a plain one is, and its narrow end gives way to the atom', () => {
  const marked = {
    ...document('doc-hit', 1).document,
    atoms: [atomWith('a1', 'C', 0, 0), atomWith('a2', 'C', 30, 0)],
    bonds: [{ ...bondWith('b1', 'a1', 'a2'), mark: { kind: 'wedge', narrow: 'begin' } }],
  }
  assert.deepEqual(hitTest(marked, pt(15, 4.9)), { kind: 'bond', id: 'b1' })
  assert.equal(hitTest(marked, pt(15, 5.1)), null)
  assert.deepEqual(hitTest(marked, pt(1, 0.5)), { kind: 'atom', id: 'a1' })
})

test('a mark is named with its words, the hashed wedge as such', () => {
  const named = (mark) => bondName({ ...bondWith('b1', 'a1', 'a2'), mark })
  assert.equal(named({ kind: 'wedge', narrow: 'end' }), 'Bond b1: single bond between a1 and a2, wedge, narrow end at a2')
  assert.equal(named({ kind: 'hash', narrow: 'begin' }), 'Bond b1: single bond between a1 and a2, hashed wedge, narrow end at a1')
  assert.equal(named({ kind: 'wavy' }), 'Bond b1: single bond between a1 and a2, wavy')
  assert.equal(bondName({ ...bondWith('b1', 'a1', 'a2', 'double'), mark: { kind: 'crossed' } }), 'Bond b1: double bond between a1 and a2, crossed')
})

/** The public two-center drawing, typed by hand: F[C@H](Cl)[C@H](Br)I with one hashed wedge per center. */
function publicFixture(marks) {
  const value = document('doc-public', 2).document
  value.atoms = [
    atomWith('a1', 'F', 0, 40), atomWith('a2', 'C', 0, 0), atomWith('a3', 'Cl', -35, -18),
    atomWith('a4', 'C', 35, -18), atomWith('a5', 'Br', 35, -58), atomWith('a6', 'I', 70, 0),
  ]
  value.bonds = [
    { ...bondWith('b1', 'a1', 'a2'), mark: marks ? { kind: 'hash', narrow: 'end' } : null },
    bondWith('b2', 'a2', 'a3'), bondWith('b3', 'a2', 'a4'),
    { ...bondWith('b4', 'a4', 'a5'), mark: marks ? { kind: 'hash', narrow: 'begin' } : null },
    bondWith('b5', 'a4', 'a6'),
  ]
  return value
}

test('the public two-center drawing is drawn with both narrow ends on the centers, the end variant swapped or not', () => {
  const fixture = publicFixture(true)
  const place = new Map(fixture.atoms.map((atom) => [atom.id, atom.position]))
  const strokes = drawingStrokes(fixture, new Map())
  const lengthOf = (segment) => distance(segment[0], segment[1])
  // b1 runs from the fluorine to a2 with its narrow end at its end (a2): its first stripe is the
  // short one next to a2 and the last, next to the fluorine, is the longest.
  const [b1] = strokes.get('b1')
  assert.ok(distance(b1.segments[0][0], place.get('a2')) < distance(b1.segments[0][0], place.get('a1')))
  assert.ok(lengthOf(b1.segments[0]) < lengthOf(b1.segments.at(-1)))
  // b4 runs from a4 to the bromine with its narrow end at its begin (a4).
  const [b4] = strokes.get('b4')
  assert.ok(distance(b4.segments[0][0], place.get('a4')) < distance(b4.segments[0][0], place.get('a5')))
  assert.ok(lengthOf(b4.segments[0]) < lengthOf(b4.segments.at(-1)))
  // The same bond written the other way round, with the narrow end flipped, is the same picture.
  const swapped = markStrokes({ kind: 'hash', narrow: 'begin' }, place.get('a2'), place.get('a1'), null, null, 1, 1.2)[0]
  assert.deepEqual(sortedPoints(swapped.segments.flat()), sortedPoints(b1.segments.flat()))
  const swappedB4 = markStrokes({ kind: 'hash', narrow: 'end' }, place.get('a5'), place.get('a4'), null, null, 1, 1.2)[0]
  assert.deepEqual(sortedPoints(swappedB4.segments.flat()), sortedPoints(b4.segments.flat()))
})

test('the public two-center drawing marks its centers as a user would: one click, then the other end', () => {
  const plain = publicFixture(false)
  const hashed = cycleMark(plain, { instance: null }, plain.bonds[0], 'hash')
  // The first click puts the narrow end at the bond's begin, the fluorine, which cannot be a center.
  assert.deepEqual(hashed.commands, [{ op: 'set_bond_mark', bond: 'b1', mark: { kind: 'hash', narrow: 'begin' } }])
  assert.match(hashed.why, /^a1 has 1 bond; a stereocenter needs 3 or 4\.$/)
  const marked = { ...plain, bonds: [{ ...plain.bonds[0], mark: { kind: 'hash', narrow: 'begin' } }, ...plain.bonds.slice(1)] }
  assert.deepEqual(cycleMark(marked, { instance: null }, marked.bonds[0], 'hash').commands, [
    { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'hash', narrow: 'end' } },
    { op: 'set_configuration_from_drawing', atoms: ['a2'] },
  ])
  // A drag from the center to the fluorine marks the same bond with its narrow end at the center.
  assert.deepEqual(markBondGesture(plain, null, { atom: 'a2' }, { atom: 'a1' }, 'hash').commands, [
    { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'hash', narrow: 'end' } },
    { op: 'set_configuration_from_drawing', atoms: ['a2'] },
  ])
})

/**
 * a1 (100, 100) with three bonds (a2, a3, a5); a4 alone; a6 with two (a7, a8); a9 with four
 * (a10 to a13); b14 joins two other atoms by a double bond.
 */
function marksDocument() {
  const value = document('doc-marks', 3).document
  const around = (center, ...others) => others.map(([id, x, y]) => atomWith(id, 'C', x, y))
  value.atoms = [
    atomWith('a1', 'C', 100, 100), atomWith('a2', 'C', 130, 100), atomWith('a3', 'C', 85, 126), atomWith('a5', 'C', 115, 74),
    atomWith('a4', 'C', 60, 60),
    atomWith('a6', 'C', 200, 100), atomWith('a7', 'C', 230, 100), atomWith('a8', 'C', 185, 126),
    atomWith('a9', 'C', 300, 100), ...around('a9', ['a10', 330, 100], ['a11', 300, 130], ['a12', 270, 100], ['a13', 300, 70]),
    atomWith('a14', 'C', 400, 100), atomWith('a15', 'C', 430, 100),
  ]
  value.bonds = [
    bondWith('b1', 'a1', 'a2'), bondWith('b2', 'a1', 'a3'), bondWith('b3', 'a1', 'a5'),
    bondWith('b4', 'a6', 'a7'), bondWith('b5', 'a6', 'a8'),
    bondWith('b6', 'a9', 'a10'), bondWith('b7', 'a9', 'a11'), bondWith('b8', 'a9', 'a12'), bondWith('b9', 'a9', 'a13'),
    bondWith('b10', 'a14', 'a15', 'double'),
  ]
  return value
}
const withMark = (value, bond, mark) => ({ ...value, bonds: value.bonds.map((b) => (b.id === bond ? { ...b, mark } : b)) })
const derive = (atoms, extra = {}) => ({ op: 'set_configuration_from_drawing', atoms, ...extra })
const newCarbon = (from, x, y) => ({ op: 'add_atom', element: 'C', position: pt(x, y), bond_to: { atom: from, order: 'single' } })

test('a drag from a center with three bonds makes a wedge at the drag start and stores what the drawing shows', () => {
  const gesture = markBondGesture(marksDocument(), null, { atom: 'a1' }, { point: pt(100, 70) }, 'wedge')
  assert.deepEqual(gesture, {
    kind: 'send',
    commands: [newCarbon('a1', 100, 70), { op: 'set_bond_mark', bond: '$0', mark: { kind: 'wedge', narrow: 'begin' } }, derive(['a1'])],
    markOnly: [newCarbon('a1', 100, 70), { op: 'set_bond_mark', bond: '$0', mark: { kind: 'wedge', narrow: 'begin' } }],
    why: null,
  })
})

test('the configuration is asked for only when the narrow end has 3 or 4 bonds once the gesture is done', () => {
  const document = marksDocument()
  const why = (from) => markBondGesture(document, null, { atom: from }, { point: pt(10, 10) }, 'hash').why
  assert.equal(why('a4'), 'a4 has 1 bond; a stereocenter needs 3 or 4.')
  assert.equal(why('a3'), 'a3 has 2 bonds; a stereocenter needs 3 or 4.')
  assert.equal(why('a9'), 'a9 has 5 bonds; a stereocenter needs 3 or 4.')
  assert.equal(why('a6'), null, 'two bonds and the new one make three')
  assert.equal(why('a1'), null, 'three and the new one make four')
  const query = { ...document, atoms: document.atoms.map((a) => (a.id === 'a1' ? { ...a, kind: 'query', query: { kind: 'any' } } : a)) }
  assert.equal(markBondGesture(query, null, { atom: 'a1' }, { point: pt(10, 10) }, 'wedge').why, 'a1 is a query atom, which cannot hold a configuration.')
  const blocked = markBondGesture(document, null, { atom: 'a4' }, { point: pt(10, 10) }, 'wedge')
  assert.equal(blocked.markOnly, null)
  assert.equal(blocked.commands.at(-1).op, 'set_bond_mark', 'no derive command')
})

test('a gesture that adds a bond between two atoms, or from empty space, sets the mark on the bond it makes', () => {
  const document = marksDocument()
  assert.deepEqual(markBondGesture(document, null, { atom: 'a1' }, { atom: 'a4' }, 'hash').commands, [
    { op: 'add_bond', begin: 'a1', end: 'a4', order: 'single' },
    { op: 'set_bond_mark', bond: '$0', mark: { kind: 'hash', narrow: 'begin' } },
    derive(['a1']),
  ])
  const toAtom = markBondGesture(document, null, { point: pt(10, 10) }, { atom: 'a6' }, 'wedge')
  assert.deepEqual(toAtom.commands, [
    { op: 'add_atom', element: 'C', position: pt(10, 10) },
    { op: 'add_bond', begin: '$0', end: 'a6', order: 'single' },
    { op: 'set_bond_mark', bond: '$1', mark: { kind: 'wedge', narrow: 'begin' } },
  ])
  assert.equal(toAtom.why, 'the new atom has one bond; a stereocenter needs 3 or 4.')
  assert.deepEqual(markBondGesture(document, null, { point: pt(10, 10) }, { point: pt(40, 10) }, 'wedge').commands, [
    { op: 'add_atom', element: 'C', position: pt(10, 10) },
    { op: 'add_atom', element: 'C', position: pt(40, 10), bond_to: { atom: '$0', order: 'single' } },
    { op: 'set_bond_mark', bond: '$1', mark: { kind: 'wedge', narrow: 'begin' } },
  ])
  assert.equal(markBondGesture(document, null, { atom: 'a1' }, { atom: 'a1' }, 'wedge'), null)
})

test('over an existing single bond a mark gesture marks it with the narrow end where the drag started', () => {
  const document = marksDocument()
  assert.deepEqual(markBondGesture(document, null, { atom: 'a1' }, { atom: 'a2' }, 'wedge').commands, [
    { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'begin' } },
    derive(['a1']),
  ])
  const fromEnd = markBondGesture(document, null, { atom: 'a2' }, { atom: 'a1' }, 'wedge')
  assert.deepEqual(fromEnd.commands, [{ op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'end' } }])
  assert.equal(fromEnd.why, 'a2 has 1 bond; a stereocenter needs 3 or 4.')
})

test('a mark over a bond that is not single is refused in words and sends nothing', () => {
  const document = marksDocument()
  assert.deepEqual(markBondGesture(document, null, { atom: 'a14' }, { atom: 'a15' }, 'wedge'), {
    kind: 'refused',
    text: 'A wedge mark needs a single bond; b10 is a double bond. Change it to single first.',
  })
  assert.equal(cycleMark(document, { instance: null }, document.bonds[9], 'hash').text, 'A hashed wedge mark needs a single bond; b10 is a double bond. Change it to single first.')
  assert.equal(cycleMark(document, { instance: null }, document.bonds[9], 'wavy').text, 'A wavy mark needs a single bond; b10 is a double bond. Change it to single first.')
})

test('a wavy mark asks for the configuration of each end that could be a center', () => {
  const document = marksDocument()
  assert.deepEqual(markBondGesture(document, null, { atom: 'a1' }, { atom: 'a6' }, 'wavy').commands.at(-1), derive(['a1', 'a6']))
  assert.deepEqual(markBondGesture(document, null, { atom: 'a1' }, { atom: 'a4' }, 'wavy').commands.at(-1), derive(['a1']))
  const neither = markBondGesture(document, null, { atom: 'a3' }, { atom: 'a4' }, 'wavy')
  assert.equal(neither.markOnly, null)
  assert.match(neither.why, /^neither end could be a stereocenter \(a3 has 2 bonds/)
})

test('the gesture names the scene instance it was drawn in for the configuration, and none without a scene', () => {
  const document = marksDocument()
  assert.deepEqual(markBondGesture(document, 's1', { atom: 'a1' }, { atom: 'a2' }, 'wedge').commands.at(-1), derive(['a1'], { instance: 's1' }))
  assert.deepEqual(markBondGesture(document, null, { atom: 'a1' }, { atom: 'a2' }, 'wedge').commands.at(-1), derive(['a1']))
})

test('repeated clicks on a bond cycle plain, narrow end at the begin, narrow end at the end, plain', () => {
  const document = marksDocument()
  const step = (bond, kind) => cycleMark(document.bonds ? { ...document, bonds: document.bonds.map((b) => (b.id === 'b1' ? bond : b)) } : document, { instance: null }, bond, kind).commands[0].mark
  const plain = document.bonds[0]
  assert.deepEqual(step(plain, 'wedge'), { kind: 'wedge', narrow: 'begin' })
  assert.deepEqual(step({ ...plain, mark: { kind: 'wedge', narrow: 'begin' } }, 'wedge'), { kind: 'wedge', narrow: 'end' })
  assert.equal(step({ ...plain, mark: { kind: 'wedge', narrow: 'end' } }, 'wedge'), null)
  assert.deepEqual(step({ ...plain, mark: { kind: 'wedge', narrow: 'end' } }, 'hash'), { kind: 'hash', narrow: 'end' }, 'the other tool keeps the narrow end')
  assert.deepEqual(step({ ...plain, mark: { kind: 'hash', narrow: 'begin' } }, 'wedge'), { kind: 'wedge', narrow: 'begin' })
  assert.deepEqual(step({ ...plain, mark: { kind: 'wavy' } }, 'hash'), { kind: 'hash', narrow: 'begin' })
  assert.deepEqual(step(plain, 'wavy'), { kind: 'wavy' })
  assert.deepEqual(step({ ...plain, mark: { kind: 'wedge', narrow: 'begin' } }, 'wavy'), { kind: 'wavy' })
  assert.equal(step({ ...plain, mark: { kind: 'wavy' } }, 'wavy'), null)
  // Clearing sends the clear alone.
  assert.deepEqual(cycleMark(document, { instance: null }, { ...plain, mark: { kind: 'wedge', narrow: 'end' } }, 'wedge'), {
    kind: 'send', commands: [{ op: 'set_bond_mark', bond: 'b1', mark: null }], markOnly: null, why: null,
  })
})

test('the inspector sets a crossed mark with the bond configuration derived, and none for several placements', () => {
  const document = marksDocument()
  const bond = document.bonds[9]
  assert.deepEqual(setMark(document, { instance: 's1' }, bond, { kind: 'crossed' }), {
    kind: 'send',
    commands: [{ op: 'set_bond_mark', bond: 'b10', mark: { kind: 'crossed' } }, { op: 'set_configuration_from_drawing', bonds: ['b10'], instance: 's1' }],
    markOnly: [{ op: 'set_bond_mark', bond: 'b10', mark: { kind: 'crossed' } }],
    why: null,
  })
  assert.equal(setMark(document, { why: 'several' }, document.bonds[0], { kind: 'wedge', narrow: 'begin' }).markOnly, null)
  assert.equal(setMark(document, { instance: null }, document.bonds[0], { kind: 'crossed' }).text, 'A crossed mark needs a double bond; b1 is a single bond. Change it to double first.')
  const scene = (ids) => ({ ...document, scene: { instances: ids.map((id) => ({ id, positions: [] })) } })
  assert.deepEqual(inspectorWhere(document), { instance: null })
  assert.deepEqual(inspectorWhere(scene(['s1'])), { instance: 's1' })
  assert.match(inspectorWhere(scene(['s1', 's2'])).why, /several placements/)
})

test('a gesture that names an atom or bond that is gone lapses, and one that names a new bond does not', () => {
  const document = marksDocument()
  const gesture = markBondGesture(document, null, { atom: 'a1' }, { atom: 'a4' }, 'wedge')
  assert.equal(stillApplicable(gesture.commands, document), gesture.commands)
  assert.equal(stillApplicable(gesture.commands, { ...document, atoms: document.atoms.filter((a) => a.id !== 'a1') }), null)
  const clear = cycleMark(withMark(document, 'b1', { kind: 'wavy' }), { instance: null }, document.bonds[0], 'wavy')
  assert.equal(stillApplicable(clear.commands, { ...document, bonds: [] }), null)
})

/** a1 = a2 (b1, double) with a3 on a1 and a4 on a2; the bond is crossed and has a stated configuration. */
function alkene() {
  const value = document('doc-alkene', 2).document
  value.atoms = [atomWith('a1', 'C', 0, 0), atomWith('a2', 'C', 30, 0), atomWith('a3', 'C', -15, 26), atomWith('a4', 'C', 45, 26)]
  value.bonds = [{ ...bondWith('b1', 'a1', 'a2', 'double'), mark: { kind: 'crossed' } }, bondWith('b2', 'a1', 'a3'), bondWith('b3', 'a2', 'a4')]
  value.stereo = { atoms: [], bonds: [{ bond: 'b1', configuration: { kind: 'unknown' } }] }
  return value
}
const clearMark = { op: 'set_bond_mark', bond: 'b1', mark: null }
const clearBond = { op: 'set_bond_configuration', bond: 'b1', configuration: null }

test('a bond tool click clears a mark first; otherwise it cycles the order after clearing a stored configuration', () => {
  const document = marksDocument()
  assert.deepEqual(clickBond(document, withMark(document, 'b1', { kind: 'wedge', narrow: 'begin' }).bonds[0]), [{ op: 'set_bond_mark', bond: 'b1', mark: null }])
  assert.deepEqual(clickBond(document, document.bonds[0]), [{ op: 'update_bond', bond: 'b1', order: 'double' }])
  const configured = { ...alkene(), bonds: alkene().bonds.map((b) => ({ ...b, mark: null })) }
  assert.deepEqual(clickBond(configured, configured.bonds[0]), [clearBond, { op: 'update_bond', bond: 'b1', order: 'triple' }])
  assert.deepEqual(clickBond(alkene(), alkene().bonds[0]), [clearMark], 'a crossed mark goes before the order changes')
})

test('what an order change would break is cleared in the same transaction', () => {
  const value = alkene()
  const [crossed] = value.bonds
  assert.deepEqual(staleStereoClears(value, crossed, 'triple'), [clearMark, clearBond])
  assert.deepEqual(staleStereoClears(value, crossed, 'double'), [], 'the same order keeps both')
  assert.deepEqual(staleStereoClears(value, crossed, 'single'), [clearMark, clearBond])
  const wedge = withMark(marksDocument(), 'b1', { kind: 'wedge', narrow: 'begin' })
  assert.deepEqual(staleStereoClears(wedge, wedge.bonds[0], 'aromatic'), [clearMark])
  assert.deepEqual(staleStereoClears(wedge, wedge.bonds[0], 'single'), [])
  assert.deepEqual(drawBond(value, null, { atom: 'a1' }, { atom: 'a2' }, 'single'), [clearMark, clearBond, { op: 'update_bond', bond: 'b1', order: 'single' }])
  assert.deepEqual(drawBond(value, null, { atom: 'a1' }, { atom: 'a2' }, 'double'), [{ op: 'update_bond', bond: 'b1', order: 'double' }])
})

test('the inspector changes the kind of a marked or configured bond after clearing what it cannot keep', () => {
  const value = alkene()
  assert.deepEqual(edits.setBondKind('b1', { order: 'single' })(value), [clearMark, clearBond, { op: 'update_bond', bond: 'b1', ...{ order: 'single' } }])
  const wedge = withMark(marksDocument(), 'b1', { kind: 'wedge', narrow: 'begin' })
  assert.deepEqual(edits.setBondKind('b1', { order: 'double' })(wedge), [clearMark, { op: 'update_bond', bond: 'b1', order: 'double' }])
  assert.equal(edits.setBondKind('b1', { order: 'single' })(wedge), null, 'no change to send')
  assert.deepEqual(edits.setBondKind('b2', { order: 'double' })(wedge), [{ op: 'update_bond', bond: 'b2', order: 'double' }])
})

/** What the service does on a refused or accepted transaction, recording what was sent. */
function marksService(...answers) {
  const bodies = []
  service({ '/transactions': answers })
  const inner = globalThis.fetch
  globalThis.fetch = (path, init) => {
    if (init?.body) bodies.push(JSON.parse(init.body))
    return inner(path, init)
  }
  const client = new DocumentClient()
  return { bodies, transact: (commands) => client.transact('doc-marks', 3, commands) }
}
const refusal = (index, reason) =>
  json(422, { api_version: 'v1', code: 'invalid_command', message: `command ${index}: atom a1: the drawing is too flat to determine a configuration`, command_index: index, reason })
const gestureWithDerive = () => markBondGesture(marksDocument(), null, { atom: 'a1' }, { atom: 'a2' }, 'wedge')

test('a mark the service will not derive a configuration for is sent again alone, at the same revision, with the reason', async () => {
  const sent = marksService(() => refusal(1, 'stereo_not_derivable_degenerate'), () => json(200, document('doc-marks', 4)))
  const result = await sendMark(sent.transact, gestureWithDerive())
  assert.equal(result.response.document.revision, 4)
  assert.deepEqual(result.outcome, { kind: 'refused', reason: 'stereo_not_derivable_degenerate', message: 'atom a1: the drawing is too flat to determine a configuration' })
  assert.equal(sent.bodies.length, 2)
  assert.deepEqual(sent.bodies.map((body) => body.expected_revision), [3, 3])
  assert.deepEqual(sent.bodies[0].commands.map((command) => command.op), ['set_bond_mark', 'set_configuration_from_drawing'])
  assert.deepEqual(sent.bodies[1].commands.map((command) => command.op), ['set_bond_mark'])
})

test('a gesture is not sent again for any other refusal, and its refusal is the caller’s', async () => {
  const cases = {
    'a refusal of an earlier command': refusal(0, 'stereo_mark_bond_order'),
    'a refusal that is not about stereo': json(422, { api_version: 'v1', code: 'invalid_command', message: 'm', command_index: 1, reason: 'object_unknown' }),
    'a refusal without a reason': json(422, { api_version: 'v1', code: 'invalid_command', message: 'm', command_index: 1 }),
    'a revision conflict': error(409, 'revision_conflict'),
  }
  for (const [name, reply] of Object.entries(cases)) {
    const sent = marksService(() => reply)
    await assert.rejects(sendMark(sent.transact, gestureWithDerive()), ApiErrorLike, name)
    assert.equal(sent.bodies.length, 1, name)
  }
  const alone = marksService(() => refusal(1, 'stereo_not_derivable_degenerate'))
  const markOnly = { ...gestureWithDerive(), markOnly: null }
  await assert.rejects(sendMark(alone.transact, markOnly), ApiErrorLike, 'a gesture with no derive command has nothing to fall back to')
  assert.equal(alone.bodies.length, 1)
})
const ApiErrorLike = (failure) => failure?.name === 'ApiError'

const stored = (neighbors, turn) => ({ kind: 'tetrahedral', neighbors, turn })
const a1Record = (turn) => ({ atom: 'a1', configuration: stored(['a2', 'a3', 'a5', 'implicit_hydrogen'], turn) })
const markedA1 = (turn) => ({ ...withMark(marksDocument(), 'b1', { kind: 'wedge', narrow: 'begin' }), stereo: { atoms: turn ? [a1Record(turn)] : [], bonds: [] } })
const unstated = () => ({ ...marksDocument(), stereo: { atoms: [], bonds: [] } })
const sent = { kind: 'sent' }

test('the status line says what was stored from the drawing, and which stored configuration it replaced', () => {
  assert.equal(
    describeStereoChange(unstated(), markedA1('clockwise'), sent),
    'Wedge mark on b1, narrow end at a1. Configuration of a1 stored from the drawing (clockwise).',
  )
  assert.equal(
    describeStereoChange({ ...unstated(), stereo: { atoms: [a1Record('anticlockwise')], bonds: [] } }, markedA1('clockwise'), sent),
    'Wedge mark on b1, narrow end at a1. Configuration of a1 stored from the drawing (clockwise). It replaced a stored configuration (anticlockwise); undo restores it.',
  )
  const same = { ...unstated(), stereo: { atoms: [a1Record('clockwise')], bonds: [] } }
  assert.equal(
    describeStereoChange(same, markedA1('clockwise'), sent),
    'Wedge mark on b1, narrow end at a1. Configuration of a1 stored from the drawing (clockwise).',
    'nothing is said to be replaced when the drawing shows what was stored',
  )
})

test('the status line says why nothing was stored: the atom cannot be a center, or the service would not derive it', () => {
  assert.equal(
    describeStereoChange(unstated(), markedA1(null), { kind: 'none', why: 'a1 has 2 bonds; a stereocenter needs 3 or 4.' }),
    'Wedge mark on b1, narrow end at a1. Nothing stored: a1 has 2 bonds; a stereocenter needs 3 or 4.',
  )
  const hashed = { ...unstated(), bonds: unstated().bonds.map((b) => (b.id === 'b1' ? { ...b, mark: { kind: 'hash', narrow: 'begin' } } : b)) }
  assert.equal(
    describeStereoChange(unstated(), hashed, { kind: 'refused', reason: 'stereo_not_derivable_degenerate', message: 'atom a1: the drawing is too flat to determine a configuration' }),
    'Hashed wedge mark on b1, narrow end at a1. Not stored: atom a1: the drawing is too flat to determine a configuration (stereo_not_derivable_degenerate).',
  )
})

test('the status line says a stored configuration is kept when its mark is cleared or its narrow end moves away', () => {
  const kept = 'The stored configuration of a1 is kept; the drawing no longer shows it.'
  assert.equal(describeStereoChange(markedA1('clockwise'), { ...unstated(), stereo: markedA1('clockwise').stereo }, sent), `Wedge mark removed from b1. ${kept}`)
  const flipped = { ...markedA1('clockwise'), bonds: markedA1('clockwise').bonds.map((b) => (b.id === 'b1' ? { ...b, mark: { kind: 'wedge', narrow: 'end' } } : b)) }
  assert.equal(describeStereoChange(markedA1('clockwise'), flipped, { kind: 'none', why: 'a2 has 1 bond; a stereocenter needs 3 or 4.' }), `Wedge mark on b1, narrow end at a2. Nothing stored: a2 has 1 bond; a stereocenter needs 3 or 4. ${kept}`)
})

test('switching a wedge to a hashed wedge at the same narrow end does not say the configuration was left behind', () => {
  const hashed = { ...markedA1('clockwise'), bonds: markedA1('clockwise').bonds.map((b) => (b.id === 'b1' ? { ...b, mark: { kind: 'hash', narrow: 'begin' } } : b)) }
  assert.equal(
    describeStereoChange(markedA1('clockwise'), hashed, sent),
    'Hashed wedge mark on b1, narrow end at a1. Configuration of a1 stored from the drawing (clockwise).',
  )
})

test('the status line says what a change of order removed: the mark, and a stored double-bond configuration', () => {
  const before = alkene()
  const after = { ...before, bonds: before.bonds.map((b) => (b.id === 'b1' ? { ...b, order: 'single', mark: null } : b)), stereo: { atoms: [], bonds: [] } }
  assert.equal(
    describeStereoChange(before, after, { kind: 'none', why: null }),
    'Crossed mark removed from b1: it needs a double bond. The stored configuration of b1 (unknown) was removed: it needs a double bond. Undo restores it.',
  )
  // A configuration the neighbor cascade cleared on a bond that stays double is the reply's notes' to tell.
  const cascaded = { ...before, stereo: { atoms: [], bonds: [] } }
  assert.equal(describeStereoChange(before, cascaded, { kind: 'none', why: null }), null)
  // The service removes the configuration record of a bond it deletes.
  const deleted = { ...before, bonds: [], stereo: { atoms: [], bonds: [] } }
  assert.equal(describeStereoChange(before, deleted, { kind: 'none', why: null }), 'Crossed mark on b1 was removed with the bond.', 'a deleted bond takes its mark with it')
})

test('a crossed mark stores the bond configuration and says so', () => {
  const before = { ...alkene(), bonds: alkene().bonds.map((b) => ({ ...b, mark: null })), stereo: { atoms: [], bonds: [{ bond: 'b1', configuration: { kind: 'relative', begin_neighbor: 'a3', end_neighbor: 'a4', relation: 'opposite_sides' } }] } }
  assert.equal(
    describeStereoChange(before, alkene(), sent),
    'Crossed mark on b1. Configuration of b1 stored from the drawing (unknown). It replaced a stored configuration (opposite sides); undo restores it.',
  )
})

// ---------------------------------------------------------------------------------------------
// Review round 1: a gesture is rebuilt on the latest document; what the status line and the
// sender say about derivation; crossed lines beside a small label box.
// ---------------------------------------------------------------------------------------------

const crowded = (value) => ({ ...value, bonds: [...value.bonds, bondWith('b11', 'a2', 'a4'), bondWith('b12', 'a2', 'a3')] })

test('a queued mark gesture is rebuilt on the latest document, so a bond whose order changed meanwhile is not sent a mark it cannot carry', () => {
  const onScreen = marksDocument()
  const intent = { kind: 'cycle', instance: null, bond: 'b1', mark: 'wedge' }
  // What was built on the screen's document and sent after another edit (Undo during the drag) made b1 double.
  const staleCommands = cycleMark(onScreen, { instance: null }, onScreen.bonds[0], 'wedge').commands
  const doubled = { ...onScreen, bonds: onScreen.bonds.map((b) => (b.id === 'b1' ? { ...b, order: 'double' } : b)) }
  assert.deepEqual(staleCommands[0], { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'begin' } })
  assert.deepEqual(buildMark(doubled, intent), {
    kind: 'refused',
    text: 'A wedge mark needs a single bond; b1 is a double bond. Change it to single first.',
  })
  // The other way round: refused on the screen's document, single now.
  assert.equal(buildMark(onScreen, intent).kind, 'send')
  assert.equal(buildMark(doubled, intent).kind, 'refused')
  assert.equal(buildMark(onScreen, { ...intent, bond: 'b99' }), null, 'a bond that is gone lapses')
})

test('a queued mark gesture asks for the configuration of an atom that became a center meanwhile', () => {
  const onScreen = marksDocument()
  const intent = { kind: 'draw', instance: null, from: { atom: 'a2' }, to: { atom: 'a1' }, mark: 'wedge' }
  const stale = markBondGesture(onScreen, null, intent.from, intent.to, 'wedge')
  assert.equal(stale.markOnly, null, 'a2 had one bond on the screen')
  // Two bonds were added to a2 by edits queued before the gesture ran.
  const built = buildMark(crowded(onScreen), intent)
  assert.deepEqual(built.commands, [{ op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'end' } }, derive(['a2'])])
  assert.equal(built.markOnly.length, 1)
  // And it stops asking for one that stopped being a center.
  const fewer = { ...onScreen, bonds: onScreen.bonds.filter((b) => b.id !== 'b2') }
  assert.equal(buildMark(fewer, { kind: 'cycle', instance: null, bond: 'b1', mark: 'wedge' }).markOnly, null, 'a1 has two bonds now')
  assert.equal(buildMark(onScreen, { ...intent, from: { atom: 'a99' } }), null, 'an atom that is gone lapses')
  assert.equal(buildMark(onScreen, { ...intent, from: { atom: 'a99' }, to: { point: pt(10, 10) } }), null, 'so does a bond drawn from it to a point')
  const placed = { ...crowded(onScreen), scene: { instances: [{ id: 's1', positions: [] }] } }
  assert.notEqual(buildMark(placed, { ...intent, instance: 's1' }), null)
  assert.equal(buildMark(placed, { ...intent, instance: 's9' }), null, 'a placement that is gone lapses')
  assert.equal(buildMark(onScreen, { ...intent, from: { atom: 'a1' }, to: { atom: 'a1' } }), null, 'a drag that draws nothing')
})

// Review round 3: an intent whose placement is gone lapses even when no command names it.
const placedIn = (value, ...ids) => ({
  ...value,
  scene: { instances: ids.map((id) => ({ id, positions: value.atoms.map((atom) => ({ atom: atom.id, x: atom.position.x, y: atom.position.y })) })) },
})

test('a queued mark cycle whose placement was deleted lapses, though no command names the placement', () => {
  const two = placedIn(marksDocument(), 's1', 's3')
  // b4 joins a6 (two bonds) and a7 (one): no center, so the gesture has no derive command and nothing in it names a placement.
  const cycle = { kind: 'cycle', instance: 's2', bond: 'b4', mark: 'wedge' }
  assert.equal(cycleMark(two, { instance: 's2' }, two.bonds.find((b) => b.id === 'b4'), 'wedge').markOnly, null, 'the fixture has no derive command')
  assert.equal(buildMark(two, cycle), null, 'the shared bond is not changed from a placement that is gone')
  assert.equal(buildMark(placedIn(marksDocument()), cycle), null, 'no placement at all')
  assert.equal(buildMark(marksDocument(), cycle), null, 'a document without placements has none to name')
  // Controls: the placement is there, or the document has no placements and the intent names none.
  assert.equal(buildMark(two, { ...cycle, instance: 's3' }).kind, 'send')
  assert.equal(buildMark(marksDocument(), { ...cycle, instance: null }).kind, 'send')
})

test('a queued mark drag whose placement was deleted lapses instead of being placed against another placement', () => {
  const two = placedIn(marksDocument(), 's1', 's3')
  // placeNewAtoms would rebuild the coordinates relative to the first placement left.
  const drag = { kind: 'draw', instance: 's2', from: { atom: 'a14' }, to: { point: pt(430, 140) }, mark: 'wedge' }
  assert.equal(markBondGesture(two, 's2', drag.from, drag.to, 'wedge').kind, 'send', 'the stale builder falls back to the first placement')
  assert.equal(buildMark(two, drag), null, 'a drag from a placement that is gone')
  assert.equal(buildMark(marksDocument(), drag), null, 'a document without placements has none to name')
  // Controls: the placement is there, or the document has no placements and the intent names none.
  assert.equal(buildMark(two, { ...drag, instance: 's1' }).kind, 'send')
  assert.equal(buildMark(marksDocument(), { ...drag, instance: null }).kind, 'send')
})

test('a queued mark gesture lapses when an atom or bond its intent names is gone', () => {
  const value = marksDocument()
  const gone = (id) => ({ ...value, atoms: value.atoms.filter((a) => a.id !== id), bonds: value.bonds.filter((b) => b.id !== id && b.begin !== id && b.end !== id) })
  assert.equal(buildMark(gone('b4'), { kind: 'cycle', instance: null, bond: 'b4', mark: 'wedge' }), null, 'the bond')
  const draw = { kind: 'draw', instance: null, from: { atom: 'a14' }, to: { atom: 'a15' }, mark: 'wedge' }
  assert.notEqual(buildMark(value, draw), null)
  assert.equal(buildMark(gone('a14'), draw), null, 'the drag-from atom')
  assert.equal(buildMark(gone('a15'), draw), null, 'the drag-to atom')
  assert.equal(buildMark(gone('a15'), { ...draw, to: { point: pt(430, 140) }, from: { atom: 'a15' } }), null, 'the drag-from atom of a drag to a point')
})

test('the status line says a stored configuration is kept only when nothing stores or shows it', () => {
  const kept = 'The stored configuration of a1 is kept; the drawing no longer shows it.'
  const wavy = (value) => withMark(value, 'b1', { kind: 'wavy' })
  const stored = { atoms: [{ atom: 'a1', configuration: { kind: 'unknown' } }], bonds: [] }
  // A wedge made wavy at the same center: the gesture stored the unknown configuration there.
  assert.equal(
    describeStereoChange(markedA1('clockwise'), { ...wavy(markedA1('clockwise')), stereo: stored }, sent),
    'Wavy mark on b1. Configuration of a1 stored from the drawing (unknown). It replaced a stored configuration (clockwise); undo restores it.',
  )
  // Made wavy where the service stored nothing (the center could not be read): a wavy bond shows both its
  // atoms (one definition for every mark), so the old record at a1 is not told as unshown.
  assert.equal(
    describeStereoChange(markedA1('clockwise'), wavy(markedA1('clockwise')), { kind: 'none', why: 'a2 has 1 bond; a stereocenter needs 3 or 4.' }),
    'Wavy mark on b1. Nothing stored: a2 has 1 bond; a stereocenter needs 3 or 4.',
  )
  // A wedge moved to the other end of the bond, nothing stored there: the record at a1 stays and nothing shows it.
  const flipped = withMark(markedA1('clockwise'), 'b1', { kind: 'wedge', narrow: 'end' })
  assert.equal(
    describeStereoChange(markedA1('clockwise'), flipped, { kind: 'none', why: 'a2 has 1 bond; a stereocenter needs 3 or 4.' }),
    `Wedge mark on b1, narrow end at a2. Nothing stored: a2 has 1 bond; a stereocenter needs 3 or 4. ${kept}`,
  )
  // Another marked bond still has its narrow end at a1.
  const both = withMark(markedA1('clockwise'), 'b2', { kind: 'hash', narrow: 'begin' })
  assert.equal(describeStereoChange(both, withMark(both, 'b1', null), sent), 'Wedge mark removed from b1.', 'b2 still shows a1')
})

const keptOf = (item) => `The stored configuration of ${item} is kept; the drawing no longer shows it.`
const unknownRecord = (atom) => ({ atom, configuration: { kind: 'unknown' } })
const stereoOf = (atoms, bonds = []) => ({ atoms, bonds })
const wavyB1WithRecords = () => ({ ...withMark(unstated(), 'b1', { kind: 'wavy' }), stereo: stereoOf([unknownRecord('a1'), unknownRecord('a2')]) })

test('a wavy mark still shows both its atoms, so clearing a wedge at one of them does not say it is no longer shown', () => {
  const both = withMark({ ...markedA1('clockwise'), stereo: stereoOf([a1Record('clockwise')]) }, 'b2', { kind: 'wavy' })
  assert.equal(describeStereoChange(both, withMark(both, 'b1', null), sent), 'Wedge mark removed from b1.')
})

test('removing the only wavy mark that showed two atoms says both stored configurations are kept, also when its bond is deleted', () => {
  const stored = wavyB1WithRecords()
  assert.equal(describeStereoChange(stored, withMark(stored, 'b1', null), sent), `Wavy mark removed from b1. ${keptOf('a1')} ${keptOf('a2')}`)
  const deleted = { ...stored, bonds: stored.bonds.filter((bond) => bond.id !== 'b1') }
  assert.equal(describeStereoChange(stored, deleted, { kind: 'none', why: null }), `Wavy mark on b1 was removed with the bond. ${keptOf('a1')} ${keptOf('a2')}`)
})

test('a wavy mark replaced by a wedge at one end says the other end is no longer shown', () => {
  const stored = wavyB1WithRecords()
  const hashed = { ...withMark(stored, 'b1', { kind: 'hash', narrow: 'begin' }), stereo: stereoOf([a1Record('clockwise'), unknownRecord('a2')]) }
  assert.equal(
    describeStereoChange(stored, hashed, sent),
    `Hashed wedge mark on b1, narrow end at a1. Configuration of a1 stored from the drawing (clockwise). It replaced a stored configuration (unknown); undo restores it. ${keptOf('a2')}`,
  )
})

test('a crossed mark shows its double bond like the drawing of it says, so removing it keeps the stored configuration told', () => {
  const crossed = { ...withMark(unstated(), 'b10', { kind: 'crossed' }), stereo: stereoOf([], [{ bond: 'b10', configuration: { kind: 'unknown' } }]) }
  assert.equal(describeStereoChange(crossed, withMark(crossed, 'b10', null), sent), `Crossed mark removed from b10. ${keptOf('b10')}`)
  const deleted = { ...crossed, bonds: crossed.bonds.filter((bond) => bond.id !== 'b10') }
  assert.equal(describeStereoChange(crossed, deleted, sent), `Crossed mark on b10 was removed with the bond. ${keptOf('b10')}`)
})

test('drawing the mark a bond already has sends nothing, unless it can still store the configuration of a center without one', () => {
  const onScreen = markedA1(null)
  const drag = (value) => markBondGesture(value, null, { atom: 'a1' }, { atom: 'a2' }, 'wedge')
  assert.deepEqual(drag(markedA1('clockwise')), { kind: 'refused', text: 'b1 is already drawn that way.' })
  // The same mark with the narrow end at the other atom, or a different kind, is a change.
  assert.equal(markBondGesture(markedA1('clockwise'), null, { atom: 'a1' }, { atom: 'a2' }, 'hash').kind, 'send')
  assert.equal(markBondGesture(markedA1('clockwise'), null, { atom: 'a2' }, { atom: 'a1' }, 'wedge').kind, 'send')
  // No configuration is stored for a1 (the mark was set in the inspector): the drag is sent to store it.
  const mark = { op: 'set_bond_mark', bond: 'b1', mark: { kind: 'wedge', narrow: 'begin' } }
  assert.deepEqual(drag(onScreen), { kind: 'send', commands: [mark, derive(['a1'])], markOnly: [mark], why: null })
  // A narrow end that cannot be a center has nothing to store.
  const flat = markBondGesture(withMark(marksDocument(), 'b1', { kind: 'wedge', narrow: 'end' }), null, { atom: 'a2' }, { atom: 'a1' }, 'wedge')
  assert.deepEqual(flat, { kind: 'refused', text: 'b1 is already drawn that way.' })
  // And the status line says what the derive-only gesture stored.
  assert.equal(describeStereoChange(onScreen, markedA1('clockwise'), sent), 'Configuration of a1 stored from the drawing (clockwise).')
  assert.equal(describeStereoChange(markedA1('clockwise'), markedA1('clockwise'), sent), null, 'nothing changed, nothing said')
  // The service would not derive it for the same mark: the reason is all that changed to say.
  const refused = { kind: 'refused', reason: 'stereo_not_derivable_degenerate', message: 'atom a1: the drawing is too flat to determine a configuration' }
  assert.equal(describeStereoChange(onScreen, onScreen, refused), 'Not stored: atom a1: the drawing is too flat to determine a configuration (stereo_not_derivable_degenerate).')
})

test('only the derive command refused for a reason it gives is sent again without it', async () => {
  for (const reason of ['stereo_mark_bond_order', 'stereo_record_invalid']) {
    const sentOnce = marksService(() => refusal(1, reason))
    await assert.rejects(sendMark(sentOnce.transact, gestureWithDerive()), ApiErrorLike, reason)
    assert.equal(sentOnce.bodies.length, 1, reason)
  }
  for (const reason of ['stereo_center_neighbors', 'stereo_double_bond_required', 'stereo_not_derivable_no_marks']) {
    const sentTwice = marksService(() => refusal(1, reason), () => json(200, document('doc-marks', 4)))
    assert.equal((await sendMark(sentTwice.transact, gestureWithDerive())).outcome.reason, reason)
  }
  // A gesture that makes the bond first: the derive command is the third, a refusal of the second is not its refusal.
  const drawn = markBondGesture(marksDocument(), null, { atom: 'a1' }, { point: pt(100, 70) }, 'wedge')
  assert.equal(drawn.commands.length, 3)
  const mark = marksService(() => refusal(1, 'stereo_not_derivable_degenerate'))
  await assert.rejects(sendMark(mark.transact, drawn), ApiErrorLike)
  assert.equal(mark.bodies.length, 1)
  const derived = marksService(() => refusal(2, 'stereo_not_derivable_degenerate'), () => json(200, document('doc-marks', 4)))
  assert.equal((await sendMark(derived.transact, drawn)).outcome.kind, 'refused')
  assert.equal(derived.bodies[1].commands.length, 2)
})

test('when the mark sent alone is refused too, that failure is the caller’s and no reason is returned', async () => {
  const conflict = marksService(() => refusal(1, 'stereo_not_derivable_degenerate'), () => error(409, 'revision_conflict'))
  await assert.rejects(sendMark(conflict.transact, gestureWithDerive()), (failure) => failure?.name === 'ApiError' && failure.code === 'revision_conflict')
  assert.equal(conflict.bodies.length, 2)
})

test('deleting a marked bond says its mark went with it, and a stored configuration that stays is told as kept', () => {
  const marked = markedA1('clockwise')
  const gone = { ...marked, bonds: marked.bonds.filter((b) => b.id !== 'b1'), stereo: { atoms: [], bonds: [] } }
  assert.equal(describeStereoChange(marked, gone, { kind: 'none', why: null }), 'Wedge mark on b1 was removed with the bond.')
  const keeps = { ...gone, stereo: marked.stereo }
  assert.equal(
    describeStereoChange(marked, keeps, { kind: 'none', why: null }),
    'Wedge mark on b1 was removed with the bond. The stored configuration of a1 is kept; the drawing no longer shows it.',
  )
  const unmarked = unstated()
  assert.equal(describeStereoChange(unmarked, { ...unmarked, bonds: unmarked.bonds.filter((b) => b.id !== 'b1') }, { kind: 'none', why: null }), null, 'a plain bond says nothing')
  // Deleting an atom takes its bonds, marked or not.
  const atomGone = { ...gone, atoms: gone.atoms.filter((a) => a.id !== 'a1'), bonds: gone.bonds.filter((b) => b.begin !== 'a1' && b.end !== 'a1') }
  assert.equal(describeStereoChange(marked, atomGone, { kind: 'none', why: null }), 'Wedge mark on b1 was removed with the bond.')
})

test('a crossed double bond keeps its ink out of label boxes the line width makes it start beside', () => {
  // A 10 pt line spaces its lines 8.3 times as far apart as a 1.2 pt one: 18 pt from the axis, outside a 20 pt tall label.
  const lineWidth = 10
  const spacing = bondSpacing(lineWidth)
  const box = (at) => ({ x: at.x - 18, y: at.y - 8, width: 36, height: 16 })
  for (const angle of [0, 0.5, Math.PI / 2, 2.4, 3.8]) {
    const begin = pt(0, 0)
    const end = pt(Math.cos(angle) * 70, Math.sin(angle) * 70)
    const boxes = [box(begin), box(end)]
    const strokes = strokesOf({ kind: 'crossed' }, begin, end, boxes, spacing, lineWidth, 'double')
    assert.equal(strokes.length, 2, `angle ${angle}`)
    for (const stroke of strokes) {
      for (let k = 0; k <= 40; k += 1) {
        const x = stroke.x1 + ((stroke.x2 - stroke.x1) * k) / 40
        const y = stroke.y1 + ((stroke.y2 - stroke.y1) * k) / 40
        for (const label of boxes) {
          const within = x > label.x + 0.01 && x < label.x + label.width - 0.01 && y > label.y + 0.01 && y < label.y + label.height - 0.01
          assert.equal(within, false, `angle ${angle}: (${x}, ${y}) is inside a label box`)
        }
      }
    }
  }
})

// ---------------------------------------------------------------------------------------------
// The stereo agreement indicator: the `chem.stereo` reply, when it may be shown, the rows and
// sentences, the one-click edits and when the service is asked.
// ---------------------------------------------------------------------------------------------
const agreementModule = await import('../apps/web/src/editor/stereo-agreement.ts')
const {
  agreementRings, agreementRows, agreementView, actionName, CheckScheduler, listedDiagnostics, NOT_JUDGED,
  resolutionCommands, resolutionStatus, rowAction, rowSentence, summarySentence, updatableRows, viewSentence,
} = agreementModule
const { readStereoInspection, readStereoDiagnostics } = await import('../apps/web/src/lib/stereo-contract.ts')

/** `value` placed in each of `ids` as the service sends a scene a document reply accepts. */
const scened = (value, ...ids) => ({
  ...value,
  molecule: { id: 'm1' },
  scene: {
    pages: [{ id: 'p1' }],
    instances: ids.map((id) => ({ id, page: 'p1', molecule: 'm1', positions: value.atoms.map((atom) => ({ atom: atom.id, x: atom.position.x, y: atom.position.y })) })),
  },
})
const tetra = (neighbors, turn) => ({ kind: 'tetrahedral', neighbors, turn })
const A2 = ['a1', 'a3', 'a4', 'implicit_hydrogen']
const A4 = ['a2', 'a5', 'a6', 'implicit_hydrogen']
const drawnOf = (instance, configuration, agreement, extra = {}) => ({ instance, configuration, reason: null, agreement, label_differs: false, ...extra })

/** The public two-center drawing in one placement; a2 stores `clockwise` (the drawing shows anticlockwise), a4 stores anticlockwise. */
function agreementDocument(instances = ['s1']) {
  const value = scened(publicFixture(true), ...instances)
  value.stereo = { atoms: [{ atom: 'a2', configuration: tetra(A2, 'clockwise') }, { atom: 'a4', configuration: tetra(A4, 'anticlockwise') }], bonds: [] }
  return value
}
const publicItems = (instances = ['s1']) => [
  { atom: 'a2', stored: tetra(A2, 'clockwise'), drawn: instances.map((id) => drawnOf(id, tetra(A2, 'anticlockwise'), 'disagrees')) },
  { atom: 'a4', stored: tetra(A4, 'anticlockwise'), drawn: instances.map((id) => drawnOf(id, tetra(A4, 'anticlockwise'), 'agrees')) },
]
const graphOf = (value) => ({ atoms: value.atoms, bonds: value.bonds, stereo: value.stereo, instances: value.scene.instances.map((i) => i.id) })
const resultOf = (value, atoms, bonds = [], instances = value.scene.instances.map((i) => i.id)) => ({
  snapshot: { document: value, session: document('x').session, recovery: null, notes: [] },
  valence_model: 'chemifree-valence-3', stereogenicity_assessed: false, instances, atoms, bonds,
})
const replyOf = (value, result, diagnostics = [
  { code: 'drawing_disagrees', severity: 'warning', message: 'Atom a2: the drawing in s1 shows a different configuration from the stored one.' },
  { code: 'stereo_not_checked', severity: 'info', message: 'Only the stored configurations are compared with the drawing.' },
]) => ({ envelope: 'operation_result', contract: 1, outcome: 'ok', model_id: 'chemifree-stereo-1', document_revision: value.revision, loss_report: [], result, diagnostics })
const inspected = async (value, reply) => {
  service({ '/api/v1/operations': [() => json(200, reply)] })
  return new DocumentClient().inspectStereo(value.id)
}

test('the public two-center drawing against its stored configurations: one disagrees, one agrees, and the row says which to update', async () => {
  const value = agreementDocument()
  const inspection = await inspected(value, replyOf(value, resultOf(value, publicItems())))
  assert.equal(inspection.model, 'chemifree-stereo-1')
  assert.deepEqual(inspection.instances, ['s1'])
  const rows = agreementRows(inspection)
  assert.deepEqual(rows.map((row) => [row.item.id, row.instance, row.agreement, rowAction(row.agreement)]), [
    ['a2', 's1', 'disagrees', 'update'],
    ['a4', 's1', 'agrees', null],
  ])
  assert.equal(rowSentence(rows[0]), 'Stored clockwise; the drawing in s1 shows anticlockwise.')
  assert.equal(rowSentence(rows[1]), 'Stored anticlockwise; the drawing in s1 shows the same.')
  assert.equal(summarySentence(rows), 'Compared with the drawing: 1 disagrees with the drawing, 1 agrees.')
  // The mirror image of the stored value agrees with a2 and disagrees with a4.
  const mirrored = agreementDocument()
  mirrored.stereo.atoms = [{ atom: 'a2', configuration: tetra(A2, 'anticlockwise') }, { atom: 'a4', configuration: tetra(A4, 'clockwise') }]
  const flipped = await inspected(mirrored, replyOf(mirrored, resultOf(mirrored, [
    { atom: 'a2', stored: tetra(A2, 'anticlockwise'), drawn: [drawnOf('s1', tetra(A2, 'anticlockwise'), 'agrees')] },
    { atom: 'a4', stored: tetra(A4, 'clockwise'), drawn: [drawnOf('s1', tetra(A4, 'anticlockwise'), 'disagrees')] },
  ])))
  assert.deepEqual(agreementRows(flipped).map((row) => row.agreement), ['agrees', 'disagrees'])
})

/** A document that shows each of the five agreements, a label that differs, and two placements for one of them. */
function mixedFixture() {
  const value = scened(marksDocument(), 's1', 's2')
  const around1 = ['a2', 'a3', 'a5']
  value.stereo = {
    atoms: [{ atom: 'a1', configuration: tetra([...around1, 'lone_pair'], 'clockwise') }],
    bonds: [{ bond: 'b10', configuration: { kind: 'unknown' } }],
  }
  value.bonds = value.bonds.map((bond) => (bond.id === 'b10' ? { ...bond, mark: { kind: 'crossed' } } : bond))
  const hydrogen = [...around1, 'implicit_hydrogen']
  const atoms = [
    { atom: 'a1', stored: value.stereo.atoms[0].configuration, drawn: [
      drawnOf('s1', tetra(hydrogen, 'clockwise'), 'agrees', { label_differs: true }),
      drawnOf('s2', tetra(hydrogen, 'anticlockwise'), 'disagrees', { label_differs: true }),
    ] },
    { atom: 'a6', stored: null, drawn: ['s1', 's2'].map((id) => ({ ...drawnOf(id, null, 'neither'), reason: 'stereo_not_derivable_degenerate' })) },
    { atom: 'a9', stored: null, drawn: ['s1', 's2'].map((id) => drawnOf(id, tetra(['a10', 'a11', 'a12', 'a13'], 'anticlockwise'), 'only_drawn')) },
  ]
  const bonds = [{ bond: 'b10', stored: { kind: 'unknown' }, drawn: ['s1', 's2'].map((id) => ({ instance: id, configuration: null, reason: 'stereo_not_derivable_no_reference', agreement: 'only_stored' })) }]
  return { value, atoms, bonds }
}

test('a reply shows every agreement, the differing label, and one row per item and placement', async () => {
  const { value, atoms, bonds } = mixedFixture()
  const inspection = await inspected(value, replyOf(value, resultOf(value, atoms, bonds)))
  const rows = agreementRows(inspection)
  assert.deepEqual(rows.map((row) => `${row.item.id}/${row.instance}/${row.agreement}`), [
    'a1/s1/agrees', 'a1/s2/disagrees', 'a6/s1/neither', 'a6/s2/neither', 'a9/s1/only_drawn', 'a9/s2/only_drawn', 'b10/s1/only_stored', 'b10/s2/only_stored',
  ])
  assert.equal(rowSentence(rows[0]), 'Stored clockwise; the drawing in s1 shows the same (the hydrogen or lone pair is named differently).')
  assert.equal(rowSentence(rows[2]), 'Nothing is stored and the drawing in s1 gives no configuration: the drawing is too flat to determine a configuration (stereo_not_derivable_degenerate).')
  assert.equal(rowSentence(rows[4]), 'The drawing in s1 shows anticlockwise; nothing is stored.')
  assert.equal(rowSentence(rows[6]), 'Stored unknown; the drawing in s1 shows nothing usable: a double-bond end has no neighbor to read a direction from (stereo_not_derivable_no_reference).')
  assert.deepEqual(rows.map((row) => rowAction(row.agreement)), [null, 'update', null, null, 'store', 'store', 'remove', 'remove'])
  // Two placements: "Update all" needs a single one.
  assert.deepEqual(updatableRows(inspection, rows), [])
})

const refused = (mutate, value = agreementDocument(), items = publicItems()) => {
  const result = structuredClone(resultOf(value, items))
  mutate(result)
  return readStereoInspection(result, graphOf(value))
}

test('the reply reader accepts the documented reply and refuses each way it can be made untrue', () => {
  const value = agreementDocument()
  assert.notEqual(refused(() => {}), null)
  const tampers = {
    'an extra field in the result': (r) => { r.extra = 1 },
    'an extra field in an atom entry': (r) => { r.atoms[0].extra = 1 },
    'an extra field in a drawn center': (r) => { r.atoms[0].drawn[0].extra = 1 },
    'a stored configuration the snapshot does not hold': (r) => { r.atoms[0].stored = tetra(A2, 'anticlockwise') },
    'a stored configuration where the snapshot has none': (r) => { r.atoms.push({ atom: 'a1', stored: tetra(A2, 'clockwise'), drawn: [drawnOf('s1', null, 'neither', { reason: 'stereo_not_derivable_no_marks' })] }) },
    'a stored record of the snapshot that is not an item': (r) => { r.atoms.pop() },
    'agrees for different turns': (r) => { r.atoms[0].drawn[0].agreement = 'agrees' },
    'disagrees for the same turn': (r) => { r.atoms[1].drawn[0].agreement = 'disagrees' },
    'agrees for another kind': (r) => { r.atoms[1].drawn[0].configuration = { kind: 'unknown', neighbors: A4 } },
    'only_drawn though a record is stored': (r) => { r.atoms[0].drawn[0].agreement = 'only_drawn' },
    'an agreement it does not have': (r) => { r.atoms[0].drawn[0].agreement = 'mostly' },
    'a reason and a configuration together': (r) => { r.atoms[0].drawn[0].reason = 'stereo_not_derivable_degenerate' },
    'neither a reason nor a configuration': (r) => { r.atoms[0].drawn[0].configuration = null },
    'a reason it does not have': (r) => { r.atoms[0].drawn[0] = drawnOf('s1', null, 'only_stored', { reason: 'stereo_not_derivable_because' }) },
    'a label flag that is not so': (r) => { r.atoms[0].drawn[0].label_differs = true },
    'a configuration for other neighbors': (r) => { r.atoms[0].drawn[0].configuration = tetra(['a1', 'a3', 'a5', 'implicit_hydrogen'], 'anticlockwise') },
    'an instance the snapshot does not have': (r) => { r.instances = ['s9']; r.atoms[0].drawn[0].instance = 's9'; r.atoms[1].drawn[0].instance = 's9' },
    'an instance left out of an entry': (r) => { r.atoms[0].drawn = [] },
    'atoms out of order': (r) => { r.atoms.reverse() },
    'an atom listed twice': (r) => { r.atoms.push(r.atoms[1]) },
    'an atom the snapshot does not have': (r) => { r.atoms[1].atom = 'a99' },
    'a stereogenicity judgment': (r) => { r.stereogenicity_assessed = true },
    'no valence model': (r) => { delete r.valence_model },
  }
  for (const [name, tamper] of Object.entries(tampers)) assert.equal(refused(tamper), null, name)
  // Two placements: entries come in the order of `instances`, and that is the snapshot's order.
  const two = agreementDocument(['s1', 's2'])
  assert.notEqual(refused(() => {}, two, publicItems(['s1', 's2'])), null)
  assert.equal(refused((r) => { r.instances.reverse(); r.atoms.forEach((a) => a.drawn.reverse()) }, two, publicItems(['s1', 's2'])), null, 'instances out of order')
  assert.equal(refused((r) => { r.atoms[0].drawn.reverse() }, two, publicItems(['s1', 's2'])), null, 'entries out of order')
  assert.equal(refused((r) => { r.instances.reverse() }, two, publicItems(['s1', 's2'])), null, 'instances named out of order')
  // Identifiers go by serial: a9 before a10, and s2 before s10.
  const nines = scened(marksDocument(), 's1')
  nines.stereo = { atoms: [], bonds: [] }
  const neither = (atom) => ({ atom, stored: null, drawn: [{ ...drawnOf('s1', null, 'neither'), reason: 'stereo_not_derivable_no_marks' }] })
  assert.notEqual(refused(() => {}, nines, [neither('a9'), neither('a10')]), null)
  assert.equal(refused(() => {}, nines, [neither('a10'), neither('a9')]), null)
  const many = agreementDocument(['s2', 's10'])
  assert.notEqual(refused(() => {}, many, publicItems(['s2', 's10'])), null)
  assert.notEqual(value, null)
})

test('a double bond item must be a double bond of the snapshot with its own stored record, and drawn entries must fit it', () => {
  const { value, atoms, bonds } = mixedFixture()
  const read = (mutate) => {
    const result = structuredClone(resultOf(value, atoms, bonds))
    mutate(result)
    return readStereoInspection(result, graphOf(value))
  }
  assert.notEqual(read(() => {}), null)
  assert.equal(read((r) => { r.bonds[0].bond = 'b1' }), null, 'a single bond')
  assert.equal(read((r) => { r.bonds.unshift({ bond: 'b1', stored: null, drawn: ['s1', 's2'].map((id) => ({ instance: id, configuration: null, reason: 'stereo_not_derivable_no_reference', agreement: 'neither' })) }) }), null, 'a single bond with nothing stored')
  assert.equal(read((r) => { r.bonds[0].stored = { kind: 'relative', begin_neighbor: 'a1', end_neighbor: 'a2', relation: 'same_side' } }), null)
  assert.equal(read((r) => { r.bonds[0].drawn[0].configuration = { kind: 'unknown' }; r.bonds[0].drawn[0].reason = null }), null, 'agreement not what unknown against unknown decides')
  assert.equal(read((r) => { r.bonds[0].drawn[0].label_differs = false }), null, 'a field a double bond does not have')
  assert.equal(read((r) => { r.bonds = [] }), null, 'the stored double-bond record is not an item')
})

test('the advisory findings may carry codes the client does not know, but must be findings', () => {
  assert.deepEqual(readStereoDiagnostics([{ code: 'a_new_code', severity: 'info', message: 'm', details: { x: 1 } }]), [{ code: 'a_new_code', severity: 'info', message: 'm' }])
  for (const bad of [null, [{ code: 'x', severity: 'error', message: 'm' }], [{ code: 'x', severity: 'info' }], [{ severity: 'info', message: 'm' }], 'none']) {
    assert.equal(readStereoDiagnostics(bad), null, JSON.stringify(bad))
  }
  const found = [
    { code: 'stereo_not_checked', severity: 'info', message: 'fixed' },
    { code: 'novel', severity: 'info', message: 'n' },
    { code: 'drawing_weak_evidence', severity: 'info', message: 'w' },
    { code: 'drawing_disagrees', severity: 'warning', message: 'd' },
  ]
  assert.deepEqual(listedDiagnostics(found).map((f) => f.code), ['drawing_disagrees', 'drawing_weak_evidence', 'novel'], 'known first in the documented order, the fixed sentence left to the panel')
  assert.equal(NOT_JUDGED, 'Only stored configurations are compared with the drawing. CIP labels describe the stored configuration, not the drawing.')
  assert.doesNotMatch(NOT_JUDGED, /No CIP label is computed/)
})

test('inspectStereo asks for every placement of the drawing and refuses what is not the service reply for it', async () => {
  const value = agreementDocument()
  const good = replyOf(value, resultOf(value, publicItems()))
  const scripted = service({ '/api/v1/operations': [() => json(200, good)] })
  await new DocumentClient().inspectStereo(value.id)
  assert.deepEqual(scripted.writes.map((write) => write.parameters), [{ document_id: value.id }])
  const incompatible = async (reply) => {
    service({ '/api/v1/operations': [() => json(200, reply)] })
    await assert.rejects(new DocumentClient().inspectStereo(value.id), { code: 'incompatible_response' })
  }
  await incompatible({ ...good, model_id: 'chemifree-stereo-2' })
  await incompatible({ ...good, loss_report: [{ feature: 'stereo' }] })
  await incompatible({ ...good, document_revision: value.revision + 1 })
  await incompatible({ ...good, diagnostics: [{ code: 'x', severity: 'error', message: 'm' }] })
  await incompatible({ ...good, result: { ...good.result, snapshot: { ...good.result.snapshot, document: { ...value, id: 'other' } } } })
  // A refusal or the limit says what the service says.
  const limit = { envelope: 'operation_result', contract: 1, outcome: 'error', model_id: 'chemifree-stereo-1', document_revision: 2, loss_report: [], result: null,
    diagnostics: [{ code: 'resource_limit', severity: 'error', message: '30000 items in 2 instances would be 60000 entries, above the limit of 20000.' }] }
  service({ '/api/v1/operations': [() => json(200, limit)] })
  await assert.rejects(new DocumentClient().inspectStereo(value.id), { code: 'stereo_unavailable', message: limit.diagnostics[0].message })
  service({ '/api/v1/operations': [() => json(200, { ...good, outcome: 'partial' })] })
  await assert.rejects(new DocumentClient().inspectStereo(value.id), { code: 'stereo_unavailable' })
})

test('a reply is shown only for the drawing on screen, positions and service process included', async () => {
  const value = agreementDocument()
  const inspection = await inspected(value, replyOf(value, resultOf(value, publicItems())))
  const ok = { kind: 'ok', generation: 0, inspection }
  const view = (shown, outcome = ok, generation = 0, busy = false, automatic = true) => agreementView(shown, outcome, generation, busy, automatic).kind
  assert.equal(view(value), 'ready')
  assert.equal(view(structuredClone(value)), 'ready')
  const moved = structuredClone(value)
  moved.scene.instances[0].positions[0].x += 1
  assert.equal(view(moved), 'checking', 'a moved atom makes the reply stale: positions are what it compares')
  assert.equal(view({ ...value, revision: value.revision + 1 }), 'checking')
  assert.equal(view(value, ok, 1), 'checking', 'another service process')
  assert.equal(view(value, null), 'checking')
  assert.equal(view(value, null, 0, false, false), 'unchecked', 'checking is off and nothing was asked for this drawing')
  assert.equal(view(value, null, 0, true, false), 'checking', 'a check the user asked for is running')
  const plain = scened(publicFixture(false), 's1')
  assert.equal(view(plain), 'none', 'a drawing with nothing stored or marked is never compared')
  const failed = { kind: 'failed', generation: 0, snapshot: value, message: 'boom' }
  assert.deepEqual(agreementView(value, failed, 0, false, true), { kind: 'failed', message: 'boom' })
  assert.equal(view(moved, failed), 'checking', 'a failure belongs to the drawing it was asked about')
  assert.deepEqual(agreementView(value, failed, 0, true, true), { kind: 'checking' }, 'pressing Check again shows that a retry is running')
  assert.deepEqual(agreementView(value, failed, 0, true, false), { kind: 'checking' }, 'also with automatic checking off')
  assert.equal(view(value, ok, 0, true), 'ready', 'a reply stays shown while the same drawing is checked again')
})

test('what the page says while checking, between checks and after one never says the drawing has no stereo', async () => {
  const value = agreementDocument()
  const inspection = await inspected(value, replyOf(value, resultOf(value, publicItems())))
  assert.equal(viewSentence({ kind: 'checking' }), 'Checking the drawing…')
  assert.equal(viewSentence({ kind: 'unchecked' }), 'Not compared with the drawing yet. Press Check now.')
  assert.equal(viewSentence({ kind: 'none' }), null)
  assert.equal(viewSentence({ kind: 'ready', inspection }), 'Compared with the drawing: 1 disagrees with the drawing, 1 agrees.')
  assert.equal(summarySentence([]), 'Nothing stored or drawn bears on a configuration, so there is nothing to compare. Whether an atom is a stereocenter is not assessed.')
  const { value: mixed, atoms, bonds } = mixedFixture()
  const rows = agreementRows(await inspected(mixed, replyOf(mixed, resultOf(mixed, atoms, bonds))))
  assert.equal(summarySentence(rows),
    'Compared with the drawing: 1 disagrees with the drawing, 1 agrees, 2 are stored only, 2 are drawn only, 2 are marked but unreadable.')
  assert.equal(summarySentence(rows.slice(4, 5)), 'Compared with the drawing: 1 is drawn only.')
  for (const sentence of [viewSentence({ kind: 'checking' }), viewSentence({ kind: 'unchecked' }), summarySentence(rows), summarySentence([]), agreementModule.NOTHING_TO_COMPARE]) {
    assert.doesNotMatch(sentence, /no stereo/i)
  }
  assert.match(agreementModule.NOTHING_TO_COMPARE, /is not assessed\.$/)
})

test('rings are for the rows that do not agree, named for the item and the placement', async () => {
  const { value, atoms, bonds } = mixedFixture()
  const rows = agreementRows(await inspected(value, replyOf(value, resultOf(value, atoms, bonds))))
  const rings = agreementRings(rows)
  assert.deepEqual(rings.map((ring) => `${ring.item.id}/${ring.instance}/${ring.agreement}`), [
    'a1/s2/disagrees', 'a6/s1/neither', 'a6/s2/neither', 'a9/s1/only_drawn', 'a9/s2/only_drawn', 'b10/s1/only_stored', 'b10/s2/only_stored',
  ])
  assert.equal(rings[0].name, 'Center a1: the stored configuration disagrees with the drawing in s2')
  assert.equal(rings[5].name, 'Double bond b10: a stored configuration that the drawing in s1 does not show')
  assert.equal(agreementRings(agreementRows(await inspected(agreementDocument(), replyOf(agreementDocument(), resultOf(agreementDocument(), publicItems()))))).length, 1)
})

test('a row button names its item and placement; "Update all" needs one placement and two disagreements', async () => {
  const value = agreementDocument()
  value.stereo.atoms[1] = { atom: 'a4', configuration: tetra(A4, 'clockwise') }
  const items = publicItems()
  items[1] = { atom: 'a4', stored: tetra(A4, 'clockwise'), drawn: [drawnOf('s1', tetra(A4, 'anticlockwise'), 'disagrees')] }
  const inspection = await inspected(value, replyOf(value, resultOf(value, items)))
  const rows = agreementRows(inspection)
  assert.deepEqual(rows.map((row) => actionName(row, rowAction(row.agreement))), ['Update a2 in s1 from the drawing', 'Update a4 in s1 from the drawing'])
  assert.deepEqual(updatableRows(inspection, rows).map((row) => row.item.id), ['a2', 'a4'])
  assert.deepEqual(updatableRows(inspection, rows.slice(0, 1)), [], 'one is not enough')
  const twice = agreementDocument(['s1', 's2'])
  const both = publicItems(['s1', 's2'])
  both[1] = { atom: 'a4', stored: tetra(A4, 'clockwise'), drawn: ['s1', 's2'].map((id) => drawnOf(id, tetra(A4, 'anticlockwise'), 'disagrees')) }
  twice.stereo.atoms[1] = { atom: 'a4', configuration: tetra(A4, 'clockwise') }
  const several = await inspected(twice, replyOf(twice, resultOf(twice, both)))
  assert.equal(agreementRows(several).filter((row) => row.agreement === 'disagrees').length, 4)
  assert.deepEqual(updatableRows(several, agreementRows(several)), [], 'which placement to read is the user\'s to choose, one row at a time')
  const { value: mixed, atoms, bonds } = mixedFixture()
  const m = agreementRows(await inspected(mixed, replyOf(mixed, resultOf(mixed, atoms, bonds))))
  assert.equal(actionName(m[4], 'store'), 'Store a9 in s1 from the drawing')
  assert.equal(actionName(m[6], 'remove'), 'Remove the stored configuration of b10 (row s1)')
})

test('a click is built from the latest drawing and lapses when what it names is gone', () => {
  const value = agreementDocument(['s1', 's2'])
  const at = { kind: 'atom', id: 'a2' }
  assert.deepEqual(resolutionCommands(value, { kind: 'update', item: at, instance: 's2' }), [derive(['a2'], { instance: 's2' })])
  assert.deepEqual(resolutionCommands(value, { kind: 'store', item: at, instance: 's1' }), [derive(['a2'], { instance: 's1' })])
  assert.deepEqual(resolutionCommands(value, { kind: 'remove', item: at, instance: 's1' }), [{ op: 'set_atom_configuration', atom: 'a2', configuration: null }])
  assert.deepEqual(resolutionCommands(value, { kind: 'update_all', items: [at, { kind: 'atom', id: 'a4' }], instance: 's1' }), [derive(['a2', 'a4'], { instance: 's1' })])
  const bond = { kind: 'bond', id: 'b3' }
  assert.deepEqual(resolutionCommands({ ...value, stereo: { atoms: [], bonds: [{ bond: 'b3', configuration: { kind: 'unknown' } }] } }, { kind: 'remove', item: bond, instance: 's1' }),
    [{ op: 'set_bond_configuration', bond: 'b3', configuration: null }])
  assert.deepEqual(resolutionCommands(value, { kind: 'update', item: bond, instance: 's1' }), [{ op: 'set_configuration_from_drawing', bonds: ['b3'], instance: 's1' }])
  // Lapses: the atom, the placement or the stored record is gone.
  const gone = { ...value, atoms: value.atoms.filter((a) => a.id !== 'a2'), bonds: value.bonds.filter((b) => b.begin !== 'a2' && b.end !== 'a2') }
  assert.equal(resolutionCommands(gone, { kind: 'update', item: at, instance: 's1' }), null)
  assert.equal(resolutionCommands(gone, { kind: 'update_all', items: [at, { kind: 'atom', id: 'a4' }], instance: 's1' }), null, 'one gone lapses all')
  assert.equal(resolutionCommands(value, { kind: 'update', item: at, instance: 's9' }), null, 'the placement is gone')
  assert.equal(resolutionCommands(placedIn(publicFixture(true)), { kind: 'update', item: at, instance: 's1' }), null, 'no placement to read')
  assert.equal(resolutionCommands({ ...value, stereo: { atoms: [], bonds: [] } }, { kind: 'remove', item: at, instance: 's1' }), null, 'nothing stored to remove')
})

test('the status after a click names the stated value it replaced or removed', () => {
  const before = agreementDocument()
  const stored = (document, atom, turn) => ({ ...document, stereo: { ...document.stereo, atoms: document.stereo.atoms.map((r) => (r.atom === atom ? { atom, configuration: tetra(r.configuration.neighbors, turn) } : r)) } })
  const item = { kind: 'atom', id: 'a2' }
  assert.equal(resolutionStatus(before, stored(before, 'a2', 'anticlockwise'), { kind: 'update', item, instance: 's1' }),
    'a2: replaced stored clockwise with anticlockwise from the drawing in s1. Undo restores it.')
  const empty = { ...before, stereo: { atoms: [before.stereo.atoms[1]], bonds: [] } }
  assert.equal(resolutionStatus(empty, before, { kind: 'store', item, instance: 's1' }), 'a2: stored clockwise from the drawing in s1.')
  assert.equal(resolutionStatus(before, empty, { kind: 'remove', item, instance: 's1' }), 'a2: removed the stored clockwise. Undo restores it.')
  assert.equal(resolutionStatus(before, before, { kind: 'update', item, instance: 's1' }), 'a2: the stored clockwise already matched the drawing in s1; nothing changed.')
  assert.equal(resolutionStatus(empty, empty, { kind: 'remove', item, instance: 's1' }), null)
  const both = stored(stored(before, 'a2', 'anticlockwise'), 'a4', 'clockwise')
  assert.equal(resolutionStatus(before, both, { kind: 'update_all', items: [item, { kind: 'atom', id: 'a4' }], instance: 's1' }),
    'Replaced 2 stored configurations with what the drawing in s1 shows: a2 (clockwise to anticlockwise), a4 (anticlockwise to clockwise). Undo restores them.')
  assert.equal(resolutionStatus(before, before, { kind: 'update_all', items: [item], instance: 's1' }), 'Nothing changed: the stored configurations already matched the drawing in s1.')
})

/** A clock the test advances; a request is a promise the test settles. */
function schedulerFixture(delay) {
  let now = 0
  let next = 1
  const tasks = new Map()
  const timers = {
    set: (task, ms) => { tasks.set(next, { at: now + ms, task }); return next++ },
    clear: (handle) => { tasks.delete(handle) },
  }
  const requests = []
  const scheduler = new CheckScheduler(() => new Promise((resolve) => { requests.push(resolve) }), timers, delay)
  const settled = () => new Promise((resolve) => setImmediate(resolve))
  return {
    scheduler, requests, timers,
    waiting: () => tasks.size,
    advance: (ms) => {
      const end = now + ms
      for (;;) {
        const due = [...tasks].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0]
        if (!due) break
        tasks.delete(due[0])
        now = due[1].at
        due[1].task()
      }
      now = end
    },
    finish: async () => { requests.shift()(); await settled() },
  }
}

test('changes within 300 ms make one request, which waits for the last change', () => {
  assert.equal(agreementModule.CHECK_DELAY_MS, 300)
  const { scheduler, requests, advance } = schedulerFixture()
  scheduler.changed()
  advance(299)
  assert.equal(requests.length, 0)
  scheduler.changed()
  advance(299)
  assert.equal(requests.length, 0, 'the second change restarted the wait')
  advance(1)
  assert.equal(requests.length, 1)
})

test('one request is in flight at a time, and any number of changes meanwhile cause exactly one more', async () => {
  const { scheduler, requests, advance, finish } = schedulerFixture()
  scheduler.changed()
  advance(300)
  assert.equal(requests.length, 1)
  scheduler.changed()
  scheduler.changed()
  scheduler.changed()
  advance(1000)
  assert.equal(requests.length, 1, 'nothing starts while one is in flight')
  await finish()
  advance(300)
  assert.equal(requests.length, 1, 'one more, not three')
  await finish()
  advance(5000)
  assert.equal(requests.length, 0, 'and then it is quiet')
})

test('no request starts while the user is dragging; one starts after the release', () => {
  const { scheduler, requests, advance } = schedulerFixture()
  scheduler.pause(true)
  scheduler.changed()
  advance(5000)
  assert.equal(requests.length, 0)
  scheduler.pause(false)
  advance(299)
  assert.equal(requests.length, 0)
  advance(1)
  assert.equal(requests.length, 1)
  const idle = schedulerFixture()
  idle.scheduler.pause(true)
  idle.scheduler.pause(false)
  idle.advance(5000)
  assert.equal(idle.requests.length, 0, 'a pause without a change asks nothing')
  const during = schedulerFixture()
  during.scheduler.changed()
  during.scheduler.pause(true)
  during.advance(5000)
  assert.equal(during.requests.length, 0, 'a change already waiting is held back too')
})

test('stopping forgets a change that has not started; asking now does not wait', async () => {
  const stopped = schedulerFixture()
  stopped.scheduler.changed()
  stopped.scheduler.stop()
  stopped.advance(5000)
  assert.equal(stopped.requests.length, 0)
  const { scheduler, requests, advance, finish } = schedulerFixture()
  scheduler.now()
  advance(0)
  assert.equal(requests.length, 1)
  scheduler.now()
  await finish()
  advance(0)
  assert.equal(requests.length, 1, 'a request asked for during one follows it at once')
  await finish()
})

test('a check that rejects anyway does not stop the scheduler or leave an unhandled rejection', async () => {
  const unhandled = []
  const record = (reason) => { unhandled.push(reason) }
  process.on('unhandledRejection', record)
  try {
    let now = 0
    let next = 1
    const tasks = new Map()
    const timers = {
      set: (task, ms) => { tasks.set(next, { at: now + ms, task }); return next++ },
      clear: (handle) => { tasks.delete(handle) },
    }
    const advance = (ms) => {
      const end = now + ms
      for (;;) {
        const due = [...tasks].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0]
        if (!due) break
        tasks.delete(due[0])
        now = due[1].at
        due[1].task()
      }
      now = end
    }
    const calls = []
    const scheduler = new CheckScheduler(() => new Promise((resolve, reject) => { calls.push({ resolve, reject }) }), timers)
    scheduler.changed()
    advance(300)
    assert.equal(calls.length, 1)
    scheduler.changed()
    calls.shift().reject(new Error('the check broke its contract'))
    await new Promise((resolve) => setImmediate(resolve))
    await new Promise((resolve) => setImmediate(resolve))
    assert.deepEqual(unhandled, [], 'the scheduler does not turn a rejection into an unhandled one')
    advance(300)
    assert.equal(calls.length, 1, 'a change queued during the rejected check still starts one more')
    calls.shift().resolve()
    await new Promise((resolve) => setImmediate(resolve))
    scheduler.changed()
    advance(300)
    assert.equal(calls.length, 1, 'and the running flag was cleared, so later changes start requests')
  } finally {
    process.off('unhandledRejection', record)
  }
})

// ---------------------------------------------------------------------------------------------
// CIP labels in the web editor: the `chem.cip` reply reader, when a reply may be shown, the
// overlay text, the panel cell, and when the service is asked (never while both switches are off).
// ---------------------------------------------------------------------------------------------
const { readCipReply, readCipFailure, CIP_REASONS } = await import('../apps/web/src/lib/cip-contract.ts')
const { cipCell, cipMarks, cipView, labelAt, unavailableSentence, wantsCip, CIP_OFF } = await import('../apps/web/src/editor/cip-labels.ts')
const { askDecision, InspectionRunner } = await import('../apps/web/src/editor/scheduled-inspection.ts')

const REASON_TEXT = {
  cip_rule_4_or_5_needed: 'The ligands tie under the rules applied (see `rules`); Rules 4 and 5, not among them, might separate them.',
  cip_ligand_stereo_unspecified: 'Rule 3 has to place a ligand double bond whose geometry the document does not state, so the answer depends on information it does not have.',
  cip_not_a_candidate: 'The named atom is not a stereocenter candidate (it needs three or four bonded neighbors) or the named bond is not a double bond.',
}
const slot = (neighbor, rank) => ({ neighbor, rank })
const PRIORITY_A2 = [slot('a3', 1), slot('a1', 2), slot('a4', 3), slot('implicit_hydrogen', 4)]
const PRIORITY_A4 = [slot('a6', 1), slot('a5', 2), slot('a2', 3), slot('implicit_hydrogen', 4)]
const cipAtom = (atom, state, extra = {}) => structuredClone({ atom, state, descriptor: null, reason: null, stored: null, priority: null, decided_by: null, ...extra })
const cipBond = (bond, state, extra = {}) => structuredClone({ bond, state, descriptor: null, reason: null, stored: null, ends: null, ...extra })
const assignedA2 = () => cipAtom('a2', 'assigned', { descriptor: 'S', stored: tetra(A2, 'clockwise'), priority: PRIORITY_A2, decided_by: '1a' })
const assignedA4 = () => cipAtom('a4', 'assigned', { descriptor: 'R', stored: tetra(A4, 'anticlockwise'), priority: PRIORITY_A4, decided_by: '1a' })
const cipDiagnostics = (scope = 'stored', warnings = []) => [
  ...warnings,
  { code: 'cip_scope', severity: 'info', message: 'Listed: every stored configuration.', path: null, details: { scope } },
  { code: 'cip_not_checked', severity: 'info', message: 'Rules 1a, 1b, 2 and 3 only.', path: null, details: {} },
]
const lossOf = (source_path, code) => ({
  feature: 'cip_descriptor', source_path, disposition: 'unknown', reason: `${code}: ${REASON_TEXT[code] ?? 'A sentence.'}`, semantic_impact: 'stereo',
})
const cipReplyOf = (value, atoms, bonds = [], { scope = 'stored', warnings = [], losses = [], diagnostics, outcome } = {}) => ({
  envelope: 'operation_result', contract: 1, request_id: 'r', operation: 'chem.cip', profile: 'preserve-semantics',
  outcome: outcome ?? (losses.length > 0 ? 'partial' : 'ok'), document_revision: value.revision, model_id: 'chemifree-cip-1',
  diagnostics: diagnostics ?? cipDiagnostics(scope, warnings), loss_report: losses,
  result: {
    snapshot: { document: value, session: document('x').session, recovery: null, notes: [] },
    model: 'chemifree-cip-1', rules: ['1a', '1b', '2', '3'], scope, atoms, bonds,
  },
})
const cipGraphOf = (value) => ({ atoms: value.atoms, bonds: value.bonds, stereo: value.stereo })
const readCip = (value, reply, scope = 'stored') => readCipReply(reply, cipGraphOf(value), scope)
const cipInspected = async (value, reply, params = { scope: 'stored' }) => {
  const scripted = service({ '/api/v1/operations': [() => json(200, reply)] })
  const inspection = await new DocumentClient().inspectCip(value.id, params)
  return { inspection, scripted }
}

/** F-C(a2)=C(a3)-Br with the Z configuration stored on the double bond b2. */
function bondDocument(relation = 'same_side') {
  const value = scened(document('doc-bond', 1).document, 's1')
  value.atoms = [atomWith('a1', 'F', 0, 0), atomWith('a2', 'C', 20, 0), atomWith('a3', 'C', 40, 0), atomWith('a4', 'Br', 60, 0)]
  value.bonds = [bondWith('b1', 'a1', 'a2'), bondWith('b2', 'a2', 'a3', 'double'), bondWith('b3', 'a3', 'a4')]
  value.stereo = { atoms: [], bonds: [{ bond: 'b2', configuration: { kind: 'relative', begin_neighbor: 'a1', end_neighbor: 'a4', relation } }] }
  value.scene.instances[0].positions = value.atoms.map((atom) => ({ atom: atom.id, x: atom.position.x, y: atom.position.y }))
  return value
}
const assignedB2 = (descriptor = 'Z') => cipBond('b2', 'assigned', {
  descriptor, stored: { kind: 'relative', begin_neighbor: 'a1', end_neighbor: 'a4', relation: 'same_side' },
  ends: [{ atom: 'a2', higher: 'a1', decided_by: '1a' }, { atom: 'a3', higher: 'a4', decided_by: '1a' }],
})

test('chem.cip asks for the stored scope only and accepts the golden replies: an assigned center, an assigned bond', async () => {
  const value = agreementDocument()
  const { inspection, scripted } = await cipInspected(value, cipReplyOf(value, [assignedA2(), assignedA4()]))
  assert.deepEqual(scripted.writes.map((write) => write.parameters), [{ document_id: value.id, scope: 'stored' }])
  assert.deepEqual(inspection.atoms.map((item) => [item.atom, item.state, item.descriptor]), [['a2', 'assigned', 'S'], ['a4', 'assigned', 'R']])
  assert.equal(inspection.model, 'chemifree-cip-1')
  assert.deepEqual(inspection.rules, ['1a', '1b', '2', '3'])
  const bonded = bondDocument()
  const z = await cipInspected(bonded, cipReplyOf(bonded, [], [assignedB2()]))
  assert.deepEqual(z.inspection.bonds.map((item) => [item.bond, item.state, item.descriptor]), [['b2', 'assigned', 'Z']])
  const e = bondDocument('opposite_sides')
  const flipped = cipBond('b2', 'assigned', { ...assignedB2('E'), stored: e.stereo.bonds[0].configuration })
  assert.equal((await cipInspected(e, cipReplyOf(e, [], [flipped]))).inspection.bonds[0].descriptor, 'E')
})

test('chem.cip golden replies: none with the stored-not-stereogenic warning, unknown, unspecified, ambiguous, unsupported, partial with loss entries', async () => {
  const value = agreementDocument()
  const none = cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise') })
  const warning = { code: 'cip_stored_not_stereogenic', severity: 'warning', message: 'Atom a2: stored but not stereogenic.', path: null, details: { item: 'atom', id: 'a2' } }
  const withWarning = readCip(value, cipReplyOf(value, [none, assignedA4()], [], { warnings: [warning] }))
  assert.equal(withWarning.atoms[0].storedNotStereogenic, true)
  assert.equal(withWarning.atoms[1].storedNotStereogenic, false)
  assert.equal(withWarning.diagnostics.at(-1).code, 'cip_not_checked')

  const unknownDoc = agreementDocument()
  unknownDoc.stereo.atoms[0].configuration = { kind: 'unknown', neighbors: A2 }
  const unknown = cipAtom('a2', 'unknown', { stored: { kind: 'unknown', neighbors: A2 }, priority: PRIORITY_A2, decided_by: '1a' })
  assert.equal(readCip(unknownDoc, cipReplyOf(unknownDoc, [unknown, assignedA4()])).atoms[0].state, 'unknown')

  // Named: a center with nothing stored is `unspecified`, and an atom that is no candidate is unsupported.
  const open = agreementDocument()
  open.stereo.atoms = [open.stereo.atoms[1]]
  const unspecified = cipAtom('a2', 'unspecified', { priority: PRIORITY_A2, decided_by: '1a' })
  const notCandidate = cipAtom('a1', 'unsupported', { reason: 'cip_not_a_candidate' })
  const named = readCipReply(cipReplyOf(open, [notCandidate, unspecified, assignedA4()], [], { scope: 'named', losses: [lossOf('atoms[a1]', 'cip_not_a_candidate')] }), cipGraphOf(open), 'named', { atoms: ['a1', 'a2', 'a4'], bonds: [] })
  assert.deepEqual(named.atoms.map((item) => item.state), ['unsupported', 'unspecified', 'assigned'])
  assert.equal(named.atoms[0].reasonSentence, REASON_TEXT.cip_not_a_candidate)

  const unsupported = cipAtom('a2', 'unsupported', { reason: 'cip_rule_4_or_5_needed', stored: tetra(A2, 'clockwise') })
  const ambiguous = cipAtom('a4', 'ambiguous', { reason: 'cip_ligand_stereo_unspecified', stored: tetra(A4, 'anticlockwise') })
  const partial = readCip(value, cipReplyOf(value, [unsupported, ambiguous], [], { losses: [lossOf('atoms[a2]', 'cip_rule_4_or_5_needed'), lossOf('atoms[a4]', 'cip_ligand_stereo_unspecified')] }))
  assert.deepEqual(partial.atoms.map((item) => [item.state, item.reasonSentence]), [
    ['unsupported', REASON_TEXT.cip_rule_4_or_5_needed], ['ambiguous', REASON_TEXT.cip_ligand_stereo_unspecified],
  ])
  // A named bond that is no double bond is listed unsupported, and bond losses follow atom losses.
  const bonded = bondDocument()
  const single = cipBond('b1', 'unsupported', { reason: 'cip_not_a_candidate' })
  const mixed = readCipReply(cipReplyOf(bonded, [cipAtom('a1', 'unsupported', { reason: 'cip_not_a_candidate' })], [single, assignedB2()], {
    scope: 'named', losses: [lossOf('atoms[a1]', 'cip_not_a_candidate'), lossOf('bonds[b1]', 'cip_not_a_candidate')],
  }), cipGraphOf(bonded), 'named', { atoms: ['a1'], bonds: ['b1', 'b2'] })
  assert.deepEqual(mixed.bonds.map((item) => item.state), ['unsupported', 'assigned'])
})

test('chem.cip reader refuses each way a reply can be made untrue, and no refusal shows a label', () => {
  const value = agreementDocument()
  const bonded = bondDocument()
  const good = () => cipReplyOf(value, [assignedA2(), assignedA4()])
  const goodBond = () => cipReplyOf(bonded, [], [assignedB2()])
  assert.notEqual(readCip(value, good()), null)
  assert.notEqual(readCip(bonded, goodBond()), null)
  const tampers = {
    'a missing field in an atom entry': (r) => { delete r.result.atoms[0].decided_by },
    'a model that is not chemifree-cip-1': (r) => { r.result.model = 'chemifree-cip-2' },
    'a model_id that is not chemifree-cip-1': (r) => { r.model_id = 'chemifree-cip-2' },
    'other rules': (r) => { r.result.rules = ['1a', '1b', '2', '3', '4a'] },
    'rules in another order': (r) => { r.result.rules = ['1b', '1a', '2', '3'] },
    'a scope the reader does not know': (r) => { r.result.scope = 'everything' },
    'a scope other than the one asked for': (r) => { r.result.scope = 'named'; r.diagnostics[0].details.scope = 'named' },
    'an outcome other than ok or partial': (r) => { r.outcome = 'error' },
    'an unknown state': (r) => { r.result.atoms[0].state = 'maybe' },
    'an assigned center without a descriptor': (r) => { r.result.atoms[0].descriptor = null },
    'a descriptor on a state that is not assigned': (r) => { r.result.atoms[0].state = 'unknown'; r.result.atoms[0].stored = { kind: 'unknown', neighbors: A2 } },
    'an E or Z label on a center': (r) => { r.result.atoms[0].descriptor = 'E' },
    'a lower-case descriptor': (r) => { r.result.atoms[0].descriptor = 's' },
    'a reason on an assigned center': (r) => { r.result.atoms[0].reason = 'cip_rule_4_or_5_needed' },
    'an unsupported center without a reason': (r) => { Object.assign(r.result.atoms[0], { state: 'unsupported', descriptor: null, priority: null, decided_by: null }) },
    'a reason outside the closed list': (r) => { Object.assign(r.result.atoms[0], { state: 'unsupported', descriptor: null, priority: null, decided_by: null, reason: 'cip_made_up' }) },
    'an ambiguous center with another reason': (r) => { Object.assign(r.result.atoms[0], { state: 'ambiguous', descriptor: null, priority: null, decided_by: null, reason: 'cip_rule_4_or_5_needed' }) },
    'an unsupported center with the ambiguity reason': (r) => { Object.assign(r.result.atoms[0], { state: 'unsupported', descriptor: null, priority: null, decided_by: null, reason: 'cip_ligand_stereo_unspecified' }) },
    'a ranking on an unsupported center': (r) => { Object.assign(r.result.atoms[0], { state: 'unsupported', descriptor: null, reason: 'cip_rule_4_or_5_needed' }) },
    'a ranking of three positions': (r) => { r.result.atoms[0].priority.pop() },
    'ranks that do not run 1 to 4': (r) => { r.result.atoms[0].priority[3].rank = 5 },
    'ranks out of order': (r) => { r.result.atoms[0].priority.reverse() },
    'a position repeated': (r) => { r.result.atoms[0].priority[1].neighbor = 'a3' },
    'a ranked atom that is not bonded to the center': (r) => { r.result.atoms[0].priority[0].neighbor = 'a6' },
    'a ranking that does not name the stored positions': (r) => { r.result.atoms[0].priority[3].neighbor = 'lone_pair' },
    'a ranking without the rule that decided it': (r) => { r.result.atoms[0].decided_by = null },
    'a deciding rule that is not one of the four': (r) => { r.result.atoms[0].decided_by = '4a' },
    'a stored record the snapshot does not hold': (r) => { r.result.atoms[0].stored = tetra(A2, 'anticlockwise') },
    'a stored record where the snapshot holds none': (r) => { r.result.atoms.unshift(cipAtom('a1', 'unsupported', { reason: 'cip_not_a_candidate', stored: tetra(A2, 'clockwise') })); r.outcome = 'partial'; r.loss_report = [lossOf('atoms[a1]', 'cip_not_a_candidate')] },
    'a null stored record where the snapshot holds one': (r) => { r.result.atoms[0].stored = null },
    'an assigned center whose stored record is unknown': (r) => { r.result.snapshot.document = { ...value, stereo: { atoms: [{ atom: 'a2', configuration: { kind: 'unknown', neighbors: A2 } }, value.stereo.atoms[1]], bonds: [] } }; r.result.atoms[0].stored = { kind: 'unknown', neighbors: A2 } },
    'an unspecified center that has a stored record': (r) => { Object.assign(r.result.atoms[0], { state: 'unspecified', descriptor: null }) },
    'an unknown center whose record is tetrahedral': (r) => { Object.assign(r.result.atoms[0], { state: 'unknown', descriptor: null }) },
    'a center that is not in the snapshot': (r) => { r.result.atoms[0].atom = 'a9' },
    'centers that do not ascend': (r) => { r.result.atoms.reverse() },
    'a center listed twice': (r) => { r.result.atoms[1] = structuredClone(r.result.atoms[0]) },
    'a bond list that holds an atom': (r) => { r.result.bonds = [assignedA2()] },
    'a descriptor that is not for this item': (r) => { r.result.atoms[1].descriptor = 'Z' },
    'a warning that names an unlisted center': (r) => { r.diagnostics.unshift({ code: 'cip_slot_unusual', severity: 'warning', message: 'm', path: null, details: { item: 'atom', id: 'a1' } }) },
    'a stored-not-stereogenic warning on an assigned center': (r) => { r.diagnostics.unshift({ code: 'cip_stored_not_stereogenic', severity: 'warning', message: 'm', path: null, details: { item: 'atom', id: 'a2' } }) },
    'a malformed diagnostic of an unknown code': (r) => { r.diagnostics.unshift({ code: 'cip_whatever', severity: 'info', details: {} }) },
    'a second scope diagnostic': (r) => { r.diagnostics.unshift(structuredClone(r.diagnostics[0])) },
    'no not-checked diagnostic': (r) => { r.diagnostics.pop() },
    'no scope diagnostic': (r) => { r.diagnostics.shift() },
    'a scope diagnostic that disagrees with the result': (r) => { r.diagnostics[0].details.scope = 'named' },
    'a loss entry on an ok reply': (r) => { r.loss_report = [lossOf('atoms[a2]', 'cip_rule_4_or_5_needed')] },
    'a labeler-invalid diagnostic on a success': (r) => { r.diagnostics.unshift({ code: 'cip_labeler_invalid', severity: 'warning', message: 'm', path: null, details: { unit: 'a2', problem: 'nodes' } }) },
    'a missing result': (r) => { r.result = null },
  }
  for (const [name, mutate] of Object.entries(tampers)) {
    const reply = good()
    mutate(reply)
    assert.equal(readCip(value, reply), null, name)
  }
  const bondTampers = {
    'a center label on a bond': (r) => { r.result.bonds[0].descriptor = 'R' },
    'a bond that is not a bond of the snapshot': (r) => { r.result.bonds[0].bond = 'b9' },
    'an assigned bond that is not a double bond': (r) => { r.result.bonds[0].bond = 'b1' },
    'ends that name other atoms': (r) => { r.result.bonds[0].ends[0].atom = 'a1' },
    'ends in the wrong order': (r) => { r.result.bonds[0].ends.reverse() },
    'one end': (r) => { r.result.bonds[0].ends.pop() },
    'a higher position that is not at the end': (r) => { r.result.bonds[0].ends[0].higher = 'a4' },
    'a higher position that is the double bond partner': (r) => { r.result.bonds[0].ends[0].higher = 'a3' },
    'no ends on an assigned bond': (r) => { r.result.bonds[0].ends = null },
    'ends on a bond that is not ranked': (r) => { Object.assign(r.result.bonds[0], { state: 'none', descriptor: null }); r.diagnostics.unshift({ code: 'cip_stored_not_stereogenic', severity: 'warning', message: 'm', path: null, details: { item: 'bond', id: 'b2' } }) },
    'a stored record the snapshot does not hold': (r) => { r.result.bonds[0].stored = { kind: 'unknown' } },
    'a bond with an unknown stored record labeled': (r) => { r.result.snapshot.document = { ...bonded, stereo: { atoms: [], bonds: [{ bond: 'b2', configuration: { kind: 'unknown' } }] } }; r.result.bonds[0].stored = { kind: 'unknown' } },
  }
  for (const [name, mutate] of Object.entries(bondTampers)) {
    const reply = goodBond()
    mutate(reply)
    assert.equal(readCip(bonded, reply), null, name)
  }
  // Losses must be one per ambiguous or unsupported item, in order, with the code and a sentence.
  const unsupported = () => cipReplyOf(value, [cipAtom('a2', 'unsupported', { reason: 'cip_rule_4_or_5_needed', stored: tetra(A2, 'clockwise') }), assignedA4()], [], { losses: [lossOf('atoms[a2]', 'cip_rule_4_or_5_needed')] })
  assert.notEqual(readCip(value, unsupported()), null)
  const lossTampers = {
    'a partial reply without its loss entry': (r) => { r.loss_report = [] },
    'an ok outcome with an unsupported item': (r) => { r.outcome = 'ok' },
    'a loss entry for another item': (r) => { r.loss_report[0].source_path = 'atoms[a4]' },
    'a loss entry with another reason code': (r) => { r.loss_report[0].reason = r.loss_report[0].reason.replace('cip_rule_4_or_5_needed', 'cip_mass_number') },
    'a loss entry without a sentence': (r) => { r.loss_report[0].reason = 'cip_rule_4_or_5_needed: ' },
    'a loss entry that is a dropped feature': (r) => { r.loss_report[0].disposition = 'dropped' },
    'a second loss entry': (r) => { r.loss_report.push(lossOf('atoms[a4]', 'cip_rule_4_or_5_needed')) },
  }
  for (const [name, mutate] of Object.entries(lossTampers)) {
    const reply = unsupported()
    mutate(reply)
    assert.equal(readCip(value, reply), null, name)
  }
  // All seventeen reasons are the closed list.
  assert.equal(CIP_REASONS.length, 17)
})

test('inspectCip refuses a reply for another snapshot or a reply that is not the service reply, and says what an error says', async () => {
  const value = agreementDocument()
  const good = cipReplyOf(value, [assignedA2(), assignedA4()])
  const incompatible = async (reply) => {
    service({ '/api/v1/operations': [() => json(200, reply)] })
    await assert.rejects(new DocumentClient().inspectCip(value.id, { scope: 'stored' }), { code: 'incompatible_response' })
  }
  await incompatible({ ...good, document_revision: value.revision + 1 })
  await incompatible({ ...good, result: { ...good.result, snapshot: { ...good.result.snapshot, document: { ...value, id: 'other' } } } })
  await incompatible({ ...good, result: { ...good.result, snapshot: null } })
  await incompatible({ ...good, model_id: 'chemifree-cip-9' })
  await incompatible(null)
  // The scope asked for is the scope that must come back.
  service({ '/api/v1/operations': [() => json(200, good)] })
  await assert.rejects(new DocumentClient().inspectCip(value.id, { atoms: ['a2'] }), { code: 'incompatible_response' })
  // A limit: the reader keeps its numbers, the client keeps the service's message.
  const limit = {
    envelope: 'operation_result', contract: 1, outcome: 'error', model_id: 'chemifree-cip-1', document_revision: 1, loss_report: [], result: null,
    diagnostics: [{ code: 'resource_limit', severity: 'error', message: '9 items need more than 1000 digraph nodes together for the centers.', path: 'parameters.atoms',
      details: { limit: 1000, request_limit: 2000000, items: 9, item_limit: 20000, stage: 'centers', parameter: 'atoms' } }],
  }
  assert.deepEqual(readCipFailure(limit), { code: 'resource_limit', message: limit.diagnostics[0].message, limit: { stage: 'centers', parameter: 'atoms', limit: 1000, request_limit: 2000000, items: 9, item_limit: 20000 } })
  service({ '/api/v1/operations': [() => json(200, limit)] })
  await assert.rejects(new DocumentClient().inspectCip(value.id, { scope: 'stored' }), { code: 'cip_unavailable', message: limit.diagnostics[0].message })
  const failures = {
    'a model that is not the labeler': { ...limit, model_id: 'chemifree-cip-2' },
    'a result on an error': { ...limit, result: {} },
    'a stage that does not match its parameter': { ...limit, diagnostics: [{ ...limit.diagnostics[0], details: { ...limit.diagnostics[0].details, stage: 'scan' } }] },
    'a path that does not match its parameter': { ...limit, diagnostics: [{ ...limit.diagnostics[0], path: 'parameters.bonds' }] },
    'a limit above the request budget': { ...limit, diagnostics: [{ ...limit.diagnostics[0], details: { ...limit.diagnostics[0].details, limit: 2000001 } }] },
    'a labeler-invalid reply without its unit': { ...limit, diagnostics: [{ code: 'cip_labeler_invalid', severity: 'error', message: 'refused', path: null, details: {} }] },
    'no diagnostic': { ...limit, diagnostics: [] },
  }
  for (const [name, reply] of Object.entries(failures)) {
    assert.equal(readCipFailure(reply), null, name)
    service({ '/api/v1/operations': [() => json(200, reply)] })
    await assert.rejects(new DocumentClient().inspectCip(value.id, { scope: 'stored' }), { code: 'incompatible_response' }, name)
  }
  const invalid = { ...limit, diagnostics: [{ code: 'cip_labeler_invalid', severity: 'error', message: 'The CIP labeler\'s answer was refused.', path: null, details: { unit: 'a2', problem: 'nodes' } }] }
  assert.equal(readCipFailure(invalid)?.code, 'cip_labeler_invalid')
  const missing = { ...limit, document_revision: null, diagnostics: [{ code: 'document_not_found', severity: 'error', message: 'document nope does not exist', path: null, details: {} }] }
  service({ '/api/v1/operations': [() => json(200, missing)] })
  await assert.rejects(new DocumentClient().inspectCip('nope', { scope: 'stored' }), { code: 'cip_unavailable', message: 'document nope does not exist' })
  service({ '/api/v1/operations': [() => json(200, { outcome: 'unsupported', diagnostics: [{ code: 'unknown_operation', message: 'No operation chem.cip.' }] })] })
  await assert.rejects(new DocumentClient().inspectCip(value.id, { scope: 'stored' }), { code: 'cip_unavailable', message: 'No operation chem.cip.' })
})

test('a CIP reply is shown only while labels are wanted, for the drawing on screen and the service process that answered', async () => {
  const value = agreementDocument()
  const { inspection } = await cipInspected(value, cipReplyOf(value, [assignedA2(), assignedA4()]))
  const ok = { kind: 'ok', generation: 0, inspection }
  assert.equal(cipView(value, ok, 0, false, true, true).kind, 'ready')
  assert.equal(cipView(structuredClone(value), ok, 0, false, true, true).kind, 'ready')
  assert.deepEqual(cipView(value, ok, 0, false, false, false), { kind: 'off' }, 'switching the labels off removes them at once')
  assert.deepEqual(cipView(value, null, 0, false, false, false), { kind: 'off' })
  assert.equal(cipView(value, null, 0, false, true, true).kind, 'checking')
  const moved = structuredClone(value)
  moved.scene.instances[0].positions[0].x += 1
  assert.equal(cipView(moved, ok, 0, false, true, true).kind, 'checking', 'a reply for another snapshot is dropped')
  assert.equal(cipView({ ...value, revision: value.revision + 1 }, ok, 0, false, true, true).kind, 'checking')
  const edited = structuredClone(value)
  edited.stereo.atoms[0] = { atom: 'a2', configuration: tetra(A2, 'anticlockwise') }
  assert.equal(cipView(edited, ok, 0, false, true, true).kind, 'checking', 'a stored configuration that changed makes the label stale')
  assert.equal(cipView(value, ok, 1, false, true, true).kind, 'checking', 'another service process')
  assert.equal(cipView(value, ok, 0, true, true, true).kind, 'ready', 'labels stay while the same drawing is asked about again')
  const failed = { kind: 'failed', generation: 0, snapshot: value, message: 'too big' }
  assert.deepEqual(cipView(value, failed, 0, false, true, true), { kind: 'failed', message: 'too big' })
  assert.equal(cipView(moved, failed, 0, false, true, true).kind, 'checking', 'a failure belongs to the drawing it was asked about')
  assert.equal(cipView(value, failed, 0, true, true, true).kind, 'checking', 'a retry is running')
  assert.equal(unavailableSentence('too big'), 'CIP labels unavailable: too big')
  assert.deepEqual(cipMarks(cipView(value, failed, 0, false, true, true), true, true), [], 'no label after a failure')
})

test('only an assigned item gets overlay text, atoms for the atom switch and bonds for the bond switch', async () => {
  const value = agreementDocument()
  const { inspection } = await cipInspected(value, cipReplyOf(value, [assignedA2(), assignedA4()]))
  const ready = { kind: 'ready', inspection }
  assert.deepEqual(cipMarks(ready, true, false).map((m) => [m.item.id, m.text]), [['a2', '(S)'], ['a4', '(R)']])
  assert.equal(cipMarks(ready, true, false)[0].name, 'Center a2: CIP label S, of the stored configuration')
  assert.deepEqual(cipMarks(ready, false, true), [], 'the bond switch draws no atom label')
  assert.deepEqual(cipMarks(ready, false, false), [])
  assert.deepEqual(cipMarks({ kind: 'checking' }, true, true), [])
  assert.deepEqual(cipMarks({ kind: 'off' }, true, true), [])
  const bonded = bondDocument()
  const z = await cipInspected(bonded, cipReplyOf(bonded, [], [assignedB2()]))
  assert.deepEqual(cipMarks({ kind: 'ready', inspection: z.inspection }, false, true).map((m) => [m.item.kind, m.item.id, m.text]), [['bond', 'b2', '(Z)']])
  assert.deepEqual(cipMarks({ kind: 'ready', inspection: z.inspection }, true, false), [])
  // Every other state draws nothing.
  const open = agreementDocument()
  open.stereo.atoms = [open.stereo.atoms[0]]
  const others = {
    none: [cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise') }), { warnings: [{ code: 'cip_stored_not_stereogenic', severity: 'warning', message: 'm', path: null, details: { item: 'atom', id: 'a2' } }] }],
    unsupported: [cipAtom('a2', 'unsupported', { reason: 'cip_rule_4_or_5_needed', stored: tetra(A2, 'clockwise') }), { losses: [lossOf('atoms[a2]', 'cip_rule_4_or_5_needed')] }],
    ambiguous: [cipAtom('a2', 'ambiguous', { reason: 'cip_ligand_stereo_unspecified', stored: tetra(A2, 'clockwise') }), { losses: [lossOf('atoms[a2]', 'cip_ligand_stereo_unspecified')] }],
  }
  for (const [state, [item, options]] of Object.entries(others)) {
    const reply = await cipInspected(open, cipReplyOf(open, [item], [], options))
    assert.deepEqual(cipMarks({ kind: 'ready', inspection: reply.inspection }, true, true), [], state)
  }
  const unknownDoc = agreementDocument()
  unknownDoc.stereo.atoms = [{ atom: 'a2', configuration: { kind: 'unknown', neighbors: A2 } }]
  const unknown = await cipInspected(unknownDoc, cipReplyOf(unknownDoc, [cipAtom('a2', 'unknown', { stored: { kind: 'unknown', neighbors: A2 }, priority: PRIORITY_A2, decided_by: '1a' })]))
  assert.deepEqual(cipMarks({ kind: 'ready', inspection: unknown.inspection }, true, true), [], 'unknown')
  const named = agreementDocument()
  named.stereo.atoms = []
  const unspecified = await cipInspected(named, cipReplyOf(named, [cipAtom('a2', 'unspecified', { priority: PRIORITY_A2, decided_by: '1a' })], [], { scope: 'named' }), { atoms: ['a2'] })
  assert.deepEqual(cipMarks({ kind: 'ready', inspection: unspecified.inspection }, true, true), [], 'unspecified')
})

test('the panel cell says the label with the agreement next to it, why nothing is labeled, and what was not examined', async () => {
  const value = agreementDocument()
  const reading = await inspected(value, replyOf(value, resultOf(value, publicItems())))
  const rows = agreementRows(reading)
  const [a2, a4] = rows
  const { inspection } = await cipInspected(value, cipReplyOf(value, [assignedA2(), assignedA4()]))
  const view = { kind: 'ready', inspection }
  assert.deepEqual(cipCell(a2, view, true, true), { text: 'CIP: S', relation: 'of the stored configuration, which disagrees with the drawing in s1 (disagrees)' }, 'the label is the stored configuration; the drawing disagrees')
  assert.deepEqual(cipCell(a4, view, true, true), { text: 'CIP: R', relation: 'of the stored configuration, which agrees with the drawing in s1 (agrees)' })
  assert.equal(cipCell(a2, { kind: 'off' }, true, true), null)
  assert.equal(cipCell(a2, { kind: 'checking' }, true, true).text, 'CIP: checking…')
  assert.equal(cipCell(a2, { kind: 'failed', message: 'x' }, true, true).text, 'CIP: unavailable')
  // Not listed by the reply: not examined, never "no label" or "none".
  const onlyA4 = { ...inspection, atoms: inspection.atoms.filter((item) => item.atom === 'a4') }
  assert.deepEqual(cipCell(a2, { kind: 'ready', inspection: onlyA4 }, true, true), { text: 'CIP: not examined', relation: null })

  const open = agreementDocument()
  open.stereo.atoms = [open.stereo.atoms[0]]
  const textFor = async (item, options = {}, doc = open) => {
    const labeled = (await cipInspected(doc, cipReplyOf(doc, [item], [], options))).inspection
    return cipCell(a2, { kind: 'ready', inspection: labeled }, true, true).text
  }
  assert.equal(await textFor(cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise') }), {
    warnings: [{ code: 'cip_stored_not_stereogenic', severity: 'warning', message: 'm', path: null, details: { item: 'atom', id: 'a2' } }],
  }), 'CIP: none, the ligands are equivalent. A configuration is stored here but makes no difference: the unit is not stereogenic under the rules applied.')
  assert.equal(await textFor(cipAtom('a2', 'unsupported', { reason: 'cip_rule_4_or_5_needed', stored: tetra(A2, 'clockwise') }), { losses: [lossOf('atoms[a2]', 'cip_rule_4_or_5_needed')] }),
    `CIP: not supported - ${REASON_TEXT.cip_rule_4_or_5_needed}`)
  assert.equal(await textFor(cipAtom('a2', 'ambiguous', { reason: 'cip_ligand_stereo_unspecified', stored: tetra(A2, 'clockwise') }), { losses: [lossOf('atoms[a2]', 'cip_ligand_stereo_unspecified')] }),
    `CIP: ambiguous - ${REASON_TEXT.cip_ligand_stereo_unspecified}`)
  const unknownDoc = agreementDocument()
  unknownDoc.stereo.atoms = [{ atom: 'a2', configuration: { kind: 'unknown', neighbors: A2 } }]
  assert.equal(await textFor(cipAtom('a2', 'unknown', { stored: { kind: 'unknown', neighbors: A2 }, priority: PRIORITY_A2, decided_by: '1a' }), {}, unknownDoc), 'CIP: unknown (stored as either)')
  // Wording that implies more than the label is never used.
  for (const text of [NOT_JUDGED, CIP_OFF]) assert.doesNotMatch(text, /pseudo|enhanced|AS|BS|group/i)
})

test('wantsCip asks for nothing while both label switches are off, and for no more than a switch covers', () => {
  const both = agreementDocument()
  const bonded = bondDocument()
  const marksOnly = scened(publicFixture(true), 's1')
  assert.equal(wantsCip(both, false, false), false, 'default: no switch, no request')
  assert.equal(wantsCip(bonded, false, false), false)
  assert.equal(wantsCip(both, true, false), true)
  assert.equal(wantsCip(both, false, true), false, 'the bond switch alone needs a stored double bond')
  assert.equal(wantsCip(bonded, false, true), true)
  assert.equal(wantsCip(bonded, true, false), false, 'the atom switch alone needs a stored center')
  assert.equal(wantsCip(marksOnly, true, true), false, 'marks without a stored configuration have nothing to label')
  assert.equal(wantsCip(scened(publicFixture(false), 's1'), true, true), false)
})

/** An `InspectionRunner` on a fake clock; each request is a promise the test settles. */
function runnerFixture(dropOnStop, drawing = agreementDocument()) {
  let now = 0
  let next = 1
  const tasks = new Map()
  const timers = { set: (task, ms) => { tasks.set(next, { at: now + ms, task }); return next++ }, clear: (handle) => { tasks.delete(handle) } }
  const asks = []
  const inspect = (id) => new Promise((resolve, reject) => { asks.push({ id, resolve, reject }) })
  const source = (document, over = {}) => ({ document, generation: 0, dragging: false, active: true, allowed: true, inspect, ...over })
  const runner = new InspectionRunner(source(drawing), timers, dropOnStop)
  const settled = () => new Promise((resolve) => setImmediate(resolve))
  return {
    runner, asks, source, drawing,
    off: (document = drawing) => runner.update(source(document, { active: false, allowed: false })),
    on: (document = drawing, over = {}) => runner.update(source(document, over)),
    advance: (ms) => {
      const end = now + ms
      for (;;) {
        const due = [...tasks].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0]
        if (!due) break
        tasks.delete(due[0])
        now = due[1].at
        due[1].task()
      }
      now = end
    },
    settle: async (index, how, value) => { asks[index][how](value); await settled(); await settled() },
  }
}
const edited = (value, n) => ({ ...structuredClone(value), revision: value.revision + n })

test('the page asks chem.cip only while labels are wanted: one request in flight, changes coalesced, nothing after switching off', async () => {
  const f = runnerFixture(true)
  f.runner.attach()
  const { drawing } = f
  // Not wanted (the default): edits of every kind ask nothing, however long the page waits.
  f.off(drawing)
  f.off(edited(drawing, 1))
  f.advance(10_000)
  assert.equal(f.asks.length, 0, 'no request while labels are not wanted')
  f.on(drawing)
  f.advance(299)
  assert.equal(f.asks.length, 0)
  f.advance(1)
  assert.equal(f.asks.length, 1)
  f.on(structuredClone(drawing))
  f.on(edited(drawing, 1))
  f.on(edited(drawing, 2))
  f.advance(1000)
  assert.equal(f.asks.length, 1, 'one in flight')
  await f.settle(0, 'resolve', {})
  f.advance(300)
  assert.equal(f.asks.length, 2, 'one more for the changes made meanwhile')
  await f.settle(1, 'resolve', {})
  // Switching off before the wait ends cancels the request that had not started.
  f.on(edited(drawing, 3))
  f.advance(100)
  f.off(edited(drawing, 3))
  f.advance(10_000)
  assert.equal(f.asks.length, 2, 'switched off, the waiting request is dropped')
  // Switching on again asks again, even for the drawing already asked about.
  f.on(edited(drawing, 3))
  f.advance(300)
  assert.equal(f.asks.length, 3)
  await f.settle(2, 'resolve', {})
  // Another service process asks again.
  f.on(edited(drawing, 3), { generation: 1 })
  f.advance(300)
  assert.equal(f.asks.length, 4)
  // A drag holds the request until it ends.
  await f.settle(3, 'resolve', {})
  f.on(edited(drawing, 4), { dragging: true })
  f.advance(1000)
  assert.equal(f.asks.length, 4)
  f.on(edited(drawing, 4), { dragging: false })
  f.advance(300)
  assert.equal(f.asks.length, 5)
  f.runner.detach()
})

test('askDecision stops without asking, keeps a repeat and asks for anything else', () => {
  const value = agreementDocument()
  const asked = { document: value, generation: 0, active: true }
  assert.equal(askDecision(null, { ...asked, active: false }), 'stop')
  assert.equal(askDecision(asked, { ...asked, document: structuredClone(value) }), 'keep')
  assert.equal(askDecision(asked, { ...asked, document: edited(value, 1) }), 'ask')
  assert.equal(askDecision({ ...asked, active: false }, asked), 'ask')
  assert.equal(askDecision(asked, { ...asked, generation: 1 }), 'ask')
})

test('turning labels off and on again shows neither an earlier failure nor an answer that arrives after switch-off', async () => {
  const f = runnerFixture(true)
  f.runner.attach()
  f.on()
  f.advance(300)
  await f.settle(0, 'reject', new Error('too big'))
  assert.equal(f.runner.getState().outcome.kind, 'failed')
  f.off()
  assert.equal(f.runner.getState().outcome, null, 'switching off clears what was shown')
  f.on()
  assert.equal(f.runner.getState().outcome, null, 'switched on again, the earlier failure is not shown while the debounce runs')
  f.advance(300)
  assert.equal(f.asks.length, 2)
  // A request in flight when the switch goes off neither reports nor leaves the page busy.
  f.off()
  assert.equal(f.runner.getState().running, false)
  await f.settle(1, 'resolve', { fresh: true })
  assert.equal(f.runner.getState().outcome, null, 'an answer nobody wants any more is discarded')
  assert.equal(f.runner.getState().running, false)
  // The agreement check keeps its outcome when it stops: its manual check is still there.
  const keeping = runnerFixture(false)
  keeping.runner.attach()
  keeping.on()
  keeping.advance(300)
  await keeping.settle(0, 'reject', new Error('boom'))
  keeping.off()
  assert.equal(keeping.runner.getState().outcome.kind, 'failed')
  f.runner.detach()
  keeping.runner.detach()
})

test('a remount retires a request in flight, leaves nothing running and asks again', async () => {
  for (const dropOnStop of [true, false]) {
    const f = runnerFixture(dropOnStop)
    f.runner.attach()
    f.on()
    f.advance(300)
    assert.equal(f.runner.getState().running, true)
    f.runner.detach()
    assert.equal(f.runner.getState().running, false, 'the request in flight at cleanup does not leave the runner busy')
    f.runner.attach()
    f.on()
    f.advance(300)
    assert.equal(f.asks.length, 2, 'the remounted runner asks again')
    await f.settle(0, 'resolve', { old: true })
    assert.equal(f.runner.getState().outcome, null, 'the answer of the retired request is not used')
    assert.equal(f.runner.getState().running, true, 'and does not clear the busy state of the request that is running')
    await f.settle(1, 'resolve', { fresh: true })
    assert.deepEqual(f.runner.getState().outcome.inspection, { fresh: true })
    f.runner.detach()
  }
})

test('a label is placed in the widest gap between the bonds of its atom and beside the middle of its double bond', () => {
  const distanceToSegment = (p, a, b) => {
    const [dx, dy] = [b.x - a.x, b.y - a.y]
    const t = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy)))
    return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
  }
  const center = { x: 0, y: 0 }
  const around = [{ x: 0, y: 40 }, { x: -35, y: -18 }, { x: 35, y: -18 }]
  // The old fixed offset (12, -12) ran along the bond to the upper right in the smoke drawing.
  assert.ok(Math.min(...around.map((p) => distanceToSegment({ x: 12, y: -12 }, center, p))) < 6)
  const at = labelAt('atom', center, around)
  for (const p of around) assert.ok(distanceToSegment(at, center, p) > 12, `clear of the bond to ${p.x},${p.y}`)
  assert.ok(at.y < 0 && Math.abs(at.x) < 1, 'in the gap above the center, between the two upper bonds')
  // A terminal atom puts it opposite its one bond; an atom with no bond uses the fixed offset.
  const opposite = labelAt('atom', center, [{ x: 30, y: 0 }])
  assert.ok(opposite.x < 0 && Math.abs(opposite.y) < 1e-9)
  assert.deepEqual(labelAt('atom', center, []), { x: 12, y: -12 })
  // A double bond: beside the middle on the upper side, whichever way the bond runs.
  const mid = { x: 100, y: 100 }
  const above = labelAt('bond', mid, [{ x: 80, y: 100 }, { x: 120, y: 100 }])
  assert.ok(above.y < 100 && Math.abs(above.x - 100) < 1e-9)
  const reversed = labelAt('bond', mid, [{ x: 120, y: 100 }, { x: 80, y: 100 }])
  assert.deepEqual(reversed, above, 'the side does not depend on the direction the bond is written')
  const slanted = labelAt('bond', mid, [{ x: 80, y: 120 }, { x: 120, y: 80 }])
  assert.ok(distanceToSegment(slanted, { x: 80, y: 120 }, { x: 120, y: 80 }) > 10)
  assert.ok(slanted.y < 100)
})

test('the reader requires the reply to list exactly what the scope lists, and keeps a diagnostic code it does not know', async () => {
  const value = agreementDocument()
  const both = () => [assignedA2(), assignedA4()]
  const bonded = bondDocument()
  const stored = (v, atoms, bonds = [], options = {}, named = { atoms: [], bonds: [] }) => readCipReply(cipReplyOf(v, atoms, bonds, options), cipGraphOf(v), options.scope ?? 'stored', named)
  assert.notEqual(stored(value, both()), null)
  assert.equal(stored(value, [assignedA4()]), null, 'a stored center left out of a stored-scope reply')
  assert.equal(stored(value, []), null, 'an empty reply for stored configurations')
  assert.equal(stored(bonded, []), null, 'a stored double bond left out')
  assert.notEqual(stored(bonded, [], [assignedB2()]), null)
  // Naming adds to the stored scope; a named scope lists exactly the named items.
  const notCandidate = cipAtom('a1', 'unsupported', { reason: 'cip_not_a_candidate' })
  const loss = [lossOf('atoms[a1]', 'cip_not_a_candidate')]
  assert.notEqual(stored(value, [notCandidate, ...both()], [], { losses: loss }, { atoms: ['a1'], bonds: [] }), null)
  assert.equal(stored(value, both(), [], {}, { atoms: ['a1'], bonds: [] }), null, 'a named item that is not listed')
  const named = (atoms, extra) => stored(value, atoms, [], { scope: 'named', losses: extra }, { atoms: ['a1'], bonds: [] })
  assert.notEqual(named([notCandidate], loss), null)
  assert.equal(named([notCandidate, assignedA2()], loss), null, 'a named scope lists nothing but the named items')
  assert.equal(named([], []), null)
  // The scan may list more than was stored or named.
  const open = agreementDocument()
  open.stereo.atoms = [open.stereo.atoms[0]]
  assert.notEqual(stored(open, [assignedA2(), cipAtom('a4', 'unspecified', { priority: PRIORITY_A4, decided_by: '1a' })], [], { scope: 'stereogenic' }), null)
  assert.equal(stored(open, [cipAtom('a4', 'unspecified', { priority: PRIORITY_A4, decided_by: '1a' })], [], { scope: 'stereogenic' }), null, 'the scan still lists what is stored')
  // Warnings come in the order the service produces them: centers ascending, then bonds ascending.
  const warn = (item, id, code = 'cip_stored_not_stereogenic') => ({ code, severity: 'warning', message: 'm', path: null, details: { item, id } })
  const nones = [cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise') }), cipAtom('a4', 'none', { stored: tetra(A4, 'anticlockwise') })]
  assert.notEqual(stored(value, nones, [], { warnings: [warn('atom', 'a2'), warn('atom', 'a4')] }), null)
  assert.equal(stored(value, nones, [], { warnings: [warn('atom', 'a4'), warn('atom', 'a2')] }), null, 'centers out of order')
  assert.notEqual(stored(value, [cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise'), priority: null }), assignedA4()], [], { warnings: [warn('atom', 'a2'), warn('atom', 'a2', 'cip_slot_unusual')] }), null, 'one item may have both warnings')
  const noneBond = cipBond('b2', 'none', { stored: bonded.stereo.bonds[0].configuration })
  const noneAtom = cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise') })
  const mixed = structuredClone(bonded)
  mixed.stereo.atoms = [{ atom: 'a2', configuration: tetra(A2, 'clockwise') }]
  assert.equal(stored(mixed, [noneAtom], [noneBond], { warnings: [warn('bond', 'b2'), warn('atom', 'a2')] }), null, 'a bond warning before a center warning')
  assert.notEqual(stored(mixed, [noneAtom], [noneBond], { warnings: [warn('atom', 'a2'), warn('bond', 'b2')] }), null)
  // A diagnostic code added within contract 1 is shown, never relied on; a malformed one is refused.
  const extra = { code: 'cip_some_future_advice', severity: 'info', message: 'Something new.', path: null, details: { anything: 1 } }
  const withExtra = readCip(value, cipReplyOf(value, both(), [], { diagnostics: [extra, ...cipDiagnostics(), { ...extra, code: 'cip_another' }] }))
  assert.notEqual(withExtra, null)
  assert.deepEqual(withExtra.diagnostics.map((item) => item.code), ['cip_some_future_advice', 'cip_scope', 'cip_not_checked', 'cip_another'])
  assert.equal(readCip(value, cipReplyOf(value, both(), [], { diagnostics: [{ code: 'cip_x', message: 'no severity' }, ...cipDiagnostics()] })), null)
})

test('inspectCip sends the scope it expects, and an error the service words itself shows that wording', async () => {
  const value = agreementDocument()
  const named = cipReplyOf(value, [cipAtom('a1', 'unsupported', { reason: 'cip_not_a_candidate' })], [], { scope: 'named', losses: [lossOf('atoms[a1]', 'cip_not_a_candidate')] })
  const scripted = service({ '/api/v1/operations': [() => json(200, named)] })
  await new DocumentClient().inspectCip(value.id, { atoms: ['a1'] })
  assert.deepEqual(scripted.writes.map((write) => write.parameters), [{ document_id: value.id, atoms: ['a1'], scope: 'named' }], 'a named request says so')
  const bare = service({ '/api/v1/operations': [() => json(200, cipReplyOf(value, [assignedA2(), assignedA4()]))] })
  await new DocumentClient().inspectCip(value.id)
  assert.deepEqual(bare.writes.map((write) => write.parameters), [{ document_id: value.id, scope: 'stored' }])
  // The replies the service really sends for a request it cannot read (driven through the CLI): the model is named and the message is its own.
  for (const [code, message, path, details] of [
    ['invalid_request', 'Invalid parameters: unknown field `nope`, expected one of `document_id`, `scope`, `atoms`, `bonds`', 'parameters', {}],
    ['invalid_request', 'scope must be "stored", "stereogenic" or "named".', 'parameters.scope', { value: 'bad' }],
    ['document_not_found', 'document nope does not exist', null, {}],
  ]) {
    const reply = { envelope: 'operation_result', contract: 1, outcome: 'error', model_id: 'chemifree-cip-1', document_revision: null, result: null, loss_report: [], diagnostics: [{ code, severity: 'error', message, path, details }] }
    service({ '/api/v1/operations': [() => json(200, reply)] })
    await assert.rejects(new DocumentClient().inspectCip('nope'), { code: 'cip_unavailable', message })
  }
})

test('the view says what is missing when a switch is on and nothing of its kind is stored, and the cell follows its own switch', async () => {
  const atomsOnly = agreementDocument()
  const bondsOnly = bondDocument()
  assert.deepEqual(cipView(atomsOnly, null, 0, false, false, false), { kind: 'off' })
  assert.deepEqual(cipView(bondsOnly, null, 0, false, true, false), { kind: 'nothing', message: 'No stored atom configurations to label.' })
  assert.deepEqual(cipView(atomsOnly, null, 0, false, false, true), { kind: 'nothing', message: 'No stored bond configurations to label.' })
  assert.deepEqual(cipView(scened(publicFixture(true), 's1'), null, 0, false, true, true), { kind: 'nothing', message: 'No stored atom or bond configurations to label.' })
  assert.equal(cipView(bondsOnly, null, 0, false, true, true).kind, 'checking', 'one covered kind is enough to ask')
  assert.notEqual(cipView(bondsOnly, null, 0, false, true, false).message, CIP_OFF, 'a switch that is on is never told to switch on')
  const reading = await inspected(atomsOnly, replyOf(atomsOnly, resultOf(atomsOnly, publicItems())))
  const [a2] = agreementRows(reading)
  const { inspection } = await cipInspected(atomsOnly, cipReplyOf(atomsOnly, [assignedA2(), assignedA4()]))
  const ready = { kind: 'ready', inspection }
  assert.equal(cipCell(a2, ready, true, false).text, `CIP: S`)
  assert.equal(cipCell(a2, ready, false, true), null, 'an atom row shows its label under the atom switch only')
  const bondRow = { ...a2, key: 'bond:b2:s1', item: { kind: 'bond', id: 'b2' } }
  assert.equal(cipCell(bondRow, ready, true, false), null, 'a bond row shows its label under the bond switch only')
  assert.equal(cipCell(bondRow, ready, false, true).text, 'CIP: not examined')
  assert.equal(cipCell(a2, { kind: 'nothing', message: 'x' }, true, true), null)
  // A row with nothing stored has no stored configuration to label: no cell, not "not examined".
  const drawnOnly = { ...a2, key: 'atom:a9:s1', item: { kind: 'atom', id: 'a9' }, agreement: 'only_drawn', stored: null }
  for (const view of [ready, { kind: 'checking' }, { kind: 'failed', message: 'x' }]) assert.equal(cipCell(drawnOnly, view, true, true), null)
  assert.equal(cipCell({ ...drawnOnly, agreement: 'neither' }, ready, true, true), null)
})

test('a reply with fields the reader does not know, at every level, still shows its labels; a missing or wrong required value does not', async () => {
  const value = agreementDocument()
  const bonded = bondDocument()
  const unsupported = cipAtom('a2', 'unsupported', { reason: 'cip_rule_4_or_5_needed', stored: tetra(A2, 'clockwise') })
  const decorate = (reply) => {
    reply.result.future = { anything: true }
    for (const item of [...reply.result.atoms, ...reply.result.bonds]) {
      item.future = 1
      for (const entry of item.priority ?? []) entry.future = 1
      for (const end of item.ends ?? []) end.future = 1
    }
    for (const item of reply.diagnostics) { item.future = 1; item.details = { ...item.details, future: 1 } }
    for (const loss of reply.loss_report) loss.future = 1
    return reply
  }
  const atoms = readCip(value, decorate(cipReplyOf(value, [unsupported, assignedA4()], [], { losses: [lossOf('atoms[a2]', 'cip_rule_4_or_5_needed')], warnings: [] })))
  assert.deepEqual(atoms.atoms.map((item) => [item.atom, item.state, item.descriptor]), [['a2', 'unsupported', null], ['a4', 'assigned', 'R']])
  assert.equal(atoms.atoms[0].reasonSentence, REASON_TEXT.cip_rule_4_or_5_needed)
  const warning = { code: 'cip_stored_not_stereogenic', severity: 'warning', message: 'm', path: null, details: { item: 'atom', id: 'a2' } }
  assert.notEqual(readCip(value, decorate(cipReplyOf(value, [cipAtom('a2', 'none', { stored: tetra(A2, 'clockwise') }), assignedA4()], [], { warnings: [warning] }))), null)
  const bond = readCip(bonded, decorate(cipReplyOf(bonded, [], [assignedB2()])))
  assert.equal(bond.bonds[0].descriptor, 'Z')
  // Through the client, and in the stored record of the snapshot's own table.
  const { inspection } = await cipInspected(value, decorate(cipReplyOf(value, [assignedA2(), assignedA4()])))
  assert.deepEqual(cipMarks({ kind: 'ready', inspection }, true, false).map((m) => m.text), ['(S)', '(R)'])
  // An error reply with extra fields in its diagnostic and details keeps the service's message.
  const limit = { outcome: 'error', model_id: 'chemifree-cip-1', result: null, future: 1, diagnostics: [{ code: 'resource_limit', severity: 'error', message: 'too much', path: 'parameters.atoms', future: 1,
    details: { limit: 1, request_limit: 2, items: 3, item_limit: 4, stage: 'centers', parameter: 'atoms', nodes: 5 } }] }
  assert.equal(readCipFailure(limit)?.limit.stage, 'centers')
  assert.equal(readCipFailure({ ...limit, diagnostics: [{ code: 'cip_labeler_invalid', severity: 'error', message: 'm', details: { unit: 'a2', problem: 'nodes', future: 1 } }] })?.code, 'cip_labeler_invalid')
  // What is read is still checked: a missing field and a wrong value are refused, extra fields or not.
  for (const [name, mutate] of Object.entries({
    'a missing result field': (r) => { delete r.result.rules },
    'a missing atom field': (r) => { delete r.result.atoms[0].stored },
    'a missing priority rank': (r) => { delete r.result.atoms[0].priority[0].rank },
    'a wrong state': (r) => { r.result.atoms[0].state = 'maybe' },
    'a wrong descriptor': (r) => { r.result.atoms[0].descriptor = 'E' },
    'a missing scope diagnostic detail': (r) => { r.diagnostics[0].details = { future: 1 } },
  })) {
    const reply = decorate(cipReplyOf(value, [assignedA2(), assignedA4()]))
    mutate(reply)
    assert.equal(readCip(value, reply), null, name)
  }
  const noEnds = decorate(cipReplyOf(bonded, [], [assignedB2()]))
  delete noEnds.result.bonds[0].ends[0].higher
  assert.equal(readCip(bonded, noEnds), null, 'a missing end field')
  assert.equal(readCipFailure({ ...limit, diagnostics: [{ ...limit.diagnostics[0], details: { limit: 1, stage: 'centers', parameter: 'atoms', future: 1 } }] }), null, 'a missing limit detail')
})

// ---------------------------------------------------------------------------------------------
// Enhanced stereo groups in the web editor (P27c): the table cell and list, which actions the
// selection allows and why not, the commands each sends, the sentence for every refusal the
// contract lists, the number field, and where a group label is drawn.
// ---------------------------------------------------------------------------------------------
const {
  GROUP_ACTIONS, KIND_MEANINGS, REFUSAL_SENTENCES, groupCell, groupLabelAt, groupMarks, groupRefusalSentence, groupRows,
  groupStatus, numberSentence, parseGroupNumber, planGroupRequest, selectedAtoms,
} = await import('../apps/web/src/editor/stereo-groups.ts')
const { readFileSync } = await import('node:fs')

const grp = (id, kind, stored, members) => ({
  id, kind, stored_number: stored, members,
  number: kind === 'absolute' ? null : (stored ?? 0),
  label: kind === 'absolute' ? 'abs' : `${kind}${stored ?? 0}`,
})
/** The public drawing with three stored centers (a2, a4, a5) and these groups. */
function groupedDocument(groups = [], extra = []) {
  const value = agreementDocument()
  value.stereo.atoms.push({ atom: 'a5', configuration: tetra(['a4', 'a6', 'implicit_hydrogen', 'lone_pair'], 'clockwise') })
  value.stereo.atoms.push(...extra)
  value.stereo.groups = groups
  return value
}
const planOf = (value, selection, action, numberText = '') => planGroupRequest(value, selection, { action, numberText })
const commandsOf = (plan) => {
  assert.equal(plan.ok, true, plan.ok ? '' : plan.why)
  return plan.commands
}

test('the Group cell and list are built from the snapshot alone, with the label, kind words and the number as stated', () => {
  const value = groupedDocument([grp('e1', 'or', 2, ['a2', 'a4']), grp('e2', 'and', null, ['a5'])])
  assert.equal(groupCell(value.stereo.groups, 'a2'), 'e1: or2 (OR)')
  assert.equal(groupCell(value.stereo.groups, 'a5'), 'e2: and0 (AND)')
  assert.equal(groupCell(value.stereo.groups, 'a1'), 'no group')
  assert.equal(groupCell([grp('e3', 'absolute', null, ['a2'])], 'a2'), 'e3: abs (absolute)')
  assert.equal(numberSentence(value.stereo.groups[1]), 'number omitted (reads as 0)')
  assert.equal(numberSentence(grp('e4', 'or', 0, ['a2'])), 'number 0')
  assert.equal(numberSentence(grp('e4', 'or', -3, ['a2'])), 'number -3')
  assert.equal(numberSentence(grp('e3', 'absolute', null, ['a2'])), 'no number (an absolute group takes none)')
  const rows = groupRows(value)
  assert.deepEqual(rows.map((r) => [r.id, r.label, r.kindWord, r.count, r.members, r.numberText]), [
    ['e1', 'or2', 'OR', 2, ['a2', 'a4'], '2'], ['e2', 'and0', 'AND', 1, ['a5'], ''],
  ])
  // An omitted list reads as no groups, and no kind states a ratio.
  const bare = agreementDocument()
  assert.deepEqual(groupRows(bare), [])
  assert.deepEqual(groupMarks(bare), [])
  assert.match(KIND_MEANINGS.and, /mixture of the drawn set of members and its all-inverted counterpart\. No ratio is stated or implied\.$/)
  assert.equal(KIND_MEANINGS.or, 'the members invert together or not at all.')
  assert.equal(KIND_MEANINGS.absolute, 'the members are as drawn.')
  for (const text of Object.values(KIND_MEANINGS)) assert.doesNotMatch(text, /1:1|equal|ratio is \d|50/)
})

test('a group number is empty, or an integer in the 32-bit range with 0 and negatives kept', () => {
  assert.deepEqual(parseGroupNumber(''), { kind: 'none' })
  assert.deepEqual(parseGroupNumber('  '), { kind: 'none' })
  assert.deepEqual(parseGroupNumber('0'), { kind: 'integer', value: 0 })
  assert.deepEqual(parseGroupNumber('-0'), { kind: 'integer', value: 0 })
  assert.equal(Object.is(parseGroupNumber('-0').value, 0), true, 'never a negative zero')
  assert.deepEqual(parseGroupNumber('-3'), { kind: 'integer', value: -3 })
  assert.deepEqual(parseGroupNumber(' 007 '), { kind: 'integer', value: 7 })
  assert.deepEqual(parseGroupNumber('2147483647'), { kind: 'integer', value: 2147483647 })
  assert.deepEqual(parseGroupNumber('-2147483648'), { kind: 'integer', value: -2147483648 })
  for (const bad of ['2147483648', '-2147483649', '99999999999999999999999', '1.5', '1e3', 'abc', '+1', '--1', '1 2', '0x10', '٣']) {
    assert.equal(parseGroupNumber(bad).kind, 'invalid', bad)
    assert.match(parseGroupNumber(bad).why, /whole number/, bad)
  }
  assert.match(parseGroupNumber('2147483648').why, /-2147483648 to 2147483647/)
})

test('creating a group needs stored tetrahedral centers in no group, and each refusal says why in text', () => {
  const value = groupedDocument([], [])
  value.stereo.atoms.push({ atom: 'a3', configuration: { kind: 'unknown', neighbors: ['a1', 'a2', 'a4', 'implicit_hydrogen'] } })
  // Nothing selected, or only bonds: nothing to group.
  assert.match(planOf(value, [], 'group_or').why, /^Select atoms with a stored tetrahedral configuration/)
  assert.match(planOf(value, ['b1', 's1'], 'group_or').why, /^Select atoms/)
  assert.deepEqual(selectedAtoms(value, ['b1', 'a2', 'a99', 'a4']), ['a2', 'a4'], 'bonds and unknown ids are ignored')
  // The commands, with bonds in the selection ignored; the number is sent as an integer or null.
  assert.deepEqual(commandsOf(planOf(value, ['a2', 'b1', 'a4'], 'group_or')), [{ op: 'create_stereo_group', kind: 'or', atoms: ['a2', 'a4'], number: null }])
  assert.deepEqual(commandsOf(planOf(value, ['a2'], 'group_and', '-2')), [{ op: 'create_stereo_group', kind: 'and', atoms: ['a2'], number: -2 }])
  assert.deepEqual(commandsOf(planOf(value, ['a2'], 'group_or', '0')), [{ op: 'create_stereo_group', kind: 'or', atoms: ['a2'], number: 0 }])
  assert.deepEqual(commandsOf(planOf(value, ['a2', 'a4'], 'make_absolute')), [{ op: 'create_stereo_group', kind: 'absolute', atoms: ['a2', 'a4'], number: null }])
  // Not a stored tetrahedral center: no configuration, or stored unknown.
  assert.equal(planOf(value, ['a2', 'a1'], 'group_or').why, 'Every selected atom must have a stored tetrahedral configuration: a1 has none.')
  assert.equal(planOf(value, ['a3'], 'group_or').why, 'Every selected atom must have a stored tetrahedral configuration: a3 is stored unknown, which cannot be a member.')
  // An absolute group takes no number; a bad number is refused before anything is sent.
  assert.equal(planOf(value, ['a2'], 'make_absolute', '1').why, 'An absolute group takes no number. Clear the number field.')
  assert.equal(planOf(value, ['a2'], 'group_or', '1.5').ok, false)
  assert.equal(planOf(value, ['a2'], 'group_or', '2147483648').ok, false)
  // Already grouped, an absolute group already there, a number already taken.
  const grouped = groupedDocument([grp('e1', 'or', null, ['a2']), grp('e2', 'absolute', null, ['a5'])])
  assert.match(planOf(grouped, ['a2', 'a4'], 'group_and').why, /^a2 is already in group e1 \(or0\); an atom belongs to at most one group\./)
  assert.equal(planOf(grouped, ['a4'], 'make_absolute').why, 'The document already has an absolute group (e2); there is at most one.')
  assert.match(planOf(grouped, ['a4'], 'group_or').why, /^OR group e1 already has the number 0 \(a group with no number reads as 0\)\./)
  assert.match(planOf(grouped, ['a4'], 'group_or', '0').why, /^OR group e1 already has number 0\./)
  assert.equal(planOf(grouped, ['a4'], 'group_or', '1').ok, true)
  assert.equal(planOf(grouped, ['a4'], 'group_and').ok, true, 'a number is told apart within a kind only')
})

test('add, remove, merge and split are offered only when their preconditions hold, with the reason in text', () => {
  const value = groupedDocument([grp('e1', 'or', 1, ['a2', 'a4']), grp('e2', 'or', 2, ['a5']), grp('e3', 'and', 1, [])].slice(0, 2))
  value.stereo.atoms.push({ atom: 'a6', configuration: tetra(['a3', 'a5', 'implicit_hydrogen', 'lone_pair'], 'clockwise') })
  // Add: members of exactly one group together with ungrouped centers.
  assert.deepEqual(commandsOf(planOf(value, ['a2', 'a6'], 'add_to_group')), [{ op: 'add_stereo_group_members', group: 'e1', atoms: ['a6'] }])
  assert.match(planOf(value, ['a6'], 'add_to_group').why, /^Select a member of the group to add to/)
  assert.match(planOf(value, ['a2', 'a5', 'a6'], 'add_to_group').why, /^The selection holds members of 2 groups \(e1, e2\)/)
  assert.match(planOf(value, ['a2', 'a4'], 'add_to_group').why, /^Every selected atom is already in e1; there is nothing to add\./)
  assert.match(planOf(value, ['a1', 'a6'], 'add_to_group').why, /^Every selected atom must have a stored tetrahedral configuration: a1 has none/)
  // Remove: members only, and never every member of a group.
  assert.deepEqual(commandsOf(planOf(value, ['a4'], 'remove_from_group')), [{ op: 'remove_stereo_group_members', group: 'e1', atoms: ['a4'] }])
  assert.match(planOf(value, ['a2', 'a4'], 'remove_from_group').why, /^Removing every member of e1 would leave it empty\. Delete the group instead\./)
  assert.match(planOf(value, ['a5'], 'remove_from_group').why, /^Removing every member of e2/)
  assert.match(planOf(value, ['a4', 'a6'], 'remove_from_group').why, /^a6 is in no group; select only group members to remove\./)
  // Remove across two groups: one command each, in group order.
  const wide = groupedDocument([grp('e1', 'or', 1, ['a2', 'a4']), grp('e2', 'or', 2, ['a5', 'a6'])])
  wide.stereo.atoms.push({ atom: 'a6', configuration: tetra(['a3', 'a5', 'implicit_hydrogen', 'lone_pair'], 'clockwise') })
  assert.deepEqual(commandsOf(planOf(wide, ['a6', 'a2'], 'remove_from_group')), [
    { op: 'remove_stereo_group_members', group: 'e1', atoms: ['a2'] }, { op: 'remove_stereo_group_members', group: 'e2', atoms: ['a6'] },
  ])
  // Merge: members of exactly two groups of one kind; the lower id keeps its kind and number.
  assert.deepEqual(commandsOf(planOf(value, ['a5', 'a2'], 'merge')), [{ op: 'merge_stereo_groups', into: 'e1', from: 'e2' }])
  assert.equal(planOf(value, ['a2'], 'merge').why, 'Merging takes members of exactly two groups; the selection holds one.')
  assert.equal(planOf(value, ['a2', 'a6'], 'merge').why, 'a6 is in no group; select only members of the two groups.')
  const mixed = groupedDocument([grp('e1', 'or', 1, ['a2']), grp('e2', 'and', 1, ['a4'])])
  assert.equal(planOf(mixed, ['a2', 'a4'], 'merge').why, 'Only groups of the same kind can be merged: e1 is OR and e2 is AND.')
  // Split: some but not all members of one non-absolute group, into a new group with a free number.
  assert.deepEqual(commandsOf(planOf(value, ['a4'], 'split', '3')), [{ op: 'split_stereo_group', group: 'e1', atoms: ['a4'], number: 3 }])
  assert.match(planOf(value, ['a4'], 'split', '2').why, /^OR group e2 already has number 2\./)
  assert.match(planOf(value, ['a4'], 'split', '1').why, /^OR group e1 already has number 1\./)
  assert.match(planOf(value, ['a2', 'a4'], 'split', '3').why, /^Every member of e1 is selected; a split moves some but not all members\./)
  assert.match(planOf(value, ['a2', 'a5'], 'split', '3').why, /^Splitting takes members of exactly one group; the selection holds 2\./)
  const absolute = groupedDocument([grp('e1', 'absolute', null, ['a2', 'a4'])])
  assert.match(planOf(absolute, ['a4'], 'split').why, /^e1 is the absolute group, which cannot be split; remove members instead\./)
  // A new number of 0 or none is the same number, so it is taken when the kind has it.
  assert.match(planOf(groupedDocument([grp('e1', 'or', null, ['a2', 'a4'])]), ['a4'], 'split').why, /^OR group e1 already has the number 0/)
})

test('deleting a group and setting its number are per group; a taken number is left to the service, which refuses it', () => {
  const value = groupedDocument([grp('e1', 'or', 1, ['a2']), grp('e2', 'or', 2, ['a4']), grp('e3', 'absolute', null, ['a5'])])
  assert.deepEqual(commandsOf(planGroupRequest(value, [], { action: 'delete_group', group: 'e2' })), [{ op: 'delete_stereo_group', group: 'e2' }])
  assert.equal(planGroupRequest(value, [], { action: 'delete_group', group: 'e9' }).ok, false)
  const set = (group, numberText) => planGroupRequest(value, [], { action: 'set_number', group, numberText })
  assert.deepEqual(commandsOf(set('e1', '0')), [{ op: 'set_stereo_group_number', group: 'e1', number: 0 }])
  assert.deepEqual(commandsOf(set('e1', '-4')), [{ op: 'set_stereo_group_number', group: 'e1', number: -4 }])
  assert.deepEqual(commandsOf(set('e1', '')), [{ op: 'set_stereo_group_number', group: 'e1', number: null }])
  assert.deepEqual(commandsOf(set('e1', '2')), [{ op: 'set_stereo_group_number', group: 'e1', number: 2 }], 'taken by e2, which only the service decides')
  assert.equal(set('e1', '1').why, 'The number is already 1.')
  assert.equal(set('e3', '1').why, 'An absolute group takes no number.')
  assert.equal(set('e1', '2147483648').ok, false)
  assert.equal(set('e1', 'x').ok, false)
  assert.equal(set('e9', '1').ok, false)
  assert.equal(GROUP_ACTIONS.length, 7)
})

test('every refusal reason in the contract has a sentence, and a sentence only for a group reason of invalid_command', () => {
  const contract = readFileSync(new URL('../docs/CONTRACT.md', import.meta.url), 'utf8')
  const table = contract.slice(contract.indexOf('| `stereo_group_unknown` |'))
  const reasons = [...table.slice(0, table.indexOf('\n\n')).matchAll(/^\| `(stereo_group_[a-z_]+)` \|/gm)].map((m) => m[1])
  assert.equal(reasons.length, 12, 'the contract lists twelve reasons')
  assert.deepEqual(Object.keys(REFUSAL_SENTENCES).sort(), [...reasons].sort(), 'the page words exactly the reasons the contract lists')
  for (const reason of reasons) {
    const sentence = groupRefusalSentence('invalid_command', reason)
    assert.ok(sentence?.endsWith(' Nothing was changed.'), reason)
    assert.doesNotMatch(sentence, /stereo_group_/, `${reason} is worded in plain language`)
  }
  assert.match(groupRefusalSentence('invalid_command', 'stereo_group_number_taken'), /merge the two groups: select a member of each and press Merge groups/)
  assert.match(groupRefusalSentence('invalid_command', 'stereo_group_number_range'), /-2147483648 to 2147483647/)
  assert.equal(groupRefusalSentence('invalid_command', 'stereo_group_future'), null)
  assert.equal(groupRefusalSentence('invalid_command', undefined), null)
  assert.equal(groupRefusalSentence('invalid_command', 'toString'), null, 'only own entries count')
  assert.equal(groupRefusalSentence('revision_conflict', 'stereo_group_same'), null)
})

test('a service refusal of a group command reaches the page with its reason, and nothing is applied', async () => {
  const scripted = service({
    '/transactions': [() => json(422, { api_version: 'v1', code: 'invalid_command', message: 'command 0: number taken', command_index: 0, reason: 'stereo_group_number_taken' })],
  })
  const failure = await new DocumentClient().transact('doc-1', 3, [{ op: 'set_stereo_group_number', group: 'e1', number: 2 }]).then(() => null, (e) => e)
  assert.equal(failure.code, 'invalid_command')
  assert.equal(failure.stateReason, 'stereo_group_number_taken')
  assert.equal(failure.commandIndex, 0)
  assert.equal(groupRefusalSentence(failure.code, failure.stateReason), REFUSAL_SENTENCES.stereo_group_number_taken + ' Nothing was changed.')
  assert.equal(scripted.writes.length, 1)
})

test('the status names the result of each request from the snapshots before and after', () => {
  const before = groupedDocument([])
  const created = groupedDocument([grp('e1', 'or', 2, ['a2', 'a4'])])
  assert.equal(groupStatus(before, created, { action: 'group_or', numberText: '2' }), 'Created group e1 (or2) over a2, a4.')
  assert.equal(groupStatus(created, before, { action: 'delete_group', group: 'e1' }), 'Deleted group e1; its members keep their stored configurations.')
  assert.equal(groupStatus(before, created, { action: 'set_number', group: 'e1', numberText: '2' }), 'Group e1 is now or2 (number 2).')
  const two = groupedDocument([grp('e1', 'or', 1, ['a2']), grp('e2', 'or', 2, ['a4'])])
  assert.match(groupStatus(two, groupedDocument([grp('e1', 'or', 1, ['a2', 'a4'])]), { action: 'merge', numberText: '' }), /^Merged e2 into the other group, which keeps its kind and number; e2 is gone\.$/)
  for (const text of [groupStatus(before, created, { action: 'group_and', numberText: '' })]) assert.doesNotMatch(text, /ratio|1:1/)
})

test('group labels read abs or the kind word and interpreted number, and stack with the CIP label without touching it', () => {
  const value = groupedDocument([grp('e1', 'or', 2, ['a2', 'a4']), grp('e2', 'and', null, ['a5']), grp('e3', 'or', -3, [])].slice(0, 2))
  assert.deepEqual(groupMarks(value).map((m) => [m.atom, m.text]), [['a2', 'OR 2'], ['a4', 'OR 2'], ['a5', 'AND 0']])
  assert.equal(groupMarks(value)[0].name, 'Center a2: member of group e1, OR 2')
  assert.deepEqual(groupMarks(groupedDocument([grp('e1', 'absolute', null, ['a2']), grp('e2', 'or', -3, ['a4'])])).map((m) => m.text), ['abs', 'OR -3'])
  // Placement: the CIP label's point (the widest gap), moved 12 pt further from the atom's level.
  const center = { x: 0, y: 0 }
  const down = [{ x: 0, y: 40 }, { x: -35, y: -18 }, { x: 35, y: -18 }]
  const cip = labelAt('atom', center, down)
  assert.ok(cip.y < 0)
  assert.deepEqual(groupLabelAt(center, down), { x: cip.x, y: cip.y - 12 }, 'above the atom the group label sits above the CIP label')
  const below = labelAt('atom', center, [{ x: 0, y: -30 }])
  assert.ok(below.y > 0)
  assert.deepEqual(groupLabelAt(center, [{ x: 0, y: -30 }]), { x: below.x, y: below.y + 12 }, 'below the atom it sits below')
  const level = labelAt('atom', center, [{ x: -30, y: 0 }])
  assert.ok(Math.abs(level.y) < 1e-9 && level.x > 0)
  assert.ok(Math.abs(groupLabelAt(center, [{ x: -30, y: 0 }]).y - 12) < 1e-6, 'level with the atom it goes down')
  assert.deepEqual(groupLabelAt(center, []), { x: 12, y: -24 }, 'an atom with no bond: above the fixed offset')
  for (const around of [down, [{ x: 0, y: -30 }], [{ x: -30, y: 0 }], []]) {
    const [a, b] = [labelAt('atom', center, around), groupLabelAt(center, around)]
    assert.equal(a.x, b.x)
    assert.ok(Math.abs(a.y - b.y) >= 12, 'the labels are a line apart, so they cannot overlap')
    assert.deepEqual(groupLabelAt(center, around), b, 'deterministic')
  }
})

test('a non-absolute group with no stored number reads as its kind and 0, never abs; only an absolute group reads abs', () => {
  const odd = groupedDocument([{ ...grp('e1', 'or', null, ['a2']), number: null }])
  assert.equal(groupMarks(odd)[0].text, 'OR 0', 'interpreted number from the kind, as the snapshot would give it')
  assert.notEqual(groupMarks(odd)[0].text, 'abs', 'a group is absolute by its kind, not by a missing number')
  assert.equal(groupMarks(groupedDocument([grp('e1', 'absolute', null, ['a2'])]))[0].text, 'abs')
})

test('group labels widen the view box only when drawn, by their placed and estimated text extent', async () => {
  const { viewBox } = await import('../apps/web/src/editor/geometry.ts')
  const { groupLabelBoxes } = await import('../apps/web/src/editor/stereo-groups.ts')
  const positions = new Map([['a1', { x: 500, y: 100 }], ['a2', { x: 470, y: 100 }]])
  const bonds = [{ begin: 'a1', end: 'a2' }]
  const mark = { atom: 'a1', text: 'AND -2147483648', name: 'n' }
  assert.deepEqual(groupLabelBoxes([], bonds, positions), [], 'switch off: no boxes')
  const [box] = groupLabelBoxes([mark], bonds, positions)
  const at = groupLabelAt(positions.get('a1'), [positions.get('a2')])
  assert.equal(box.x + box.width / 2, at.x)
  assert.equal(box.y + box.height / 2, at.y)
  assert.ok(box.width >= mark.text.length * 6, 'wide enough for the longest text')
  assert.deepEqual(groupLabelBoxes([{ ...mark, atom: 'a9' }], bonds, positions), [], 'an atom the placement lacks has no box')
  const atoms = [...positions.values()]
  const without = viewBox(atoms, [])
  const withLabel = viewBox(atoms, [box])
  assert.ok(withLabel.x + withLabel.width >= box.x + box.width, 'the label is inside the view box')
  assert.ok(withLabel.x + withLabel.width > without.x + without.width, 'and it needed more room than the atoms alone')
})

test('a group number field follows the stored number unless the person is typing in it', async () => {
  const { reseedDraft } = await import('../apps/web/src/editor/stereo-groups.ts')
  assert.equal(reseedDraft('5', '5', '5', false), '5', 'nothing changed')
  assert.equal(reseedDraft('abc', '5', '5', true), 'abc', 'nothing changed: a draft is kept')
  assert.equal(reseedDraft('9', '5', '5', false), '9', 'a draft not yet sent survives an unrelated render')
  assert.equal(reseedDraft('5', '5', '7', false), '7', 'an idle field follows an undo')
  assert.equal(reseedDraft('9', '5', '7', true), '9', 'typing is kept, with its focus')
  assert.equal(reseedDraft('5', '5', '7', true), '7', 'focused but untouched follows')
})
