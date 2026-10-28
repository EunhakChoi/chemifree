// Checks the requirement and declaration traceability files against the roadmap, the
// conformance suite and its recorded run, npm scripts, and (with CHEMIFREE_SPEC_DIR) the spec.
//
// What a pass establishes is exactly the list of rules in checkInputs(); see
// docs/TRACEABILITY.ko.md. It does not establish that a status judgement is right, that a
// check script asserts the right thing, or that the repository holds no private data.
//
// Inputs are read once by loadInputs(); checkInputs() is pure, so tests mutate inputs in memory.

import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const STATUSES = [
  'implemented',
  'partial',
  'unverified',
  'not_implemented',
  'intentionally_rejected',
]
export const TOTALS_PREFIX = '현재 집계:'
export const TOTALS_DOCS = ['docs/TRACEABILITY.ko.md', 'docs/HANDOFF.ko.md']
const ROUTE_STATUSES = ['implemented', 'partial', 'unverified', 'not_implemented']
const OUTCOMES = ['passed', 'failed', 'not_run']
const KINDS = ['accept', 'reject']
const PART_FIELDS = [
  'id',
  'summary',
  'status',
  'catch_all',
  'implementation',
  'evidence',
  'followups',
  'reason',
]
const RESULT_FIELDS = [
  'id',
  'title',
  'requirements',
  'axis',
  'kind',
  'outcome',
  'pending',
  'failure',
  'case_sha256',
]
const SHA256 = /^[0-9a-f]{64}$/
const SCRIPT_NAME = /^[A-Za-z0-9:_.@/-]+$/
const REQUIREMENT_FIELDS = ['id', 'domain', 'summary', 'operations', 'status', 'parts']
const REQUIREMENT_ID = /^[A-Z]+-[0-9]{2}$/
const ROADMAP_ID = /^[A-Z][0-9]{1,3}$/

/** The repository root for a module URL inside scripts/. */
export function rootFrom(moduleUrl) {
  return path.resolve(path.dirname(fileURLToPath(moduleUrl)), '..')
}

export const ROOT = rootFrom(import.meta.url)

/**
 * The case-path rule shared with the Rust loader (check_case_path): relative, no drive prefix,
 * and no empty, '.' or '..' segment when split on both '/' and '\'. Returns a reason or null.
 */
export function casePathProblem(raw) {
  if (typeof raw !== 'string') return 'is not a string'
  if (raw.startsWith('/') || raw.startsWith('\\')) return 'is absolute'
  if (/^[A-Za-z]:/.test(raw)) return 'has a drive prefix'
  for (const segment of raw.split(/[/\\]/)) {
    if (segment === '' || segment === '.' || segment === '..') {
      return "has an empty, '.', or '..' segment"
    }
  }
  return null
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function readText(file) {
  try {
    return { text: readFileSync(file, 'utf8') }
  } catch (error) {
    return { error: error.message }
  }
}

function readJson(file) {
  const { text, error } = readText(file)
  if (error) return { error }
  try {
    return { value: JSON.parse(text), text }
  } catch (parseError) {
    return { error: parseError.message, text }
  }
}

/** Reads every input once. Nothing here decides whether a rule holds. */
export function loadInputs(root = ROOT, env = process.env) {
  const at = (relative) => path.join(root, relative)
  const inputs = {
    requirements: readJson(at('docs/traceability/requirements.json')),
    declarations: readJson(at('docs/traceability/declarations.json')),
    roadmap: readJson(at('docs/roadmap/implementation-roadmap.json')),
    packageJson: readJson(at('package.json')),
    suite: readJson(at('conformance/suite.json')),
    cases: [],
    record: readText(at('docs/verification/conformance-report.json')),
    fresh: readText(at('target/conformance/report.json')),
    docs: Object.fromEntries(TOTALS_DOCS.map((doc) => [doc, readText(at(doc))])),
    pathExists: new Map(),
    spec: null,
  }
  const entries = Array.isArray(inputs.suite.value?.cases) ? inputs.suite.value.cases : []
  for (const entry of entries) {
    if (casePathProblem(entry) !== null) continue
    const file = path.join(root, 'conformance', entry)
    try {
      const bytes = readFileSync(file)
      let value = null
      let error = null
      try {
        value = JSON.parse(bytes.toString('utf8'))
      } catch (parseError) {
        error = parseError.message
      }
      inputs.cases.push({ path: entry, sha256: sha256(bytes), value, error })
    } catch (readError) {
      inputs.cases.push({ path: entry, sha256: null, value: null, error: readError.message })
    }
  }
  for (const requirement of arrayOr(inputs.requirements.value?.requirements)) {
    for (const part of arrayOr(requirement?.parts)) {
      for (const file of arrayOr(part?.implementation)) {
        if (typeof file === 'string' && casePathProblem(file) === null) {
          inputs.pathExists.set(file, existsSync(at(file)))
        }
      }
    }
  }
  if (env.CHEMIFREE_SPEC_DIR) inputs.spec = loadSpec(env.CHEMIFREE_SPEC_DIR, inputs)
  return inputs
}

function arrayOr(value) {
  return Array.isArray(value) ? value : []
}

/** Reads the spec files the traceability files name, keeping only derived facts. */
function loadSpec(dir, inputs) {
  const spec = { dir, files: {}, requirementIds: null, operationNames: null, declarations: null }
  const sources = [
    ...arrayOr(inputs.requirements.value?.sources),
    inputs.declarations.value?.source,
  ].filter((source) => typeof source?.path === 'string' && sourcePathProblem(source.path) === null)
  for (const source of sources) {
    const file = path.join(dir, source.path)
    let bytes
    try {
      bytes = readFileSync(file)
    } catch (error) {
      spec.files[source.path] = { error: error.message }
      continue
    }
    spec.files[source.path] = { sha256: sha256(bytes) }
    let value
    try {
      value = JSON.parse(bytes.toString('utf8'))
    } catch (error) {
      spec.files[source.path].error = error.message
      continue
    }
    if (source.role === 'requirements') {
      spec.requirementIds = arrayOr(value.requirements).map((item) => item?.requirement_id)
    } else if (source.role === 'operations') {
      spec.operationNames = arrayOr(value.operations)
    } else if (source.role === 'declarations') {
      spec.declarations = countDeclarations(arrayOr(value.requirements))
    }
  }
  return spec
}

function countDeclarations(items) {
  const bySource = {}
  const routes = {}
  for (const item of items) {
    const source = item?.source_contract
    const route = item?.proposed_route
    bySource[source] = (bySource[source] ?? 0) + 1
    routes[route] ??= { declarations: 0, by_source: {} }
    routes[route].declarations += 1
    routes[route].by_source[source] = (routes[route].by_source[source] ?? 0) + 1
  }
  return { total: items.length, by_source: bySource, routes }
}

/** A source path: relative, under spec/, forward slashes, no empty, '.' or '..' segment. */
export function sourcePathProblem(raw) {
  const problem = casePathProblem(raw)
  if (problem !== null) return problem
  if (raw.includes('\\')) return 'uses a backslash'
  if (!raw.startsWith('spec/')) return 'is not under spec/'
  return null
}

/** The requirement status its parts imply. */
export function deriveStatus(statuses) {
  if (statuses.length === 0) return null
  if (statuses.every((status) => status === 'intentionally_rejected')) {
    return 'intentionally_rejected'
  }
  if (statuses.every((status) => status === 'implemented' || status === 'intentionally_rejected')) {
    return 'implemented'
  }
  if (statuses.some((status) => status === 'implemented' || status === 'partial')) return 'partial'
  if (statuses.some((status) => status === 'unverified')) return 'unverified'
  return 'not_implemented'
}

/**
 * Scripts that `npm run check` invokes. Only a "pure" script is followed: one whose every
 * `&&`-separated segment is exactly `npm run <name>` (three words, the name made of
 * [A-Za-z0-9:_.@/-]). Its named scripts are invoked, in order, as long as the earlier ones
 * succeed. Any other reached script is a leaf: it is invoked, but what it runs is not read.
 * If `check` itself is not pure, nothing is reachable. `pre<name>`/`post<name>` scripts,
 * which npm runs implicitly, are refused for `check` and every reached script.
 * Returns the reached names, `check`'s own steps in order, and errors.
 */
export function reachableScripts(scripts) {
  const errors = []
  const reached = new Set()
  const direct = []
  if (typeof scripts?.check !== 'string') {
    errors.push('package.json: script "check" does not exist')
    return { reached, direct, errors }
  }
  if (pureSteps(scripts.check) === null) {
    errors.push('package.json: script "check" is not a pure chain of npm run steps; nothing is reachable')
    return { reached, direct, errors }
  }
  const queue = ['check']
  while (queue.length > 0) {
    const name = queue.shift()
    if (reached.has(name)) continue
    if (typeof scripts[name] !== 'string') {
      errors.push(`package.json: script ${JSON.stringify(name)} does not exist`)
      continue
    }
    reached.add(name)
    for (const hook of [`pre${name}`, `post${name}`]) {
      if (hook in scripts) {
        errors.push(`package.json: script ${JSON.stringify(hook)} would run implicitly around ${JSON.stringify(name)}`)
      }
    }
    const steps = pureSteps(scripts[name])
    if (steps === null) continue
    if (name === 'check') direct.push(...steps)
    queue.push(...steps)
  }
  return { reached, direct, errors }
}

/** The script names of a pure `npm run a && npm run b` chain, or null. */
function pureSteps(text) {
  const steps = []
  for (const segment of text.split('&&')) {
    const words = segment.trim().split(/\s+/)
    if (words.length !== 3 || words[0] !== 'npm' || words[1] !== 'run' || !SCRIPT_NAME.test(words[2])) {
      return null
    }
    steps.push(words[2])
  }
  return steps
}

function isPlainObject(value) {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isCount(value) {
  return Number.isSafeInteger(value) && value >= 0
}

function sameJson(a, b) {
  return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b))
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical)
  if (isPlainObject(value)) {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonical(value[key])]),
    )
  }
  return value
}

function recount(results) {
  const tally = () => ({ passed: 0, failed: 0, not_run: 0 })
  const counts = { accept: tally(), reject: tally(), total: tally() }
  for (const result of results) {
    if (!OUTCOMES.includes(result?.outcome)) continue
    counts.total[result.outcome] += 1
    if (KINDS.includes(result.kind)) counts[result.kind][result.outcome] += 1
  }
  return counts
}

/** The one totals line both documents must carry. */
export function totalsLine(inputs) {
  const requirements = arrayOr(inputs.requirements.value?.requirements)
  const parts = requirements.flatMap((requirement) => arrayOr(requirement?.parts))
  const byStatus = (items) => {
    const counted = STATUSES.map((status) => [
      status,
      items.filter((item) => item?.status === status).length,
    ]).filter(([, count]) => count > 0)
    return counted.map(([status, count]) => `${status} ${count}`).join(', ')
  }
  let record = null
  try {
    record = JSON.parse(inputs.record.text ?? '')
  } catch {
    record = null
  }
  const counts = recount(arrayOr(record?.results))
  const cases = arrayOr(record?.results).length
  const declarations = inputs.declarations.value ?? {}
  const routes = arrayOr(declarations.routes)
  const sum = (field) => routes.reduce((total, route) => total + (isCount(route?.[field]) ? route[field] : 0), 0)
  return (
    `${TOTALS_PREFIX} 요구사항 ${requirements.length}개(${byStatus(requirements)}) · ` +
    `부분 ${parts.length}개(${byStatus(parts)}) · ` +
    `conformance 사례 ${cases}개(passed ${counts.total.passed}: accept ${counts.accept.passed}·reject ${counts.reject.passed}, ` +
    `not_run ${counts.total.not_run}, failed ${counts.total.failed}) · ` +
    `공개 선언 ${declarations.total ?? 0}개(구현 ${sum('implemented')}, 단일 계획 할당 ${sum('assigned_to_single_plan_entry')})`
  )
}

/** Every rule. Returns a list of problems; empty means every rule holds. */
export function checkInputs(inputs) {
  const errors = []
  const fail = (message) => errors.push(message)
  for (const [name, input] of Object.entries({
    'docs/traceability/requirements.json': inputs.requirements,
    'docs/traceability/declarations.json': inputs.declarations,
    'docs/roadmap/implementation-roadmap.json': inputs.roadmap,
    'package.json': inputs.packageJson,
    'conformance/suite.json': inputs.suite,
  })) {
    if (input.error) fail(`${name}: ${input.error}`)
  }
  if (errors.length > 0) return errors

  const rows = checkRoadmap(inputs.roadmap.value, fail)
  const openRow = (id) => rows.has(id) && rows.get(id).status !== 'done'
  const checkFollowups = (where, followups) => {
    for (const id of followups) {
      if (!rows.has(id)) fail(`${where}: follow-up ${JSON.stringify(id)} is not a roadmap row`)
      else if (!openRow(id)) fail(`${where}: follow-up ${id} is a done roadmap row`)
    }
  }

  const scripts = inputs.packageJson.value?.scripts ?? {}
  const { reached, direct, errors: scriptErrors } = reachableScripts(scripts)
  scriptErrors.forEach(fail)
  for (const name of ['test:conformance', 'test:traceability', 'test:checks']) {
    if (!direct.includes(name)) fail(`package.json: check does not run npm run ${name}`)
  }
  for (const later of ['test:traceability', 'test:checks']) {
    if (direct.indexOf(later) !== -1 && direct.indexOf(later) < direct.indexOf('test:conformance')) {
      fail(`package.json: check runs ${later} before test:conformance`)
    }
  }

  const { cases, record } = checkConformance(inputs, fail)
  const requirementIds = checkRequirements(inputs, { rows, reached, cases, record, checkFollowups, fail })
  for (const { value } of inputs.cases) {
    if (!isPlainObject(value)) continue
    for (const id of arrayOr(value.requirements)) {
      if (!requirementIds.has(id)) fail(`case ${value.id}: requirement ${id} is not traced`)
    }
    if (value.pending !== undefined) {
      checkFollowups(`case ${value.id} pending`, [value.pending?.followup])
    }
  }
  checkMapping(inputs, fail)
  checkDeclarations(inputs, checkFollowups, fail)
  checkTotals(inputs, fail)
  if (inputs.spec) checkSpec(inputs, fail)
  return errors
}

function checkRoadmap(roadmap, fail) {
  const rows = new Map()
  for (const row of arrayOr(roadmap?.rows)) {
    if (typeof row?.id !== 'string' || !ROADMAP_ID.test(row.id)) {
      fail(`roadmap: malformed row id ${JSON.stringify(row?.id)}`)
      continue
    }
    if (rows.has(row.id)) fail(`roadmap: row ${row.id} is repeated`)
    rows.set(row.id, row)
    if (row.status !== 'done') continue
    const delivered = row.delivered
    const valid =
      isPlainObject(delivered) &&
      Array.isArray(delivered.prs) &&
      delivered.prs.length > 0 &&
      delivered.prs.every((pr) => Number.isSafeInteger(pr) && pr > 0) &&
      typeof delivered.merge_commit === 'string' &&
      /^[0-9a-f]{7,40}$/.test(delivered.merge_commit) &&
      typeof delivered.main_ci === 'string' &&
      delivered.main_ci.startsWith('https://')
    if (!valid) fail(`roadmap: done row ${row.id} lacks delivered {prs, merge_commit, main_ci}`)
  }
  if (rows.size === 0) fail('roadmap: no rows')
  return rows
}

/** The roadmap's requirement_mapping keeps one entry per traced requirement, with its status. */
function checkMapping(inputs, fail) {
  const where = 'docs/roadmap/implementation-roadmap.json'
  const traced = new Map(
    arrayOr(inputs.requirements.value?.requirements).map((requirement) => [requirement?.id, requirement?.status]),
  )
  const seen = new Set()
  for (const entry of arrayOr(inputs.roadmap.value?.requirement_mapping)) {
    const id = entry?.id
    if (seen.has(id)) fail(`${where}: requirement_mapping lists ${id} twice`)
    seen.add(id)
    if (!traced.has(id)) {
      fail(`${where}: requirement_mapping lists untraced requirement ${JSON.stringify(id)}`)
    } else if (entry.current_status !== traced.get(id)) {
      fail(
        `${where}: requirement_mapping ${id} current_status ${entry.current_status} differs from the traced status ${traced.get(id)}`,
      )
    }
  }
  for (const id of traced.keys()) {
    if (!seen.has(id)) fail(`${where}: requirement_mapping has no entry for ${id}`)
  }
}

function checkConformance(inputs, fail) {
  const suite = inputs.suite.value
  const cases = new Map()
  if (!isPlainObject(suite) || !Array.isArray(suite.cases)) {
    fail('conformance/suite.json: cases must be a list')
    return { cases, record: null }
  }
  for (const entry of suite.cases) {
    const problem = casePathProblem(entry)
    if (problem !== null) fail(`conformance/suite.json: case path ${JSON.stringify(entry)} ${problem}`)
  }
  for (const loaded of inputs.cases) {
    if (loaded.error) fail(`conformance/${loaded.path}: ${loaded.error}`)
    else if (isPlainObject(loaded.value)) cases.set(loaded.value.id, loaded)
  }

  if (inputs.record.error) {
    fail(`docs/verification/conformance-report.json: ${inputs.record.error}`)
    return { cases, record: null }
  }
  let record
  try {
    record = JSON.parse(inputs.record.text)
  } catch (error) {
    fail(`docs/verification/conformance-report.json: ${error.message}`)
    return { cases, record: null }
  }
  const where = 'docs/verification/conformance-report.json'
  if (record.report_version !== 1) fail(`${where}: report_version must be 1`)
  if (record.suite_version !== suite.suite_version) {
    fail(`${where}: suite_version ${record.suite_version} differs from the suite's ${suite.suite_version}`)
  }
  const results = arrayOr(record.results)
  if (results.length !== inputs.cases.length) {
    fail(`${where}: ${results.length} results for ${inputs.cases.length} suite cases`)
  }
  inputs.cases.forEach((loaded, index) => {
    const result = results[index]
    const value = loaded.value
    if (!isPlainObject(result) || !isPlainObject(value)) return
    const at = `${where}: result ${index} (${result.id})`
    const keys = Object.keys(result).sort()
    if (!sameJson(keys, [...RESULT_FIELDS].sort())) fail(`${at}: fields are ${keys.join(', ')}`)
    for (const field of ['id', 'title', 'requirements', 'axis', 'kind']) {
      if (!sameJson(result[field], value[field])) {
        fail(`${at}: ${field} differs from conformance/${loaded.path}`)
      }
    }
    if (!sameJson(result.pending, value.pending ?? null)) {
      fail(`${at}: pending differs from conformance/${loaded.path}`)
    }
    if (result.case_sha256 !== loaded.sha256) {
      fail(`${at}: case_sha256 is not the SHA-256 of conformance/${loaded.path}; record the run again`)
    }
    if (!OUTCOMES.includes(result.outcome)) fail(`${at}: unknown outcome ${result.outcome}`)
    if ((result.outcome === 'not_run') !== (result.pending !== null && result.pending !== undefined)) {
      fail(`${at}: outcome not_run must go with pending, and only with it`)
    }
    if (result.outcome === 'failed') fail(`${at}: failed: ${result.failure}`)
    if (result.outcome !== 'failed' && result.failure !== null) fail(`${at}: failure is set`)
  })
  if (!sameJson(record.counts, recount(results))) fail(`${where}: counts differ from a recount of the results`)

  if (inputs.fresh.error) {
    fail('target/conformance/report.json is missing: run npm run test:conformance first')
  } else if (inputs.fresh.text !== inputs.record.text) {
    fail(
      'target/conformance/report.json differs from docs/verification/conformance-report.json: ' +
        'run npm run conformance:record and review the change',
    )
  }
  return { cases, record: new Map(results.filter(isPlainObject).map((result) => [result.id, result])) }
}

function checkSources(sources, wanted, where, fail) {
  for (const source of sources) {
    if (!isPlainObject(source)) {
      fail(`${where}: a source is not an object`)
      continue
    }
    const problem = sourcePathProblem(source.path)
    if (problem !== null) fail(`${where}: source path ${JSON.stringify(source.path)} ${problem}`)
    if (typeof source.sha256 !== 'string' || !SHA256.test(source.sha256)) {
      fail(`${where}: source ${JSON.stringify(source.path)} needs a lowercase 64-hex sha256`)
    }
  }
  const roles = sources.map((source) => source?.role)
  for (const role of wanted) {
    const count = roles.filter((found) => found === role).length
    if (count !== 1) fail(`${where}: needs exactly one ${role} source, found ${count}`)
  }
  for (const role of roles) {
    if (!wanted.includes(role)) fail(`${where}: unexpected source role ${JSON.stringify(role)}`)
  }
}

function checkRequirements(inputs, context) {
  const { reached, cases, record, checkFollowups, fail } = context
  const file = inputs.requirements.value
  const where = 'docs/traceability/requirements.json'
  checkSources(arrayOr(file?.sources), ['requirements', 'operations'], where, fail)
  const ids = new Set()
  const partIds = new Set()
  for (const requirement of arrayOr(file?.requirements)) {
    const rid = requirement?.id
    if (typeof rid !== 'string' || !REQUIREMENT_ID.test(rid)) {
      fail(`${where}: malformed requirement id ${JSON.stringify(rid)}`)
      continue
    }
    if (ids.has(rid)) fail(`${where}: requirement ${rid} is repeated`)
    ids.add(rid)
    for (const key of Object.keys(requirement)) {
      if (!REQUIREMENT_FIELDS.includes(key)) fail(`${rid}: unknown field ${key}`)
    }
    if (typeof requirement.summary !== 'string' || requirement.summary.trim() === '') {
      fail(`${rid}: summary is empty`)
    }
    if (!Array.isArray(requirement.operations) || !requirement.operations.every((name) => typeof name === 'string')) {
      fail(`${rid}: operations must be a list of names`)
    }
    if (!STATUSES.includes(requirement.status)) fail(`${rid}: unknown status ${requirement.status}`)
    const parts = arrayOr(requirement.parts)
    if (parts.length === 0) {
      fail(`${rid}: has no parts`)
      continue
    }
    const derived = deriveStatus(parts.map((part) => part?.status))
    if (requirement.status !== derived) {
      fail(`${rid}: status ${requirement.status} differs from ${derived}, derived from its parts`)
    }
    const catchAll = parts.filter((part) => part?.catch_all === true)
    if (catchAll.length !== 1) fail(`${rid}: needs exactly one catch-all part, found ${catchAll.length}`)
    for (const part of parts) {
      checkPart(rid, part, { partIds, reached, cases, record, checkFollowups, fail, inputs })
    }
  }
  return ids
}

function checkPart(rid, part, context) {
  const { partIds, reached, cases, record, checkFollowups, fail, inputs } = context
  if (!isPlainObject(part)) {
    fail(`${rid}: a part is not an object`)
    return
  }
  const pid = typeof part.id === 'string' ? part.id : JSON.stringify(part.id)
  const at = `${rid} part ${pid}`
  if (typeof part.id !== 'string' || !part.id.startsWith(`${rid}.`) || part.id.length <= rid.length + 1) {
    fail(`${at}: id must start with ${rid}.`)
  }
  if (partIds.has(part.id)) fail(`${at}: part id is repeated`)
  partIds.add(part.id)
  for (const key of Object.keys(part)) {
    if (!PART_FIELDS.includes(key)) fail(`${at}: unknown field ${key}`)
  }
  if (typeof part.summary !== 'string' || part.summary.trim() === '') fail(`${at}: summary is empty`)
  if (!STATUSES.includes(part.status)) fail(`${at}: unknown status ${part.status}`)
  if (part.catch_all !== undefined && part.catch_all !== true) fail(`${at}: catch_all must be true or absent`)
  if (part.catch_all === true && !['not_implemented', 'unverified'].includes(part.status)) {
    fail(`${at}: a catch-all part can only be not_implemented or unverified`)
  }
  if (part.status === 'intentionally_rejected') {
    if (typeof part.reason !== 'string' || part.reason.trim() === '') fail(`${at}: a rejected part needs a reason`)
  } else if (part.reason !== undefined) {
    fail(`${at}: only a rejected part has a reason`)
  }
  const lists = {}
  for (const field of ['implementation', 'evidence', 'followups']) {
    if (!Array.isArray(part[field]) || !part[field].every((item) => typeof item === 'string')) {
      fail(`${at}: ${field} must be a list of strings`)
      lists[field] = []
    } else {
      lists[field] = part[field]
    }
  }
  for (const file of lists.implementation) {
    const problem = casePathProblem(file)
    if (problem !== null) fail(`${at}: implementation path ${JSON.stringify(file)} ${problem}`)
    else if (inputs.pathExists.get(file) !== true) fail(`${at}: implementation path ${file} does not exist`)
  }
  let executing = 0
  for (const evidence of lists.evidence) {
    const problem = evidenceProblem(rid, evidence, { reached, cases, record })
    if (problem === null) executing += 1
    else fail(`${at}: evidence ${evidence} ${problem}`)
  }
  if (part.status === 'implemented' || part.status === 'partial') {
    if (lists.implementation.length === 0) fail(`${at}: an ${part.status} part needs an implementation path`)
    if (executing === 0) fail(`${at}: an ${part.status} part needs evidence that executes`)
  }
  if (['not_implemented', 'unverified', 'partial'].includes(part.status) && lists.followups.length === 0) {
    fail(`${at}: a ${part.status} part needs follow-ups`)
  }
  if (part.status === 'implemented' && lists.followups.length > 0) {
    fail(`${at}: an implemented part has no follow-ups`)
  }
  checkFollowups(at, lists.followups)
}

/** Null when the evidence executes, else why not. */
function evidenceProblem(rid, evidence, { reached, cases, record }) {
  if (evidence.startsWith('case:')) {
    const id = evidence.slice('case:'.length)
    const loaded = cases.get(id)
    if (!loaded) return 'names no suite case'
    if (!arrayOr(loaded.value.requirements).includes(rid)) return `is a case that does not name ${rid}`
    if (record?.get(id)?.outcome !== 'passed') return 'did not pass in the recorded run'
    return null
  }
  if (evidence.startsWith('check:')) {
    const name = evidence.slice('check:'.length)
    if (!reached.has(name)) return 'is not a script npm run check invokes'
    return null
  }
  return 'is neither case:<id> nor check:<npm script>'
}

function checkDeclarations(inputs, checkFollowups, fail) {
  const file = inputs.declarations.value
  const where = 'docs/traceability/declarations.json'
  checkSources(isPlainObject(file?.source) ? [file.source] : [], ['declarations'], where, fail)
  if (typeof file?.about !== 'string' || !file.about.includes('not a feature count')) {
    fail(`${where}: about must say that a declaration count is not a feature count`)
  }
  if (!isCount(file?.total)) fail(`${where}: total must be a non-negative integer`)
  const bySource = isPlainObject(file?.by_source) ? file.by_source : {}
  if (!isPlainObject(file?.by_source) || !Object.values(bySource).every(isCount)) {
    fail(`${where}: by_source must map sources to non-negative integers`)
  }
  const sumOf = (values) => values.reduce((total, value) => total + (isCount(value) ? value : 0), 0)
  if (sumOf(Object.values(bySource)) !== file?.total) fail(`${where}: by_source does not sum to total`)
  const routes = arrayOr(file?.routes)
  const names = new Set()
  const perSource = {}
  for (const route of routes) {
    const at = `${where}: route ${JSON.stringify(route?.route)}`
    if (typeof route?.route !== 'string' || route.route === '') {
      fail(`${at}: needs a name`)
      continue
    }
    if (names.has(route.route)) fail(`${at}: repeated`)
    names.add(route.route)
    for (const field of ['declarations', 'implemented', 'assigned_to_single_plan_entry']) {
      if (!isCount(route[field])) fail(`${at}: ${field} must be a non-negative integer`)
    }
    if (!(route.implemented <= route.assigned_to_single_plan_entry && route.assigned_to_single_plan_entry <= route.declarations)) {
      fail(`${at}: needs 0 <= implemented <= assigned_to_single_plan_entry <= declarations`)
    }
    const routeSources = isPlainObject(route.by_source) ? route.by_source : {}
    if (!isPlainObject(route.by_source) || !Object.values(routeSources).every(isCount)) {
      fail(`${at}: by_source must map sources to non-negative integers`)
    }
    if (sumOf(Object.values(routeSources)) !== route.declarations) fail(`${at}: by_source does not sum to declarations`)
    for (const [source, count] of Object.entries(routeSources)) {
      if (!(source in bySource)) fail(`${at}: source ${source} is not in the top-level by_source`)
      perSource[source] = (perSource[source] ?? 0) + (isCount(count) ? count : 0)
    }
    if (!ROUTE_STATUSES.includes(route.status)) fail(`${at}: unknown status ${route.status}`)
    const expected =
      route.implemented === 0
        ? ['not_implemented', 'unverified']
        : route.implemented === route.declarations
          ? ['implemented']
          : ['partial']
    if (!expected.includes(route.status)) {
      fail(`${at}: status ${route.status} does not fit ${route.implemented} of ${route.declarations} implemented`)
    }
    const followups = arrayOr(route.followups)
    if (route.status !== 'implemented' && followups.length === 0) fail(`${at}: needs follow-ups`)
    checkFollowups(at, followups)
  }
  if (sumOf(routes.map((route) => route?.declarations)) !== file?.total) fail(`${where}: routes do not sum to total`)
  for (const [source, count] of Object.entries(bySource)) {
    if ((perSource[source] ?? 0) !== count) fail(`${where}: routes count ${perSource[source] ?? 0} for ${source}, by_source ${count}`)
  }
}

function checkTotals(inputs, fail) {
  const expected = totalsLine(inputs)
  for (const doc of TOTALS_DOCS) {
    const input = inputs.docs[doc]
    if (!input || input.error) {
      fail(`${doc}: ${input?.error ?? 'missing'}`)
      continue
    }
    const lines = input.text.split(/\r?\n/).filter((line) => line.startsWith(TOTALS_PREFIX))
    if (lines.length !== 1 || lines[0] !== expected) {
      fail(`${doc}: needs exactly one line starting ${TOTALS_PREFIX} (found ${lines.length}); paste:\n${expected}`)
    }
  }
}

function checkSpec(inputs, fail) {
  const { spec } = inputs
  const sources = [...arrayOr(inputs.requirements.value?.sources), inputs.declarations.value?.source]
  for (const source of sources) {
    if (typeof source?.path !== 'string') continue
    const found = spec.files[source.path]
    if (!found) fail(`spec: ${source.path} was not read`)
    else if (found.error) fail(`spec: ${source.path}: ${found.error}`)
    else if (found.sha256 !== source.sha256) {
      fail(`spec: ${source.path} has SHA-256 ${found.sha256}, recorded ${source.sha256}`)
    }
  }
  const requirements = arrayOr(inputs.requirements.value?.requirements)
  if (spec.requirementIds === null) fail('spec: requirement ids were not read')
  else {
    const traced = requirements.map((requirement) => requirement?.id).sort()
    if (!sameJson(traced, [...spec.requirementIds].sort())) {
      fail(`spec: traced requirement ids differ from the spec's ${spec.requirementIds.length}`)
    }
  }
  if (spec.operationNames === null) fail('spec: operation names were not read')
  else {
    for (const requirement of requirements) {
      for (const name of arrayOr(requirement?.operations)) {
        if (!spec.operationNames.includes(name)) fail(`spec: ${requirement.id} names unknown operation ${name}`)
      }
    }
  }
  if (spec.declarations === null) fail('spec: declarations were not read')
  else {
    const file = inputs.declarations.value
    if (file?.total !== spec.declarations.total) fail(`spec: declaration total ${spec.declarations.total}, recorded ${file?.total}`)
    if (!sameJson(file?.by_source, spec.declarations.by_source)) fail('spec: declaration counts per source differ')
    const recorded = Object.fromEntries(
      arrayOr(file?.routes).map((route) => [route?.route, { declarations: route?.declarations, by_source: route?.by_source }]),
    )
    if (!sameJson(recorded, spec.declarations.routes)) fail('spec: declaration counts per route differ')
  }
}

function main() {
  const inputs = loadInputs()
  const errors = checkInputs(inputs)
  if (errors.length > 0) {
    for (const error of errors) console.error(`- ${error}`)
    console.error(`traceability: ${errors.length} problem(s)`)
    process.exitCode = 1
    return
  }
  console.log(`traceability: ok\n${totalsLine(inputs)}`)
  console.log(
    inputs.spec
      ? `spec: sources, requirement ids, operation names and declaration counts compared with ${inputs.spec.dir}`
      : 'spec: not compared (set CHEMIFREE_SPEC_DIR to the directory that holds spec/)',
  )
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) main()
