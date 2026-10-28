#!/usr/bin/env node
// Checks what Git tracks for material that must stay out of this public repository
// (AGENTS.md, "Independent implementation"):
//
// - local-only directories: private/, agent and tool state (.agents/, .claude/, .archify/),
//   and anything under an investigation, evidence, or decompilation directory;
// - executables and native libraries, by extension and by file signature (ELF, PE, Mach-O,
//   Java class);
// - files over 1 MiB, except the npm lockfile, so a data payload cannot slip in unnoticed;
// - the names of the program whose public behavior the specification describes, which public
//   files do not use (listed by the SHA-256 of the word, so this file does not name them);
// - with CHEMIFREE_SPEC_DIR set to the local specification workspace, any file identical to a
//   file under its spec/ directory (the specification is referenced by path and hash, never
//   copied). Only spec/ is read, and only to hash it.
//
// It cannot prove that a file holds no private data that has been reworded or re-encoded; it
// catches the copies and artifacts above. Usage: clean-room-check.mjs [REPOSITORY]

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const root = process.argv[2] ?? new URL('..', import.meta.url).pathname
const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8' }).split('\0').filter(Boolean)

const LOCAL_ONLY = [
  /^private\//,
  /(^|\/)\.(agents|claude|archify)\//,
  /(^|\/)skills-lock\.json$/,
  /(^|\/)(investigation|evidence|decompil\w*)\//i,
]
const NATIVE_EXTENSION = /\.(exe|dll|ocx|sys|so|dylib|lib|a|pdb|msi|cab|class|jar)$/i
const MAX_BYTES = 1024 * 1024
const LARGE_ALLOWED = new Set(['package-lock.json'])
/** SHA-256 of a word, and whether letter case matters when comparing it. */
const VENDOR_NAMES = new Map([
  ['b036609ee2568aca81481ac3eafc86738f27f410f5e28b57692a435fbc0c3b2a', 'any case'],
  ['e518db6fc7064994771ae1c771bf40d0f3180636a69227dd73396dc06d707e9d', 'exact case'],
])

/** The kind of native binary `bytes` starts like, if any. */
function signature(bytes) {
  const head = bytes.subarray(0, 4).toString('hex')
  if (head === '7f454c46') return 'ELF'
  if (['feedface', 'feedfacf', 'cefaedfe', 'cffaedfe'].includes(head)) return 'Mach-O'
  if (head === 'cafebabe') return 'Java class or Mach-O universal'
  if (bytes.length >= 64 && bytes[0] === 0x4d && bytes[1] === 0x5a) {
    const offset = bytes.readUInt32LE(0x3c)
    if (offset + 4 <= bytes.length && bytes.subarray(offset, offset + 4).toString('latin1') === 'PE\0\0') return 'PE'
  }
  return null
}

const isText = (bytes) => !bytes.subarray(0, 8000).includes(0)
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex')

/** Whether `text` has a word whose hash is listed in VENDOR_NAMES. */
function namesVendor(text) {
  for (const word of new Set(text.match(/[A-Za-z]+/g) ?? [])) {
    if (VENDOR_NAMES.get(sha256(word)) === 'exact case') return true
    if (VENDOR_NAMES.get(sha256(word.toLowerCase())) === 'any case') return true
  }
  return false
}

const problems = []
const hashes = new Map()
for (const path of tracked) {
  if (LOCAL_ONLY.some((pattern) => pattern.test(path))) problems.push(`${path}: a local-only path`)
  if (NATIVE_EXTENSION.test(path)) problems.push(`${path}: an executable or native library by its extension`)
  let bytes
  try {
    bytes = readFileSync(join(root, path))
  } catch (error) {
    // A file deleted from the working tree but not yet from the index is not checked here.
    if (error.code === 'ENOENT') continue
    throw error
  }
  const kind = signature(bytes)
  if (kind) problems.push(`${path}: a native binary (${kind})`)
  if (bytes.length > MAX_BYTES && !LARGE_ALLOWED.has(path)) problems.push(`${path}: ${bytes.length} bytes, over 1 MiB`)
  if (isText(bytes) && namesVendor(bytes.toString('utf8'))) problems.push(`${path}: names a listed program`)
  hashes.set(sha256(bytes), path)
}

const spec = process.env.CHEMIFREE_SPEC_DIR
let specFiles = 0
if (spec) {
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name)
      if (entry.isDirectory()) walk(path)
      else if (entry.isFile()) {
        specFiles += 1
        const copy = hashes.get(sha256(readFileSync(path)))
        if (copy) problems.push(`${copy}: identical to the specification file ${path.slice(spec.length + 1)}`)
      }
    }
  }
  walk(join(spec, 'spec'))
}

if (problems.length > 0) {
  console.error(`Clean-room check failed:\n${problems.map((problem) => `  ${problem}`).join('\n')}`)
  process.exit(1)
}
console.log(
  `Clean-room check passed: ${tracked.length} tracked files` +
    (spec ? `, none identical to the ${specFiles} specification files.` : '.'),
)
