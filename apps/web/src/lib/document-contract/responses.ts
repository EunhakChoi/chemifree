import { API_VERSION } from '@/lib/contract'
import { readComputeStop, type ComputeStop, type StopExpectation } from '@/lib/compute-stop'
import { stereoOf, type StereoNote } from '@/lib/stereo-contract'
import type { AtomView, BondView, DocumentView } from '@/lib/document-contract/model'
import { ATOM_STATE_FIELDS, isRecord } from '@/lib/document-contract/guards'

export interface SessionView {
  saved_revision: number
  unsaved_changes: boolean
  undo_depth: number
  redo_depth: number
  /** The revision this session's recovery draft holds, or null when it has none. */
  draft_revision: number | null
  /**
   * The service's latest failed attempt to draft work the draft does not hold, while it
   * retries. Added within v1, so a service from before it leaves it out.
   */
  draft_failure?: DraftFailureView | null
}

export interface DraftFailureView {
  /** Failed attempts in a row. */
  attempts: number
  retry_in_ms: number
  /**
   * The latest attempt's error code and, for `storage_failed`, its reason, as in the v1 error
   * envelope. Added within v1 after the record itself, so an earlier service leaves them out.
   */
  code?: string
  reason?: StorageReason
  message: string
}

export type RecoveryState = 'recoverable' | 'outdated' | 'unreadable' | 'in_use'

/** An abandoned recovery draft: unsaved work of a session whose process ended. */
export interface RecoveryView {
  state: RecoveryState
  revision: number | null
  base_revision: number | null
  saved_at_ms: number | null
  message: string | null
}

export interface DocumentResponse {
  api_version: typeof API_VERSION
  document: DocumentView
  session: SessionView
  recovery: RecoveryView | null
  /**
   * What this request cleared of stereo (a transaction that changed the neighbors of a stated
   * configuration). Empty for every other reply, and absent from an older service's.
   */
  notes?: StereoNote[]
}

/** A document saved as a new one; the copy is open in its own session. */
export interface CopyResponse extends DocumentResponse {
  source: { id: string; closed: boolean; discarded_changes: boolean }
}

export interface DocumentSummary {
  id: string
  title: string
  revision: number
  atom_count: number
  bond_count: number
  open: boolean
  unsaved_changes: boolean
  recovery: RecoveryView | null
  /** When storage last committed it, in milliseconds since the Unix epoch. */
  saved_at_ms: number | null
}

/** Title order, or most recently saved first. */
export type ListSort = 'title' | 'recent'

export interface ListQuery {
  /** Text the title must contain, ignoring case. */
  query: string
  sort: ListSort
}

export interface TrashedSummary {
  id: string
  title: string
  revision: number
  atom_count: number
  bond_count: number
  deleted_at_ms: number | null
}

export interface TrashListResponse {
  api_version: typeof API_VERSION
  documents: TrashedSummary[]
  unreadable: { id: string; message: string }[]
  /** True when the service's cap of 1,000 documents left some unread; absent from a service that predates it. */
  truncated?: boolean
}

export interface DocumentListResponse {
  api_version: typeof API_VERSION
  documents: DocumentSummary[]
  unreadable: { id: string; message: string }[]
  /** True when the service's cap of 1,000 documents left some unread; absent from a service that predates it. */
  truncated?: boolean
}

/** Why storage failed (`storage_failed`), so the page can say whether retrying can help. */
export type StorageReason = 'read_only' | 'busy' | 'io'

export const isStorageReason = (value: unknown): value is StorageReason =>
  value === 'read_only' || value === 'busy' || value === 'io'

/** Optional fields of a v1 error envelope. */
export interface ErrorDetails {
  currentRevision?: number
  commandIndex?: number
  reason?: StorageReason
  /** The code of a refused state, scene rule, or document/new-document setting. */
  stateReason?: string
}

/**
 * A v1 error envelope, or a failure found by the client. Status 0 means no reply arrived
 * (code `unreachable`), so the request may or may not have taken effect.
 */
export class ApiError extends Error {
  readonly code: string
  readonly status: number
  readonly currentRevision?: number
  readonly commandIndex?: number
  readonly reason?: StorageReason
  readonly stateReason?: string

  constructor(code: string, message: string, status: number, details: ErrorDetails = {}) {
    super(message)
    this.name = 'ApiError'
    this.code = code
    this.status = status
    this.currentRevision = details.currentRevision
    this.commandIndex = details.commandIndex
    this.reason = details.reason
    this.stateReason = details.stateReason
  }
}

/**
 * The service stopped the operation at its time limit or on a cancel. It produced no result and
 * changed nothing; `stop` says which of the two it was.
 */
export class ComputeStopped extends ApiError {
  readonly stop: ComputeStop

  constructor(stop: ComputeStop, message: string) {
    super(stop.kind, message, 200)
    this.name = 'ComputeStopped'
    this.stop = stop
  }
}

/**
 * Throws the stop a reply carries: `ComputeStopped` for a stop, `incompatible_response` for a stop
 * code with the wrong shape. Any other reply returns. Call it on the raw reply before the
 * operation's own outcome checks, so a stop is never read as an ordinary failure.
 */
export function throwIfStopped(value: unknown, expect: StopExpectation): void {
  const stop = readComputeStop(value, expect)
  if (stop === null) return
  const message = firstDiagnostic(value).message
  if (stop === 'malformed' || message === null) {
    throw new ApiError('incompatible_response', 'The local service returned an incompatible stop reply.', 200)
  }
  throw new ComputeStopped(stop, message)
}

/** What an operation reply says first about why it failed, as far as it can be read. */
export function firstDiagnostic(value: unknown): { code: string | null; message: string | null; details: Record<string, unknown> } {
  const first: unknown = isRecord(value) && Array.isArray(value.diagnostics) ? value.diagnostics[0] : null
  return {
    code: isRecord(first) && typeof first.code === 'string' ? first.code : null,
    message: isRecord(first) && typeof first.message === 'string' ? first.message : null,
    details: isRecord(first) && isRecord(first.details) ? first.details : {},
  }
}

/** A document whose atoms and bonds carry every recorded-state field, defaults filled in. */
export function withStates(response: DocumentResponse): DocumentResponse {
  return {
    ...response,
    notes: response.notes ?? [],
    document: withDocumentStates(response.document),
  }
}

// Transport provenance stays in memory; it is never an API or saved-document field.
export const snapshotOrigins = new WeakMap<object, { client: object; generation: number }>()

export function withDocumentStates(document: DocumentView): DocumentView {
  const atoms = document.atoms as Array<AtomView | Omit<AtomView, (typeof ATOM_STATE_FIELDS)[number]>>
  const bonds = document.bonds as Array<BondView | Omit<BondView, 'query' | 'mark'> | Omit<BondView, 'mark'>>
  const normalized: DocumentView = {
    ...document,
    stereo: stereoOf(document),
    atoms: atoms.map((atom) =>
      'kind' in atom
        ? atom
        : { ...atom, kind: 'element', query: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [] },
    ),
    bonds: bonds.map((bond) => ({
      ...bond,
      query: 'query' in bond ? bond.query : null,
      mark: 'mark' in bond ? bond.mark : null,
    })),
  }
  const origin = snapshotOrigins.get(document)
  if (origin) snapshotOrigins.set(normalized, origin)
  return normalized
}
