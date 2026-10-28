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

/**
 * A scripted service. `answers` maps a write's path suffix to the replies for its successive
 * requests (a function is called with the request; returning `lost` drops the reply). After
 * `restart()`, the service issues a new token and refuses the old one, as a new process
 * would.
 */
function service(answers) {
  let token = 'token-1'
  const writes = []
  const asked = new Map()
  globalThis.fetch = async (path, init) => {
    if (path === '/api/v1/session') return json(200, { header: 'x-chemifree-session', token })
    const sentWith = init.headers['x-chemifree-session']
    const body = JSON.parse(init.body)
    writes.push({ path, token: sentWith, requestId: body.request_id })
    if (sentWith !== token) return error(401, 'unauthorized')
    const suffix = Object.keys(answers).find((end) => path.endsWith(end))
    assert.ok(suffix, `no scripted answer for ${path}`)
    const count = asked.get(suffix) ?? 0
    asked.set(suffix, count + 1)
    const answer = answers[suffix][count]
    assert.ok(answer, `no scripted answer for request ${count + 1} to ${suffix}`)
    return answer()
  }
  return {
    writes,
    restart() {
      token = `token-${Number(token.slice(6)) + 1}`
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
  })
  const created = await new DocumentClient().create('Kept')
  assert.equal(created.document.id, 'doc-2')
  const ids = scripted.writes.map((write) => write.requestId)
  assert.equal(ids.length, 2)
  assert.equal(ids[1], ids[0], 'one request ID')
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
