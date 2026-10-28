#!/usr/bin/env node
// Checks that the traceability records agree with the conformance suite, the committed
// conformance report, and the roadmap:
//
// - docs/traceability/requirements.json lists 33 distinct requirements; every part has a known
//   status, and each requirement's status is the one its parts make; an unverified part names
//   its code; a part that is implemented or partial names evidence; a not-implemented part names
//   the plan entries that will deliver it; every plan entry exists in the roadmap.
// - Every evidence reference resolves: `case:` to a case in conformance/suite.json, the others
//   to a file in the repository; a `check:` script is one that npm run check runs. Every case
//   is referenced by a requirement it names, and every requirement a case names is in the table.
// - docs/verification/conformance-report.json is a run of the current suite with no failures:
//   each result restates its case, the counts are recomputed, and when a fresh run exists
//   (target/conformance-report.json, from npm run test:conformance) the record equals it.
// - docs/traceability/declarations.json routes every declaration it counts to existing plan
//   entries; every done roadmap entry names its merged PRs, merge commit, and main CI run; and
//   the roadmap HTML embeds exactly docs/roadmap/implementation-roadmap.json.
// - docs/TRACEABILITY.ko.md and docs/HANDOFF.ko.md quote the current status totals.
//
// With CHEMIFREE_SPEC_DIR set to a local copy of the functional specification, it also checks
// the recorded source hashes, the requirement identifiers, and the declaration counts per route
// against it. The specification itself is never copied into the repository.

import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const read = (path) => JSON.parse(readFileSync(join(root, path), 'utf8'))

const STATUSES = new Set(['implemented', 'partial', 'not_implemented', 'unverified', 'intentionally_rejected'])

/** A requirement's status as its parts' statuses make it (docs/TRACEABILITY.ko.md). */
function derivedStatus(parts) {
  const statuses = parts.map((part) => part.status)
  const all = (...allowed) => statuses.every((status) => allowed.includes(status))
  const any = (...wanted) => statuses.some((status) => wanted.includes(status))
  if (all('intentionally_rejected')) return 'intentionally_rejected'
  if (all('implemented', 'intentionally_rejected')) return 'implemented'
  if (any('implemented', 'partial')) return 'partial'
  if (any('unverified')) return 'unverified'
  return 'not_implemented'
}
const EVIDENCE = new Set(['case', 'test', 'smoke', 'report', 'check'])

const trace = read('docs/traceability/requirements.json')
const declarations = read('docs/traceability/declarations.json')
const roadmap = read('docs/roadmap/implementation-roadmap.json')
const suite = read('conformance/suite.json')
const report = read('docs/verification/conformance-report.json')

const planIds = new Set(roadmap.rows.map((row) => row.id))
const delivered = new Set(roadmap.rows.filter((row) => row.status === 'done').map((row) => row.id))
/** A follow-up names a plan entry that has yet to deliver. */
const assertFollowup = (id, where) => {
  assert.ok(planIds.has(id), `${where}: plan entry ${id} is in the roadmap`)
  assert.ok(!delivered.has(id), `${where}: ${id} is already done; update what it delivered`)
}
const recorded = new Map(report.results.map((result) => [result.id, result.status]))
/** Scripts that npm run check runs, by the paths their commands name. */
const scripts = read('package.json').scripts
const checked = new Set(
  scripts.check
    .split('&&')
    .map((step) => /^npm run (\S+)$/.exec(step.trim())?.[1])
    .flatMap((name) => (scripts[name] ?? '').match(/scripts\/[\w.-]+\.mjs/g) ?? []),
)
const cases = new Map(suite.cases.map((path) => [read(join('conformance', path)).id, read(join('conformance', path))]))

assert.equal(trace.format, 'chemifree-traceability/1')
const ids = trace.requirements.map((row) => row.id)
assert.equal(ids.length, 33, 'the table lists 33 requirements')
assert.equal(new Set(ids).size, ids.length, 'requirement identifiers are distinct')

const referenced = new Set()
for (const row of trace.requirements) {
  assert.ok(row.parts.length > 0, `${row.id} has parts`)
  assert.ok(STATUSES.has(row.status), `${row.id}: status ${row.status}`)
  assert.equal(row.status, derivedStatus(row.parts), `${row.id}: the status its parts make`)
  for (const part of row.parts) {
    const where = `${row.id} / ${part.part}`
    assert.ok(STATUSES.has(part.status), `${where}: status ${part.status}`)
    const evidence = part.evidence ?? []
    if (part.status === 'implemented' || part.status === 'partial') {
      assert.ok(evidence.length > 0, `${where}: an implemented part names its evidence`)
    }
    if (part.status === 'not_implemented') {
      assert.ok((part.followups ?? []).length > 0, `${where}: a missing part names its plan entries`)
    }
    if (part.status === 'unverified') {
      assert.ok((part.implementation ?? []).length > 0, `${where}: an unverified part names the code that is not checked`)
    }
    for (const id of part.followups ?? []) assertFollowup(id, where)
    if (part.status === 'implemented') {
      assert.equal((part.followups ?? []).length, 0, `${where}: an implemented part has nothing left to follow up`)
    }
    for (const path of part.implementation ?? []) assert.ok(existsSync(join(root, path)), `${where}: ${path} exists`)
    for (const reference of evidence) {
      const [kind, target] = reference.split(/:(.*)/s)
      assert.ok(EVIDENCE.has(kind), `${where}: evidence kind ${kind}`)
      if (kind === 'check') assert.ok(checked.has(target), `${where}: ${target} is run by npm run check`)
      if (kind === 'case') {
        const found = cases.get(target)
        assert.ok(found, `${where}: case ${target} is in the suite`)
        assert.ok(found.requirements.includes(row.id), `${where}: case ${target} names ${row.id}`)
        if (part.status === 'implemented' || part.status === 'partial') {
          assert.ok(!found.pending, `${where}: case ${target} is not run yet, so it cannot be evidence`)
          assert.equal(recorded.get(target), 'passed', `${where}: case ${target} passed in the recorded run`)
        }
        referenced.add(target)
      } else {
        assert.ok(existsSync(join(root, target)), `${where}: ${target} exists`)
      }
    }
  }
}
for (const [id, found] of cases) {
  assert.ok(referenced.has(id), `case ${id} is referenced by the table`)
  for (const requirement of found.requirements) assert.ok(ids.includes(requirement), `case ${id}: ${requirement} is in the table`)
  if (found.pending) assertFollowup(found.pending.followup, `case ${id}`)
}

assert.equal(report.format, 'chemifree-conformance-report/1')
assert.equal(report.suite_version, suite.suite_version, 'the report is a run of the current suite version')
assert.deepEqual(report.results.map((result) => result.id), [...cases.keys()], 'the report covers the suite, in its order')
// Each result restates its case as the suite has it now, and the counts are those of the results.
const recounted = {}
for (const result of report.results) {
  const found = cases.get(result.id)
  const expected = {
    id: found.id,
    requirements: found.requirements,
    axis: found.axis,
    kind: found.kind,
    status: found.pending ? 'not_run' : 'passed',
    ...(found.pending ? { detail: found.pending.reason, followup: found.pending.followup } : {}),
  }
  assert.deepEqual(result, expected, `${result.id}: the recorded result matches the case and has not failed`)
  const key = `${result.status}/${result.kind}`
  recounted[key] = (recounted[key] ?? 0) + 1
}
assert.deepEqual(report.counts, recounted, 'the recorded counts are those of its results')
// npm run check runs the suite first (test:conformance); the record must be that run exactly.
const fresh = join(root, 'target/conformance-report.json')
if (existsSync(fresh)) {
  assert.deepEqual(read('target/conformance-report.json'), report, 'the recorded report is a fresh run of the suite (npm run conformance:record)')
}

assert.equal(declarations.format, 'chemifree-declaration-trace/1')
const routed = declarations.routes.reduce((sum, route) => sum + route.declarations, 0)
assert.equal(routed, declarations.source.declarations, 'every declaration is in a route')
assert.equal(declarations.assigned.to_route, routed)
for (const route of declarations.routes) {
  assert.ok(STATUSES.has(route.status), `${route.route}: status`)
  const counted = (values) => Object.values(values).reduce((sum, count) => sum + count, 0)
  assert.equal(counted(route.by_source), route.declarations, `${route.route}: sources add up`)
  assert.equal(counted(route.by_evidence), route.declarations, `${route.route}: evidence add up`)
  if (route.status === 'not_implemented') assert.ok(route.followups.length > 0, `${route.route}: names its plan entries`)
  for (const id of route.followups) assertFollowup(id, route.route)
}

for (const row of roadmap.rows.filter((row) => row.status === 'done')) {
  const delivered = row.delivered ?? {}
  assert.ok(
    delivered.prs?.length > 0 && /^[0-9a-f]{7,40}$/.test(delivered.merge_commit ?? '') && /\/actions\/runs\/\d+$/.test(delivered.main_ci ?? ''),
    `${row.id}: a done plan entry names its merged PRs, merge commit, and main CI run`,
  )
}

const html = readFileSync(join(root, 'docs/roadmap/implementation-roadmap.html'), 'utf8')
const embedded = /<script id="roadmap-data" type="application\/json">([\s\S]*?)<\/script>/.exec(html)
assert.ok(embedded, 'the roadmap HTML embeds its data')
assert.deepEqual(JSON.parse(embedded[1]), roadmap, 'the roadmap HTML embeds the roadmap JSON')

const spec = process.env.CHEMIFREE_SPEC_DIR
if (spec) {
  const sha256 = (path) => createHash('sha256').update(readFileSync(join(spec, path))).digest('hex')
  for (const source of [...trace.sources, declarations.source]) {
    assert.equal(sha256(source.path), source.sha256, `${source.path} is the recorded version`)
  }
  const requirements = JSON.parse(readFileSync(join(spec, trace.sources[0].path), 'utf8')).requirements
  assert.deepEqual(requirements.map((row) => row.requirement_id).sort(), [...ids].sort(), 'the same 33 requirements')
  const inventory = JSON.parse(readFileSync(join(spec, declarations.source.path), 'utf8')).requirements
  const perRoute = new Map()
  for (const item of inventory) perRoute.set(item.proposed_route, (perRoute.get(item.proposed_route) ?? 0) + 1)
  assert.deepEqual(
    Object.fromEntries([...perRoute].sort()),
    Object.fromEntries(declarations.routes.map((route) => [route.route, route.declarations]).sort()),
    'declaration counts per route match the inventory',
  )
}

const statusCounts = {}
for (const row of trace.requirements) statusCounts[row.status] = (statusCounts[row.status] ?? 0) + 1

// The handoff documents quote the current totals in these exact words.
const ORDER = ['implemented', 'partial', 'unverified', 'intentionally_rejected', 'not_implemented']
const withStatus = (status) => trace.requirements.filter((row) => row.status === status).map((row) => row.id)
const present = ORDER.filter((status) => withStatus(status).length > 0)
const totals =
  `${ids.length}개 중 ` +
  present
    .map((status) => {
      const listed = withStatus(status)
      return status === 'not_implemented' ? `${status} ${listed.length}개` : `${status} ${listed.length}개(${listed.join(', ')})`
    })
    .join(', ') +
  '.'
const shortTotals = `현재 ${present.map((status) => `${status} ${withStatus(status).length}`).join('·')}`
for (const [path, text] of [
  ['docs/TRACEABILITY.ko.md', totals],
  ['docs/HANDOFF.ko.md', shortTotals],
]) {
  assert.ok(readFileSync(join(root, path), 'utf8').includes(text), `${path} quotes the current totals as: ${text}`)
}
console.log(
  `Traceability checked: ${ids.length} requirements ${JSON.stringify(statusCounts)}, ${cases.size} cases, ` +
    `${routed} declarations in ${declarations.routes.length} routes${spec ? ', specification sources verified' : ''}.`,
)
