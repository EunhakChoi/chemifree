import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { test } from 'node:test'

import { observeRequestLog } from './request-log.mjs'

function request(path, failure = null) {
  return {
    url: () => `http://127.0.0.1:5173${path}`,
    method: () => 'GET',
    failure: () => failure,
    response() { throw new Error('request.response: Target page, context or browser has been closed') },
  }
}

test('response status is recorded synchronously at completion, including HTTP errors', () => {
  const page = new EventEmitter()
  const lines = []
  let time = 100
  observeRequestLog(page, (line) => lines.push(line), () => time)
  const sent = request('/api/v1/documents?sort=recent')
  time = 110
  page.emit('request', sent)
  time = 115
  page.emit('response', { request: () => sent, status: () => 503 })
  assert.deepEqual(lines, [], 'headers do not count as a completed body')
  time = 125
  page.emit('requestfinished', sent)
  assert.deepEqual(lines, ['+10 GET /api/v1/documents?sort=recent 503 15ms'])
})

test('a network failure without response headers is still recorded', () => {
  const page = new EventEmitter()
  const lines = []
  let time = 0
  observeRequestLog(page, (line) => lines.push(line), () => time)
  const sent = request('/api/v1/session', { errorText: 'net::ERR_CONNECTION_RESET' })
  time = 4
  page.emit('request', sent)
  time = 11
  page.emit('requestfailed', sent)
  assert.deepEqual(lines, ['+4 GET /api/v1/session net::ERR_CONNECTION_RESET 7ms'])
})

test('an interrupted body uses the failure reason even after successful response headers', () => {
  const page = new EventEmitter()
  const lines = []
  observeRequestLog(page, (line) => lines.push(line), () => 0)
  const sent = request('/api/v1/documents', { errorText: 'net::ERR_ABORTED' })
  page.emit('request', sent)
  page.emit('response', { request: () => sent, status: () => 200 })
  page.emit('requestfailed', sent)
  assert.deepEqual(lines, ['+0 GET /api/v1/documents net::ERR_ABORTED 0ms'])
})

test('stopping before page disposal leaves no request callbacks or pending response reads', () => {
  const page = new EventEmitter()
  const lines = []
  const browserErrors = []
  page.on('pageerror', (error) => browserErrors.push(error.message))
  const stop = observeRequestLog(page, (line) => lines.push(line), () => 0)
  const sent = request('/late')
  page.emit('request', sent)
  page.emit('response', { request: () => sent, status: () => 200 })
  page.emit('requestfinished', sent) // The closed-target response() stub must never be called.
  assert.deepEqual(lines, ['+0 GET /late 200 0ms'], 'completion is recorded before teardown starts')
  stop()
  stop()
  assert.equal(page.listenerCount('request'), 0)
  assert.equal(page.listenerCount('response'), 0)
  assert.equal(page.listenerCount('requestfinished'), 0)
  assert.equal(page.listenerCount('requestfailed'), 0)
  page.emit('requestfinished', request('/during-disposal'))
  assert.deepEqual(lines, ['+0 GET /late 200 0ms'])
  page.emit('pageerror', new Error('A real browser error'))
  assert.deepEqual(browserErrors, ['A real browser error'], 'request cleanup preserves browser error detection')
})

test('a missing response event while observation is active fails visibly', () => {
  const page = new EventEmitter()
  observeRequestLog(page, () => {}, () => 0)
  const sent = request('/missing-status')
  page.emit('request', sent)
  assert.throws(() => page.emit('requestfinished', sent), /Completed request has no observed response status/)
})
