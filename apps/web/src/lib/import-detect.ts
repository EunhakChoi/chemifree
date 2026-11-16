// Chooses how the Import card routes a pasted text or a picked file. Detection
// never parses chemistry: the readers stay the authority for what a format means.

export type ImportFormat = 'smiles' | 'mol' | 'sdf' | 'cdxml' | 'cdx'

export type DetectionBasis =
  | 'extension'
  | 'cdx-signature'
  | 'cdxml-element'
  | 'sdf-delimiter'
  | 'mol-end'
  | 'mol-counts'
  | 'smiles-text'

export type AmbiguityReason = 'conflict' | 'xml-not-cdxml' | 'binary-not-cdx' | 'not-recognized'

export type ImportDetection =
  | { kind: 'detected'; format: ImportFormat; basis: DetectionBasis }
  | {
      kind: 'ambiguous'
      reason: AmbiguityReason
      extension: ImportFormat | null
      content: ImportFormat | null
    }

/** What every import flow exposes to the Import card, which calls reset() in the same handler that changes the source. */
export interface ImportFlowHandle {
  reset(): void
}

/** The largest file the Import card reads. No format accepts more. */
export const MAX_IMPORT_FILE_BYTES = 200 * 1024

/**
 * Text past this many UTF-16 code units is not scanned. Such text is at least
 * as many UTF-8 bytes, which is over every per-format cap.
 */
export const MAX_DETECT_TEXT_UNITS = 256 * 1024

type Sniff =
  | { format: ImportFormat; basis: DetectionBasis; strong: boolean }
  | { ambiguous: 'xml-not-cdxml' | 'not-recognized' }

type AmbiguousDetection = Extract<ImportDetection, { kind: 'ambiguous' }>

type DecodeResult = { ok: true; source: string } | { ok: false; message: string }

const EXTENSIONS = new Map<string, ImportFormat>([
  ['smi', 'smiles'],
  ['smiles', 'smiles'],
  ['mol', 'mol'],
  ['sdf', 'sdf'],
  ['sd', 'sdf'],
  ['cdxml', 'cdxml'],
  ['cdx', 'cdx'],
])

// The canonical file name extension each format's messages cite.
const CANONICAL_EXTENSION: Record<ImportFormat, string> = {
  smiles: '.smi',
  mol: '.mol',
  sdf: '.sdf',
  cdxml: '.cdxml',
  cdx: '.cdx',
}

const SMILES_TOKEN = /^[A-Za-z0-9@+\-[\]()=#$%\\/.:*~>]+$/
const COUNTS_LINE = /^\s*\d{1,3}\s*\d{1,3}.*V[23]000\s*$/
// Anchored on purpose: an unanchored strip of trailing blanks backtracks
// quadratically on a long run of spaces.
const SDF_DELIMITER = /^\$\$\$\$[\r \t]*$/
// The first four bytes of a CDX file ("VjCD"). The reader checks the full
// header and gives its own refusal for a truncated or other-version file.
const CDX_SIGNATURE = [0x56, 0x6a, 0x43, 0x44]
const NOT_UTF8 = 'The selected file is not valid UTF-8.'

/** Maps a file name's extension to a format; null leaves the decision to the content. */
export function formatFromExtension(name: string): ImportFormat | null {
  const dot = name.lastIndexOf('.')
  if (dot < 0) {
    return null
  }
  return EXTENSIONS.get(name.slice(dot + 1).toLowerCase()) ?? null
}

/** Detects pasted text. It never returns cdx, which is binary and only arrives as a file. Null for blank text. */
export function detectImportText(text: string): ImportDetection | null {
  if (text.trim() === '') {
    return null
  }
  return detectionFromSniff(sniffText(text))
}

/**
 * Detects a picked or dropped file from its name and bytes. A strong content
 * sniff that names another format is reported as a conflict for the user to resolve.
 */
export function detectImportFile(name: string, bytes: Uint8Array): ImportDetection {
  const extension = formatFromExtension(name)
  if (hasCdxSignature(bytes)) {
    return detectionWithExtension(extension, { format: 'cdx', basis: 'cdx-signature', strong: true })
  }
  const text = decodeUtf8(bytes, false)
  if (text === null) {
    // Not text, so a known extension is the only route; decodeImportFile reports the UTF-8 error.
    return extension === null
      ? { kind: 'ambiguous', reason: 'binary-not-cdx', extension: null, content: null }
      : { kind: 'detected', format: extension, basis: 'extension' }
  }
  return detectionWithExtension(extension, sniffText(text))
}

/** The bytes in the form the reader expects: canonical base64 for CDX, strict UTF-8 text for the rest. */
export function decodeImportFile(bytes: Uint8Array, format: ImportFormat): DecodeResult {
  switch (format) {
    case 'cdx':
      return { ok: true, source: toBase64(bytes) }
    case 'cdxml':
      return textResult(decodeUtf8(bytes, true))
    case 'mol':
    case 'sdf':
    case 'smiles':
      return textResult(decodeUtf8(bytes, false))
    default:
      return unreachable(format)
  }
}

/** Canonical padded base64, which is what the operation API requires for CDX. */
export function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000))
  }
  return btoa(binary)
}

export function formatLabel(format: ImportFormat): 'SMILES' | 'MOL' | 'SDF' | 'CDXML' | 'CDX' {
  switch (format) {
    case 'smiles':
      return 'SMILES'
    case 'mol':
      return 'MOL'
    case 'sdf':
      return 'SDF'
    case 'cdxml':
      return 'CDXML'
    case 'cdx':
      return 'CDX'
    default:
      return unreachable(format)
  }
}

/** The one-line explanation the card shows for a detection and the format the user chose, if any. */
export function detectionSentence(detection: ImportDetection, chosen: ImportFormat | null): string {
  switch (detection.kind) {
    case 'detected':
      if (chosen !== null && chosen !== detection.format) {
        return `Detected ${formatLabel(detection.format)}; you chose ${formatLabel(chosen)}.`
      }
      return detectedSentence(detection.format, detection.basis)
    case 'ambiguous': {
      const base = ambiguousSentence(detection)
      return chosen === null ? base : `${base} You chose ${formatLabel(chosen)}.`
    }
    default:
      return unreachable(detection)
  }
}

/** Sniffs pasted text or decoded file text. The first matching rule wins. Only a strong result can conflict with a file name. */
export function sniffText(text: string): Sniff {
  const unmarked = text.startsWith('\uFEFF') ? text.slice(1) : text
  const scanned = unmarked.slice(0, MAX_DETECT_TEXT_UNITS)
  if (scanned.trimStart().startsWith('<')) {
    return scanned.includes('<CDXML')
      ? { format: 'cdxml', basis: 'cdxml-element', strong: true }
      : { ambiguous: 'xml-not-cdxml' }
  }
  const lines = scanned.split('\n')
  // Before MOL: every SDF record also has an M  END line.
  if (lines.some((line) => SDF_DELIMITER.test(line))) {
    return { format: 'sdf', basis: 'sdf-delimiter', strong: true }
  }
  if (lines.some((line) => line.startsWith('M  END'))) {
    return { format: 'mol', basis: 'mol-end', strong: true }
  }
  if (lines.some((line) => COUNTS_LINE.test(line))) {
    return { format: 'mol', basis: 'mol-counts', strong: true }
  }
  const first = lines[0].replace(/\r$/, '')
  const token = first.split(/[ \t]/, 1)[0]
  if (SMILES_TOKEN.test(token)) {
    return { format: 'smiles', basis: 'smiles-text', strong: false }
  }
  return { ambiguous: 'not-recognized' }
}

function detectionFromSniff(sniff: Sniff): ImportDetection {
  if ('ambiguous' in sniff) {
    return { kind: 'ambiguous', reason: sniff.ambiguous, extension: null, content: null }
  }
  return { kind: 'detected', format: sniff.format, basis: sniff.basis }
}

function detectionWithExtension(extension: ImportFormat | null, sniff: Sniff): ImportDetection {
  if (extension === null) {
    return detectionFromSniff(sniff)
  }
  // A weak or ambiguous sniff never contradicts a name. A one-record SDF need
  // not end in $$$$, so a MOL sniff is compatible with an .sdf name.
  if (
    'ambiguous' in sniff ||
    !sniff.strong ||
    sniff.format === extension ||
    (extension === 'sdf' && sniff.format === 'mol')
  ) {
    return { kind: 'detected', format: extension, basis: 'extension' }
  }
  return { kind: 'ambiguous', reason: 'conflict', extension, content: sniff.format }
}

function hasCdxSignature(bytes: Uint8Array): boolean {
  return bytes.length >= CDX_SIGNATURE.length && CDX_SIGNATURE.every((byte, index) => bytes[index] === byte)
}

/** Strict UTF-8 decoding, or null when the bytes are not UTF-8. `keepBom` keeps a leading byte-order mark. */
function decodeUtf8(bytes: Uint8Array, keepBom: boolean): string | null {
  try {
    // TextDecoder's ignoreBOM option is true when the byte-order mark is kept in the output.
    return new TextDecoder('utf-8', { fatal: true, ignoreBOM: keepBom }).decode(bytes)
  } catch (error: unknown) {
    if (error instanceof TypeError) {
      return null
    }
    throw new Error('Decoding the selected file failed', { cause: error })
  }
}

function textResult(text: string | null): DecodeResult {
  return text === null ? { ok: false, message: NOT_UTF8 } : { ok: true, source: text }
}

function detectedSentence(format: ImportFormat, basis: DetectionBasis): string {
  switch (basis) {
    case 'extension':
      return `Detected ${formatLabel(format)} from the ${CANONICAL_EXTENSION[format]} file name.`
    case 'cdx-signature':
      return 'Detected CDX from its VjCD signature.'
    case 'cdxml-element':
      return 'Detected CDXML from its <CDXML> element.'
    case 'sdf-delimiter':
      return 'Detected SDF from its $$$$ record delimiter.'
    case 'mol-end':
      return 'Detected MOL from its M  END line.'
    case 'mol-counts':
      return 'Detected MOL from its V2000 or V3000 counts line.'
    case 'smiles-text':
      return 'Detected SMILES from its first line.'
    default:
      return unreachable(basis)
  }
}

function ambiguousSentence(detection: AmbiguousDetection): string {
  switch (detection.reason) {
    case 'conflict': {
      if (detection.extension === null || detection.content === null) {
        throw new Error('A format conflict needs both an extension format and a content format.')
      }
      const nameFormat = formatLabel(detection.extension)
      const contentFormat = formatLabel(detection.content)
      return `Could not tell: the ${CANONICAL_EXTENSION[detection.extension]} name says ${nameFormat} but the content looks like ${contentFormat}.`
    }
    case 'xml-not-cdxml':
      return 'Could not tell: the content is XML without a <CDXML> element.'
    case 'binary-not-cdx':
      return 'Could not tell: the content is binary and has no CDX signature.'
    case 'not-recognized':
      return 'Could not tell: the content matches no supported format.'
    default:
      return unreachable(detection.reason)
  }
}

function unreachable(value: never): never {
  throw new Error(`Unhandled import case: ${String(value)}`)
}
