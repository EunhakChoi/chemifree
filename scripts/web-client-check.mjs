// Checks the web client's handling of requests whose outcome is unknown, against a scripted
// service instead of a browser, so that orderings a browser cannot produce on demand (a
// restart noticed by another request while one is being asked about) run every time. Also
// checks which runtime replies the client accepts.
import assert from 'node:assert/strict'
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

