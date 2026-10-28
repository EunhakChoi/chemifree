// Checks docs/traceability against the code, the conformance suite and its recorded run, the
// roadmap, and the totals quoted in the docs. With CHEMIFREE_SPEC_DIR set to the specification
// workspace root, it also checks source hashes, requirement IDs, operation names, and
// declaration counts against spec/ there; without it (as in CI) only those comparisons are
// skipped. Exits 1 and lists every problem when anything disagrees.
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const STATUSES = ['implemented', 'partial', 'not_implemented', 'unverified', 'intentionally_rejected']
const EVIDENCE_KINDS = ['case', 'test', 'smoke', 'report', 'check', 'doc']
const TOTALS_PREFIX = '현재 집계:'
const TOTALS_DOCS = ['docs/TRACEABILITY.ko.md', 'docs/HANDOFF.ko.md']
const REPORT = 'docs/verification/conformance-report.json'
const FRESH_REPORT = 'target/conformance/report.json'

/** The repository root for a module in scripts/, from its URL (which may encode spaces). */
export function rootFromModuleUrl(url) {
  return path.resolve(path.dirname(fileURLToPath(url)), '..')
}

/**
 * The status of a requirement, derived from its parts' statuses by the one documented rule:
 * every part intentionally_rejected → intentionally_rejected; every part implemented or
 * intentionally_rejected → implemented; otherwise any part implemented or partial → partial;
 * otherwise any part unverified → unverified; otherwise not_implemented.
 */
export function deriveStatus(statuses) {
  if (statuses.every((status) => status === 'intentionally_rejected')) return 'intentionally_rejected'
  if (statuses.every((status) => status === 'implemented' || status === 'intentionally_rejected')) return 'implemented'
  if (statuses.some((status) => status === 'implemented' || status === 'partial')) return 'partial'
  if (statuses.includes('unverified')) return 'unverified'
  return 'not_implemented'
}

const readJson = (file) => JSON.parse(readFileSync(file, 'utf8'))
const readText = (file) => (existsSync(file) ? readFileSync(file, 'utf8') : null)

/** Everything the check reads from the repository at `root`. */
export function loadRepository(root) {
  const at = (relative) => path.join(root, relative)
  const suite = readJson(at('conformance/suite.json'))
  return {
    root,
    requirements: readJson(at('docs/traceability/requirements.json')),
    declarations: readJson(at('docs/traceability/declarations.json')),
    roadmap: readJson(at('docs/roadmap/implementation-roadmap.json')),
    packageJson: readJson(at('package.json')),
    suite,
    cases: suite.cases.map((file) => readJson(path.join(root, 'conformance', file))),
    report: readJson(at(REPORT)),
    freshReport: existsSync(at(FRESH_REPORT)) ? readJson(at(FRESH_REPORT)) : null,
    docs: Object.fromEntries(TOTALS_DOCS.map((doc) => [doc, readText(at(doc))])),
  }
}

/** The npm scripts `npm run check` runs, following `npm run X` through nested scripts. */
export function scriptsRunByCheck(scripts) {
  const reached = new Set()
  const visit = (name) => {
    if (reached.has(name) || typeof scripts[name] !== 'string') return
    reached.add(name)
    for (const command of scripts[name].split('&&')) {
      const match = /^npm run ([\w:.-]+)$/.exec(command.trim())
      if (match) visit(match[1])
    }
  }
  visit('check')
  reached.delete('check')
  return reached
}

const isRelativePath = (value) =>
  typeof value === 'string' && value !== '' && !path.isAbsolute(value) && !value.split(/[\\/]/).includes('..')

/** Checks the inputs from `loadRepository`; returns the problems and the derived totals. */
export function checkTraceability(inputs) {
  const problems = []
  const problem = (message) => problems.push(message)
  const exists = (relative) => isRelativePath(relative) && existsSync(path.join(inputs.root, relative))
  const roadmapRows = new Map((inputs.roadmap.rows ?? []).map((row) => [row.id, row]))

  checkRoadmap(roadmapRows, problem)
  const followupsUsable = (owner, followups) => {
    for (const followup of followups) {
      const row = roadmapRows.get(followup)
      if (!row) problem(`${owner}: follow-up ${followup} is not in the roadmap`)
      else if (row.status === 'done') problem(`${owner}: follow-up ${followup} is already done`)
    }
  }

  const cases = checkCases(inputs, problem, followupsUsable)
  const report = checkReport(inputs, cases, problem)
  const outcomes = new Map(report.results.map((result) => [result.id, result.outcome]))
  const requirementIds = checkRequirements(inputs, { problem, exists, followupsUsable, cases, outcomes })
  for (const testCase of inputs.cases) {
    for (const id of testCase.requirements ?? []) {
      if (!requirementIds.has(id)) problem(`case ${testCase.id}: requirement ${id} is not traced`)
    }
  }
  checkDeclarations(inputs.declarations, problem, followupsUsable)

  const totals = deriveTotals(inputs)
  const sentence = totalsSentence(totals)
  for (const [doc, text] of Object.entries(inputs.docs)) {
    if (text === null) {
      problem(`${doc} is missing`)
      continue
    }
    const lines = text.split('\n').filter((line) => line.startsWith(TOTALS_PREFIX))
    if (lines.length !== 1 || lines[0] !== sentence) {
      problem(
        `${doc}: the totals line must be exactly one line equal to the derived totals` +
          `${lines.length === 1 ? '' : ` (found ${lines.length} lines starting with "${TOTALS_PREFIX}")`}. Paste:\n${sentence}`,
      )
    }
  }
  return { problems, totals }
}

function checkRoadmap(rows, problem) {
  for (const row of rows.values()) {
    if (row.status !== 'done') continue
    const delivered = row.delivered
    const valid =
      delivered &&
      Array.isArray(delivered.prs) &&
      delivered.prs.length > 0 &&
      delivered.prs.every((pr) => Number.isInteger(pr) && pr > 0) &&
      typeof delivered.merge_commit === 'string' &&
      /^[0-9a-f]{7,40}$/.test(delivered.merge_commit) &&
      typeof delivered.main_ci === 'string' &&
      delivered.main_ci.startsWith('https://')
    if (!valid) problem(`roadmap ${row.id} is done but lacks delivered {prs, merge_commit, main_ci}`)
  }
}

/** Case files as listed by the suite; returns them by id. */
function checkCases(inputs, problem, followupsUsable) {
  const byId = new Map()
  for (const testCase of inputs.cases) {
    if (byId.has(testCase.id)) problem(`case id ${testCase.id} is used twice`)
    byId.set(testCase.id, testCase)
    if (testCase.pending) followupsUsable(`case ${testCase.id}`, [testCase.pending.followup])
  }
  return byId
}

const sameJson = (left, right) => JSON.stringify(left) === JSON.stringify(right)

/** The recorded report must be a run of exactly the current cases, with consistent counts. */
function checkReport(inputs, cases, problem) {
  const report = inputs.report
  const results = Array.isArray(report.results) ? report.results : []
  if (report.report_version !== 1) problem(`${REPORT}: report_version must be 1`)
  if (report.suite_version !== inputs.suite.suite_version) {
    problem(`${REPORT}: suite_version ${report.suite_version} differs from the suite's ${inputs.suite.suite_version}`)
  }
  const expectedIds = inputs.cases.map((testCase) => testCase.id)
  const recordedIds = results.map((result) => result.id)
  if (!sameJson(expectedIds, recordedIds)) {
    problem(`${REPORT}: results must list the suite's cases in order; run npm run conformance:record`)
  }
  for (const result of results) {
    const testCase = cases.get(result.id)
    if (!testCase) continue
    for (const field of ['title', 'requirements', 'axis', 'kind']) {
      if (!sameJson(result[field], testCase[field])) {
        problem(`${REPORT}: ${result.id} records a different ${field} than its case file`)
      }
    }
    if (!sameJson(result.pending ?? null, testCase.pending ?? null)) {
      problem(`${REPORT}: ${result.id} records a different pending follow-up or reason than its case file`)
    }
    const expectedNotRun = Boolean(testCase.pending)
    if ((result.outcome === 'not_run') !== expectedNotRun) {
      problem(`${REPORT}: ${result.id} must be not_run exactly when it is pending`)
    }
    if (!['passed', 'failed', 'not_run'].includes(result.outcome)) {
      problem(`${REPORT}: ${result.id} has unknown outcome ${result.outcome}`)
    }
    if (result.outcome === 'failed') problem(`${REPORT}: ${result.id} failed in the recorded run`)
  }
  const recount = { accept: blankCounts(), reject: blankCounts(), total: blankCounts() }
  for (const result of results) {
    const key = result.outcome === 'not_run' ? 'not_run' : result.outcome
    if (!(key in recount.total)) continue
    if (result.kind in recount && result.kind !== 'total') recount[result.kind][key] += 1
    recount.total[key] += 1
  }
  if (!sameJson(report.counts, recount)) {
    problem(`${REPORT}: counts ${JSON.stringify(report.counts)} differ from a recount of its results ${JSON.stringify(recount)}`)
  }
  if (inputs.freshReport && !sameJson(inputs.freshReport, report)) {
    problem(`${REPORT} differs from the fresh run in ${FRESH_REPORT}; run npm run conformance:record`)
  }
  return { results }
}

const blankCounts = () => ({ passed: 0, failed: 0, not_run: 0 })

function checkRequirements(inputs, context) {
  const { problem, exists, followupsUsable, cases, outcomes } = context
  const document = inputs.requirements
  const ids = new Set()
  if (document.schema_version !== 1) problem('requirements.json: schema_version must be 1')
  for (const source of document.sources ?? []) {
    if (!isRelativePath(source.path) || !/^[0-9a-f]{64}$/.test(source.sha256 ?? '')) {
      problem(`requirements.json: source ${source.path} needs a relative path and a SHA-256`)
    }
  }
  const checkScripts = scriptsRunByCheck(inputs.packageJson.scripts ?? {})
  const scriptCommands = Object.values(inputs.packageJson.scripts ?? {})
  for (const requirement of document.requirements ?? []) {
    const owner = requirement.id
    if (!/^[A-Z][A-Z0-9]*-\d{2}$/.test(owner ?? '')) problem(`requirement id ${owner} is malformed`)
    if (ids.has(owner)) problem(`requirement ${owner} is listed twice`)
    ids.add(owner)
    if (!STATUSES.includes(requirement.status)) problem(`${owner}: unknown status ${requirement.status}`)
    for (const operation of requirement.operations ?? []) {
      if (!/^[a-z]+\.[a-z_]+$/.test(operation)) problem(`${owner}: operation ${operation} is malformed`)
    }
    const parts = requirement.parts ?? []
    if (parts.length === 0) problem(`${owner}: has no parts`)
    if (parts.filter((part) => part.catch_all === true).length !== 1) {
      problem(`${owner}: needs exactly one catch_all part for what is not covered`)
    }
    const partIds = new Set()
    for (const part of parts) {
      const where = `${owner} part ${part.id}`
      if (typeof part.id !== 'string' || !part.id.startsWith(`${owner}.`)) problem(`${where}: id must start with ${owner}.`)
      if (partIds.has(part.id)) problem(`${where}: id is used twice`)
      partIds.add(part.id)
      if (!STATUSES.includes(part.status)) problem(`${where}: unknown status ${part.status}`)
      if (typeof part.summary !== 'string' || part.summary.trim() === '') problem(`${where}: needs a summary`)
      const claimed = part.status === 'implemented' || part.status === 'partial'
      const implementation = part.implementation ?? []
      const evidence = part.evidence ?? []
      const followups = part.followups ?? []
      if (claimed && implementation.length === 0) problem(`${where}: an ${part.status} part names its implementation`)
      if (claimed && evidence.length === 0) problem(`${where}: an ${part.status} part needs evidence`)
      for (const file of implementation) {
        if (!exists(file)) problem(`${where}: implementation path ${file} does not exist`)
      }
      for (const ref of evidence) {
        const issue = evidenceProblem(ref, { exists, cases, outcomes, claimed, checkScripts, scriptCommands, root: inputs.root })
        if (issue) problem(`${where}: evidence ${ref} ${issue}`)
      }
      if ((part.status === 'not_implemented' || part.status === 'partial') && followups.length === 0) {
        problem(`${where}: a ${part.status} part needs follow-ups`)
      }
      if (part.status === 'implemented' && followups.length > 0) problem(`${where}: an implemented part has no follow-ups`)
      followupsUsable(where, followups)
    }
    const derived = deriveStatus(parts.map((part) => part.status))
    if (requirement.status !== derived) {
      problem(`${owner}: stored status ${requirement.status} differs from the status derived from its parts, ${derived}`)
    }
  }
  return ids
}

/** Why an evidence reference does not resolve, or null when it does. */
export function evidenceProblem(ref, context) {
  const separator = typeof ref === 'string' ? ref.indexOf(':') : -1
  if (separator < 1) return 'is not of the form kind:target'
  const kind = ref.slice(0, separator)
  const target = ref.slice(separator + 1)
  if (!EVIDENCE_KINDS.includes(kind)) return `has unknown kind ${kind}`
  switch (kind) {
    case 'case': {
      if (!context.cases.has(target)) return 'names no case in the suite'
      if (context.claimed && context.outcomes.get(target) !== 'passed') {
        return 'must be a case that passed in the recorded report'
      }
      return null
    }
    case 'check':
      return context.checkScripts.has(target) ? null : 'is not a script that npm run check runs'
    case 'smoke': {
      if (!context.exists(target)) return 'names a missing file'
      return context.scriptCommands.some((command) => command.split(/\s+/).includes(target))
        ? null
        : 'is not run by any npm script'
    }
    case 'report': {
      if (!context.exists(target)) return 'names a missing file'
      try {
        JSON.parse(readFileSync(path.join(context.root, target), 'utf8'))
        return null
      } catch {
        return 'is not a JSON report'
      }
    }
    case 'doc':
      return context.exists(target) ? null : 'names a missing file'
    case 'test': {
      const [file, name] = target.split('#')
      if (!context.exists(file)) return 'names a missing file'
      if (name === undefined) return null
      const text = readFileSync(path.join(context.root, file), 'utf8')
      const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const defined = new RegExp(`fn\\s+${escaped}\\s*[(<]|test\\(\\s*['"\`]${escaped}['"\`]`).test(text)
      return defined ? null : `names no test ${name} in ${file}`
    }
    default:
      return `has unknown kind ${kind}`
  }
}

function checkDeclarations(declarations, problem, followupsUsable) {
  const where = 'declarations.json'
  const sum = (values) => Object.values(values ?? {}).reduce((total, value) => total + value, 0)
  const routes = declarations.routes ?? []
  if (routes.reduce((total, route) => total + route.declarations, 0) !== declarations.declarations) {
    problem(`${where}: route counts do not add up to ${declarations.declarations}`)
  }
  if (sum(declarations.by_source) !== declarations.declarations) problem(`${where}: by_source does not add up`)
  if (!String(declarations.about ?? '').includes('not a count of features')) {
    problem(`${where}: about must say that a declaration count is not a count of features`)
  }
  let implemented = 0
  let assigned = 0
  for (const route of routes) {
    const owner = `${where} route ${route.route}`
    if (sum(route.by_source) !== route.declarations) problem(`${owner}: by_source does not add up`)
    if (sum(route.by_evidence_status) !== route.declarations) problem(`${owner}: by_evidence_status does not add up`)
    if (!Number.isInteger(route.implemented) || !Number.isInteger(route.assigned_to_single_plan_entry)) {
      problem(`${owner}: implemented and assigned_to_single_plan_entry are counts`)
    }
    implemented += route.implemented
    assigned += route.assigned_to_single_plan_entry
    const expectedStatus =
      route.implemented === 0 ? 'not_implemented' : route.implemented === route.declarations ? 'implemented' : 'partial'
    if (route.status !== expectedStatus) problem(`${owner}: status ${route.status} does not match ${route.implemented} implemented`)
    if (route.status !== 'implemented' && (route.followups ?? []).length === 0) problem(`${owner}: needs follow-ups`)
    followupsUsable(owner, route.followups ?? [])
  }
  if (implemented !== declarations.implemented) problem(`${where}: implemented does not match its routes`)
  if (assigned !== declarations.assigned_to_single_plan_entry) {
    problem(`${where}: assigned_to_single_plan_entry does not match its routes`)
  }
}

/** Totals derived from the traceability files and the recorded conformance run. */
export function deriveTotals(inputs) {
  const requirements = inputs.requirements.requirements ?? []
  const count = (statuses) => Object.fromEntries(STATUSES.map((status) => [status, statuses.filter((s) => s === status).length]))
  const parts = requirements.flatMap((requirement) => requirement.parts ?? [])
  return {
    requirements: requirements.length,
    byStatus: count(requirements.map((requirement) => deriveStatus((requirement.parts ?? []).map((part) => part.status)))),
    partial: requirements
      .filter((requirement) => deriveStatus((requirement.parts ?? []).map((part) => part.status)) === 'partial')
      .map((requirement) => requirement.id),
    parts: parts.length,
    partsByStatus: count(parts.map((part) => part.status)),
    declarations: inputs.declarations.declarations,
    declarationsImplemented: inputs.declarations.implemented,
    declarationsAssigned: inputs.declarations.assigned_to_single_plan_entry,
    cases: inputs.report.results?.length ?? 0,
    counts: inputs.report.counts,
  }
}

/** The one-line totals sentence the docs must quote exactly. */
export function totalsSentence(totals) {
  const statuses = (counts) => STATUSES.map((status) => `${status} ${counts[status]}`).join(', ')
  const counts = totals.counts ?? { accept: blankCounts(), reject: blankCounts(), total: blankCounts() }
  return (
    `${TOTALS_PREFIX} 요구사항 ${totals.requirements}개 — ${statuses(totals.byStatus)}` +
    ` (partial: ${totals.partial.join(', ') || '없음'}). 부분 항목 ${totals.parts}개 — ${statuses(totals.partsByStatus)}.` +
    ` 공개 선언 ${totals.declarations}개 중 구현 ${totals.declarationsImplemented}개, 단일 계획 항목 배정 ${totals.declarationsAssigned}개.` +
    ` 적합성 사례 ${totals.cases}개 — accept 통과 ${counts.accept.passed}·실패 ${counts.accept.failed}·미실행 ${counts.accept.not_run},` +
    ` reject 통과 ${counts.reject.passed}·실패 ${counts.reject.failed}·미실행 ${counts.reject.not_run}.`
  )
}

const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex')

/** Comparisons against the specification workspace at `specDir` (its root, holding spec/). */
export function checkAgainstSpec(inputs, specDir) {
  const problems = []
  const problem = (message) => problems.push(message)
  const specFile = (relative) => {
    if (!isRelativePath(relative) || !relative.startsWith('spec/')) {
      problem(`source ${relative} must be a relative path under spec/`)
      return null
    }
    const file = path.join(specDir, relative)
    if (!existsSync(file)) {
      problem(`source ${relative} is missing from ${specDir}`)
      return null
    }
    return file
  }
  const sources = [...(inputs.requirements.sources ?? []), inputs.declarations.source].filter(Boolean)
  for (const source of sources) {
    const file = specFile(source.path)
    if (file && sha256(file) !== source.sha256) problem(`source ${source.path}: SHA-256 differs from the recorded one`)
  }
  const bySuffix = (suffix) => (inputs.requirements.sources ?? []).find((source) => source.path.endsWith(suffix))
  const requirementSource = bySuffix('readiness-requirements-v2.json')
  const operationSource = bySuffix('operations.json')
  const ours = inputs.requirements.requirements ?? []
  if (requirementSource && specFile(requirementSource.path)) {
    const spec = readJson(path.join(specDir, requirementSource.path))
    const specIds = (spec.requirements ?? []).map((requirement) => requirement.requirement_id)
    const ourIds = ours.map((requirement) => requirement.id)
    if (specIds.length !== 33 || !sameJson([...specIds].sort(), [...ourIds].sort())) {
      problem(`requirement IDs differ from the specification's ${specIds.length}`)
    }
    const domains = new Map((spec.requirements ?? []).map((requirement) => [requirement.requirement_id, requirement.domain]))
    for (const requirement of ours) {
      if (domains.has(requirement.id) && domains.get(requirement.id) !== requirement.domain) {
        problem(`${requirement.id}: domain ${requirement.domain} differs from the specification's`)
      }
    }
  } else problem('requirements.json names no requirement source')
  if (operationSource && specFile(operationSource.path)) {
    const operations = new Set(readJson(path.join(specDir, operationSource.path)).operations ?? [])
    for (const requirement of ours) {
      for (const operation of requirement.operations ?? []) {
        if (!operations.has(operation)) problem(`${requirement.id}: operation ${operation} is not in the specification`)
      }
    }
  } else problem('requirements.json names no operation source')
  const declarationFile = inputs.declarations.source && specFile(inputs.declarations.source.path)
  if (declarationFile) problems.push(...declarationCountProblems(inputs.declarations, readJson(declarationFile)))
  return problems
}

/** Recounts the specification's declarations per route, source, and evidence status. */
export function declarationCountProblems(declarations, spec) {
  const problems = []
  const sourceLabel = new Map(Object.entries(declarations.source_labels ?? {}).map(([label, name]) => [name, label]))
  const evidenceLabel = new Map(Object.entries(declarations.evidence_status_labels ?? {}).map(([label, name]) => [name, label]))
  const recount = new Map()
  const bySource = {}
  const items = spec.requirements ?? []
  for (const item of items) {
    const source = sourceLabel.get(item.source_contract) ?? `unlabelled:${item.source_contract}`
    const evidence = evidenceLabel.get(item.evidence_status) ?? `unlabelled:${item.evidence_status}`
    const route = recount.get(item.proposed_route) ?? { declarations: 0, by_source: {}, by_evidence_status: {} }
    route.declarations += 1
    route.by_source[source] = (route.by_source[source] ?? 0) + 1
    route.by_evidence_status[evidence] = (route.by_evidence_status[evidence] ?? 0) + 1
    recount.set(item.proposed_route, route)
    bySource[source] = (bySource[source] ?? 0) + 1
  }
  const sorted = (object) => Object.fromEntries(Object.entries(object).sort(([a], [b]) => a.localeCompare(b)))
  if (items.length !== declarations.declarations) {
    problems.push(`declarations: the specification has ${items.length}, the file records ${declarations.declarations}`)
  }
  if (!sameJson(sorted(bySource), sorted(declarations.by_source ?? {}))) {
    problems.push(`declarations: by_source differs from the specification's ${JSON.stringify(sorted(bySource))}`)
  }
  const recorded = new Map((declarations.routes ?? []).map((route) => [route.route, route]))
  for (const name of new Set([...recount.keys(), ...recorded.keys()])) {
    const expected = recount.get(name)
    const route = recorded.get(name)
    if (!expected || !route) {
      problems.push(`declarations: route ${name} is ${expected ? 'missing from the file' : 'not in the specification'}`)
      continue
    }
    for (const field of ['declarations', 'by_source', 'by_evidence_status']) {
      const want = typeof expected[field] === 'number' ? expected[field] : sorted(expected[field])
      const have = typeof route[field] === 'number' ? route[field] : sorted(route[field] ?? {})
      if (!sameJson(want, have)) {
        problems.push(`declarations: route ${name} ${field} ${JSON.stringify(have)} differs from the specification's ${JSON.stringify(want)}`)
      }
    }
  }
  return problems
}

function main() {
  const root = rootFromModuleUrl(import.meta.url)
  const inputs = loadRepository(root)
  const { problems, totals } = checkTraceability(inputs)
  const specDir = process.env.CHEMIFREE_SPEC_DIR
  if (specDir) problems.push(...checkAgainstSpec(inputs, path.resolve(specDir)))
  for (const message of problems) console.error(`traceability: ${message}`)
  if (problems.length > 0) {
    console.error(`traceability: ${problems.length} problem(s)`)
    process.exit(1)
  }
  console.log(totalsSentence(totals))
  console.log(
    specDir
      ? 'traceability: checked, including the specification comparisons'
      : 'traceability: checked; specification comparisons skipped (set CHEMIFREE_SPEC_DIR to run them)',
  )
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
