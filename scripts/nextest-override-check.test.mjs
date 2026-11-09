// Tests for the rules of nextest-override-check.mjs: how an override filter is read, and which exact
// test names the listing lacks. Fixtures are inline; the last test reads the real .config/nextest.toml.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

import { countExactTestMentions, missingTests, parseOverrideFilters, unreadMentions } from './nextest-override-check.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const CLOSING_TEST = 'closing_the_connection_stops_a_running_conversion'
const LARGEST_TEST = 'the_largest_documents_are_searched_within_the_budget'
const LIMIT_TEST = 'a_disconnected_query_in_a_large_target_ends_at_a_documented_limit_in_bounded_time'

// The table at line 1 has a single-line filter; the table at line 5 has a multi-line ''' filter.
const SINGLE_AND_MULTI = `[[profile.default.overrides]]
filter = "package(chemifree-api) & binary(http_disconnect) & test(=closing_the_connection_stops_a_running_conversion)"
threads-required = 4

[[profile.default.overrides]]
filter = '''
package(chemifree-domain) & binary(substructure) & (
  test(=the_largest_documents_are_searched_within_the_budget)
  | test(=a_disconnected_query_in_a_large_target_ends_at_a_documented_limit_in_bounded_time)
)
'''
threads-required = 4
`

// One override table with a single-line filter, for the tests about a renamed or misplaced test.
const SINGLE_TABLE = `[[profile.default.overrides]]
filter = "package(chemifree-api) & binary(http_disconnect) & test(=closing_the_connection_stops_a_running_conversion)"
threads-required = 4
`

/**
 * One suite of `cargo nextest list --message-format json`: its package, its binary and the tests it
 * lists. The check reads only the package, binary and test names; the other fields only make the
 * fixture resemble nextest's listing.
 */
function listedSuite(packageName, binaryName, testNames) {
  const testcases = Object.fromEntries(
    testNames.map((name) => [name, { ignored: false, 'filter-match': { status: 'matches' } }]),
  )
  return {
    'binary-id': `${packageName}::${binaryName}`,
    'package-name': packageName,
    'binary-name': binaryName,
    kind: 'test',
    testcases,
  }
}

/** The parsed JSON of `cargo nextest list --message-format json` with the given suites. */
function nextestList(...suites) {
  return { 'rust-suites': Object.fromEntries(suites.map((item) => [item['binary-id'], item])) }
}

/** A config with one override table, whose single-line filter is the given string. */
function overrideConfig(filter) {
  return `[[profile.default.overrides]]
filter = "${filter}"
threads-required = 4
`
}

test('single-line and multi-line filters parse to their package, binary and test names', () => {
  assert.deepEqual(parseOverrideFilters(SINGLE_AND_MULTI), [
    { line: 1, package: 'chemifree-api', binary: 'http_disconnect', tests: [CLOSING_TEST] },
    { line: 5, package: 'chemifree-domain', binary: 'substructure', tests: [LARGEST_TEST, LIMIT_TEST] },
  ])
})

test('an override table whose filter names no test(=) is skipped', () => {
  const toml = `[[profile.default.overrides]]
filter = "package(chemifree-api) & binary(http_disconnect)"
threads-required = 4

[[profile.default.overrides]]
filter = "package(chemifree-api) & binary(http_disconnect) & test(=closing_the_connection_stops_a_running_conversion)"
`
  assert.deepEqual(parseOverrideFilters(toml), [
    { line: 5, package: 'chemifree-api', binary: 'http_disconnect', tests: [CLOSING_TEST] },
  ])
})

test('a test(=) with no binary(...) is reported as a problem, not skipped', () => {
  const toml = `[[profile.default.overrides]]
filter = "package(chemifree-api) & test(=closing_the_connection_stops_a_running_conversion)"
`
  const suites = nextestList(listedSuite('chemifree-api', 'http_disconnect', [CLOSING_TEST]))
  assert.deepEqual(missingTests(parseOverrideFilters(toml), suites), [
    'override at nextest.toml line 1 names test(=...) without binary(...)',
  ])
})

test('every named test listed under its package and binary gives no problem', () => {
  const filters = parseOverrideFilters(SINGLE_AND_MULTI)
  // Without this, an empty parse would pass the check below for the wrong reason.
  assert.equal(filters.length, 2, 'both override tables are parsed')
  const suites = nextestList(
    listedSuite('chemifree-api', 'http_disconnect', [CLOSING_TEST]),
    listedSuite('chemifree-domain', 'substructure', [LARGEST_TEST, LIMIT_TEST]),
  )
  assert.deepEqual(missingTests(filters, suites), [])
})

test('a renamed test is one problem naming its package, binary and test', () => {
  const suites = nextestList(
    listedSuite('chemifree-api', 'http_disconnect', ['closing_the_connection_ends_a_running_conversion']),
  )
  assert.deepEqual(missingTests(parseOverrideFilters(SINGLE_TABLE), suites), [
    `package chemifree-api binary http_disconnect: test ${CLOSING_TEST} matches no listed test`,
  ])
})

test('a test listed under another binary or another package is still a problem', () => {
  // A test's identity is its package, binary and name together, so the same name elsewhere does not count.
  const suites = nextestList(
    listedSuite('chemifree-api', 'stereo', [CLOSING_TEST]),
    listedSuite('chemifree-domain', 'http_disconnect', [CLOSING_TEST]),
  )
  assert.deepEqual(missingTests(parseOverrideFilters(SINGLE_TABLE), suites), [
    `package chemifree-api binary http_disconnect: test ${CLOSING_TEST} matches no listed test`,
  ])
})

test('test(=) mentions are counted outside full-line and trailing comments', () => {
  const toml = `# test(=commented_out_on_its_own_line)
[[profile.default.overrides]]
filter = "package(chemifree-domain) & binary(substructure) & (test(=x) | test(=y))"
threads-required = 4 # was test(=old)

[[profile.default.overrides]]
filter = "package(chemifree-api) & binary(http_disconnect) & test(=z)"
`
  assert.equal(countExactTestMentions(toml), 3)
})

test('an inline-array override is not read, and unreadMentions reports its mention', () => {
  const toml = `[profile.default]
overrides = [ { filter = "package(p) & binary(b) & test(=x)", threads-required = 4 } ]
`
  assert.deepEqual(parseOverrideFilters(toml), [])
  assert.deepEqual(unreadMentions(toml, parseOverrideFilters(toml)), [
    'nextest.toml mentions 1 test(=...) filters but none were read; nothing was checked',
  ])
})

test('an override table with a literal-quoted key is not read, and unreadMentions reports its mention', () => {
  const toml = `[[profile.default.'overrides']]
filter = "package(p) & binary(b) & test(=x)"
threads-required = 4
`
  assert.deepEqual(parseOverrideFilters(toml), [])
  assert.deepEqual(unreadMentions(toml, parseOverrideFilters(toml)), [
    'nextest.toml mentions 1 test(=...) filters but none were read; nothing was checked',
  ])
})

test('a fully readable config has no unread mentions', () => {
  assert.deepEqual(unreadMentions(SINGLE_AND_MULTI, parseOverrideFilters(SINGLE_AND_MULTI)), [])
})

test('an inline-array override beside a readable one is reported as unread', () => {
  const toml = `${SINGLE_TABLE}
[profile.default]
overrides = [ { filter = "package(p) & binary(b) & test(=x)", threads-required = 4 } ]
`
  assert.deepEqual(unreadMentions(toml, parseOverrideFilters(toml)), [
    'nextest.toml mentions 2 test(=...) filters but only 1 were read (an override form this check does not read?)',
  ])
})

test('a config with only comments reports that nothing was checked', () => {
  const toml = `# Exact test names in overrides go here.
# [[profile.default.overrides]]
`
  assert.deepEqual(unreadMentions(toml, parseOverrideFilters(toml)), [
    'nextest.toml mentions 0 test(=...) filters but none were read; nothing was checked',
  ])
})

// Matcher forms other than plain names are refused, and the message names the predicate.
const REFUSED_MATCHERS = [
  {
    name: 'a glob in package(...)',
    filter: 'package(chemifree-*) & binary(http_disconnect) & test(=x)',
    message: /package\(chemifree-\*\) is a matcher form this check does not read/,
  },
  {
    name: 'a substring matcher in package(...)',
    filter: 'package(~sub) & binary(http_disconnect) & test(=x)',
    message: /package\(~sub\) is a matcher form this check does not read/,
  },
  {
    name: 'a regex in binary(...)',
    filter: 'package(chemifree-api) & binary(/re/) & test(=x)',
    message: /binary\(\/re\/\) is a matcher form this check does not read/,
  },
]

for (const { name, filter, message } of REFUSED_MATCHERS) {
  test(`refuses ${name}`, () => {
    assert.throws(() => parseOverrideFilters(overrideConfig(filter)), message)
  })
}

test('a package name with a leading = is read as the plain name', () => {
  const toml = overrideConfig(`package(=chemifree-api) & binary(http_disconnect) & test(=${CLOSING_TEST})`)
  assert.deepEqual(parseOverrideFilters(toml), [
    { line: 1, package: 'chemifree-api', binary: 'http_disconnect', tests: [CLOSING_TEST] },
  ])
})

// Boolean structure this check does not model is refused, and the message names that structure.
const UNMODELLED_STRUCTURE = [
  {
    name: 'a top-level | beside test(=) names',
    filter: 'package(a) & binary(b) & test(=x) | test(=y)',
    message: /top-level \|/,
  },
  {
    name: 'package(...) inside parentheses',
    filter: 'binary(b) & (package(a) & test(=x))',
    message: /package\(\.\.\.\) inside parentheses/,
  },
  {
    name: 'an unclosed parenthesis',
    filter: 'package(a) & binary(b) & (test(=x)',
    message: /unbalanced parentheses/,
  },
  {
    name: 'an extra closing parenthesis',
    filter: 'package(a) & binary(b) & test(=x))',
    message: /unbalanced parentheses/,
  },
]

for (const { name, filter, message } of UNMODELLED_STRUCTURE) {
  test(`refuses ${name}`, () => {
    assert.throws(() => parseOverrideFilters(overrideConfig(filter)), message)
  })
}

test('a trailing comment that names test(=) on another key is ignored', () => {
  const toml = `[[profile.default.overrides]]
filter = "package(chemifree-api) & binary(http_disconnect) & test(=${CLOSING_TEST})"
threads-required = 4 # was test(=old)
`
  const filters = parseOverrideFilters(toml)
  assert.deepEqual(filters, [{ line: 1, package: 'chemifree-api', binary: 'http_disconnect', tests: [CLOSING_TEST] }])
  assert.equal(countExactTestMentions(toml), 1)
  assert.deepEqual(unreadMentions(toml, filters), [])
})

test('the real .config/nextest.toml reads every exact test name it mentions', () => {
  const text = readFileSync(path.join(ROOT, '.config', 'nextest.toml'), 'utf8')
  const mentioned = countExactTestMentions(text)
  // Without this, a config with no mentions would pass the equality below for the wrong reason.
  assert.ok(mentioned > 0, 'the real config mentions at least one test(=) name')
  const names = parseOverrideFilters(text).flatMap((filter) => filter.tests)
  assert.equal(names.length, mentioned, 'every test(=) mention in the real config is read')
})
