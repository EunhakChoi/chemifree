// Checks the web client's handling of requests whose outcome is unknown, against a scripted
// service instead of a browser, so that orderings a browser cannot produce on demand (a
// restart noticed by another request while one is being asked about) run every time.
import assert from 'node:assert/strict'
import { register } from 'node:module'
import { test } from 'node:test'

register('./lib/web-alias-hooks.mjs', import.meta.url)
const { DocumentClient } = await import('../apps/web/src/lib/document-contract.ts')

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

test("a draft failure's code and reason are read when present and a malformed reason is refused", async () => {
  const failing = document('doc-1', 2)
  const failure = { attempts: 1, retry_in_ms: 2000, code: 'storage_failed', reason: 'read_only', message: 'permission denied' }
  failing.session = { ...failing.session, draft_failure: failure }
  const malformed = document('doc-2', 2)
  malformed.session = { ...malformed.session, draft_failure: { ...failure, reason: 'full' } }
  service({ 'GET /api/v1/documents/doc-1': [() => json(200, failing)], 'GET /api/v1/documents/doc-2': [() => json(200, malformed)] })
  const client = new DocumentClient()
  assert.deepEqual((await client.get('doc-1')).session.draft_failure, failure)
  await assert.rejects(client.get('doc-2'), { code: 'incompatible_response' })
})
