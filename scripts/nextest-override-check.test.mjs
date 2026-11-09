// Tests for the rules of nextest-override-check.mjs: how an override filter is read, and which exact
// test names the listing lacks. Fixtures are inline; the last test reads the real .config/nextest.toml.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

import { missingTests, parseOverrideFilters } from './nextest-override-check.mjs'

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

test('the real .config/nextest.toml parses to at least 30 exact test names', () => {
  const text = readFileSync(path.join(ROOT, '.config', 'nextest.toml'), 'utf8')
  const count = parseOverrideFilters(text).reduce((total, filter) => total + filter.tests.length, 0)
  assert.ok(count >= 30, `expected at least 30 exact test names, found ${count}`)
})
