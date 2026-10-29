// Checks the web client's handling of requests whose outcome is unknown, against a scripted
// service instead of a browser, so that orderings a browser cannot produce on demand (a
// restart noticed by another request while one is being asked about) run every time. Also
// checks which runtime replies the client accepts.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { register } from 'node:module'
import { test } from 'node:test'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { DocumentClient, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { DUPLICATE_OFFSET, deletePlacement, duplicatePlacement, movePlacement, newAtomPlacement, placeNewAtoms, placementAt } =
  await import(
  '../apps/web/src/editor/placement.ts'
)
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')
const { readRuntime } = await import('../apps/web/src/lib/contract.ts')

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
const lost = () => {
  throw new TypeError('fetch failed')
}

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
    writes.push({ path, token: sentWith, requestId: body.request_id })
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
const { bondName, bondRings, bondSides, bondStrokes, clipped, hitTest, leaningRight, placementFrame, viewBox } = await import(
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
const bondWith = (id, begin, end, order = 'single', query = null) => ({ id, begin, end, order, query })

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

test('a bond squeezed between two labels keeps a short stroke in the middle, never reversed', () => {
  const box = atomLabel(atomWith('a1', 'Cl', 0, 0, { hydrogens: { mode: 'specified', count: 2 } }), 1).box
  const [from, to] = clipped({ x: 0, y: 0 }, { x: 12, y: 0 }, box, { ...box, x: box.x + 12 })
  assert.ok(to.x > from.x, `drawn forward: ${from.x} → ${to.x}`)
  assert.equal(Math.round((to.x - from.x) * 100) / 100, 3)
  assert.equal(Math.round((from.x + to.x) * 50) / 100, 6, 'centred between the atoms')
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

const {
  addGraphicCommand,
  canvasName,
  deleteObjectsCommand,
  groupCommand,
  moveObjectsCommand,
  objectName,
  pressedObjects,
  sceneRows,
  selectionAfter,
  selectionAfterReply,
  selectionOnShow,
  selections,
  selectionSet,
  ungroupCommands,
} = await import('../apps/web/src/editor/scene.ts')
const { graphicBox, graphicShape, sceneLayout } = await import('../apps/web/src/editor/geometry.ts')
const { sceneObjects } = await import('../apps/web/src/lib/document-contract.ts')

/**
 * C1–O2 drawn in s1 and in s2 (100 pt lower, 50 pt right), and group s4 holding s2 and the
 * rectangle s3 from (0, 200) to (40, 220), with s3 and a1 selected.
 */
function grouped() {
  const value = drawnTwice()
  value.document.scene.objects = [
    { kind: 'molecule', id: 's1', parent: 'p1', molecule: 'm1' },
    { kind: 'group', id: 's4', parent: 'p1', children: ['s2', 's3'] },
    { kind: 'molecule', id: 's2', parent: 's4', molecule: 'm1' },
    { kind: 'graphic', id: 's3', parent: 's4', shape: 'rectangle', points: [{ x: 0, y: 200 }, { x: 40, y: 220 }] },
  ]
  value.document.selection = { objects: ['s3'], atoms: ['a1'], bonds: [] }
  return value
}

test('a document reply with a scene tree and a selection is accepted', () => {
  assert.equal(isDocumentResponse(grouped()), true)
  const flat = drawnTwice()
  flat.document.scene.objects = [
    { kind: 'molecule', id: 's1', parent: 'p1', molecule: 'm1' },
    { kind: 'molecule', id: 's2', parent: 'p1', molecule: 'm1' },
  ]
  flat.document.selection = { objects: [], atoms: [], bonds: [] }
  assert.equal(isDocumentResponse(flat), true, 'every instance on the page, nothing selected')
})

for (const [name, change] of [
  ['objects without a selection', (value) => delete value.document.selection],
  ['a selection without objects', (value) => delete value.document.scene.objects],
  ['a repeated object', (value) => value.document.scene.objects.push(value.document.scene.objects[3])],
  ['a child that names another parent', (value) => (value.document.scene.objects[3].parent = 'p1')],
  ['a listed child that names another object as its parent', (value) => (value.document.scene.objects[3].parent = 's1')],
  ['a group that does not list its child', (value) => (value.document.scene.objects[1].children = ['s2'])],
  ['a group naming an unknown child', (value) => value.document.scene.objects[1].children.push('s9')],
  ['an object that the page does not reach', (value) => (value.document.scene.objects[1].parent = 's3')],
  ['a group inside itself', (value) => (value.document.scene.objects[1].parent = 's4')],
  ['a molecule object that is not an instance', (value) => (value.document.scene.objects[0].id = 's7')],
  ['an instance without its object', (value) => value.document.scene.objects.splice(0, 1)],
  ['an unknown shape', (value) => (value.document.scene.objects[3].shape = 'ellipse')],
  ['children listed before their group', (value) => (value.document.scene.objects = [value.document.scene.objects[2], value.document.scene.objects[3], value.document.scene.objects[1], value.document.scene.objects[0]])],
  ["a group's children listed in another order than its own", (value) => (value.document.scene.objects = [value.document.scene.objects[0], value.document.scene.objects[1], value.document.scene.objects[3], value.document.scene.objects[2]])],
  ['a root object listed among the children of a group', (value) => (value.document.scene.objects = [value.document.scene.objects[1], value.document.scene.objects[2], value.document.scene.objects[0], value.document.scene.objects[3]])],
  ['a graphic with three points', (value) => value.document.scene.objects[3].points.push({ x: 1, y: 1 })],
  ['a graphic with a non-numeric point', (value) => (value.document.scene.objects[3].points[0].x = '0')],
  ['an unknown kind', (value) => (value.document.scene.objects[3].kind = 'text')],
  ['a selected object the document lacks', (value) => (value.document.selection.objects = ['s9'])],
  ['a selected atom the graph lacks', (value) => (value.document.selection.atoms = ['a9'])],
  ['a selected bond the graph lacks', (value) => (value.document.selection.bonds = ['b9'])],
  ['a selection without its bonds', (value) => delete value.document.selection.bonds],
]) {
  test(`a document reply with ${name} is refused`, () => {
    const value = grouped()
    change(value)
    assert.equal(isDocumentResponse(value), false)
  })
}

test('the scene objects may list the page roots in any order, each followed by its subtree', () => {
  const value = grouped()
  const [s1, s4, s2, s3] = value.document.scene.objects
  value.document.scene.objects = [s4, s2, s3, s1]
  assert.equal(isDocumentResponse(value), true)
})

test('a service from before groups and graphics shows its instances as page objects', () => {
  const { document } = drawnTwice()
  assert.deepEqual(sceneObjects(document), [
    { kind: 'molecule', id: 's1', parent: 'p1', molecule: 'm1' },
    { kind: 'molecule', id: 's2', parent: 'p1', molecule: 'm1' },
  ])
  delete document.scene
  assert.deepEqual(sceneObjects(document), [], 'and one from before scene instances shows none')
})

test('the scene rows are in tree order with their depth', () => {
  assert.deepEqual(
    sceneRows(grouped().document).map((row) => [row.object.id, row.depth]),
    [['s1', 0], ['s4', 0], ['s2', 1], ['s3', 1]],
  )
})

/** grouped() before s4: s1, s2, and the rectangle s3 on the page. */
function flat() {
  const value = grouped()
  value.document.scene.objects = value.document.scene.objects
    .filter((object) => object.id !== 's4')
    .map((object) => ({ ...object, parent: 'p1' }))
  value.document.selection = { objects: [], atoms: [], bonds: [] }
  return value
}

test('grouping is built from the latest document and only for objects that share a parent', () => {
  const { document } = flat()
  const group = (ids, latest = document) => groupCommand(latest, pressedObjects(document, new Set(ids)))
  assert.deepEqual(group(['s3', 's2']), { op: 'group', objects: ['s2', 's3'] }, 'in tree order')
  assert.deepEqual(pressedObjects(document, new Set(['s3', 'a1', 's2'])), [
    { id: 's2', parent: 'p1' },
    { id: 's3', parent: 'p1' },
  ], 'an atom is not an object')
  assert.equal(group(['s1', 's9']), null, 'an object a queued edit removed is not grouped')
  const nested = grouped().document
  assert.equal(groupCommand(nested, pressedObjects(nested, new Set(['s1', 's3']))), null, 'different parents')
})

test('a group press needs two objects and is not sent again once they are grouped', () => {
  const { document } = flat()
  const pressed = pressedObjects(document, new Set(['s2', 's3']))
  assert.equal(groupCommand(document, pressedObjects(document, new Set(['s3']))), null, 'one object is not grouped')
  // A second press queued behind the first: the objects now have the new group as parent.
  assert.equal(groupCommand(grouped().document, pressed), null, 'their parent changed since the press')
  // A press after the reply: they are all the children of their group already.
  const nested = grouped().document
  assert.equal(groupCommand(nested, pressedObjects(nested, new Set(['s2', 's3']))), null, 'already exactly one group')
  // Every root object of the page may still be grouped: the page is not a group.
  assert.deepEqual(groupCommand(nested, pressedObjects(nested, new Set(['s4', 's1']))), { op: 'group', objects: ['s1', 's4'] })
})

test('a group sends the new group as $N, the group command\'s index, in the selection after it', () => {
  const { document } = flat()
  const live = new Set(['s2', 's3', 'a1'])
  const group = { op: 'group', objects: ['s2', 's3'] }
  assert.deepEqual(selections(live, [group], document), {
    before: { objects: ['s2', 's3'], atoms: ['a1'], bonds: [] },
    after: { objects: ['$0'], atoms: ['a1'], bonds: [] },
  }, 'the group takes the place of the objects it contains')
  const move = { op: 'move_objects', objects: ['s2'], offset: { x: 1, y: 0 } }
  assert.deepEqual(selectionAfter(new Set(['s1', 's2', 's3']), [move, { ...group }], document).objects, ['s1', '$1'])
})

test('after a group reply the new group takes the place of the grouped objects in the live selection', () => {
  const before = flat().document
  const after = grouped().document
  after.selection = { objects: ['s4'], atoms: ['a1'], bonds: [] }
  const commands = [{ op: 'group', objects: ['s2', 's3'] }]
  const reply = (live) => [...selectionAfterReply(new Set(live), commands, before, after)].sort()
  assert.deepEqual(reply(['s2', 's3', 'a1']), ['a1', 's4'])
  // While the request ran, s1 and a2 were selected and a1 deselected: they stay as they are.
  assert.deepEqual(reply(['s2', 's3', 's1', 'a2']), ['a2', 's1', 's4'], 'changes made during the request are kept')
  assert.deepEqual(reply(['s2', 's3', 's9', 'b9']), ['s4'], 'what the reply does not hold is dropped')
  const move = [{ op: 'move_objects', objects: ['s2'], offset: { x: 1, y: 0 } }]
  assert.equal(selectionAfterReply(new Set(['s2']), move, before, after), null, 'no group, the live selection stays')
  delete after.selection
  assert.equal(selectionAfterReply(new Set(['s2']), commands, before, after), null, 'a service without selections')
})

const { pressEdit } = await import('../apps/web/src/hooks/use-document-editor.ts')

test('an edit carries the selection it was made with, even when the selection changes before it is sent', () => {
  const shown = grouped()
  // Delete clears the live selection as it queues the edit; the request is built later.
  const pressed = new Set(['a2', 'b1', 's3'])
  const deleteAtom = [{ op: 'delete_atoms', atoms: ['a2'] }]
  const build = pressEdit(deleteAtom, pressed, shown.document)
  pressed.clear()
  assert.deepEqual(build(grouped()), {
    revision: 2,
    commands: deleteAtom,
    selections: {
      before: { objects: ['s3'], atoms: ['a2'], bonds: ['b1'] },
      after: { objects: ['s3'], atoms: [], bonds: [] },
    },
  }, 'undo restores what the edit acted on')
  // An edit built from the latest document uses its revision, and the selection when pressed.
  const later = grouped()
  later.document.revision = 5
  const latest = pressEdit((document) => [{ op: 'delete_atoms', atoms: [document.atoms[1].id] }], new Set(['a2']), shown.document)
  assert.deepEqual(latest(later).selections.before.atoms, ['a2'])
  assert.equal(latest(later).revision, 5)
  // A list is built from the document on screen: it is sent with that revision, never to another
  // document, and after a restart only when the restarted service matched the same view.
  assert.equal(build(later).revision, 2)
  const other = grouped()
  other.document.id = 'doc-2'
  assert.equal(build(other), null, 'another document is shown')
  assert.equal(build(later, later), null, 'the restarted service matched a newer view')
  assert.equal(build(later, grouped()).revision, 5, 'the same view, at the restarted revision')
  const older = drawnTwice()
  assert.equal(pressEdit(deleteAtom, new Set(['a2']), older.document)(older).selections, undefined, 'a service without selections')
  assert.equal(pressEdit([], new Set(), shown.document)(grouped()), null, 'nothing to send')
})

test('an edit carries the selection before it and after it', () => {
  const { document } = grouped()
  const live = new Set(['s4', 's3', 'a1', 's9'])
  assert.deepEqual(selections(live, [{ op: 'delete_objects', objects: ['s4'], mode: 'subtree' }], document), {
    before: { objects: ['s4', 's3'], atoms: ['a1'], bonds: [] },
    after: { objects: [], atoms: ['a1'], bonds: [] },
  }, 'the selection before names only what the document holds before the edit')
})

test('a reply that replaces the shown document brings its selection; an edit reply keeps the live one', () => {
  const reply = grouped().document
  const live = new Set(['s1'])
  for (const how of ['open', 'undo', 'redo', 'recover', 'discard', 'reload']) {
    assert.deepEqual([...selectionOnShow(live, 'doc-1', reply, how)], ['s3', 'a1'], how)
  }
  assert.equal(selectionOnShow(live, 'doc-1', reply, 'edit'), live, 'an edit reply keeps it')
  assert.deepEqual([...selectionOnShow(live, 'doc-2', reply, 'edit')], ['s3', 'a1'], 'another document brings its own')
  assert.deepEqual([...selectionOnShow(live, 'doc-1', null, 'edit')], [], 'nothing shown, nothing selected')
  const older = drawnTwice().document
  assert.deepEqual([...selectionOnShow(live, 'doc-1', older, 'reload')], [], 'a service from before selections')
})

test('each scene object is named with its kind and the group that contains it', () => {
  const objects = grouped().document.scene.objects
  assert.deepEqual(objects.map(objectName), ['s1, placement', 's4, group of 2', 's2, placement, in group s4', 's3, rectangle, in group s4'])
})

test('the canvas is named with what it draws', () => {
  const { document } = grouped()
  assert.equal(canvasName(document), 'Placed: 2 atoms, 1 bond, 1 graphic, 1 group')
  assert.equal(canvasName(drawnTwice().document), 'Placed: 2 atoms, 1 bond')
  const empty = flat().document
  empty.atoms = []
  empty.bonds = []
  empty.scene.objects.push({ kind: 'graphic', id: 's5', parent: 'p1', shape: 'line', points: [{ x: 0, y: 0 }, { x: 1, y: 0 }] })
  assert.equal(canvasName(empty), 'Placed: 0 atoms, 0 bonds, 2 graphics')
})

test('the graphics layer is memoized, so hovering does not redraw it', () => {
  const source = readFileSync(new URL('../apps/web/src/editor/structure-canvas.tsx', import.meta.url), 'utf8')
  assert.match(source, /const GraphicMark = memo\(function GraphicMark\(/)
})

test('ungrouping names each selected group that the latest document still holds', () => {
  const { document } = grouped()
  assert.deepEqual(ungroupCommands(document, new Set(['s4', 's3'])), [{ op: 'ungroup', group: 's4' }])
  assert.equal(ungroupCommands(document, new Set(['s3'])), null, 'no group selected')
  const ungrouped = grouped().document
  ungrouped.scene.objects = ungrouped.scene.objects
    .filter((object) => object.id !== 's4')
    .map((object) => ({ ...object, parent: 'p1' }))
  assert.equal(ungroupCommands(ungrouped, new Set(['s4'])), null, 'a group a queued ungroup removed')
})

test('deleting carries the chosen mode and never deletes the last instance', () => {
  const { document } = grouped()
  assert.deepEqual(deleteObjectsCommand(document, new Set(['s4']), 'subtree'), {
    op: 'delete_objects',
    objects: ['s4'],
    mode: 'subtree',
  })
  assert.deepEqual(deleteObjectsCommand(document, new Set(['s4']), 'object_only'), {
    op: 'delete_objects',
    objects: ['s4'],
    mode: 'object_only',
  })
  assert.equal(deleteObjectsCommand(document, new Set(['s1', 's4']), 'subtree'), null, 'it would delete both instances')
  assert.deepEqual(
    deleteObjectsCommand(document, new Set(['s1', 's4']), 'object_only'),
    { op: 'delete_objects', objects: ['s1', 's4'], mode: 'object_only' },
    's2 stays, moved to the page',
  )
  assert.equal(deleteObjectsCommand(document, new Set(['s9']), 'subtree'), null, 'nothing selected remains')
})

test('moving sends the selection as is: a group and its child both, for the service to move each leaf once', () => {
  const { document } = grouped()
  assert.deepEqual(moveObjectsCommand(document, new Set(['s3', 's4', 'a1']), { x: 0, y: -10 }), {
    op: 'move_objects',
    objects: ['s4', 's3'],
    offset: { x: 0, y: -10 },
  })
  assert.equal(moveObjectsCommand(document, new Set(['a1']), { x: 10, y: 0 }), null)
})

test('a new graphic goes below everything drawn, at the left edge, and on an empty page at a fixed spot', () => {
  const drawn = grouped().document
  // The lowest point is the rectangle's (40, 220); the leftmost is a1 and the rectangle at x 0.
  assert.deepEqual(addGraphicCommand(drawn, 'rectangle'), {
    op: 'add_graphic',
    shape: 'rectangle',
    points: [{ x: 0, y: 260 }, { x: 60, y: 290 }],
  })
  assert.deepEqual(addGraphicCommand(drawn, 'arrow').points, [{ x: 0, y: 275 }, { x: 60, y: 275 }], 'a horizontal arrow')
  const added = grouped().document
  added.scene.objects.push({ kind: 'graphic', id: 's5', parent: 'p1', shape: 'line', points: [{ x: 0, y: 275 }, { x: 60, y: 275 }] })
  assert.deepEqual(addGraphicCommand(added, 'line').points, [{ x: 0, y: 330 }, { x: 60, y: 330 }], 'the next press steps down')
  const empty = document('doc-1').document
  assert.deepEqual(addGraphicCommand(empty, 'line').points, [{ x: 40, y: 55 }, { x: 100, y: 55 }])
})

test('the selection sent with an edit drops what the edit deletes', () => {
  const { document } = grouped()
  const selected = new Set(['s3', 's4', 's2', 'a1', 'a2', 'b1', 's9'])
  const send = (commands) => selectionAfter(selected, commands, document)
  assert.deepEqual(send([]), { objects: ['s3', 's4', 's2'], atoms: ['a1', 'a2'], bonds: ['b1'] }, 'unknown ids are left out')
  assert.deepEqual(send([{ op: 'delete_objects', objects: ['s4'], mode: 'subtree' }]).objects, [])
  assert.deepEqual(send([{ op: 'delete_objects', objects: ['s4'], mode: 'object_only' }]).objects, ['s3', 's2'])
  assert.deepEqual(send([{ op: 'ungroup', group: 's4' }]).objects, ['s3', 's2'])
  assert.deepEqual(send([{ op: 'delete_instance', instance: 's2' }]).objects, ['s3', 's4'])
  assert.deepEqual(send([{ op: 'delete_atoms', atoms: ['a2'] }]), { objects: ['s3', 's4', 's2'], atoms: ['a1'], bonds: [] })
  assert.deepEqual(send([{ op: 'delete_bonds', bonds: ['b1'] }]).bonds, [])
})

test('the transaction body carries the selection only when one is given', async () => {
  const bodies = []
  globalThis.fetch = async (path, init = {}) => {
    if (path === '/api/v1/session') return json(200, { header: 'x-chemifree-session', token: 'token-1' })
    bodies.push(JSON.parse(init.body))
    return json(200, grouped())
  }
  const client = new DocumentClient()
  const selection = { objects: ['$0'], atoms: [], bonds: [] }
  const before = { objects: ['s2', 's3'], atoms: [], bonds: [] }
  await client.transact('doc-1', 2, [{ op: 'group', objects: ['s2', 's3'] }], { before, after: selection })
  await client.transact('doc-1', 3, [])
  assert.deepEqual([bodies[0].selection, bodies[0].selection_before], [selection, before])
  assert.equal('selection' in bodies[1] || 'selection_before' in bodies[1], false, 'a service from before selections gets no field')
})

test('undo and redo replies carry the selection the editor takes over', async () => {
  const before = grouped()
  before.document.selection = { objects: ['s4'], atoms: [], bonds: ['b1'] }
  globalThis.fetch = async (path) => {
    if (path === '/api/v1/session') return json(200, { header: 'x-chemifree-session', token: 'token-1' })
    return json(200, before)
  }
  const reply = await new DocumentClient().revisionAction('doc-1', 'undo', 3)
  assert.deepEqual([...selectionSet(reply.document.selection)], ['s4', 'b1'])
  assert.deepEqual([...selectionSet(undefined)], [], 'a service from before selections clears it')
})

test('graphics are drawn from their points, and an arrow has a head at its end', () => {
  const arrow = { id: 's3', shape: 'arrow', points: [{ x: 0, y: 0 }, { x: 30, y: 0 }] }
  assert.deepEqual(graphicShape(arrow).head, [{ x: 30, y: 0 }, { x: 22, y: 3.5 }, { x: 22, y: -3.5 }])
  assert.deepEqual(graphicShape({ id: 's3', shape: 'rectangle', points: [{ x: 40, y: 20 }, { x: 0, y: 0 }] }), {
    kind: 'rectangle',
    x: 0,
    y: 0,
    width: 40,
    height: 20,
  })
  assert.deepEqual(graphicBox(arrow), { x: -4, y: -7.5, width: 38, height: 15 }, 'the head and half the underlay')
})

test('the view box holds every graphic and every group frame, however far it is drawn', () => {
  const objects = [
    { kind: 'molecule', id: 's1', parent: 'p1', molecule: 'm1' },
    { kind: 'group', id: 's5', parent: 'p1', children: ['s4'] },
    { kind: 'group', id: 's4', parent: 's5', children: ['s2', 's3'] },
    { kind: 'molecule', id: 's2', parent: 's4', molecule: 'm1' },
    { kind: 'graphic', id: 's3', parent: 's4', shape: 'line', points: [{ x: -500, y: 900 }, { x: 2000, y: -300 }] },
    { kind: 'group', id: 's6', parent: 'p1', children: [] },
  ]
  const s1 = { x: -10, y: -10, width: 50, height: 20 }
  const s2 = { x: 40, y: 90, width: 50, height: 20 }
  const layout = sceneLayout(objects, new Map([['s1', s1], ['s2', s2]]))
  assert.deepEqual(layout.graphics.map((item) => item.graphic.id), ['s3'])
  assert.deepEqual([...layout.frames.keys()].sort(), ['s4', 's5'], 'an empty group draws no frame')
  const s4 = layout.frames.get('s4')
  assert.ok(within(layout.graphics[0].box, s4.rect) && within(s2, s4.rect), 'the frame holds its subtree')
  assert.ok(within(s4.box, layout.frames.get('s5').rect), 'and a parent frame holds its child frame')
  const box = viewBox([], [s1, s2, ...layout.graphics.map((item) => item.box), ...[...layout.frames.values()].map((frame) => frame.box)])
  for (const inner of [layout.graphics[0].box, layout.frames.get('s5').box]) {
    assert.ok(within(inner, box), JSON.stringify({ inner, box }))
  }
})
