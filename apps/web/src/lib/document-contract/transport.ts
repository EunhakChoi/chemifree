import { API_VERSION } from '@/lib/contract'
import { holdsStereo } from '@/lib/stereo-contract'
import type { DocumentSettings, DocumentView, SettingsEdit } from '@/lib/document-contract/model'
import { isCount, isRecord } from '@/lib/document-contract/guards'
import { ApiError, type DocumentResponse, withStates } from '@/lib/document-contract/responses'
import { isDocumentResponse, isSettings } from '@/lib/document-contract/readers'

export const TIMEOUT_MS = 10_000
/**
 * While the service reports that a request whose reply was lost is still running, its
 * outcome is asked for this often, at most `OUTCOME_CHECKS` times. The service answers every
 * such request within its own time budget (about 8 s plus the write itself).
 */
export const OUTCOME_CHECK_INTERVAL_MS = 1000
export const OUTCOME_CHECKS = 12
export const GATEWAY_STATUSES = new Set([502, 503, 504])

/** The colour theme of the user interface. */
export type Theme = 'system' | 'light' | 'dark'
export const THEMES: readonly Theme[] = ['system', 'light', 'dark']

/** A computation preference: where it will belong, and that nothing stores or applies it yet. */
export interface ComputationSetting {
  status: 'not_implemented'
  owner: string
  followup: string
  note: string
}

/**
 * The preferences of the service's data directory. They choose the settings a new document
 * starts from (copied once, when it is created) and the theme; they never change an existing
 * document. Added within v1.
 */
export interface PreferencesResponse {
  revision: number
  ui: { theme: Theme }
  /** The fields new documents start from, as stored; the rest are the built-in defaults. */
  new_documents: SettingsEdit
  /** The settings a document created now starts from. */
  new_document_settings: DocumentSettings
  computation: {
    nmr: ComputationSetting
    numeric_precision: ComputationSetting
    /** Added within v1; absent from older services. */
    mass_precision?: { status: 'available'; owner: 'preferences'; decimals: number }
  }
}

/** The preferences a write sets: all of them, replacing what is stored. */
export interface PreferencesChange {
  ui: { theme: Theme }
  new_documents: SettingsEdit
  computation?: { mass_precision: { decimals: number } }
}

/** Where a reset takes a document's settings from. */
export type SettingsSource = 'defaults' | 'preferences'

function isComputationSetting(value: unknown): value is ComputationSetting {
  return (
    isRecord(value) &&
    value.status === 'not_implemented' &&
    typeof value.owner === 'string' &&
    typeof value.followup === 'string' &&
    typeof value.note === 'string'
  )
}

export function isPreferencesResponse(value: unknown): value is PreferencesResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    isCount(value.revision) &&
    isRecord(value.ui) &&
    (THEMES as readonly unknown[]).includes(value.ui.theme) &&
    isSettings(value.new_documents, false, 'open') &&
    isSettings(value.new_document_settings, true, 'open') &&
    isRecord(value.computation) &&
    isComputationSetting(value.computation.nmr) &&
    isComputationSetting(value.computation.numeric_precision) &&
    (value.computation.mass_precision === undefined || (
      isRecord(value.computation.mass_precision) &&
      value.computation.mass_precision.status === 'available' &&
      value.computation.mass_precision.owner === 'preferences' &&
      isCount(value.computation.mass_precision.decimals) &&
      value.computation.mass_precision.decimals <= 8
    ))
  )
}

export function pause(ms: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, ms))
}

/** No reply arrived, or the service still runs the request: either way it may take effect. */
export function outcomeOpen(error: ApiError) {
  return error.status === 0 || error.code === 'request_in_progress'
}

/**
 * A write whose outcome is unknown. `sentTo` is the session token of the service that may
 * have run it: each service process has its own token, and a different process has no record
 * of the request, so asking it again with the same ID would run the request a second time.
 */
export interface Unconfirmed {
  requestId: string
  sentTo?: string
}

/** One compared drawing in a reply's stereo report: its flag key, loss path and snapshot (null when absent). */
interface StereoSide {
  key: string
  path: string
  snapshot: DocumentView | null
}

/**
 * Checks what a comparison reply says about stereo against the drawings it is about, and
 * returns each side's flag, or null when the reply is inconsistent. The model never reads
 * stereo, so `compared` is false whatever the drawings hold; a flag must equal what the
 * snapshot really holds; and the reply is `partial` with exactly one `stereo` loss per side
 * that holds some, or `ok` with none. A reply without the report comes from a service that
 * could not hold stereo, so it is consistent only for drawings that hold none.
 */
export function readStereoReport(
  reply: Record<string, unknown>,
  result: Record<string, unknown>,
  sides: StereoSide[],
): Record<string, boolean | null> | null {
  const report = result.stereo
  if (report !== undefined && (!isRecord(report) || report.compared !== false)) {
    return null
  }
  const present: Record<string, boolean | null> = {}
  for (const side of sides) {
    const flag = isRecord(report) ? report[side.key] : undefined
    if (side.snapshot === null) {
      if (flag !== undefined && flag !== null) return null
      present[side.key] = null
      continue
    }
    const held = holdsStereo(side.snapshot)
    // Without a report nothing is stated, which is read as "none": true only if none is held.
    if ((report === undefined ? false : flag) !== held) return null
    present[side.key] = held
  }
  const paths = sides.filter((side) => present[side.key] === true).map((side) => side.path)
  // A reply that leaves `loss_report` out reports none.
  const losses = reply.loss_report ?? []
  if (reply.outcome !== (paths.length === 0 ? 'ok' : 'partial') || !Array.isArray(losses) || losses.length !== paths.length) return null
  const reported = paths.every((path, index) => {
    const loss: unknown = losses[index]
    return isRecord(loss) && loss.feature === 'stereo' && loss.disposition === 'unknown' && loss.semantic_impact === 'stereo' &&
      loss.source_path === path && typeof loss.reason === 'string'
  })
  return reported ? present : null
}

/** A snapshot an operation reply carries (`document`, `session`, `recovery`), as a document view. */
export function snapshotOf(raw: Record<string, unknown>): DocumentView {
  return documentResponse({ api_version: API_VERSION, ...raw }).document
}

/** A document reply checked at runtime, with older replies' missing states filled in. */
export function documentResponse(value: unknown): DocumentResponse {
  if (!isDocumentResponse(value)) {
    throw new ApiError('incompatible_response', 'The local service returned an incompatible document.', 200)
  }
  return withStates(value)
}

export function restarted(status: number) {
  return new ApiError(
    'service_restarted',
    'The local service restarted, so unsaved edits it held are gone. Reload to continue from the last save.',
    status,
  )
}

export const PREFERENCES_PATH = '/api/v1/preferences'

export function documentPath(id: string) {
  return `/api/v1/documents/${encodeURIComponent(id)}`
}

export function requestId() {
  return crypto.randomUUID()
}
