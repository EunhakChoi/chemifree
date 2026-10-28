/** Hand-maintained mirror of the v1 document contract in crates/application/src/contract.rs. */
import { API_VERSION } from '@/lib/contract'

export type BondOrder = 'single' | 'double' | 'triple'

export interface Position {
  x: number
  y: number
}

export interface AtomView {
  id: string
  element: string
  charge: number
  isotope: number | null
  position: Position
}

export interface BondView {
  id: string
  begin: string
  end: string
  order: BondOrder
}

export interface DocumentView {
  id: string
  title: string
  revision: number
  atoms: AtomView[]
  bonds: BondView[]
}

export interface SessionView {
  saved_revision: number
  unsaved_changes: boolean
  undo_depth: number
  redo_depth: number
  /** The revision this session's recovery draft holds, or null when it has none. */
  draft_revision: number | null
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
}

export interface DocumentListResponse {
  api_version: typeof API_VERSION
  documents: DocumentSummary[]
  unreadable: { id: string; message: string }[]
}

/** `"$N"` names the atom added by command N earlier in the same transaction. */
export type AtomRef = string

export type Command =
  | { op: 'add_atom'; element: string; position: Position; bond_to?: { atom: AtomRef; order: BondOrder } }
  | { op: 'add_bond'; begin: AtomRef; end: AtomRef; order: BondOrder }
  | { op: 'update_atom'; atom: string; element?: string; charge?: number; isotope?: number | null }
  | { op: 'move_atoms'; atoms: string[]; delta: Position }
  | { op: 'update_bond'; bond: string; order: BondOrder }
  | { op: 'delete_atoms'; atoms: string[] }
  | { op: 'delete_bonds'; bonds: string[] }
  | { op: 'rename'; title: string }

/** A v1 error envelope, or a transport failure reported with code `unreachable`. */
export class ApiError extends Error {
  readonly code: string
  readonly status: number
  readonly currentRevision?: number
  readonly commandIndex?: number

  constructor(code: string, message: string, status: number, currentRevision?: number, commandIndex?: number) {
    super(message)
    this.name = 'ApiError'
    this.code = code
    this.status = status
    this.currentRevision = currentRevision
    this.commandIndex = commandIndex
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

const isCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0
const isFiniteNumber = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)
const isOrder = (value: unknown): value is BondOrder => value === 'single' || value === 'double' || value === 'triple'

function isAtom(value: unknown): value is AtomView {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.element === 'string' &&
    Number.isSafeInteger(value.charge) &&
    (value.isotope === null || isCount(value.isotope)) &&
    isRecord(value.position) &&
    isFiniteNumber(value.position.x) &&
    isFiniteNumber(value.position.y)
  )
}

function isBond(value: unknown): value is BondView {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.begin === 'string' &&
    typeof value.end === 'string' &&
    isOrder(value.order)
  )
}

const isCountOrNull = (value: unknown) => value === null || isCount(value)

function isRecovery(value: unknown): value is RecoveryView | null {
  return (
    value === null ||
    (isRecord(value) &&
      (value.state === 'recoverable' || value.state === 'outdated' || value.state === 'unreadable' || value.state === 'in_use') &&
      isCountOrNull(value.revision) &&
      isCountOrNull(value.base_revision) &&
      isCountOrNull(value.saved_at_ms) &&
      (value.message === null || typeof value.message === 'string'))
  )
}

/** Rejects malformed snapshots, including bonds that name atoms the snapshot lacks. */
export function isDocumentResponse(value: unknown): value is DocumentResponse {
  if (!isRecord(value) || value.api_version !== API_VERSION) return false
  const { document, session } = value
  if (
    !isRecord(document) ||
    typeof document.id !== 'string' ||
    typeof document.title !== 'string' ||
    !isCount(document.revision) ||
    !Array.isArray(document.atoms) ||
    !Array.isArray(document.bonds) ||
    !document.atoms.every(isAtom) ||
    !document.bonds.every(isBond)
  ) {
    return false
  }
  const atomIds = new Set(document.atoms.map((atom) => atom.id))
  const bondIds = new Set(document.bonds.map((bond) => bond.id))
  return (
    atomIds.size === document.atoms.length &&
    bondIds.size === document.bonds.length &&
    document.bonds.every((bond) => atomIds.has(bond.begin) && atomIds.has(bond.end)) &&
    isRecord(session) &&
    isCount(session.saved_revision) &&
    typeof session.unsaved_changes === 'boolean' &&
    isCount(session.undo_depth) &&
    isCount(session.redo_depth) &&
    isCountOrNull(session.draft_revision) &&
    isRecovery(value.recovery)
  )
}

function isCopy(value: unknown): value is CopyResponse {
  if (!isDocumentResponse(value)) return false
  const { source } = value as { source?: unknown }
  return (
    isRecord(source) &&
    typeof source.id === 'string' &&
    typeof source.closed === 'boolean' &&
    typeof source.discarded_changes === 'boolean'
  )
}

function isSummary(value: unknown): value is DocumentSummary {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    isCount(value.revision) &&
    isCount(value.atom_count) &&
    isCount(value.bond_count) &&
    typeof value.open === 'boolean' &&
    typeof value.unsaved_changes === 'boolean' &&
    isRecovery(value.recovery)
  )
}

function isList(value: unknown): value is DocumentListResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    Array.isArray(value.documents) &&
    value.documents.every(isSummary) &&
    Array.isArray(value.unreadable) &&
    value.unreadable.every((item) => isRecord(item) && typeof item.id === 'string' && typeof item.message === 'string')
  )
}

function isClose(value: unknown): value is { discarded_changes: boolean } {
  return isRecord(value) && value.api_version === API_VERSION && typeof value.discarded_changes === 'boolean'
}

const TIMEOUT_MS = 10_000

/** Client for the local document API. Writes carry the per-process session token. */
export class DocumentClient {
  private token: Promise<{ header: string; token: string }> | null = null
  /**
   * Writes that create a document and whose outcome is unknown (no response arrived), by what
   * they asked for. They are not guarded by a revision, so a retry reuses the request ID and
   * the API replays the document it already committed instead of creating a second one.
   */
  private unconfirmed = new Map<string, string>()
  private readonly restartListeners = new Set<() => void>()

  /**
   * Calls `listener` each time a write finds that the service restarted (its session token was
   * refused). Returns a function that removes the listener.
   */
  onRestart(listener: () => void): () => void {
    this.restartListeners.add(listener)
    return () => {
      this.restartListeners.delete(listener)
    }
  }

  private async session() {
    this.token ??= this.request('GET', '/api/v1/session').then((value) => {
      if (!isRecord(value) || typeof value.header !== 'string' || typeof value.token !== 'string') {
        throw new ApiError('incompatible_response', 'The local service returned an invalid session.', 200)
      }
      return { header: value.header, token: value.token }
    })
    try {
      return await this.token
    } catch (error) {
      this.token = null
      throw error
    }
  }

  private async request(method: 'GET' | 'POST', path: string, body?: unknown, headers: Record<string, string> = {}) {
    let response: Response
    try {
      response = await fetch(path, {
        method,
        headers: {
          Accept: 'application/json',
          ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
          ...headers,
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(TIMEOUT_MS),
      })
    } catch (error) {
      const timeout = error instanceof Error && error.name === 'TimeoutError'
      throw new ApiError(
        'unreachable',
        timeout ? 'The local service did not respond in time.' : 'The local service could not be reached.',
        0,
      )
    }
    const value: unknown = await response.json().catch(() => null)
    if (!response.ok) {
      if (isRecord(value) && typeof value.code === 'string' && typeof value.message === 'string') {
        throw new ApiError(
          value.code,
          value.message,
          response.status,
          isCount(value.current_revision) ? value.current_revision : undefined,
          isCount(value.command_index) ? value.command_index : undefined,
        )
      }
      throw new ApiError('http_error', `The local service returned HTTP ${response.status}.`, response.status)
    }
    return value
  }

  /**
   * A restarted API issues a new token and has lost its working copies, so revisions this
   * page has seen may since name different states. A first create attempt is retried with the
   * new token; an edit to an open document is not, and asks for a reload instead.
   */
  private async write(path: string, body: Record<string, unknown>, onRestart: 'retry' | 'reload') {
    for (let attempt = 0; ; attempt += 1) {
      const { header, token } = await this.session()
      try {
        return await this.request('POST', path, body, { [header]: token })
      } catch (error) {
        if (!(error instanceof ApiError && error.code === 'unauthorized')) throw error
        this.token = null
        for (const listener of this.restartListeners) listener()
        if (onRestart === 'retry' && attempt === 0) continue
        if (onRestart === 'reload') {
          throw new ApiError(
            'service_restarted',
            'The local service restarted, so unsaved edits it held are gone. Reload to continue from the last save.',
            error.status,
          )
        }
        throw error
      }
    }
  }

  private static document(value: unknown): DocumentResponse {
    if (!isDocumentResponse(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible document.', 200)
    }
    return value
  }

  async list(): Promise<DocumentListResponse> {
    const value = await this.request('GET', '/api/v1/documents')
    if (!isList(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible list.', 200)
    }
    return value
  }

  async get(id: string) {
    return DocumentClient.document(await this.request('GET', documentPath(id)))
  }

  /**
   * Sends a write that creates a document at most once. A restarted service has forgotten the
   * request_id of an earlier unanswered attempt, so that attempt is not resent: if it was
   * committed, a resend would create a second document.
   */
  private async createOnce(key: string, path: string, body: Record<string, unknown>, unknown: ApiError) {
    const earlier = this.unconfirmed.get(key)
    const id = earlier ?? requestId()
    this.unconfirmed.set(key, id)
    try {
      const value = await this.write(path, { request_id: id, ...body }, earlier ? 'reload' : 'retry')
      this.unconfirmed.delete(key)
      return value
    } catch (error) {
      // Status 0 means no answer arrived, so the document may exist; any other failure is an answer.
      if (!(error instanceof ApiError && error.status === 0)) this.unconfirmed.delete(key)
      if (error instanceof ApiError && error.code === 'service_restarted') throw unknown
      throw error
    }
  }

  async create(title?: string) {
    return DocumentClient.document(
      await this.createOnce(
        `create:${title ?? ''}`,
        '/api/v1/documents',
        { title },
        new ApiError(
          'create_unconfirmed',
          'The local service restarted before confirming the earlier new document, so it may already exist. Check Saved documents before creating another.',
          401,
        ),
      ),
    )
  }

  private static copy(value: unknown): CopyResponse {
    if (!isCopy(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible copy.', 200)
    }
    return value
  }

  private static copyUnconfirmed() {
    return new ApiError(
      'copy_unconfirmed',
      'The local service restarted before confirming the copy, so it may already exist. Check Saved documents before copying again.',
      401,
    )
  }

  /** Saves the working copy as a new document; the source returns to its last save. */
  async saveAs(id: string, expectedRevision: number, title?: string) {
    return DocumentClient.copy(
      await this.createOnce(
        `save-as:${id}:${expectedRevision}:${title ?? ''}`,
        `${documentPath(id)}/save-as`,
        { expected_revision: expectedRevision, title },
        DocumentClient.copyUnconfirmed(),
      ),
    )
  }

  async recoverAsCopy(id: string, title?: string) {
    return DocumentClient.copy(
      await this.createOnce(
        `recover-as-copy:${id}:${title ?? ''}`,
        `${documentPath(id)}/recover-as-copy`,
        { title },
        DocumentClient.copyUnconfirmed(),
      ),
    )
  }

  /**
   * Writes the recovery draft. After a restart the new service holds none of this page's
   * unsaved work, so the page must reload (where the old process's draft is offered).
   */
  async autosave(id: string) {
    return DocumentClient.document(await this.write(`${documentPath(id)}/autosave`, { request_id: requestId() }, 'reload'))
  }

  /** Recovering again after a restart is safe: the draft is abandoned again either way. */
  async recover(id: string) {
    return DocumentClient.document(await this.write(`${documentPath(id)}/recover`, { request_id: requestId() }, 'retry'))
  }

  async discardDraft(id: string) {
    const value = await this.write(`${documentPath(id)}/discard-draft`, { request_id: requestId() }, 'retry')
    if (!isRecord(value) || value.api_version !== API_VERSION || typeof value.id !== 'string') {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible reply.', 200)
    }
  }

  async transact(id: string, expectedRevision: number, commands: Command[]) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/transactions`,
        { request_id: requestId(), expected_revision: expectedRevision, commands },
        'reload',
      ),
    )
  }

  async revisionAction(id: string, action: 'undo' | 'redo' | 'save', expectedRevision: number) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/${action}`,
        { request_id: requestId(), expected_revision: expectedRevision },
        'reload',
      ),
    )
  }

  async close(id: string, discardUnsaved: boolean, expectedRevision?: number) {
    // A refused token is retried: the revision check still guards what a close may discard.
    const value = await this.write(
      `${documentPath(id)}/close`,
      { request_id: requestId(), discard_unsaved: discardUnsaved, expected_revision: expectedRevision },
      'retry',
    )
    if (!isClose(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible reply.', 200)
    }
    return value
  }
}

function documentPath(id: string) {
  return `/api/v1/documents/${encodeURIComponent(id)}`
}

function requestId() {
  return crypto.randomUUID()
}
