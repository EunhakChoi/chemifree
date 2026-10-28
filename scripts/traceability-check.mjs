#!/usr/bin/env node
// Checks that the traceability records agree with the conformance suite, the committed
// conformance report, and the roadmap:
//
// - docs/traceability/requirements.json lists 33 distinct requirements; every part has a known
//   status; a part that is implemented or partial names evidence; a not-implemented part names
//   the plan entries that will deliver it; every plan entry exists in the roadmap.
// - Every evidence reference resolves: `case:` to a case in conformance/suite.json, the others
//   to a file in the repository. Every case is referenced by a requirement it names, and every
//   requirement a case names is in the table.
// - docs/verification/conformance-report.json is a run of the current suite with no failures.
// - docs/traceability/declarations.json routes every declaration it counts to existing plan
//   entries; every done roadmap entry names its merged PRs, merge commit, and main CI run; and
//   the roadmap HTML embeds exactly docs/roadmap/implementation-roadmap.json.
//
// With CHEMIFREE_SPEC_DIR set to a local copy of the functional specification, it also checks
// the recorded source hashes, the requirement identifiers, and the declaration counts per route
// against it. The specification itself is never copied into the repository.

import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('..', import.meta.url).pathname
const read = (path) => JSON.parse(readFileSync(join(root, path), 'utf8'))

const STATUSES = new Set(['implemented', 'partial', 'not_implemented', 'unverified', 'intentionally_rejected'])
const EVIDENCE = new Set(['case', 'test', 'smoke', 'report', 'check'])

const trace = read('docs/traceability/requirements.json')
const declarations = read('docs/traceability/declarations.json')
const roadmap = read('docs/roadmap/implementation-roadmap.json')
const suite = read('conformance/suite.json')
const report = read('docs/verification/conformance-report.json')

const planIds = new Set(roadmap.rows.map((row) => row.id))
const cases = new Map(suite.cases.map((path) => [read(join('conformance', path)).id, read(join('conformance', path))]))

assert.equal(trace.format, 'chemifree-traceability/1')
const ids = trace.requirements.map((row) => row.id)
assert.equal(ids.length, 33, 'the table lists 33 requirements')
assert.equal(new Set(ids).size, ids.length, 'requirement identifiers are distinct')

const referenced = new Set()
for (const row of trace.requirements) {
  assert.ok(row.parts.length > 0, `${row.id} has parts`)
  assert.ok(STATUSES.has(row.status), `${row.id}: status ${row.status}`)
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
    for (const id of part.followups ?? []) assert.ok(planIds.has(id), `${where}: plan entry ${id} is in the roadmap`)
    for (const path of part.implementation ?? []) assert.ok(existsSync(join(root, path)), `${where}: ${path} exists`)
    for (const reference of evidence) {
      const [kind, target] = reference.split(/:(.*)/s)
      assert.ok(EVIDENCE.has(kind), `${where}: evidence kind ${kind}`)
      if (kind === 'case') {
        const found = cases.get(target)
        assert.ok(found, `${where}: case ${target} is in the suite`)
        assert.ok(found.requirements.includes(row.id), `${where}: case ${target} names ${row.id}`)
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
  if (found.pending) assert.ok(planIds.has(found.pending.followup), `case ${id}: follow-up ${found.pending.followup}`)
}

assert.equal(report.format, 'chemifree-conformance-report/1')
assert.equal(report.suite_version, suite.suite_version, 'the report is a run of the current suite version')
assert.deepEqual(report.results.map((result) => result.id).sort(), [...cases.keys()].sort(), 'the report covers the suite')
assert.equal(report.results.filter((result) => result.status === 'failed').length, 0, 'the recorded run has no failures')
for (const result of report.results) {
  assert.equal(result.status === 'not_run', Boolean(cases.get(result.id).pending), `${result.id}: not run exactly when pending`)
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
  for (const id of route.followups) assert.ok(planIds.has(id), `${route.route}: plan entry ${id}`)
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
console.log(
  `Traceability checked: ${ids.length} requirements ${JSON.stringify(statusCounts)}, ${cases.size} cases, ` +
    `${routed} declarations in ${declarations.routes.length} routes${spec ? ', specification sources verified' : ''}.`,
)
