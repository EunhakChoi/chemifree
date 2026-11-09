// Checks that each exact test name in an override filter of .config/nextest.toml is a test that
// `cargo nextest list` lists under the override's package and binary. An override whose test was
// renamed or moved would otherwise stop applying without any failure.
//
// What a pass establishes: every test(=NAME) in an override table that names package(...) and
// binary(...) is a listed test of that package and binary. It does not establish that the override
// is still needed.
//
// What is read: override filters of package(NAME), binary(NAME) and test(=NAME) predicates joined by &,
// with | only inside parentheses; package and binary are plain names, optionally '='-prefixed. Refused,
// by throwing: any other predicate or matcher form (~substring, /regex/, globs), a top-level |,
// package() or binary() inside parentheses, unbalanced parentheses, and any test(=) mention the reader
// does not take, since every mention in the config is counted. Comments are ignored. The listing uses
// --workspace --locked with default features, as the nextest run in check:rust does.

import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const NEXTEST_CONFIG = path.join(ROOT, '.config', 'nextest.toml')
const LIST_ARGS = ['nextest', 'list', '--workspace', '--locked', '--message-format', 'json']
// Node's default spawn buffer is 1 MiB, which the listing of a large workspace can exceed.
const LIST_MAX_BUFFER = 256 * 1024 * 1024
const OVERRIDE_TABLE_HEADER = /^\s*\[\[\s*profile\.[^\]]*\.overrides\s*\]\]/
const TABLE_HEADER = /^\s*\[/
const FILTER_KEY = /^\s*filter\s*=\s*/
const NAMES_EXACT_TEST = /\btest\(\s*=/
const EXACT_TEST_MENTION = /\btest\(\s*=/g
const OVERRIDES_KEY = /overrides"?\s*\]/
// A package() or binary() argument is the text up to its closing parenthesis, which plainName
// checks. A test(=) name is a run of characters that are neither space nor parentheses.
const PACKAGE_PREDICATE = /\bpackage\(([^()]*)\)/g
const BINARY_PREDICATE = /\bbinary\(([^()]*)\)/g
const TEST_PREDICATE = /\btest\(\s*=\s*([^()\s]+)\s*\)/g
// A plain name, optionally with a leading '=': the only package() and binary() forms that are read.
const PLAIN_MATCHER = /^=?[A-Za-z0-9_-]+$/
// A TOML string closes with the delimiter that opened it. A basic string is read only without a
// backslash: its escapes would need decoding, which this reader does not do.
const TOML_STRINGS = [
  { delimiter: "'''", multiline: true, basic: false },
  { delimiter: '"""', multiline: true, basic: true },
  { delimiter: "'", multiline: false, basic: false },
  { delimiter: '"', multiline: false, basic: true },
]

/**
 * One entry per override table whose filter names an exact test: the table's line in the config,
 * its package and binary (undefined when the filter has none), and one test name per test(=)
 * mention, repeats included. package and binary must be plain names, optionally with a leading '=',
 * and are compared exactly. Tables whose filter has no test(=) are not entries. Throws on any
 * predicate besides package(), binary() and test(=), on any other matcher form, on a top-level |,
 * on a package() or binary() inside parentheses, on unbalanced parentheses, on a second package or
 * binary, and on a TOML string form this reader does not take.
 */
export function parseOverrideFilters(tomlText) {
  return readOverrideTables(tomlText.split(/\r?\n/))
    .filter((table) => table.text !== null && NAMES_EXACT_TEST.test(table.text))
    .map((table) => overrideEntry(table.line, table.text))
}

/**
 * The problems as messages: an entry with no package or no binary, and each exact test name that
 * no listed test has under that package and binary. `suites` is the parsed JSON of
 * `cargo nextest list --message-format json`; throws when it does not have the expected shape.
 */
export function missingTests(filters, suites) {
  const listed = listedTests(suites)
  const problems = []
  for (const filter of filters) {
    if (filter.package === undefined || filter.binary === undefined) {
      const lacking = []
      if (filter.package === undefined) lacking.push('package(...)')
      if (filter.binary === undefined) lacking.push('binary(...)')
      problems.push(`override at nextest.toml line ${filter.line} names test(=...) without ${lacking.join(' and ')}`)
      continue
    }
    for (const name of filter.tests) {
      if (!listed.has(testKey(filter.package, filter.binary, name))) {
        problems.push(`package ${filter.package} binary ${filter.binary}: test ${name} matches no listed test`)
      }
    }
  }
  return [...new Set(problems)]
}

/**
 * The number of test(= mentions in the TOML text, not counting comments: full lines, and the text
 * from a '#' that starts a trailing comment. Every mention counts wherever it stands, so a mention
 * that parseOverrideFilters does not read makes this number differ from the names it reads.
 */
export function countExactTestMentions(tomlText) {
  return tomlText
    .split(/\r?\n/)
    .reduce((total, line) => total + [...withoutComment(line).matchAll(EXACT_TEST_MENTION)].length, 0)
}

/**
 * The problems as messages when `filters`, the result of parseOverrideFilters(tomlText), do not
 * account for the test(=) mentions in the config, or when none were read. Repeats count on both sides.
 */
export function unreadMentions(tomlText, filters) {
  const mentioned = countExactTestMentions(tomlText)
  const read = filters.reduce((total, filter) => total + filter.tests.length, 0)
  if (read === 0) {
    return [`nextest.toml mentions ${mentioned} test(=...) filters but none were read; nothing was checked`]
  }
  if (read !== mentioned) {
    const message = `nextest.toml mentions ${mentioned} test(=...) filters but only ${read} were read`
    return [`${message} (an override form this check does not read?)`]
  }
  return []
}

/** Each override table of the config lines, with the text of its filter string or null. */
function readOverrideTables(lines) {
  const tables = []
  let table = null
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    if (OVERRIDE_TABLE_HEADER.test(line)) {
      table = { line: index + 1, text: null }
      tables.push(table)
    } else if (TABLE_HEADER.test(line)) {
      // Skipping an override table spelled another way would drop its tests without a trace.
      if (OVERRIDES_KEY.test(line)) {
        throw new Error(`nextest.toml line ${index + 1}: a table header names overrides in a form this check does not read`)
      }
      table = null
    } else if (table !== null && FILTER_KEY.test(line)) {
      if (table.text !== null) throw new Error(`nextest.toml line ${index + 1}: override table has two filters`)
      const value = readTomlString(lines, index, line.replace(FILTER_KEY, ''))
      table.text = value.text
      index = value.last
    } else if (table !== null && NAMES_EXACT_TEST.test(withoutComment(line))) {
      throw new Error(`nextest.toml line ${index + 1}: override table names test(=...) outside a filter key`)
    }
  }
  return tables
}

/**
 * Reads the TOML string that begins `rest`, the value of the key on lines[index]. Returns the
 * string's text and the index of the last line it spans. A multi-line string runs to its closing
 * delimiter on a later line, and only a comment may follow the delimiter.
 */
function readTomlString(lines, index, rest) {
  const form = TOML_STRINGS.find(({ delimiter }) => rest.startsWith(delimiter))
  if (form === undefined) throw new Error(`nextest.toml line ${index + 1}: filter is not a TOML string`)
  let body = rest.slice(form.delimiter.length)
  let last = index
  while (!body.includes(form.delimiter)) {
    if (!form.multiline || last + 1 >= lines.length) {
      throw new Error(`nextest.toml line ${index + 1}: filter string is not closed`)
    }
    last += 1
    body += `\n${lines[last]}`
  }
  const end = body.indexOf(form.delimiter)
  if (!/^\s*(#.*)?$/.test(body.slice(end + form.delimiter.length))) {
    throw new Error(`nextest.toml line ${index + 1}: text follows the filter string`)
  }
  const text = body.slice(0, end)
  if (form.basic && text.includes('\\')) {
    throw new Error(`nextest.toml line ${index + 1}: basic filter string has a backslash; use a literal string`)
  }
  return { text, last }
}

/** The entry for one override table whose filter names an exact test. */
function overrideEntry(line, text) {
  const unread = text.replace(PACKAGE_PREDICATE, ' ').replace(BINARY_PREDICATE, ' ').replace(TEST_PREDICATE, ' ')
  if (!/^[\s&|()]*$/.test(unread)) {
    throw new Error(`nextest.toml line ${line}: filter has text this check does not read: ${JSON.stringify(unread.trim())}`)
  }
  refuseUnmodelledStructure(line, text)
  const packages = matchAll(text, PACKAGE_PREDICATE).map((argument) => plainName(line, 'package', argument))
  const binaries = matchAll(text, BINARY_PREDICATE).map((argument) => plainName(line, 'binary', argument))
  if (packages.length > 1 || binaries.length > 1) {
    throw new Error(`nextest.toml line ${line}: filter names more than one package() or binary()`)
  }
  return { line, package: packages[0], binary: binaries[0], tests: matchAll(text, TEST_PREDICATE) }
}

/**
 * Refuses boolean structure this check does not model: a top-level |, a package() or binary()
 * inside parentheses, and unbalanced parentheses. Each predicate first becomes one letter, so the
 * parentheses of a predicate do not count toward the depth.
 */
function refuseUnmodelledStructure(line, text) {
  const shape = text.replace(PACKAGE_PREDICATE, 'P').replace(BINARY_PREDICATE, 'B').replace(TEST_PREDICATE, 'T')
  let depth = 0
  for (const char of shape) {
    if (char === '(') {
      depth += 1
    } else if (char === ')') {
      depth -= 1
    } else if (char === '|' && depth === 0) {
      throw new Error(`nextest.toml line ${line}: a top-level | in a filter that names test(=...) is not read by this check`)
    } else if ((char === 'P' || char === 'B') && depth > 0) {
      const predicate = char === 'P' ? 'package' : 'binary'
      throw new Error(`nextest.toml line ${line}: ${predicate}(...) inside parentheses is not read by this check`)
    }
    if (depth < 0) throw new Error(`nextest.toml line ${line}: filter has unbalanced parentheses`)
  }
  if (depth !== 0) throw new Error(`nextest.toml line ${line}: filter has unbalanced parentheses`)
}

/** The name that a package() or binary() argument matches exactly. Any other matcher form throws. */
function plainName(line, predicate, argument) {
  const name = argument.trim()
  if (!PLAIN_MATCHER.test(name)) {
    const message = `${predicate}(${name}) is a matcher form this check does not read (plain names only)`
    throw new Error(`nextest.toml line ${line}: ${message}`)
  }
  return name.replace(/^=/, '')
}

/** The line before the '#' that starts a trailing comment: a '#' outside a quoted string. */
function withoutComment(line) {
  let quote = null
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index]
    if (quote === null) {
      if (char === '#') return line.slice(0, index)
      if (char === '"' || char === "'") quote = char
    } else if (char === quote) {
      quote = null
    } else if (char === '\\' && quote === '"') {
      index += 1
    }
  }
  return line
}

function matchAll(text, pattern) {
  return [...text.matchAll(pattern)].map((match) => match[1])
}

/** The tests a parsed `cargo nextest list` output lists, as keys from testKey. */
function listedTests(suites) {
  const suitesByBinaryId = isPlainObject(suites) ? suites['rust-suites'] : undefined
  if (!isPlainObject(suitesByBinaryId)) throw new Error('cargo nextest list output has no rust-suites object')
  const listed = new Set()
  for (const [binaryId, suite] of Object.entries(suitesByBinaryId)) {
    const shaped =
      isPlainObject(suite) &&
      typeof suite['package-name'] === 'string' &&
      typeof suite['binary-name'] === 'string' &&
      isPlainObject(suite.testcases)
    if (!shaped) {
      throw new Error(`cargo nextest list output: suite ${JSON.stringify(binaryId)} lacks package-name, binary-name or testcases`)
    }
    for (const name of Object.keys(suite.testcases)) {
      listed.add(testKey(suite['package-name'], suite['binary-name'], name))
    }
  }
  return listed
}

function testKey(packageName, binaryName, testName) {
  return JSON.stringify([packageName, binaryName, testName])
}

function isPlainObject(value) {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Runs cargo nextest list for the workspace and returns its parsed JSON. Throws on failure. */
function listSuites() {
  const result = spawnSync('cargo', LIST_ARGS, {
    cwd: ROOT,
    encoding: 'utf8',
    maxBuffer: LIST_MAX_BUFFER,
    stdio: ['ignore', 'pipe', 'inherit'],
  })
  if (result.error) throw new Error(`cargo nextest list could not run: ${result.error.message}`, { cause: result.error })
  if (result.status !== 0) {
    const how = result.status === null ? `signal ${result.signal}` : `status ${result.status}`
    throw new Error(`cargo nextest list failed with ${how}; its output is above`)
  }
  try {
    return JSON.parse(result.stdout)
  } catch (error) {
    throw new Error('cargo nextest list printed no JSON on stdout', { cause: error })
  }
}

function main() {
  try {
    const tomlText = readFileSync(NEXTEST_CONFIG, 'utf8')
    const filters = parseOverrideFilters(tomlText)
    const problems = [...unreadMentions(tomlText, filters), ...missingTests(filters, listSuites())]
    if (problems.length > 0) {
      for (const problem of problems) console.error(`- ${problem}`)
      console.error(`nextest overrides: ${problems.length} problem(s)`)
      process.exitCode = 1
      return
    }
    const matched = new Set(
      filters.flatMap((filter) => filter.tests.map((name) => testKey(filter.package, filter.binary, name))),
    )
    console.log(`nextest overrides: ${matched.size} exact test names match listed tests`)
  } catch (error) {
    console.error(`nextest overrides check failed: ${error.message}`)
    process.exitCode = 1
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) main()
