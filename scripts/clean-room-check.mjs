// Checks that the files Git tracks hold nothing that belongs outside this public repository:
// local-only paths, executables and native libraries, files over 1 MiB, the names of the
// program the specification describes, and (with CHEMIFREE_SPEC_DIR set to the specification
// workspace root) byte-identical copies of files under its spec/ directory.
//
// It catches copies and artifacts. It cannot recognise private data that was reworded,
// re-encoded, or split, so it is a guard, not proof that nothing private is here.
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readdirSync, readFileSync, readlinkSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const MAX_TRACKED_BYTES = 1024 * 1024
const SIZE_EXEMPT = new Set(['package-lock.json'])

/**
 * Names that must never appear, stored only as SHA-256 hashes of the lowercase word so this
 * file does not contain them either. `word` matches a whole run of letters; `substring` also
 * matches inside a longer run (e.g. a product name followed by a version number or embedded
 * in an identifier).
 */
export const FORBIDDEN_NAMES = [
  {
    label: 'specified program name',
    sha256: 'b036609ee2568aca81481ac3eafc86738f27f410f5e28b57692a435fbc0c3b2a',
    length: 8,
    match: 'substring',
  },
  {
    label: 'analysis workspace abbreviation',
    sha256: 'c4599aab5cbf87f8189071b86ce3c16a7dc83fd6a365466c4374939ef6b04812',
    length: 3,
    match: 'word',
  },
]

/** Path segments (compared ignoring case) that only exist in local, private workspaces. */
const LOCAL_SEGMENTS = new Set([
  'private',
  'localdb',
  'investigations',
  'investigation',
  'analysis',
  'evidence',
  '.agents',
  '.claude',
  '.archify',
  'skills-lock.json',
  'ghidra',
])
const LOCAL_EXTENSIONS = new Set(['.db', '.sqlite', '.sqlite3', '.pem', '.key', '.idb', '.i64', '.gzf'])
const EXECUTABLE_EXTENSIONS = new Set([
  '.exe', '.dll', '.ocx', '.sys', '.so', '.dylib', '.o', '.obj', '.a', '.lib', '.class', '.jar', '.node', '.pdb',
])

/** Why a tracked path (or a symlink's target) is local-only, or null. */
export function localPathProblem(relative) {
  const segments = relative.split(/[\\/]/).filter((segment) => segment !== '' && segment !== '.')
  for (const [index, segment] of segments.entries()) {
    const lower = segment.toLowerCase()
    if (LOCAL_SEGMENTS.has(lower)) return `local-only path segment ${segment}`
    if (segment === 'DATA') return 'local-only DATA directory'
    if (lower === 'data' && segments[index + 1]?.toLowerCase() === 'private') return 'local-only data/private'
    if (lower.startsWith('decompil')) return `decompilation output ${segment}`
  }
  const base = segments.at(-1) ?? ''
  const lowerBase = base.toLowerCase()
  if ((lowerBase === '.env' || lowerBase.startsWith('.env.')) && lowerBase !== '.env.example') {
    return `environment file ${base}`
  }
  const extension = path.extname(lowerBase)
  if (LOCAL_EXTENSIONS.has(extension)) return `local-only file type ${extension}`
  if (EXECUTABLE_EXTENSIONS.has(extension) || /\.so(\.\d+)+$/.test(lowerBase)) {
    return `executable or native library extension ${extension || base}`
  }
  return null
}

/** Why the first bytes of a file mark it as an executable or native library, or null. */
export function magicProblem(bytes) {
  const starts = (...values) => values.every((value, index) => bytes[index] === value)
  if (starts(0x7f, 0x45, 0x4c, 0x46)) return 'ELF executable or library'
  if (starts(0xca, 0xfe, 0xba, 0xbe)) return 'Java class or universal Mach-O binary'
  for (const magic of [
    [0xfe, 0xed, 0xfa, 0xce],
    [0xfe, 0xed, 0xfa, 0xcf],
    [0xce, 0xfa, 0xed, 0xfe],
    [0xcf, 0xfa, 0xed, 0xfe],
  ]) {
    if (starts(...magic)) return 'Mach-O binary'
  }
  if (starts(0x4d, 0x5a) && bytes.length >= 0x40) {
    const offset = bytes.readUInt32LE(0x3c)
    if (offset + 4 <= bytes.length && bytes.toString('latin1', offset, offset + 4) === 'PE\0\0') {
      return 'PE (Windows) executable or library'
    }
  }
  return null
}

const sha256 = (data) => createHash('sha256').update(data).digest('hex')

/** Lowercase runs of ASCII letters in `text`. */
const words = (text) => text.toLowerCase().match(/[a-z]+/g) ?? []

/** The labels of forbidden names found in `text`. */
export function forbiddenNamesIn(text, names = FORBIDDEN_NAMES) {
  return forbiddenNamesInWords(new Set(words(text)), names)
}

function forbiddenNamesInWords(distinctWords, names) {
  const found = new Set()
  for (const word of distinctWords) {
    for (const name of names) {
      if (found.has(name.label) || word.length < name.length) continue
      if (name.match === 'word') {
        if (word.length === name.length && sha256(word) === name.sha256) found.add(name.label)
        continue
      }
      for (let start = 0; start + name.length <= word.length; start += 1) {
        if (sha256(word.slice(start, start + name.length)) === name.sha256) {
          found.add(name.label)
          break
        }
      }
    }
  }
  return [...found]
}

/** Tracked entries with their index mode, from `git ls-files --stage`. */
function trackedEntries(root) {
  const output = execFileSync('git', ['ls-files', '--stage', '-z'], { cwd: root, maxBuffer: 64 * 1024 * 1024 })
  return output
    .toString('utf8')
    .split('\0')
    .filter(Boolean)
    .map((line) => {
      const [meta, file] = line.split('\t')
      return { mode: meta.split(' ')[0], file }
    })
}

/** SHA-256 of every file under `specDir`/spec, read only to hash it. */
function specHashes(specDir) {
  const hashes = new Map()
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name)
      if (entry.isDirectory()) visit(full)
      else if (entry.isFile()) hashes.set(sha256(readFileSync(full)), path.relative(specDir, full))
    }
  }
  const spec = path.join(specDir, 'spec')
  if (!existsSync(spec)) throw new Error(`${spec} does not exist`)
  visit(spec)
  return hashes
}

/** Checks the tracked files under `root`; returns the problems found. */
export function checkCleanRoom({ root, specDir = null, names = FORBIDDEN_NAMES }) {
  const problems = []
  const problem = (file, message) => problems.push(`${file}: ${message}`)
  const copies = specDir ? specHashes(specDir) : null
  const distinctWords = new Set()
  const wordSources = new Map()
  const collect = (file, text) => {
    for (const word of words(text)) {
      distinctWords.add(word)
      if (!wordSources.has(word)) wordSources.set(word, file)
    }
  }
  for (const { mode, file } of trackedEntries(root)) {
    collect(file, file)
    const pathProblem = localPathProblem(file)
    if (pathProblem) problem(file, pathProblem)
    if (mode === '160000') {
      problem(file, 'a nested repository (gitlink) is not a regular file')
      continue
    }
    const full = path.join(root, file)
    let stats
    try {
      stats = lstatSync(full)
    } catch {
      continue // Deleted in the working tree; nothing to read.
    }
    if (stats.isSymbolicLink()) {
      const target = readlinkSync(full)
      collect(file, target)
      const resolved = path.normalize(path.join(path.dirname(file), target))
      if (path.isAbsolute(target) || resolved.startsWith('..')) {
        problem(file, `symlink points outside the repository (${target})`)
      }
      const targetProblem = localPathProblem(resolved)
      if (targetProblem) problem(file, `symlink target: ${targetProblem}`)
      continue
    }
    if (!stats.isFile()) {
      problem(file, 'tracked entry is not a regular file')
      continue
    }
    if (stats.size > MAX_TRACKED_BYTES && !SIZE_EXEMPT.has(file)) {
      problem(file, `${stats.size} bytes exceeds the ${MAX_TRACKED_BYTES}-byte limit`)
    }
    const bytes = readFileSync(full)
    const magic = magicProblem(bytes)
    if (magic) problem(file, magic)
    if (copies?.has(sha256(bytes))) problem(file, 'byte-identical to a specification file')
    collect(file, bytes.toString('latin1'))
  }
  for (const label of forbiddenNamesInWords(distinctWords, names)) {
    const sources = [...wordSources.entries()]
      .filter(([word]) => forbiddenNamesInWords(new Set([word]), names).includes(label))
      .map(([, file]) => file)
    problems.push(`${[...new Set(sources)].join(', ')}: contains the ${label}`)
  }
  return problems
}

function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
  const specDir = process.env.CHEMIFREE_SPEC_DIR ? path.resolve(process.env.CHEMIFREE_SPEC_DIR) : null
  const problems = checkCleanRoom({ root, specDir })
  for (const message of problems) console.error(`clean-room: ${message}`)
  if (problems.length > 0) {
    console.error(`clean-room: ${problems.length} problem(s)`)
    process.exit(1)
  }
  console.log(
    specDir
      ? 'clean-room: tracked files checked, including copies of specification files'
      : 'clean-room: tracked files checked; set CHEMIFREE_SPEC_DIR to also refuse copies of specification files',
  )
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
