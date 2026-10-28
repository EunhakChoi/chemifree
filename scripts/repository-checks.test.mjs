// Tests for scripts/traceability-check.mjs and scripts/clean-room-check.mjs. The traceability
// tests start from this repository's real files and break one thing at a time; the clean-room
// tests build small Git repositories in a temporary directory.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { after, describe, test } from 'node:test'

import { checkCleanRoom, forbiddenNamesIn, localPathProblem, magicProblem } from './clean-room-check.mjs'
import {
  checkAgainstSpec,
  checkTraceability,
  deriveStatus,
  loadRepository,
  rootFromModuleUrl,
  scriptsRunByCheck,
  totalsSentence,
} from './traceability-check.mjs'

const root = rootFromModuleUrl(import.meta.url)
const pristine = loadRepository(root)
const temporary = mkdtempSync(path.join(tmpdir(), 'chemifree checks '))
after(() => rmSync(temporary, { recursive: true, force: true }))

/** A deep copy of the repository's inputs and their derived totals. */
function inputs() {
  const copy = structuredClone(pristine)
  const { totals } = checkTraceability(copy)
  return { copy, totals }
}

/** The problems `change` adds to the repository's inputs. */
function problemsAfter(change) {
  const { copy } = inputs()
  const baseline = new Set(checkTraceability(copy).problems)
  change(copy)
  return checkTraceability(copy).problems.filter((problem) => !baseline.has(problem))
}

const requirement = (copy, id) => copy.requirements.requirements.find((entry) => entry.id === id)
const part = (copy, id) =>
  copy.requirements.requirements.flatMap((entry) => entry.parts).find((entry) => entry.id === id)

function assertProblem(problems, pattern) {
  assert.ok(
    problems.some((problem) => pattern.test(problem)),
    `expected a problem matching ${pattern}, got:\n${problems.join('\n')}`,
  )
}

describe('traceability check', () => {
  test('the repository has no problems', () => {
    assert.deepEqual(checkTraceability(structuredClone(pristine)).problems, [])
  })

  test('the status rule derives each status from the parts', () => {
    const cases = [
      [['intentionally_rejected', 'intentionally_rejected'], 'intentionally_rejected'],
      [['implemented', 'intentionally_rejected'], 'implemented'],
      [['implemented'], 'implemented'],
      [['implemented', 'not_implemented'], 'partial'],
      [['partial', 'unverified'], 'partial'],
      [['unverified', 'not_implemented'], 'unverified'],
      [['not_implemented', 'intentionally_rejected'], 'not_implemented'],
    ]
    for (const [statuses, expected] of cases) assert.equal(deriveStatus(statuses), expected, statuses.join(','))
  })

  test('a stored status that differs from the derived one is refused', () => {
    const problems = problemsAfter((copy) => {
      requirement(copy, 'OPS-02').status = 'implemented'
    })
    assertProblem(problems, /OPS-02: stored status implemented differs .* partial/)
  })

  test('marking the catch-all part implemented changes the derived status', () => {
    const problems = problemsAfter((copy) => {
      const catchAll = part(copy, 'MODEL-01.uncovered')
      catchAll.status = 'implemented'
      catchAll.followups = []
    })
    assertProblem(problems, /MODEL-01: stored status partial differs .* implemented/)
  })

  test('every requirement keeps exactly one catch-all part', () => {
    const problems = problemsAfter((copy) => {
      delete part(copy, 'CHEM-01.uncovered').catch_all
    })
    assertProblem(problems, /CHEM-01: needs exactly one catch_all part/)
  })

  test('implementation paths must exist', () => {
    const problems = problemsAfter((copy) => {
      part(copy, 'OPS-02.history').implementation.push('crates/domain/src/missing.rs')
    })
    assertProblem(problems, /implementation path crates\/domain\/src\/missing.rs does not exist/)
  })

  test('evidence must resolve', () => {
    const problems = problemsAfter((copy) => {
      part(copy, 'OPS-02.history').evidence.push(
        'case:no-such-case',
        'test:crates/domain/tests/document.rs#no_such_test',
        'test:crates/domain/tests/missing.rs',
        'report:docs/verification/missing.json',
        'smoke:scripts/lib/request-interception.mjs',
        'proof:anything',
      )
    })
    assertProblem(problems, /evidence case:no-such-case names no case/)
    assertProblem(problems, /evidence test:.*#no_such_test names no test no_such_test/)
    assertProblem(problems, /evidence test:crates\/domain\/tests\/missing.rs names a missing file/)
    assertProblem(problems, /evidence report:docs\/verification\/missing.json names a missing file/)
    assertProblem(problems, /evidence smoke:scripts\/lib\/request-interception.mjs is not run by any npm script/)
    assertProblem(problems, /evidence proof:anything has unknown kind proof/)
  })

  test('check evidence must be a script npm run check runs', () => {
    assert.ok(scriptsRunByCheck(pristine.packageJson.scripts).has('test:clean-room'))
    assert.ok(!scriptsRunByCheck(pristine.packageJson.scripts).has('test:browser'))
    const problems = problemsAfter((copy) => {
      part(copy, 'OPS-02.history').evidence.push('check:test:browser')
    })
    assertProblem(problems, /evidence check:test:browser is not a script that npm run check runs/)
    const dropped = problemsAfter((copy) => {
      copy.packageJson.scripts.check = copy.packageJson.scripts.check.replace(' && npm run test:clean-room', '')
    })
    assertProblem(dropped, /evidence check:test:clean-room is not a script that npm run check runs/)
  })

  test('case evidence for implemented or partial parts must have passed', () => {
    const pending = problemsAfter((copy) => {
      part(copy, 'OPS-02.history').evidence.push('case:chem-formula-mass')
    })
    assertProblem(pending, /evidence case:chem-formula-mass must be a case that passed/)
    const failed = problemsAfter((copy) => {
      copy.report.results.find((result) => result.id === 'ops-undo-redo').outcome = 'failed'
    })
    assertProblem(failed, /evidence case:ops-undo-redo must be a case that passed/)
  })

  test('follow-ups are required, forbidden, or must be open as the status says', () => {
    const missing = problemsAfter((copy) => {
      part(copy, 'CHEM-01.uncovered').followups = []
    })
    assertProblem(missing, /CHEM-01.uncovered: a not_implemented part needs follow-ups/)
    const partial = problemsAfter((copy) => {
      part(copy, 'CLEANROOM-01.public-repository').followups = []
    })
    assertProblem(partial, /a partial part needs follow-ups/)
    const extra = problemsAfter((copy) => {
      part(copy, 'OPS-02.history').followups = ['P11']
    })
    assertProblem(extra, /OPS-02.history: an implemented part has no follow-ups/)
    const unknown = problemsAfter((copy) => {
      part(copy, 'CHEM-01.uncovered').followups.push('P999')
    })
    assertProblem(unknown, /follow-up P999 is not in the roadmap/)
    const done = problemsAfter((copy) => {
      part(copy, 'CHEM-01.uncovered').followups.push('P01')
    })
    assertProblem(done, /follow-up P01 is already done/)
  })

  test('done roadmap rows must record what was delivered', () => {
    const problems = problemsAfter((copy) => {
      delete copy.roadmap.rows.find((row) => row.id === 'P03').delivered.main_ci
    })
    assertProblem(problems, /roadmap P03 is done but lacks delivered/)
  })

  test('the recorded report must match the current cases', () => {
    const reordered = problemsAfter((copy) => {
      copy.report.results.reverse()
    })
    assertProblem(reordered, /results must list the suite's cases in order/)
    const reason = problemsAfter((copy) => {
      copy.cases.find((testCase) => testCase.id === 'chem-formula-mass').pending.reason = 'changed'
    })
    assertProblem(reason, /chem-formula-mass records a different pending follow-up or reason/)
    const kind = problemsAfter((copy) => {
      copy.cases.find((testCase) => testCase.id === 'ops-undo-redo').kind = 'reject'
    })
    assertProblem(kind, /ops-undo-redo records a different kind/)
    const requirements = problemsAfter((copy) => {
      copy.cases.find((testCase) => testCase.id === 'ops-undo-redo').requirements.push('OPS-01')
    })
    assertProblem(requirements, /ops-undo-redo records a different requirements/)
    const axis = problemsAfter((copy) => {
      copy.cases.find((testCase) => testCase.id === 'ops-undo-redo').axis = 'graph'
    })
    assertProblem(axis, /ops-undo-redo records a different axis/)
    const added = problemsAfter((copy) => {
      copy.cases.push({ ...copy.cases[0], id: 'a-new-case' })
    })
    assertProblem(added, /results must list the suite's cases in order/)
  })

  test('the recorded counts must equal a recount of the results', () => {
    const problems = problemsAfter((copy) => {
      copy.report.counts.accept.passed += 1
    })
    assertProblem(problems, /counts .* differ from a recount/)
    const moved = problemsAfter((copy) => {
      copy.report.counts.accept.passed -= 1
      copy.report.counts.reject.passed += 1
    })
    assertProblem(moved, /counts .* differ from a recount/)
  })

  test('a pending case is exactly a not-run result', () => {
    const problems = problemsAfter((copy) => {
      const result = copy.report.results.find((entry) => entry.id === 'chem-formula-mass')
      result.outcome = 'passed'
      copy.report.counts.accept.not_run -= 1
      copy.report.counts.accept.passed += 1
      copy.report.counts.total.not_run -= 1
      copy.report.counts.total.passed += 1
    })
    assertProblem(problems, /chem-formula-mass must be not_run exactly when it is pending/)
  })

  test('the recorded report must equal a fresh run when one exists', () => {
    const problems = problemsAfter((copy) => {
      copy.freshReport = structuredClone(copy.report)
      copy.freshReport.suite_version = '0.0.0'
    })
    assertProblem(problems, /differs from the fresh run/)
  })

  test('case requirement IDs must be traced and pending follow-ups open', () => {
    const problems = problemsAfter((copy) => {
      const testCase = copy.cases.find((entry) => entry.id === 'chem-formula-mass')
      testCase.requirements = ['CHEM-99']
      testCase.pending.followup = 'P02'
    })
    assertProblem(problems, /case chem-formula-mass: requirement CHEM-99 is not traced/)
    assertProblem(problems, /case chem-formula-mass: follow-up P02 is already done/)
  })

  test('declaration counts must add up and stay unimplemented without a plan entry', () => {
    const problems = problemsAfter((copy) => {
      copy.declarations.routes[0].declarations += 1
      copy.declarations.routes[1].implemented = 3
    })
    assertProblem(problems, /route counts do not add up/)
    assertProblem(problems, /route document: by_source does not add up/)
    assertProblem(problems, /route client_integration: status not_implemented does not match 3 implemented/)
    assertProblem(problems, /implemented does not match its routes/)
  })

  test('the docs must quote the derived totals exactly, and the check prints what to paste', () => {
    const { copy, totals } = inputs()
    const expected = totalsSentence(totals)
    assert.match(expected, /^현재 집계: 요구사항 33개 — implemented 0, partial 7,/)
    copy.docs['docs/TRACEABILITY.ko.md'] = `intro\n${expected}\n`
    copy.docs['docs/HANDOFF.ko.md'] = `intro\n${expected.replace('요구사항 33개', '요구사항 32개')}\n`
    const wrong = checkTraceability(copy).problems.filter((problem) => problem.includes('totals line'))
    assert.equal(wrong.length, 1, wrong.join('\n'))
    assert.ok(wrong[0].startsWith('docs/HANDOFF.ko.md: '), wrong[0])
    assert.ok(wrong[0].endsWith(`Paste:\n${expected}`), wrong[0])
    copy.docs['docs/HANDOFF.ko.md'] = `${expected}\n${expected}\n`
    const twice = checkTraceability(copy).problems.filter((problem) => problem.includes('totals line'))
    assert.equal(twice.length, 1)
    assert.match(twice[0], /found 2 lines/)
    copy.docs['docs/HANDOFF.ko.md'] = null
    assertProblem(checkTraceability(copy).problems, /docs\/HANDOFF.ko.md is missing/)
  })

  test('the repository root resolves from a module path with spaces', () => {
    const url = pathToFileURL('/tmp/a checkout with spaces/scripts/traceability-check.mjs').href
    assert.match(url, /%20/)
    assert.equal(rootFromModuleUrl(url), '/tmp/a checkout with spaces')
  })
})

describe('traceability check against a specification workspace', () => {
  const sha256 = (text) => createHash('sha256').update(text).digest('hex')

  /** A spec/ tree consistent with the repository's records, and inputs whose hashes match it. */
  function fixture(change = () => {}) {
    const { copy } = inputs()
    const spec = {
      requirements: {
        requirements: copy.requirements.requirements.map((entry) => ({ requirement_id: entry.id, domain: entry.domain })),
      },
      operations: { operations: [...new Set(copy.requirements.requirements.flatMap((entry) => entry.operations))] },
      declarations: { requirements: [] },
    }
    const sourceName = Object.fromEntries(Object.entries(copy.declarations.source_labels))
    const evidenceName = Object.fromEntries(Object.entries(copy.declarations.evidence_status_labels))
    for (const route of copy.declarations.routes) {
      const sources = Object.entries(route.by_source).flatMap(([label, count]) => Array(count).fill(sourceName[label]))
      const evidence = Object.entries(route.by_evidence_status).flatMap(([label, count]) =>
        Array(count).fill(evidenceName[label]),
      )
      sources.forEach((source, index) =>
        spec.declarations.requirements.push({
          proposed_route: route.route,
          source_contract: source,
          evidence_status: evidence[index],
        }),
      )
    }
    change(spec)
    const directory = mkdtempSync(path.join(temporary, 'spec '))
    const files = {
      [copy.requirements.sources[0].path]: spec.requirements,
      [copy.requirements.sources[1].path]: spec.operations,
      [copy.declarations.source.path]: spec.declarations,
    }
    for (const [relative, content] of Object.entries(files)) {
      const text = JSON.stringify(content)
      mkdirSync(path.dirname(path.join(directory, relative)), { recursive: true })
      writeFileSync(path.join(directory, relative), text)
      for (const source of [...copy.requirements.sources, copy.declarations.source]) {
        if (source.path === relative) source.sha256 = sha256(text)
      }
    }
    return { copy, directory }
  }

  test('a consistent specification has no problems', () => {
    const { copy, directory } = fixture()
    assert.deepEqual(checkAgainstSpec(copy, directory), [])
  })

  test('a changed source file is refused by its hash', () => {
    const { copy, directory } = fixture()
    copy.declarations.source.sha256 = '0'.repeat(64)
    assertProblem(checkAgainstSpec(copy, directory), /declaration-requirements.json: SHA-256 differs/)
  })

  test('requirement IDs must equal the specification\'s', () => {
    const { copy, directory } = fixture((spec) => {
      spec.requirements.requirements.pop()
    })
    assertProblem(checkAgainstSpec(copy, directory), /requirement IDs differ from the specification's 32/)
  })

  test('operations must be named by the specification', () => {
    const { copy, directory } = fixture((spec) => {
      spec.operations.operations = spec.operations.operations.filter((operation) => operation !== 'chem.properties')
    })
    assertProblem(checkAgainstSpec(copy, directory), /CHEM-01: operation chem.properties is not in the specification/)
  })

  test('per-route declaration counts must equal a recount of the specification', () => {
    const { copy, directory } = fixture((spec) => {
      spec.declarations.requirements.find((item) => item.proposed_route === 'codec_chem').proposed_route = 'chem_graph'
    })
    const problems = checkAgainstSpec(copy, directory)
    assertProblem(problems, /route codec_chem declarations 6 differs from the specification's 5/)
    assertProblem(problems, /route chem_graph declarations 174 differs from the specification's 175/)
  })
})

describe('clean-room check', () => {
  const testName = { label: 'test name', sha256: createHash('sha256').update('zorblax').digest('hex'), length: 7 }
  const names = [
    { ...testName, match: 'substring' },
    { label: 'short name', sha256: createHash('sha256').update('qvx').digest('hex'), length: 3, match: 'word' },
  ]

  /** A Git repository whose index holds `files` ({path: content | {link}}). */
  function repository(files, extra = () => {}) {
    const directory = mkdtempSync(path.join(temporary, 'repo '))
    const git = (...args) => execFileSync('git', args, { cwd: directory, stdio: 'pipe' })
    git('init', '-q')
    for (const [relative, content] of Object.entries(files)) {
      const full = path.join(directory, relative)
      mkdirSync(path.dirname(full), { recursive: true })
      if (content && typeof content === 'object' && 'link' in content) symlinkSync(content.link, full)
      else writeFileSync(full, content)
    }
    git('add', '--force', '--', ...Object.keys(files))
    extra(git, directory)
    return directory
  }

  const check = (directory, specDir = null) => checkCleanRoom({ root: directory, specDir, names })

  test('ordinary files pass', () => {
    const directory = repository({
      'README.md': 'A drawing editor.\n',
      'src/main.rs': 'fn main() {}\n',
      '.env.example': 'PORT=3001\n',
      'docs/data/readme.txt': 'public data\n',
      'package-lock.json': Buffer.alloc(1024 * 1024 + 1, 0x20),
      'link.md': { link: 'README.md' },
    })
    assert.deepEqual(check(directory), [])
  })

  test('local-only paths are refused as files, directories, and symlink targets', () => {
    const directory = repository({
      'private/notes.md': 'x',
      'DATA/model.json': '{}',
      'tools/localdb/x.txt': 'x',
      'investigations/a.txt': 'x',
      'docs/investigation/a.txt': 'x',
      'analysis/a.txt': 'x',
      'evidence/a.txt': 'x',
      '.agents/skill.md': 'x',
      '.claude/settings.json': '{}',
      '.archify/x': 'x',
      'skills-lock.json': '{}',
      '.env': 'TOKEN=1',
      '.env.local': 'TOKEN=1',
      'data/private/a.txt': 'x',
      'cache.sqlite': 'x',
      'store.db': 'x',
      'server.pem': 'x',
      'server.key': 'x',
      'out/decompiled/main.c': 'x',
      'notes/innocent.md': { link: '../private/notes.md' },
      'escape.md': { link: '../../outside.md' },
      'absolute.md': { link: '/etc/hostname' },
    })
    const problems = check(directory)
    for (const pattern of [
      /^private\/notes.md: local-only path segment private/m,
      /^DATA\/model.json: local-only DATA directory/m,
      /^tools\/localdb\/x.txt: local-only path segment localdb/m,
      /^investigations\/a.txt: /m,
      /^docs\/investigation\/a.txt: /m,
      /^analysis\/a.txt: /m,
      /^evidence\/a.txt: /m,
      /^\.agents\/skill.md: /m,
      /^\.claude\/settings.json: /m,
      /^\.archify\/x: /m,
      /^skills-lock.json: /m,
      /^\.env: environment file/m,
      /^\.env.local: environment file/m,
      /^data\/private\/a.txt: local-only data\/private/m,
      /^cache.sqlite: local-only file type/m,
      /^store.db: /m,
      /^server.pem: /m,
      /^server.key: /m,
      /^out\/decompiled\/main.c: decompilation output/m,
      /^notes\/innocent.md: symlink target: local-only path segment private/m,
      /^escape.md: symlink points outside the repository/m,
      /^absolute.md: symlink points outside the repository/m,
    ]) {
      assertProblem(problems, pattern)
    }
  })

  test('a nested repository is refused', () => {
    const directory = repository({ 'README.md': 'x' }, (git) => {
      git('update-index', '--add', '--cacheinfo', `160000,${'1'.repeat(40)},vendor/nested`)
    })
    assertProblem(check(directory), /^vendor\/nested: a nested repository/)
  })

  test('executables are refused by extension and by magic bytes', () => {
    const pe = Buffer.alloc(0x80)
    pe.write('MZ', 0, 'latin1')
    pe.writeUInt32LE(0x40, 0x3c)
    pe.write('PE\0\0', 0x40, 'latin1')
    const directory = repository({
      'bin/tool.exe': 'x',
      'lib/native.dll': 'x',
      'lib/libx.so.1.2': 'x',
      'lib/libx.dylib': 'x',
      'classes/A.class': 'x',
      'innocent.txt': Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]),
      'image.dat': pe,
      'fat.bin': Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2]),
      'macho.bin': Buffer.from([0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]),
      'notes.txt': 'MZ is how this line starts, and it is plain text.\n'.repeat(4),
    })
    const problems = check(directory)
    for (const pattern of [
      /^bin\/tool.exe: executable/m,
      /^lib\/native.dll: executable/m,
      /^lib\/libx.so.1.2: executable/m,
      /^lib\/libx.dylib: executable/m,
      /^classes\/A.class: executable/m,
      /^innocent.txt: ELF/m,
      /^image.dat: PE/m,
      /^fat.bin: Java class or universal Mach-O/m,
      /^macho.bin: Mach-O/m,
    ]) {
      assertProblem(problems, pattern)
    }
    assert.ok(!problems.some((problem) => problem.startsWith('notes.txt')), problems.join('\n'))
    assert.equal(magicProblem(Buffer.from('MZ plain')), null)
  })

  test('tracked files over 1 MiB are refused, except package-lock.json', () => {
    const directory = repository({ 'big.json': Buffer.alloc(1024 * 1024 + 1, 0x20), 'ok.json': Buffer.alloc(1024 * 1024, 0x20) })
    const problems = check(directory)
    assertProblem(problems, /^big.json: 1048577 bytes exceeds/m)
    assert.ok(!problems.some((problem) => problem.startsWith('ok.json')))
  })

  test('forbidden names are refused in text and paths, by hash and word', () => {
    const directory = repository({
      'a.md': 'Compatible with Zorblax files.\n',
      'b.md': 'IZorblaxCtl\n',
      'docs/zorblax-notes/c.md': 'x',
      'd.md': 'the QVX workspace\n',
      'e.md': 'qvxa and aqvx are other words\n',
    })
    const problems = check(directory)
    assertProblem(problems, /contains the test name/)
    const embedded = repository({ 'b.md': 'IZorblaxCtl\n' })
    assertProblem(check(embedded), /^b.md: contains the test name/m)
    assertProblem(problems, /contains the short name/)
    assert.deepEqual(forbiddenNamesIn('Zorblax', names), ['test name'])
    assert.deepEqual(forbiddenNamesIn('IZorblaxCtl', names), ['test name'], 'inside a longer identifier')
    assert.deepEqual(forbiddenNamesIn('zorblax16', names), ['test name'], 'followed by a version number')
    assert.deepEqual(forbiddenNamesIn('qvxa aqvx', names), [])
    assert.deepEqual(forbiddenNamesIn('a QVX b', names), ['short name'])
    assert.deepEqual(forbiddenNamesIn('zorbla x', names), [])
    const clean = repository({ 'a.md': 'qvxa aqvx zorbla\n' })
    assert.deepEqual(check(clean), [])
  })

  test('the check itself holds no forbidden name in plain text', () => {
    assert.deepEqual(checkCleanRoom({ root }).filter((problem) => problem.includes('contains the')), [])
  })

  test('with a specification workspace, byte-identical copies of its files are refused', () => {
    const spec = mkdtempSync(path.join(temporary, 'spec '))
    mkdirSync(path.join(spec, 'spec', 'nested'), { recursive: true })
    writeFileSync(path.join(spec, 'spec', 'nested', 'contract.md'), 'a specified behaviour\n')
    const directory = repository({ 'docs/copied.md': 'a specified behaviour\n', 'docs/own.md': 'a specified behaviour.\n' })
    const problems = check(directory, spec)
    assertProblem(problems, /^docs\/copied.md: byte-identical to a specification file/m)
    assert.ok(!problems.some((problem) => problem.startsWith('docs/own.md')))
    assert.deepEqual(check(directory), [], 'without the workspace, copies are not compared')
  })

  test('local path rules leave ordinary names alone', () => {
    for (const ok of ['apps/web/src/lib/data.ts', 'docs/verification/x.json', 'crates/domain/src/document.rs', '.env.example']) {
      assert.equal(localPathProblem(ok), null, ok)
    }
  })
})
