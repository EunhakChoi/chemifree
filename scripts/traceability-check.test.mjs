// One test per rule of traceability-check.mjs: each mutates the real inputs in memory so
// that exactly one rule is broken, and expects that rule's message.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  ROOT,
  TOTALS_DOCS,
  casePathProblem,
  checkInputs,
  deriveStatus,
  loadInputs,
  reachableScripts,
  rootFrom,
  totalsLine,
} from './traceability-check.mjs'

const loaded = loadInputs(ROOT, {})

/** The real inputs, made independent of a fresh run and of the documents' current text. */
function base() {
  const inputs = structuredClone(loaded)
  inputs.fresh = { text: inputs.record.text }
  syncDocs(inputs)
  return inputs
}

function syncDocs(inputs) {
  for (const doc of TOTALS_DOCS) inputs.docs[doc] = { text: `intro\n${totalsLine(inputs)}\n` }
}

function requirement(inputs, id) {
  const found = inputs.requirements.value.requirements.find((item) => item.id === id)
  assert.ok(found, `requirement ${id} exists`)
  return found
}

function part(inputs, id) {
  const [rid] = id.split('.')
  const found = requirement(inputs, rid).parts.find((item) => item.id === id)
  assert.ok(found, `part ${id} exists`)
  return found
}

function record(inputs) {
  return JSON.parse(inputs.record.text)
}

function setRecord(inputs, value) {
  inputs.record = { text: `${JSON.stringify(value, null, 2)}\n` }
  inputs.fresh = { text: inputs.record.text }
  syncDocs(inputs)
}

function expectProblem(inputs, pattern) {
  const errors = checkInputs(inputs)
  assert.ok(
    errors.some((error) => pattern.test(error)),
    `expected a problem matching ${pattern}, got:\n${errors.join('\n') || '(none)'}`,
  )
}

test('the repository inputs pass', () => {
  assert.deepEqual(checkInputs(base()), [])
})

test('status derivation follows the stated rule', () => {
  const cases = [
    [['intentionally_rejected'], 'intentionally_rejected'],
    [['implemented', 'intentionally_rejected'], 'implemented'],
    [['implemented', 'not_implemented'], 'partial'],
    [['partial'], 'partial'],
    [['unverified', 'not_implemented'], 'unverified'],
    [['not_implemented', 'intentionally_rejected'], 'not_implemented'],
    [[], null],
  ]
  for (const [statuses, expected] of cases) {
    assert.equal(deriveStatus(statuses), expected, JSON.stringify(statuses))
  }
})

test('a requirement status must equal the derived status', () => {
  const inputs = base()
  requirement(inputs, 'OPS-02').status = 'implemented'
  expectProblem(inputs, /OPS-02: status implemented differs from partial/)
})

test('unknown requirement fields are refused', () => {
  const inputs = base()
  requirement(inputs, 'OPS-02').verified = true
  expectProblem(inputs, /OPS-02: unknown field verified/)
})

test('a requirement without parts is refused', () => {
  const inputs = base()
  requirement(inputs, 'CHEM-01').parts = []
  expectProblem(inputs, /CHEM-01: has no parts/)
})

test('exactly one catch-all part', () => {
  const none = base()
  delete part(none, 'CHEM-01.rest').catch_all
  expectProblem(none, /CHEM-01: needs exactly one catch-all part, found 0/)
  const two = base()
  part(two, 'MODEL-02.2').catch_all = true
  expectProblem(two, /MODEL-02: needs exactly one catch-all part, found 2/)
})

test('a catch-all part is only not_implemented or unverified', () => {
  const inputs = base()
  const rest = part(inputs, 'OPS-01.rest')
  rest.status = 'intentionally_rejected'
  rest.reason = 'out of scope'
  rest.followups = []
  expectProblem(inputs, /OPS-01 part OPS-01.rest: a catch-all part can only be/)
})

test('a rejected part needs a reason', () => {
  const inputs = base()
  const target = part(inputs, 'MODEL-02.2')
  target.status = 'intentionally_rejected'
  requirement(inputs, 'MODEL-02').status = 'partial'
  expectProblem(inputs, /MODEL-02.2: a rejected part needs a reason/)
})

test('an implemented part needs an implementation path', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').implementation = []
  expectProblem(inputs, /OPS-01.1: an implemented part needs an implementation path/)
})

test('implementation paths must exist', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').implementation = ['crates/domain/src/missing.rs']
  expectProblem(inputs, /implementation path crates\/domain\/src\/missing.rs does not exist/)
})

test('an implemented part needs evidence that executes', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').evidence = []
  expectProblem(inputs, /OPS-01.1: an implemented part needs evidence that executes/)
})

test('a pending case is not executing evidence', () => {
  const inputs = base()
  part(inputs, 'API-01.3').evidence = ['case:versioned-operation-contract']
  expectProblem(inputs, /evidence case:versioned-operation-contract did not pass in the recorded run/)
})

test('case evidence must name the owning requirement', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').evidence = ['case:request-id-replay']
  expectProblem(inputs, /evidence case:request-id-replay is a case that does not name OPS-01/)
})

test('case evidence must name a suite case', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').evidence = ['case:no-such-case']
  expectProblem(inputs, /evidence case:no-such-case names no suite case/)
})

test('check evidence must be a script npm run check invokes', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').evidence = ['check:test:browser']
  expectProblem(inputs, /evidence check:test:browser is not a script npm run check invokes/)
})

test('evidence of another kind is refused', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').evidence.push('test:crates/domain/tests/document.rs')
  expectProblem(inputs, /is neither case:<id> nor check:<npm script>/)
})

test('open statuses need follow-ups', () => {
  const inputs = base()
  part(inputs, 'CHEM-01.rest').followups = []
  expectProblem(inputs, /CHEM-01.rest: a not_implemented part needs follow-ups/)
})

test('implemented parts have no follow-ups', () => {
  const inputs = base()
  part(inputs, 'OPS-01.1').followups = ['P11']
  expectProblem(inputs, /OPS-01.1: an implemented part has no follow-ups/)
})

test('a follow-up must be an open roadmap row', () => {
  const done = base()
  part(done, 'CHEM-01.rest').followups = ['P07']
  expectProblem(done, /follow-up P07 is a done roadmap row/)
  const unknown = base()
  part(unknown, 'CHEM-01.rest').followups = ['P999']
  expectProblem(unknown, /follow-up "P999" is not a roadmap row/)
})

test('done roadmap rows carry delivered', () => {
  const inputs = base()
  delete inputs.roadmap.value.rows.find((row) => row.id === 'P07').delivered.main_ci
  expectProblem(inputs, /done row P07 lacks delivered/)
})

test('case requirement ids must be traced', () => {
  const inputs = base()
  inputs.cases[0].value.requirements = ['XYZ-01']
  expectProblem(inputs, /requirement XYZ-01 is not traced/)
})

test('a pending case follow-up must be an open roadmap row', () => {
  const inputs = base()
  const pending = inputs.cases.find((item) => item.value.pending)
  pending.value.pending.followup = 'A1'
  expectProblem(inputs, /pending: follow-up A1 is a done roadmap row/)
})

test('sources: exactly one per role, under spec/, with a SHA-256', () => {
  const missing = base()
  missing.requirements.value.sources = missing.requirements.value.sources.filter((source) => source.role !== 'operations')
  expectProblem(missing, /needs exactly one operations source, found 0/)
  const twice = base()
  twice.requirements.value.sources.push({ ...twice.requirements.value.sources[0] })
  expectProblem(twice, /needs exactly one requirements source, found 2/)
  const outside = base()
  outside.requirements.value.sources[0].path = 'docs/requirements.json'
  expectProblem(outside, /source path "docs\/requirements.json" is not under spec\//)
  const absolute = base()
  absolute.declarations.value.source.path = '/spec/x.json'
  expectProblem(absolute, /source path "\/spec\/x.json" is absolute/)
  const hash = base()
  hash.declarations.value.source.sha256 = 'ABC'
  expectProblem(hash, /needs a lowercase 64-hex sha256/)
  const role = base()
  role.declarations.value.source.role = 'requirements'
  expectProblem(role, /needs exactly one declarations source, found 0/)
})

test('declarations: integers that add up', () => {
  const fraction = base()
  fraction.declarations.value.routes[0].declarations += 0.5
  expectProblem(fraction, /declarations must be a non-negative integer/)
  const routes = base()
  routes.declarations.value.routes[1].declarations += 1
  routes.declarations.value.routes[1].by_source['public-command-contracts.json'] += 1
  expectProblem(routes, /routes do not sum to total/)
  const sources = base()
  const first = sources.declarations.value.routes[0].by_source
  first['public-automation-contracts.json'] -= 1
  first['public-control-contracts.json'] += 1
  expectProblem(sources, /routes count .* for public-automation-contracts.json/)
  const total = base()
  total.declarations.value.by_source['public-control-contracts.json'] += 1
  expectProblem(total, /by_source does not sum to total/)
})

test('declarations: implemented <= assigned <= declarations', () => {
  const over = base()
  over.declarations.value.routes[0].implemented = 1
  expectProblem(over, /needs 0 <= implemented <= assigned_to_single_plan_entry <= declarations/)
  const assigned = base()
  const route = assigned.declarations.value.routes.at(-1)
  route.assigned_to_single_plan_entry = route.declarations + 1
  expectProblem(assigned, /needs 0 <= implemented <= assigned_to_single_plan_entry <= declarations/)
})

test('record: results follow the suite cases', () => {
  const order = base()
  const swapped = record(order)
  ;[swapped.results[0], swapped.results[1]] = [swapped.results[1], swapped.results[0]]
  setRecord(order, swapped)
  expectProblem(order, /result 0 .*: id differs/)
  const title = base()
  const renamed = record(title)
  renamed.results[2].title = 'Something else'
  setRecord(title, renamed)
  expectProblem(title, /result 2 .*: title differs/)
  const axis = base()
  const moved = record(axis)
  moved.results[2].axis = 'graph'
  setRecord(axis, moved)
  expectProblem(axis, /result 2 .*: axis differs/)
})

test('record: case hashes are current', () => {
  const inputs = base()
  inputs.cases[3].sha256 = '0'.repeat(64)
  expectProblem(inputs, /result 3 .*: case_sha256 is not the SHA-256/)
})

test('record: not_run exactly when pending', () => {
  const inputs = base()
  const changed = record(inputs)
  const index = changed.results.findIndex((result) => result.pending !== null)
  changed.results[index].outcome = 'passed'
  changed.counts.total.passed += 1
  changed.counts.total.not_run -= 1
  changed.counts.accept.passed += 1
  changed.counts.accept.not_run -= 1
  setRecord(inputs, changed)
  expectProblem(inputs, /outcome not_run must go with pending/)
})

test('record: no failed result', () => {
  const inputs = base()
  const changed = record(inputs)
  changed.results[0].outcome = 'failed'
  changed.results[0].failure = 'step 1: broke'
  changed.counts = { ...changed.counts }
  setRecord(inputs, changed)
  expectProblem(inputs, /result 0 .*: failed: step 1: broke/)
})

test('record: counts equal a recount', () => {
  const inputs = base()
  const changed = record(inputs)
  changed.counts.reject.passed += 1
  setRecord(inputs, changed)
  expectProblem(inputs, /counts differ from a recount/)
})

test('the fresh run must exist and equal the record', () => {
  const missing = base()
  missing.fresh = { error: 'ENOENT' }
  expectProblem(missing, /run npm run test:conformance first/)
  const differs = base()
  differs.fresh = { text: differs.record.text.replace('"passed"', '"failed"') }
  expectProblem(differs, /differs from docs\/verification\/conformance-report.json/)
})

test('the totals line must be present once and current', () => {
  const stale = base()
  stale.docs['docs/HANDOFF.ko.md'] = { text: '현재 집계: 요구사항 33개\n' }
  const errors = checkInputs(stale)
  const message = errors.find((error) => error.startsWith('docs/HANDOFF.ko.md'))
  assert.ok(message?.endsWith(totalsLine(stale)), `the message gives the line to paste: ${message}`)
  const twice = base()
  const line = totalsLine(twice)
  twice.docs['docs/TRACEABILITY.ko.md'] = { text: `${line}\n${line}\n` }
  expectProblem(twice, /docs\/TRACEABILITY.ko.md: needs exactly one line .*found 2/)
})

test('npm run check must run the conformance and traceability steps in order', () => {
  const inputs = base()
  const scripts = inputs.packageJson.value.scripts
  scripts.check = scripts.check.replace(' && npm run test:traceability', '')
  expectProblem(inputs, /check does not run npm run test:traceability/)
  const order = base()
  const orderScripts = order.packageJson.value.scripts
  orderScripts.check = orderScripts.check.replace('npm run test:conformance && ', '') + ' && npm run test:conformance'
  expectProblem(order, /check runs test:traceability before test:conformance/)
})

test('script reachability follows only exact npm run segments of && chains', () => {
  const { reached, errors } = reachableScripts({
    check: 'npm run a && npm run b -- --x && FOO=1 npm run c && npm run d',
    a: 'echo a',
    b: 'echo b',
    c: 'echo c',
    d: 'npm run e || true',
    e: 'echo e',
  })
  assert.deepEqual([...reached].sort(), ['a', 'check', 'd'])
  assert.ok(errors.some((error) => error.includes('"d" is reached from check but is not a plain && chain')))
  for (const text of ['npm run a # && npm run b', 'npm run a && npm run "b"', 'npm run a && npm run $B', 'npm run a && (npm run b)']) {
    const shell = reachableScripts({ check: text, a: 'echo a', b: 'echo b' })
    assert.ok(
      shell.errors.some((error) => error.includes('"check" is reached from check but is not a plain && chain')),
      text,
    )
    assert.ok(!shell.reached.has('b'), `${text} must not count b as invoked`)
  }
  const missing = reachableScripts({ check: 'npm run gone' })
  assert.ok(missing.errors.some((error) => error.includes('"gone" does not exist')))
})

test('case paths: the same rule as the Rust loader', () => {
  const refused = [
    ['/abs/case.json', 'is absolute'],
    ['\\abs\\case.json', 'is absolute'],
    ['C:case.json', 'has a drive prefix'],
    ['c:/cases/case.json', 'has a drive prefix'],
    ['cases//case.json', "has an empty, '.', or '..' segment"],
    ['cases/./case.json', "has an empty, '.', or '..' segment"],
    ['cases/../case.json', "has an empty, '.', or '..' segment"],
    ['cases\\..\\case.json', "has an empty, '.', or '..' segment"],
    ['cases/', "has an empty, '.', or '..' segment"],
    ['', "has an empty, '.', or '..' segment"],
  ]
  for (const [raw, reason] of refused) assert.equal(casePathProblem(raw), reason, raw)
  assert.equal(casePathProblem('cases/a.json'), null)
  assert.equal(casePathProblem('cases/..a.json'), null)
  const inputs = base()
  inputs.suite.value.cases[0] = 'cases/../cases/x.json'
  expectProblem(inputs, /case path "cases\/..\/cases\/x.json" has an empty/)
})

test('the root resolves from a file URL with spaces', () => {
  assert.equal(rootFrom('file:///tmp/a%20b/scripts/traceability-check.mjs'), '/tmp/a b')
})

test('with the spec: hashes, requirement ids, operations and route counts are compared', () => {
  const fakeSpec = (inputs) => {
    const routes = Object.fromEntries(
      inputs.declarations.value.routes.map((route) => [route.route, { declarations: route.declarations, by_source: { ...route.by_source } }]),
    )
    const files = {}
    for (const source of [...inputs.requirements.value.sources, inputs.declarations.value.source]) {
      files[source.path] = { sha256: source.sha256 }
    }
    return {
      dir: '/spec',
      files,
      requirementIds: inputs.requirements.value.requirements.map((item) => item.id),
      operationNames: [...new Set(inputs.requirements.value.requirements.flatMap((item) => item.operations))],
      declarations: {
        total: inputs.declarations.value.total,
        by_source: { ...inputs.declarations.value.by_source },
        routes,
      },
    }
  }
  const matching = base()
  matching.spec = fakeSpec(matching)
  assert.deepEqual(checkInputs(matching), [])

  const hash = base()
  hash.spec = fakeSpec(hash)
  hash.spec.files[hash.declarations.value.source.path].sha256 = 'f'.repeat(64)
  expectProblem(hash, /spec: spec\/phase3-design-v3\/declaration-requirements.json has SHA-256 f+/)
  const ids = base()
  ids.spec = fakeSpec(ids)
  ids.spec.requirementIds.push('NEW-01')
  expectProblem(ids, /spec: traced requirement ids differ/)
  const operations = base()
  operations.spec = fakeSpec(operations)
  operations.spec.operationNames = operations.spec.operationNames.filter((name) => name !== 'chem.properties')
  expectProblem(operations, /names unknown operation chem.properties/)
  const routes = base()
  routes.spec = fakeSpec(routes)
  routes.spec.declarations.routes.document.declarations += 1
  expectProblem(routes, /spec: declaration counts per route differ/)
})
