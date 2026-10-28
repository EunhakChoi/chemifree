// Checks that the files Git tracks hold nothing that belongs outside this public repository:
// local-only paths, executables and native libraries, files over 1 MiB, the names of the
// program the specification describes, and (with CHEMIFREE_SPEC_DIR set to the specification
// workspace root) byte-identical copies of files under its spec/ directory.
//
// It catches copies and artifacts. It cannot recognise private data that was reworded,
// re-encoded, or split, so it is a guard, not proof that nothing private is here.
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
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

const strictUtf8 = new TextDecoder('utf-8', { fatal: true })

/**
 * The index entries Git tracks, from `git ls-files --stage -z`: mode, blob ID, and the name as
 * text (invalid UTF-8 bytes shown as U+FFFD, with `utf8: false`). Names are split at the first
 * tab only, as they may contain tabs.
 */
function trackedEntries(root) {
  const output = execFileSync('git', ['ls-files', '--stage', '-z'], { cwd: root, maxBuffer: 256 * 1024 * 1024 })
  const entries = []
  let start = 0
  while (start < output.length) {
    let end = output.indexOf(0, start)
    if (end === -1) end = output.length
    const line = output.subarray(start, end)
    start = end + 1
    if (line.length === 0) continue
    const tab = line.indexOf(0x09)
    const [mode, blob] = line.subarray(0, tab).toString('latin1').split(' ')
    const nameBytes = line.subarray(tab + 1)
    let file
    let utf8 = true
    try {
      file = strictUtf8.decode(nameBytes)
    } catch {
      file = nameBytes.toString('utf8')
      utf8 = false
    }
    entries.push({ mode, blob, file, utf8 })
  }
  return entries
}

/** The contents of `blobs` as Git stores them, read with `git cat-file --batch`. */
function blobContents(root, blobs) {
  const contents = new Map()
  const unique = [...new Set(blobs)]
  if (unique.length === 0) return contents
  const output = execFileSync('git', ['cat-file', '--batch'], {
    cwd: root,
    input: `${unique.join('\n')}\n`,
    maxBuffer: 1024 * 1024 * 1024,
  })
  let offset = 0
  for (const blob of unique) {
    const header = output.subarray(offset, output.indexOf(0x0a, offset)).toString('latin1')
    offset += header.length + 1
    const [id, type, size] = header.split(' ')
    if (id !== blob || type !== 'blob') throw new Error(`git cat-file returned ${header} for ${blob}`)
    contents.set(blob, output.subarray(offset, offset + Number(size)))
    offset += Number(size) + 1
  }
  return contents
}

/** SHA-256 of every file under `specDir`/spec, read only to hash it. */
function specHashes(specDir) {
  const hashes = new Map()
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name)
      if (entry.isDirectory()) visit(full)
      else if (entry.isFile()) {
        const bytes = readFileSync(full)
        // An empty file has no content to copy; comparing it would flag every empty file.
        if (bytes.length > 0) hashes.set(sha256(bytes), path.relative(specDir, full))
      }
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
  // Contents come from the index, not the working tree: what Git tracks is what is published.
  const entries = trackedEntries(root)
  const contents = blobContents(
    root,
    entries.filter((entry) => entry.mode !== '160000').map((entry) => entry.blob),
  )
  for (const { mode, blob, file, utf8 } of entries) {
    collect(file, file)
    if (!utf8) problem(file, 'the tracked name is not valid UTF-8')
    const pathProblem = localPathProblem(file)
    if (pathProblem) problem(file, pathProblem)
    if (mode === '160000') {
      problem(file, 'a nested repository (gitlink) is not a regular file')
      continue
    }
    const bytes = contents.get(blob)
    if (mode === '120000') {
      const target = bytes.toString('utf8')
      collect(file, target)
      const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(file), target))
      if (path.posix.isAbsolute(target) || resolved.startsWith('..')) {
        problem(file, `symlink points outside the repository (${target})`)
      }
      const targetProblem = localPathProblem(resolved)
      if (targetProblem) problem(file, `symlink target: ${targetProblem}`)
      continue
    }
    if (mode !== '100644' && mode !== '100755') {
      problem(file, `tracked entry has mode ${mode}, not a regular file`)
      continue
    }
    if (bytes.length > MAX_TRACKED_BYTES && !SIZE_EXEMPT.has(file)) {
      problem(file, `${bytes.length} bytes exceeds the ${MAX_TRACKED_BYTES}-byte limit`)
    }
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
