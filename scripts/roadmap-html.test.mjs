// Tests for the rules of roadmap-html.mjs: how the embed is written and read, and which problems the check reports.
// Fixtures are inline. Only the last test reads files: the repository's real roadmap pair.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { mock, test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { checkRoadmapHtml, embedRoadmap, readEmbedded } from './roadmap-html.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const EMBED_OPEN = '<script id="roadmap-data" type="application/json">'
const EMBED_CLOSE = '</script>'

const DATA = {
  title: 'Sample roadmap',
  date: '2026-11-11',
  rows: [
    { id: 'A1', title: 'First row', status: 'done' },
    { id: 'A2', title: 'Second row', status: 'open' },
  ],
  tracks: {
    order: [{ id: 'T1', title: 'Track one', rows: ['A1', 'A2'] }],
  },
}

/** The JSON file text the check accepts for `data`: canonical, with a final newline. */
function canonicalText(data) {
  return `${JSON.stringify(data, null, 1)}\n`
}

/** A page whose roadmap-data element holds `content`, with markup around it. */
function pageWith(content) {
  return `<!doctype html>\n<html>\n<body>\n${EMBED_OPEN}${content}${EMBED_CLOSE}\n</body>\n</html>\n`
}

/** Asserts that `problems` holds exactly one message and that the message matches `pattern`. */
function assertOneProblem(problems, pattern) {
  assert.equal(problems.length, 1, `expected one problem, got ${JSON.stringify(problems)}`)
  assert.match(problems[0], pattern)
}

test('embedRoadmap writes the compact JSON and readEmbedded reads it back', () => {
  const html = embedRoadmap(pageWith('stale text'), DATA)
  assert.equal(html, pageWith(JSON.stringify(DATA)))
  assert.deepEqual(readEmbedded(html), DATA)
})

test('embedRoadmap keeps the bytes around the element and writes replacement patterns verbatim', () => {
  const data = { ...DATA, title: 'Costs $& and $1' }
  const before = '<!doctype html>\r\n<title>로드맵</title>\r\n'
  const after = '\n<script>var kept = true;</script>\n'
  const html = embedRoadmap(`${before}${EMBED_OPEN}{"old":true}${EMBED_CLOSE}${after}`, data)
  assert.equal(html, `${before}${EMBED_OPEN}${JSON.stringify(data)}${EMBED_CLOSE}${after}`)
})

test('embedRoadmap throws when the html has no embed element', () => {
  assert.throws(() => embedRoadmap('<!doctype html>\n<p>no embed</p>\n', DATA), /has no element that opens with/)
})

test('embedRoadmap throws when the html has two embed elements', () => {
  assert.throws(() => embedRoadmap(pageWith('') + pageWith(''), DATA), /has 2 elements that open with/)
})

test('readEmbedded throws when the html has no embed element', () => {
  assert.throws(() => readEmbedded('<p>no embed</p>'), /has no element that opens with/)
})

test('readEmbedded throws when the element does not hold JSON', () => {
  assert.throws(() => readEmbedded(pageWith('{"title":')), /does not hold valid JSON/)
})

test('checkRoadmapHtml passes a consistent pair', () => {
  const html = embedRoadmap(pageWith(''), DATA)
  assert.deepEqual(checkRoadmapHtml({ json: canonicalText(DATA), html }), [])
})

test('checkRoadmapHtml reports a stale embed and names the first top-level key that differs', () => {
  const stale = { ...DATA, title: 'Old title', date: '2026-01-01' }
  const problems = checkRoadmapHtml({ json: canonicalText(DATA), html: embedRoadmap(pageWith(''), stale) })
  assertOneProblem(problems, /embed is stale.* at top-level key "title"$/)
})

test('checkRoadmapHtml reports a JSON file that is not canonical text', () => {
  const json = `${JSON.stringify(DATA, null, 2)}\n`
  assertOneProblem(checkRoadmapHtml({ json, html: embedRoadmap(pageWith(''), DATA) }), /is not canonical text/)
})

test('checkRoadmapHtml reports data whose serialization would end the script element', () => {
  const markup = { ...DATA, title: 'Sample </b> roadmap' }
  const problems = checkRoadmapHtml({ json: canonicalText(markup), html: embedRoadmap(pageWith(''), markup) })
  assertOneProblem(problems, /with "<\/", which would end the script element/)
})

test('checkRoadmapHtml reports data that would open a comment or a nested script', () => {
  for (const title of ['Sample <!-- roadmap', 'Sample <SCRIPT roadmap']) {
    const markup = { ...DATA, title }
    const problems = checkRoadmapHtml({ json: canonicalText(markup), html: embedRoadmap(pageWith(''), markup) })
    assertOneProblem(problems, /with "<!--" or "<script"/)
  }
})

test('checkRoadmapHtml reports an embed with the same data that is not the compact serialization', () => {
  const html = embedRoadmap(pageWith(''), DATA).replace(JSON.stringify(DATA), JSON.stringify(DATA, null, 1))
  assertOneProblem(checkRoadmapHtml({ json: canonicalText(DATA), html }), /not the compact serialization/)
  const reordered = embedRoadmap(pageWith(''), { rows: DATA.rows, tracks: DATA.tracks, date: DATA.date, title: DATA.title })
  assertOneProblem(checkRoadmapHtml({ json: canonicalText(DATA), html: reordered }), /not the compact serialization/)
})

test('checkRoadmapHtml reports a row id that is repeated', () => {
  const repeated = { ...DATA, rows: [DATA.rows[0], { ...DATA.rows[0], title: 'Repeated row' }, DATA.rows[1]] }
  const problems = checkRoadmapHtml({ json: canonicalText(repeated), html: embedRoadmap(pageWith(''), repeated) })
  assertOneProblem(problems, /repeats row id "A1"/)
})

test('checkRoadmapHtml reports a track row that names no row', () => {
  const unknown = { ...DATA, tracks: { order: [{ id: 'T1', title: 'Track one', rows: ['A1', 'Z9'] }] } }
  const problems = checkRoadmapHtml({ json: canonicalText(unknown), html: embedRoadmap(pageWith(''), unknown) })
  assertOneProblem(problems, /entry "T1" names "Z9", which is not a row id/)
})

test('checkRoadmapHtml reports rows that are not objects with a string id', () => {
  const malformed = { ...DATA, rows: ['A1', 'A2'] }
  const problems = checkRoadmapHtml({ json: canonicalText(malformed), html: embedRoadmap(pageWith(''), malformed) })
  assertOneProblem(problems, /needs rows as objects with a string id/)
})

test('checkRoadmapHtml reports a JSON file that does not parse', () => {
  assertOneProblem(checkRoadmapHtml({ json: '{"rows":', html: pageWith('') }), /is not valid JSON/)
})

test('checkRoadmapHtml reports an html without the embed element instead of throwing', () => {
  const problems = checkRoadmapHtml({ json: canonicalText(DATA), html: '<!doctype html>\n' })
  assertOneProblem(problems, /has no element that opens with/)
})

test('importing the module runs no command', async () => {
  const log = mock.method(console, 'log', () => {})
  const error = mock.method(console, 'error', () => {})
  try {
    // A query makes Node evaluate a second instance of the module, which runs its entry-point guard again.
    await import('./roadmap-html.mjs?imported-by-test')
    assert.equal(log.mock.callCount() + error.mock.callCount(), 0)
  } finally {
    mock.restoreAll()
  }
})

test('the command line checks the repository pair, and refuses an unknown argument', () => {
  const script = path.join(ROOT, 'scripts', 'roadmap-html.mjs')
  const checked = spawnSync(process.execPath, [script, '--check'], { encoding: 'utf8' })
  assert.equal(checked.status, 0, checked.stderr)
  assert.match(checked.stdout, /embed matches/)
  const refused = spawnSync(process.execPath, [script, '--bogus'], { encoding: 'utf8' })
  assert.equal(refused.status, 1)
  assert.match(refused.stderr, /usage: node scripts\/roadmap-html\.mjs/)
})

test('the repository roadmap JSON and HTML pair passes the check', () => {
  const json = readFileSync(path.join(ROOT, 'docs', 'roadmap', 'implementation-roadmap.json'), 'utf8')
  const html = readFileSync(path.join(ROOT, 'docs', 'roadmap', 'implementation-roadmap.html'), 'utf8')
  assert.deepEqual(checkRoadmapHtml({ json, html }), [])
})
