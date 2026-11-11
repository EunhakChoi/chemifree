// Keeps the roadmap page in step with the roadmap data it embeds. docs/roadmap/implementation-roadmap.html
// holds all of implementation-roadmap.json in one <script id="roadmap-data" type="application/json"> element,
// and a copy that is out of step shows an old roadmap without any error.
//
// What a pass establishes: the JSON file is canonical text (JSON.stringify(data, null, 1) and a final newline);
// its compact serialization, which the embed holds, has no '</', '<!--' or '<script' (case-insensitive) that
// would end the script element or change how it parses; every row has a string id, no row id repeats, and every
// id in tracks.order[].rows is a row id; the embed is exactly that compact serialization of the JSON file's data.
// It does not establish that the roadmap content is right, and it does not look for other script-data hazards.
//
// What is read: both files as UTF-8 text, and from the HTML only the element that opens with the embed tag.
// --write replaces that content with the compact serialization of the JSON data and keeps every other byte of
// the HTML. It never writes the JSON file, and it writes nothing while the check reports a problem, including
// a JSON problem that --write cannot fix. Refused, by throwing: embedRoadmap and readEmbedded when the HTML does
// not hold exactly one such element, and readEmbedded when the content is not JSON. checkRoadmapHtml reports
// those, and a JSON file that does not parse or lacks the expected shape, as problems instead of throwing.

import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { isDeepStrictEqual } from 'node:util'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const JSON_NAME = 'docs/roadmap/implementation-roadmap.json'
const HTML_NAME = 'docs/roadmap/implementation-roadmap.html'
const JSON_PATH = path.join(ROOT, JSON_NAME)
const HTML_PATH = path.join(ROOT, HTML_NAME)
const EMBED_OPEN = '<script id="roadmap-data" type="application/json">'
const EMBED_CLOSE = '</script>'
const MODES = ['--check', '--write']

/**
 * The html with the content of its roadmap-data element replaced by the compact serialization of `data`;
 * everything outside the element is kept as it is. Throws unless the html holds exactly one such element.
 */
export function embedRoadmap(html, data) {
  const serialized = JSON.stringify(data)
  if (serialized === undefined) throw new Error('the roadmap data has no JSON form')
  const { start, end } = embedSpan(html)
  return `${html.slice(0, start)}${serialized}${html.slice(end)}`
}

/**
 * The value parsed from the content of the roadmap-data element. Throws unless the html holds exactly one such
 * element and its content is JSON.
 */
export function readEmbedded(html) {
  const { start, end } = embedSpan(html)
  try {
    return JSON.parse(html.slice(start, end))
  } catch (error) {
    throw new Error(`roadmap HTML: the roadmap-data element does not hold valid JSON: ${error.message}`, {
      cause: error,
    })
  }
}

/**
 * The problems, as messages, with the roadmap JSON text and the html that embeds it; empty when they agree.
 * `json` is the text of the JSON file and `html` the text of the page. Bad input is reported, not thrown.
 */
export function checkRoadmapHtml({ json, html }) {
  let data
  try {
    data = parseRoadmap(json)
  } catch (error) {
    return [error.message]
  }
  const problems = []
  if (json !== `${JSON.stringify(data, null, 1)}\n`) {
    problems.push(`${JSON_NAME} is not canonical text; it must be JSON.stringify(data, null, 1) and a final newline`)
  }
  const serialized = JSON.stringify(data)
  if (serialized.includes('</')) {
    problems.push(`${JSON_NAME} serializes to text with "</", which would end the script element that embeds it`)
  }
  if (/<!--|<script/i.test(serialized)) {
    problems.push(`${JSON_NAME} serializes to text with "<!--" or "<script", which changes how the embedding script element parses`)
  }
  problems.push(...rowProblems(data), ...embedProblems(html, data))
  return problems
}

/** The start and end offsets of the content of the one roadmap-data element. Throws unless there is exactly one. */
function embedSpan(html) {
  const count = html.split(EMBED_OPEN).length - 1
  if (count === 0) throw new Error(`roadmap HTML has no element that opens with ${EMBED_OPEN}`)
  if (count > 1) throw new Error(`roadmap HTML has ${count} elements that open with ${EMBED_OPEN}; expected one`)
  const start = html.indexOf(EMBED_OPEN) + EMBED_OPEN.length
  const end = html.indexOf(EMBED_CLOSE, start)
  if (end === -1) throw new Error(`roadmap HTML: the element that opens with ${EMBED_OPEN} has no ${EMBED_CLOSE}`)
  return { start, end }
}

/** Problems with the embed: no single element, content that is not JSON, or data that differs from the JSON file. */
function embedProblems(html, data) {
  let embedded
  try {
    embedded = readEmbedded(html)
  } catch (error) {
    return [error.message]
  }
  if (isDeepStrictEqual(embedded, data)) {
    const { start, end } = embedSpan(html)
    if (html.slice(start, end) === JSON.stringify(data)) return []
    return [`${HTML_NAME} embed is not the compact serialization of ${JSON_NAME} that --write produces`]
  }
  const key = firstDifferingKey(embedded, data)
  const where = key === undefined ? '' : ` at top-level key ${JSON.stringify(key)}`
  return [`${HTML_NAME} embed is stale: its data differs from ${JSON_NAME}${where}`]
}

/** The first top-level key, in the JSON file's order, whose value differs; undefined when no key can be named. */
function firstDifferingKey(embedded, data) {
  if (!isObject(embedded) || !isObject(data)) return undefined
  const keys = new Set([...Object.keys(data), ...Object.keys(embedded)])
  return [...keys].find((key) => !isDeepStrictEqual(embedded[key], data[key]))
}

/**
 * Problems with the row ids: rows or tracks.order without the expected shape (reported alone), a repeated row id,
 * and a track row that names no row.
 */
function rowProblems(data) {
  const rows = isObject(data) ? data.rows : undefined
  const order = isObject(data) && isObject(data.tracks) ? data.tracks.order : undefined
  if (!Array.isArray(rows) || !Array.isArray(order) || !rows.every(isRow) || !order.every(isTrack)) {
    return [`${JSON_NAME} needs rows as objects with a string id, and tracks.order as objects with a rows array`]
  }
  const rowIds = new Set()
  const repeated = new Set()
  for (const { id } of rows) {
    if (rowIds.has(id)) repeated.add(id)
    rowIds.add(id)
  }
  const problems = [...repeated].map((id) => `${JSON_NAME} repeats row id ${JSON.stringify(id)}`)
  for (const track of order) {
    for (const id of track.rows) {
      if (!rowIds.has(id)) {
        const entry = JSON.stringify(track.id)
        problems.push(`${JSON_NAME} tracks.order entry ${entry} names ${JSON.stringify(id)}, which is not a row id`)
      }
    }
  }
  return problems
}

function isRow(row) {
  return isObject(row) && typeof row.id === 'string'
}

function isTrack(track) {
  return isObject(track) && Array.isArray(track.rows)
}

function isObject(value) {
  return typeof value === 'object' && value !== null
}

/** The parsed roadmap JSON text. Throws, naming the file, when the text is not JSON. */
function parseRoadmap(json) {
  try {
    return JSON.parse(json)
  } catch (error) {
    throw new Error(`${JSON_NAME} is not valid JSON: ${error.message}`, { cause: error })
  }
}

/** The file's text as UTF-8. Bytes that are not UTF-8 throw, and a byte order mark stays in the text. */
function readText(file) {
  return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(readFileSync(file))
}

/**
 * Writes the embed of the JSON data when the JSON file passes the check, and returns no problems. Otherwise it
 * returns the problems and writes nothing. The HTML file is written only when its text changes.
 */
function writeEmbed(json, html) {
  const next = embedRoadmap(html, parseRoadmap(json))
  const problems = checkRoadmapHtml({ json, html: next })
  if (problems.length > 0) return problems
  if (next !== html) writeFileSync(HTML_PATH, next)
  return []
}

/** Prints each problem and a count to stderr, and sets the exit code to 1. */
function reportProblems(problems, note = '') {
  for (const problem of problems) console.error(`- ${problem}`)
  console.error(`roadmap html: ${problems.length} problem(s)${note}`)
  process.exitCode = 1
}

function main(args) {
  try {
    const mode = args.length === 0 ? '--check' : args[0]
    if (args.length > 1 || !MODES.includes(mode)) {
      throw new Error(`usage: node scripts/roadmap-html.mjs [--check | --write] (got ${JSON.stringify(args)})`)
    }
    const json = readText(JSON_PATH)
    const html = readText(HTML_PATH)
    if (mode === '--write') {
      const problems = writeEmbed(json, html)
      if (problems.length > 0) reportProblems(problems, ', nothing written')
      return
    }
    const problems = checkRoadmapHtml({ json, html })
    if (problems.length > 0) {
      reportProblems(problems)
    } else {
      console.log(`roadmap html: embed matches ${JSON_NAME}`)
    }
  } catch (error) {
    console.error(`roadmap html: ${error.message}`)
    process.exitCode = 1
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  main(process.argv.slice(2))
}
