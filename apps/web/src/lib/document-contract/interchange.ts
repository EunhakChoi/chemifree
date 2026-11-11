import { ctRefusalSentence, type CtDirection } from '@/lib/ct-conversion'
import type { DocumentView } from '@/lib/document-contract/model'
import { isCount, isFiniteNumber, isRecord } from '@/lib/document-contract/guards'
import { ApiError } from '@/lib/document-contract/responses'
import { sameJson } from '@/lib/document-contract/inspection'

export interface InterchangeLoss {
  feature: string
  source_path: string
  disposition: string
  reason: string
  semantic_impact: string
}

export interface InterchangeChoice {
  page_id: number
  fragment_id: number
  page_path: string
  fragment_path: string
}

/** CDXML travels as text; CDX travels as canonical base64 of its bytes. */
export type InterchangeFormat = 'cdxml' | 'cdx'

/** A drawing export is bound to the exact snapshot the service wrote. CDX content is base64. */
export interface InterchangeExport {
  content: string
  format: InterchangeFormat
  snapshot: DocumentView
  losses: CtLoss[]
}

/** The picture's canvas, in pt. */
export interface SvgViewBox {
  x: number
  y: number
  width: number
  height: number
}

/** An SVG picture bound to the exact snapshot the service wrote. It is a picture, not an interchange file. */
export interface SvgExport {
  content: string
  media_type: 'image/svg+xml'
  view_box: SvgViewBox
  font_table: string
  model: string
  snapshot: DocumentView
  losses: CtLoss[]
}

export interface InterchangeInput {
  format: InterchangeFormat
  source: string
  title: string
  page_id: number
  fragment_id: number
}

export interface InterchangePreview {
  digest: string
  atomLabels: string[]
  bonds: number
  mapping: InterchangeMapping
  losses: InterchangeLoss[]
}

interface InterchangeIdMapping {
  source_path: string
  source_id: number | null
  target_id: string
}

export interface InterchangeMapping {
  document: InterchangeIdMapping
  page: InterchangeIdMapping
  fragment: InterchangeIdMapping
  atoms: InterchangeIdMapping[]
  bonds: InterchangeIdMapping[]
}

function isInterchangeIdMapping(value: unknown): value is InterchangeIdMapping {
  return isRecord(value) && typeof value.source_path === 'string' &&
    (value.source_id === null || isCount(value.source_id)) && typeof value.target_id === 'string'
}

export function isInterchangeMapping(value: unknown): value is InterchangeMapping {
  return isRecord(value) && isInterchangeIdMapping(value.document) && isInterchangeIdMapping(value.page) &&
    isInterchangeIdMapping(value.fragment) && Array.isArray(value.atoms) && value.atoms.every(isInterchangeIdMapping) &&
    Array.isArray(value.bonds) && value.bonds.every(isInterchangeIdMapping)
}

export function interchangeReply(value: unknown): { result: Record<string, unknown>; losses: InterchangeLoss[] } {
  if (!isRecord(value) || !Array.isArray(value.diagnostics)) {
    throw new ApiError('incompatible_response', 'The service returned an invalid interchange response.', 200)
  }
  if (value.outcome === 'error' || value.outcome === 'unsupported') {
    const problem: unknown = value.diagnostics[0]
    if (isRecord(problem) && typeof problem.code === 'string' && typeof problem.message === 'string') {
      // A binary format names the offending byte; the server keeps it out of the message.
      const offset = isRecord(problem.details) ? problem.details.offset : undefined
      const where = typeof offset === 'number' && Number.isSafeInteger(offset) && offset >= 0 ? ` (byte offset ${offset})` : ''
      throw new ApiError(problem.code, `${problem.message}${where}`, 200)
    }
    throw new ApiError('conversion_failed', 'The interchange operation failed.', 200)
  }
  if ((value.outcome !== 'ok' && value.outcome !== 'partial') || !isRecord(value.result) ||
      !Array.isArray(value.loss_report) || !value.loss_report.every((loss): loss is InterchangeLoss =>
        isRecord(loss) && typeof loss.feature === 'string' && typeof loss.source_path === 'string' &&
        typeof loss.disposition === 'string' && typeof loss.reason === 'string' && typeof loss.semantic_impact === 'string')) {
    throw new ApiError('incompatible_response', 'The service returned an invalid interchange response.', 200)
  }
  return { result: value.result, losses: value.loss_report }
}

export type CtFormat = 'mol' | 'sdf'

export interface CtLoss {
  feature: string
  source_path: string
  disposition: string
  reason: string
  semantic_impact: string
}

export interface CtPreview {
  digest: string
  snapshot: DocumentView
  records: Array<{ title: string; atoms: number; bonds: number; fields: Array<{ name: string; value: string }> }>
  losses: CtLoss[]
}

export interface CtImport {
  documentId: string
  revision: number
  format: CtFormat
  source: string
  record: number
  digest: string
}

interface CtSourceRecord {
  title: string
  atoms: unknown[]
  bonds: unknown[]
  fields: Array<{ name: string; value: string }>
}

export function isCtSourceRecord(item: unknown): item is CtSourceRecord {
  return isRecord(item) && typeof item.title === 'string' && Array.isArray(item.atoms) &&
    Array.isArray(item.bonds) && Array.isArray(item.fields) && item.fields.every((field: unknown) =>
      isRecord(field) && typeof field.name === 'string' && typeof field.value === 'string')
}

export function ctReply(value: unknown, direction: CtDirection): { result: Record<string, unknown>; losses: CtLoss[] } {
  if (!isRecord(value) || !Array.isArray(value.diagnostics)) {
    throw new ApiError('incompatible_response', 'The service returned an incompatible conversion response.', 200)
  }
  if (value.outcome === 'error' || value.outcome === 'unsupported') {
    const diagnostic: unknown = value.diagnostics[0]
    if (isRecord(diagnostic) && typeof diagnostic.code === 'string' && typeof diagnostic.message === 'string') {
      const details = isRecord(diagnostic.details) ? diagnostic.details : {}
      if (diagnostic.code === 'loss_consent_required' && Array.isArray(details.losses) &&
          details.losses.every(isLoss) && Array.isArray(details.features) &&
          details.features.every((feature): feature is string => typeof feature === 'string')) {
        throw new ConsentRequired(
          diagnostic.message, details.features, details.losses,
          typeof details.loss_digest === 'string' ? details.loss_digest : undefined,
        )
      }
      const record = isCount(details.record) ? ` Record ${details.record}.` : ''
      const line = isCount(details.line) ? ` Line ${details.line}.` : ''
      const sentence = ctRefusalSentence(diagnostic.code, direction)
      const said = `${diagnostic.message}${record}${line}`
      throw new ApiError(diagnostic.code, sentence === null ? said : `${sentence} Service message: ${said}`, 200, {
        currentRevision: isCount(details.current_revision) ? details.current_revision : undefined,
      })
    }
    throw new ApiError('conversion_failed', 'The conversion failed.', 200)
  }
  if ((value.outcome !== 'ok' && value.outcome !== 'partial') || !isRecord(value.result) ||
      !Array.isArray(value.loss_report) || !value.loss_report.every((loss): loss is CtLoss =>
        isRecord(loss) && typeof loss.feature === 'string' && typeof loss.source_path === 'string' &&
        typeof loss.disposition === 'string' && typeof loss.reason === 'string' && typeof loss.semantic_impact === 'string')) {
    throw new ApiError('incompatible_response', 'The service returned an incompatible conversion response.', 200)
  }
  return { result: value.result, losses: value.loss_report }
}

const isLoss = (loss: unknown): loss is CtLoss =>
  isRecord(loss) && typeof loss.feature === 'string' && typeof loss.source_path === 'string' &&
  typeof loss.disposition === 'string' && typeof loss.reason === 'string' && typeof loss.semantic_impact === 'string'

export type SmilesStyle = 'recorded' | 'canonical'

/** A part of the source: bytes are the API's unit, characters are what a person counts. */
export interface SmilesSpan {
  offset: number
  length: number
  character_offset: number
  character_length: number
}

export const isSmilesSpan = (value: unknown): value is SmilesSpan =>
  isRecord(value) && isCount(value.offset) && isCount(value.length) && isCount(value.character_offset) &&
  isCount(value.character_length)

/** The place in the text, in characters (counted from 1) and in bytes (first and last). */
export function describeSpan(span: SmilesSpan): string {
  const first = span.character_offset + 1
  if (span.length === 0) return `the position before character ${first} (byte ${span.offset})`
  const last = span.character_offset + span.character_length
  const lastByte = span.offset + span.length - 1
  const characters = last === first ? `character ${first}` : `characters ${first} to ${last}`
  const bytes = lastByte === span.offset ? `byte ${span.offset}` : `bytes ${span.offset} to ${lastByte}`
  return `${characters} (${bytes})`
}

export interface SmilesPreview {
  digest: string
  snapshot: DocumentView
  atoms: number
  bonds: number
  ignoredText: SmilesSpan | null
  losses: CtLoss[]
  valenceModel: string
  aromaticityModel: string
}

export interface SmilesImport {
  documentId: string
  revision: number
  source: string
  digest: string
  acceptLoss: boolean
}

export interface SmilesExport {
  content: string
  style: SmilesStyle
  snapshot: DocumentView
  losses: CtLoss[]
  model: string
}

/** The service refused until the listed losses are accepted. `digest` is set for an export. */
export class ConsentRequired extends ApiError {
  readonly features: string[]
  readonly losses: CtLoss[]
  readonly digest?: string

  constructor(message: string, features: string[], losses: CtLoss[], digest?: string) {
    super('loss_consent_required', message, 200)
    this.name = 'ConsentRequired'
    this.features = features
    this.losses = losses
    this.digest = digest
  }
}

export function smilesReply(value: unknown): { result: Record<string, unknown>; losses: CtLoss[]; model: string } {
  if (!isRecord(value) || !Array.isArray(value.diagnostics)) {
    throw new ApiError('incompatible_response', 'The service returned an incompatible SMILES response.', 200)
  }
  if (value.outcome === 'error' || value.outcome === 'unsupported') {
    const diagnostic: unknown = value.diagnostics[0]
    if (isRecord(diagnostic) && typeof diagnostic.code === 'string' && typeof diagnostic.message === 'string') {
      const details = isRecord(diagnostic.details) ? diagnostic.details : {}
      if (diagnostic.code === 'loss_consent_required' && Array.isArray(details.losses) &&
          details.losses.every(isLoss) && Array.isArray(details.features) &&
          details.features.every((feature): feature is string => typeof feature === 'string')) {
        throw new ConsentRequired(
          diagnostic.message, details.features, details.losses,
          typeof details.loss_digest === 'string' ? details.loss_digest : undefined,
        )
      }
      const span = isCount(details.offset) && isCount(details.length) && isCount(details.character_offset) &&
        isCount(details.character_length)
        ? ` Problem at ${describeSpan({
            offset: details.offset, length: details.length,
            character_offset: details.character_offset, character_length: details.character_length,
          })}.`
        : ''
      const reason = typeof details.reason === 'string' ? ` Reason: ${details.reason}.` : ''
      throw new ApiError(diagnostic.code, `${diagnostic.message}${span}${reason}`, 200, {
        currentRevision: isCount(details.current_revision) ? details.current_revision : undefined,
      })
    }
    throw new ApiError('conversion_failed', 'The SMILES operation failed.', 200)
  }
  if ((value.outcome !== 'ok' && value.outcome !== 'partial') || !isRecord(value.result) ||
      !Array.isArray(value.loss_report) || !value.loss_report.every(isLoss) || typeof value.model_id !== 'string') {
    throw new ApiError('incompatible_response', 'The service returned an incompatible SMILES response.', 200)
  }
  return { result: value.result, losses: value.loss_report, model: value.model_id }
}

/** The drawing export branch accepts only the documented loss vocabulary. */
export function isDrawingExportLoss(value: unknown): value is CtLoss {
  return isLoss(value) && value.feature.length > 0 && value.source_path.length > 0 &&
    ['retained', 'normalized', 'dropped', 'rejected', 'unknown'].includes(value.disposition) &&
    ['none', 'chemistry', 'document', 'geometry', 'stereo', 'rendering', 'unknown'].includes(value.semantic_impact)
}

export function isCanonicalBase64(value: string): boolean {
  if (value.length === 0 || value.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(value)) return false
  try {
    return btoa(atob(value)) === value
  } catch {
    // Invalid padding and unused padding bits must not reach the download decoder.
    return false
  }
}

export function drawingExportReply(value: unknown, request: string): {
  result: Record<string, unknown>
  losses: CtLoss[]
  revision: number
  model: string | undefined
} {
  const incompatible = () => new ApiError('incompatible_response', 'The service returned an incompatible drawing export.', 200)
  if (!isRecord(value) || value.contract !== 1 || value.envelope !== 'operation_result' ||
      value.request_id !== request || value.operation !== 'document.export' || value.profile !== 'preserve-semantics' ||
      !Array.isArray(value.diagnostics)) {
    throw incompatible()
  }
  if (value.outcome === 'error' || value.outcome === 'unsupported') {
    const problem: unknown = value.diagnostics[0]
    if (value.result !== null || !isRecord(problem) || typeof problem.code !== 'string' || typeof problem.message !== 'string') {
      throw incompatible()
    }
    if (problem.code === 'loss_consent_required') {
      const details = problem.details
      if (!isRecord(details) || typeof details.loss_digest !== 'string' || !/^[a-f0-9]{64}$/.test(details.loss_digest) ||
          !Array.isArray(details.losses) || details.losses.length === 0 || !details.losses.every(isDrawingExportLoss) ||
          !Array.isArray(details.features) || !details.features.every((feature): feature is string => typeof feature === 'string') ||
          !sameJson(details.features, [...new Set(details.losses.map((loss) => loss.feature))])) {
        throw incompatible()
      }
      throw new ConsentRequired(problem.message, details.features, details.losses, details.loss_digest)
    }
    throw new ApiError(problem.code, problem.message, 200, {
      currentRevision: isRecord(problem.details) && isCount(problem.details.current_revision)
        ? problem.details.current_revision : undefined,
    })
  }
  if ((value.outcome !== 'ok' && value.outcome !== 'partial') || value.diagnostics.length !== 0 ||
      !isRecord(value.result) || !isCount(value.document_revision) ||
      !Array.isArray(value.loss_report) || !value.loss_report.every(isDrawingExportLoss)) {
    throw incompatible()
  }
  return {
    result: value.result, losses: value.loss_report, revision: value.document_revision,
    model: typeof value.model_id === 'string' ? value.model_id : undefined,
  }
}

/** Returns a fresh `{ x, y, width, height }` copy of a valid view box, or undefined. Unknown fields are not copied. */
export function svgViewBox(value: unknown): SvgViewBox | undefined {
  if (!isRecord(value)) return undefined
  const { x, y, width, height } = value
  if (!isFiniteNumber(x) || !isFiniteNumber(y) || !isFiniteNumber(width) || !isFiniteNumber(height) ||
      width < 0 || height < 0) {
    return undefined
  }
  return { x, y, width, height }
}
