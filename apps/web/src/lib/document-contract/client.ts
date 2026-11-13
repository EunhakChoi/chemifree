import { API_VERSION } from '@/lib/contract'
import { sceneClipboardCopyMatches, sceneClipboardCutHasIncoming, sceneClipboardCutMatches, sceneClipboardPasteMapping } from '@/lib/scene-clipboard-contract'
import { CIP_MODEL, readCipFailure, readCipReply, type CipScope } from '@/lib/cip-contract'
import { ctabParameters, DEFAULT_CTAB, writtenDialectMatches, type CtabChoice } from '@/lib/ct-conversion'
import {
  isStereoNotes,
  readStereoDiagnostics,
  readStereoInspection,
  stereoOf,
} from '@/lib/stereo-contract'
import {
  type Position, type SelectionView, type SelectionSnapshots,
  type DocumentView,
} from '@/lib/document-contract/model'
import {
  isCount, isFiniteNumber, isRecord,
} from '@/lib/document-contract/guards'
import {
  ApiError, firstDiagnostic, isStorageReason, snapshotOrigins, throwIfStopped, withDocumentStates, withStates,
  type CopyResponse, type DocumentListResponse, type DocumentResponse,
  type ListQuery, type TrashListResponse,
} from '@/lib/document-contract/responses'
import {
  isClose, isCopy, isList, isMoved, isSelection, isStringArray, isTrashList,
} from '@/lib/document-contract/readers'
import {
  SUBSTRUCTURE_OUTCOMES, SubstructureRefusal, sameConversionSnapshot, sameInspectionSnapshot, sameJson, settingsOf,
  type AromaticitySystem, type AromaticityInspection, type CanonicalIdentity, type CipInspection, type CipParameters,
  type IdentityInspection, type IdentityMode, type MatchPair, type PropertiesInspection, type PropertiesScope,
  type PropertyComponent, type PropertyKind, type StereoInspection, type SubstructureMapping, type SubstructureSearch,
  type ValenceAtom, type ValenceInspection,
} from '@/lib/document-contract/inspection'
import {
  MAX_CLIPBOARD_MOL_BYTES, MAX_CLIPBOARD_SMILES_BYTES, clipboardIncompatible, clipboardId, clipboardMappings,
  clipboardPayloadIds, clipboardReply, clipboardSelection, clipboardTextFits, operationDataReply, sameClipboardIds,
  type Command, type NativeClipboardCopy, type NativeClipboardCutAttempt, type NativeClipboardMolImportAttempt,
  type NativeClipboardPaste, type NativeClipboardPasteAttempt, type NativeSceneClipboardCopy,
  type NativeSceneClipboardPasteAttempt, type RingInsertionAttempt, type RingPattern, type RingPlacement, type RingSize,
  type SmilesClipboardImportAttempt, type TemplateAttachmentOffer, type TemplateInspection, type TemplateParameters,
  type TemplateReplacementOffer, type TemplateReplacementParameters,
} from '@/lib/document-contract/editing'
import {
  ConsentRequired, ctReply, drawingExportReply, interchangeReply, isCanonicalBase64, isCtSourceRecord,
  isDrawingExportLoss, isInterchangeMapping, isSmilesSpan, smilesReply, svgViewBox,
  type CtFormat, type CtImport, type CtLoss, type CtPreview, type InterchangeChoice, type InterchangeExport,
  type InterchangeFormat, type InterchangeInput, type InterchangePreview, type SmilesExport, type SmilesImport,
  type SmilesPreview, type SmilesStyle, type SvgExport,
} from '@/lib/document-contract/interchange'
import {
  GATEWAY_STATUSES, OUTCOME_CHECKS, OUTCOME_CHECK_INTERVAL_MS, PREFERENCES_PATH, TIMEOUT_MS, documentPath,
  documentResponse, isPreferencesResponse, outcomeOpen, pause, readStereoReport, requestId, restarted, snapshotOf,
  type PreferencesChange, type PreferencesResponse, type SettingsSource, type Unconfirmed,
} from '@/lib/document-contract/transport'

/** Client for the local document API. Writes carry the per-process session token. */
export class DocumentClient {
  private generation = 0
  private token: Promise<{ header: string; token: string }> | null = null
  /**
   * `reread` found that the process holding `token` was replaced. Writes keep presenting the
   * token so they meet the restart; the next `get` learns the new process's token.
   */
  private replaced = false
  /**
   * Writes that create a document and whose outcome is unknown (no response arrived), by what
   * they asked for. They are not guarded by a revision, so a retry reuses the request ID and
   * the API replays the document it already committed instead of creating a second one.
   */
  private unconfirmed = new Map<string, Unconfirmed>()
  /** Saves whose outcome is unknown, by document and revision. */
  private unconfirmedSaves = new Map<string, Unconfirmed>()
  private unconfirmedImports = new Map<string, Unconfirmed>()
  private readonly restartListeners = new Set<() => void>()

  /** Local transport epoch; a new token alone does not rebind an older snapshot. */
  get serviceGeneration() { return this.generation }

  /** Whether this snapshot came from an HTTP attempt in this client's current process. */
  isCurrentSnapshot(response: DocumentResponse): boolean {
    const origin = snapshotOrigins.get(response.document)
    return origin?.client === this && origin.generation === this.generation
  }

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

  private static sessionOf(value: unknown) {
    if (!isRecord(value) || typeof value.header !== 'string' || typeof value.token !== 'string') {
      throw new ApiError('incompatible_response', 'The local service returned an invalid session.', 200)
    }
    return { header: value.header, token: value.token }
  }

  private notifyRestart() {
    this.generation += 1
    for (const listener of this.restartListeners) listener()
  }

  private async session() {
    this.token ??= this.request('GET', '/api/v1/session').then((value) => DocumentClient.sessionOf(value))
    try {
      return await this.token
    } catch (error) {
      this.token = null
      throw error
    }
  }

  private async request(method: 'GET' | 'POST' | 'PUT', path: string, body?: unknown, headers: Record<string, string> = {}, generation = this.generation) {
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
        throw new ApiError(value.code, value.message, response.status, {
          currentRevision: isCount(value.current_revision) ? value.current_revision : undefined,
          commandIndex: isCount(value.command_index) ? value.command_index : undefined,
          reason: isStorageReason(value.reason) ? value.reason : undefined,
          stateReason:
            (value.code === 'invalid_command' || value.code === 'invalid_request') && typeof value.reason === 'string'
              ? value.reason
              : undefined,
        })
      }
      // A gateway status without a v1 body comes from the proxy in front of the service (Vite's
      // answers 502 when the connection drops): the service's own reply never arrived.
      if (GATEWAY_STATUSES.has(response.status)) {
        throw new ApiError('unreachable', `The local service did not answer (HTTP ${response.status} from the proxy).`, 0)
      }
      throw new ApiError('http_error', `The local service returned HTTP ${response.status}.`, response.status)
    }
    if (isRecord(value)) {
      const result = isRecord(value.result) ? value.result : null
      const snapshot = result && isRecord(result.snapshot) ? result.snapshot : null
      for (const container of [value, result, snapshot]) {
        if (container && isRecord(container.document)) {
          snapshotOrigins.set(container.document, { client: this, generation })
        }
      }
    }
    return value
  }

  /**
   * A restarted API issues a new token and has lost its working copies, so revisions this
   * page has seen may since name different states. A first create attempt is retried with the
   * new token; an edit to an open document is not, and asks for a reload instead.
   * `sending` is called with the token each attempt is about to be sent with, and may throw
   * to stop it.
   */
  private async write(
    path: string,
    body: Record<string, unknown>,
    onRestart: 'retry' | 'reload',
    sending?: (token: string) => void,
    method: 'POST' | 'PUT' = 'POST',
  ) {
    for (let attempt = 0; ; attempt += 1) {
      const generation = this.generation
      const { header, token } = await this.session()
      sending?.(token)
      try {
        return await this.request(method, path, body, { [header]: token }, generation)
      } catch (error) {
        if (!(error instanceof ApiError && error.code === 'unauthorized')) throw error
        this.token = null
        this.replaced = false
        this.notifyRestart()
        if (onRestart === 'retry' && attempt === 0) continue
        if (onRestart === 'reload') throw restarted(error.status)
        throw error
      }
    }
  }

  private static document(value: unknown): DocumentResponse {
    return documentResponse(value)
  }

  /** Freeze a bounded ring insertion; outcome checks reuse its exact body and process. */
  createRingInsertion(destination: DocumentView, instance: string, center: Position, selectionBefore: SelectionView, size: RingSize = 6, pattern: RingPattern = 'single'): RingInsertionAttempt {
    return this.createRingAttempt(destination, instance, selectionBefore, size, { kind: 'independent', center }, pattern)
  }

  /** Freeze the target, direction, size and pattern of a ring attached by a single bond. */
  createRingAttachment(destination: DocumentView, instance: string, atom: string, direction: { dx: number; dy: number }, selectionBefore: SelectionView, size: RingSize = 6, pattern: RingPattern = 'single'): RingInsertionAttempt {
    return this.createRingAttempt(destination, instance, selectionBefore, size, { kind: 'attach', atom, direction }, pattern)
  }

  private createRingAttempt(destination: DocumentView, instance: string, selectionBefore: SelectionView, size: RingSize, placement: RingPlacement, pattern: RingPattern = 'single'): RingInsertionAttempt {
    const before = structuredClone(withDocumentStates(destination))
    if (before.scene?.instances.length !== 1 || before.scene.instances[0].id !== instance) {
      throw new ApiError('ring_destination_invalid', 'Ring insertion requires one existing placement.', 0)
    }
    if (size !== 5 && size !== 6) throw new ApiError('ring_size_invalid', 'Ring size must be 5 or 6.', 0)
    if ((pattern !== 'single' && pattern !== 'kekule') || (pattern === 'kekule' && size !== 6)) {
      throw new ApiError('ring_pattern_invalid', 'Kekulé pattern requires a six-carbon ring.', 0)
    }
    let point: Position
    let unit: { dx: number; dy: number } | null = null
    let target: string | null = null
    let command: Command
    if (placement.kind === 'independent') {
      point = { ...placement.center }
      if (!Number.isFinite(point.x) || !Number.isFinite(point.y)) {
        throw new ApiError('ring_destination_invalid', 'Ring insertion requires a finite center.', 0)
      }
      command = { op: 'insert_ring', instance, center: point, ...(size === 6 ? {} : { size }), ...(pattern === 'single' ? {} : { pattern }) }
    } else {
      const source = before.atoms.find((atom) => atom.id === placement.atom)
      if (!source) throw new ApiError('ring_destination_invalid', 'The attachment atom no longer exists.', 0)
      target = source.id
      point = { ...source.position }
      const direction = { ...placement.direction }
      const scale = Math.max(Math.abs(direction.dx), Math.abs(direction.dy))
      if (!Number.isFinite(direction.dx) || !Number.isFinite(direction.dy) || scale === 0) {
        throw new ApiError('ring_direction_invalid', 'Ring direction must be finite and nonzero.', 0)
      }
      const dx = direction.dx / scale
      const dy = direction.dy / scale
      const norm = Math.hypot(dx, dy)
      unit = { dx: dx / norm, dy: dy / norm }
      command = { op: 'attach_ring', instance, atom: source.id, direction, ...(size === 6 ? {} : { size }), ...(pattern === 'single' ? {} : { pattern }) }
    }
    const selection = clipboardSelection(selectionBefore)
    return this.clipboardAttempt('ring', `${documentPath(before.id)}/transactions`, {
      expected_revision: before.revision,
      commands: [command],
      selection_before: selection,
    }, (value) => {
      const snapshot = DocumentClient.document(value)
      const after = snapshot.document
      const oldAtoms = new Set(before.atoms.map((atom) => atom.id))
      const oldBonds = new Set(before.bonds.map((bond) => bond.id))
      const atoms = after.atoms.filter((atom) => !oldAtoms.has(atom.id))
      const bonds = after.bonds.filter((bond) => !oldBonds.has(bond.id))
      // Scene captures follow paint order; the service returns ID-ordered sets.
      const selected = after.selection
      if (after.revision !== before.revision + 1 || atoms.length !== size || bonds.length !== size + (target ? 1 : 0) ||
          !selected || !sameClipboardIds(selected.objects, selection.objects) ||
          !sameClipboardIds(selected.atoms, selection.atoms) || !sameClipboardIds(selected.bonds, selection.bonds)) throw clipboardIncompatible()
      const length = settingsOf(before).bond_length_pt
      const height = length * (Math.sqrt(3) / 2)
      const radius = length / (2 * Math.sin(Math.PI / 5))
      const offsets = size === 6
        ? [[length, 0], [length / 2, height], [-length / 2, height],
          [-length, 0], [-length / 2, -height], [length / 2, -height]]
        : Array.from({ length: 5 }, (_, index) => [radius * Math.cos(index * 2 * Math.PI / 5),
          radius * Math.sin(index * 2 * Math.PI / 5)])
      for (const [index, atom] of atoms.entries()) {
        const [x, y] = offsets[index]
        const ringRadius = offsets[0][0]
        const axial = length + (ringRadius - x)
        const expected = unit
          ? { x: point.x + axial * unit.dx + y * unit.dy, y: point.y + axial * unit.dy - y * unit.dx }
          : { x: point.x + x, y: point.y + y }
        const tolerance = 1e-10
        if (atom.element !== 'C' || atom.kind !== 'element' || atom.query !== null || atom.charge !== 0 ||
            atom.isotope !== null || atom.hydrogens.mode !== 'inferred' || atom.radical !== 'none' ||
            atom.attachment_points.length !== 0 || Math.abs(atom.position.x - expected.x) > tolerance ||
            Math.abs(atom.position.y - expected.y) > tolerance || bonds[index].begin !== atom.id ||
            bonds[index].end !== atoms[(index + 1) % size].id || bonds[index].order !== (pattern === 'kekule' && index % 2 === 0 ? 'double' : 'single') ||
            bonds[index].query !== null || bonds[index].mark !== null) throw clipboardIncompatible()
      }
      if (target) {
        const link = bonds[size]
        if (link.begin !== target || link.end !== atoms[0].id || link.order !== 'single' || link.query !== null || link.mark !== null) throw clipboardIncompatible()
      }
      const originalStereo = stereoOf(before)
      const touchedBonds = new Set(before.bonds.filter((bond) => bond.begin === target || bond.end === target).map((bond) => bond.id))
      const removedCenters = originalStereo.atoms.filter((record) => record.atom === target)
      const removedBonds = originalStereo.bonds.filter((record) => touchedBonds.has(record.bond))
      const groups = originalStereo.groups ?? []
      const affectedGroup = groups.find((group) => group.members.includes(target ?? ''))
      const expectedStereo = {
        atoms: originalStereo.atoms.filter((record) => record.atom !== target),
        bonds: originalStereo.bonds.filter((record) => !touchedBonds.has(record.bond)),
        groups: groups.map((group) => ({ ...group, members: group.members.filter((atom) => atom !== target) }))
          .filter((group) => group.members.length > 0),
      }
      if (!sameJson(stereoOf(after), expectedStereo)) throw clipboardIncompatible()
      const expectedNotes = [
        ...removedCenters.map((record) => `stereo_configuration:atoms[${record.atom}]`),
        ...removedBonds.map((record) => `stereo_configuration:bonds[${record.bond}]`),
        ...(affectedGroup ? [`stereo_group_member:atoms[${target}]`,
          ...(affectedGroup.members.length === 1 ? [`stereo_group:groups[${affectedGroup.id}]`] : [])] : []),
      ]
      const notes = snapshot.notes ?? []
      const noteKeys = notes.map((note) => `${note.feature}:${note.source_path}`)
      if (notes.length !== expectedNotes.length || new Set(noteKeys).size !== noteKeys.length ||
          notes.some((note) => note.disposition !== 'dropped' || note.semantic_impact !== 'stereo') ||
          expectedNotes.some((key) => !noteKeys.includes(key))) throw clipboardIncompatible()
      const fresh = new Set(atoms.map((atom) => atom.id))
      const retained = { ...after, revision: before.revision, selection: before.selection, stereo: before.stereo,
        atoms: after.atoms.filter((atom) => oldAtoms.has(atom.id)),
        bonds: after.bonds.filter((bond) => oldBonds.has(bond.id)),
        scene: after.scene && { ...after.scene, instances: after.scene.instances.map((placement) => ({
          ...placement, positions: placement.positions.filter((position) => !fresh.has(position.atom)),
        })) },
      }
      if (!sameJson(retained, before)) throw clipboardIncompatible()
      return snapshot
    })
  }

  /** Copies an exact structural selection without changing its source document. */
  async clipboardCopy(id: string, selection: SelectionView, instance: string): Promise<NativeClipboardCopy> {
    const requested = clipboardSelection(selection)
    const request = requestId()
    const { result, revision, losses } = clipboardReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.clipboard_copy',
      parameters: { document_id: id, selection: requested, instance },
    }, 'reload'), request, 'document.clipboard_copy')
    if (!isRecord(result.snapshot) || result.instance !== instance || !isSelection(result.copied) ||
        result.copied.objects.length !== 0 || typeof result.payload !== 'string' || !clipboardTextFits(result.payload) ||
        !isStringArray(result.boundary_bonds) || losses.length !== 0 || requested.objects.length !== 0) throw clipboardIncompatible()
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
    const copied = result.copied
    if (snapshot.document.id !== id || snapshot.document.revision !== revision || snapshot.notes?.length ||
        !snapshot.document.scene?.instances.some((placement) => placement.id === instance) ||
        !copied.atoms.every((atom) => clipboardId(atom, 'a')) || !copied.bonds.every((bond) => clipboardId(bond, 'b')) ||
        !result.boundary_bonds.every((bond) => clipboardId(bond, 'b'))) throw clipboardIncompatible()
    const atoms = new Set(requested.atoms)
    for (const id of requested.bonds) {
      const bond = snapshot.document.bonds.find((bond) => bond.id === id)
      if (!bond) throw clipboardIncompatible()
      atoms.add(bond.begin)
      atoms.add(bond.end)
    }
    const induced = snapshot.document.bonds.filter((bond) => atoms.has(bond.begin) && atoms.has(bond.end)).map((bond) => bond.id)
    const boundary = snapshot.document.bonds.filter((bond) => atoms.has(bond.begin) !== atoms.has(bond.end)).map((bond) => bond.id)
    const payload = clipboardPayloadIds(result.payload)
    if (!sameClipboardIds(copied.atoms, [...atoms]) || !copied.atoms.every((id) => snapshot.document.atoms.some((atom) => atom.id === id)) ||
        !sameClipboardIds(copied.bonds, induced) || !sameClipboardIds(result.boundary_bonds, boundary) ||
        !sameClipboardIds(payload.atoms, copied.atoms) || !sameClipboardIds(payload.bonds.map((bond) => bond.id), copied.bonds)) throw clipboardIncompatible()
    return { snapshot, instance, copied, boundaryBonds: result.boundary_bonds, payload: result.payload }
  }

  /** Copies only the captured graphic/group forest; mixed graph targets are never filtered out. */
  async clipboardSceneCopy(id: string, selection: SelectionView): Promise<NativeSceneClipboardCopy> {
    const requested = clipboardSelection(selection)
    const request = requestId()
    const { result, revision, losses } = clipboardReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.clipboard_scene_copy',
      parameters: { document_id: id, selection: requested },
    }, 'reload'), request, 'document.clipboard_scene_copy')
    if (!isRecord(result.snapshot) || !isSelection(result.copied) || !isStringArray(result.roots) ||
        typeof result.payload !== 'string' || !clipboardTextFits(result.payload) || losses.length !== 0) throw clipboardIncompatible()
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
    if (snapshot.document.id !== id || snapshot.document.revision !== revision || snapshot.notes?.length ||
        !sceneClipboardCopyMatches(result.payload, result.copied, result.roots, requested, snapshot.document, settingsOf(snapshot.document))) {
      throw clipboardIncompatible()
    }
    return { snapshot, copied: result.copied, roots: result.roots, payload: result.payload }
  }

  /** Freeze one clipboard mutation body, process generation and request ID for every outcome check. */
  private clipboardAttempt<T>(
    kind: 'paste' | 'cut' | 'mol_import' | 'scene_paste' | 'scene_cut' | 'smiles_import' | 'ring',
    path: string,
    body: Record<string, unknown>,
    parse: (value: unknown, request: string) => T,
  ): { send(): Promise<T> } {
    const action = kind === 'ring' ? 'ring insertion' : kind === 'mol_import' ? 'MOL import' : kind === 'smiles_import' ? 'SMILES import' : kind === 'scene_paste' ? 'graphics paste' : kind === 'scene_cut' ? 'graphics Cut' : kind
    const request = requestId()
    const preparedSession = this.token
    let sentTo: string | undefined
    let uncertain = false
    let restartedAttempt = false
    let settled: T | undefined
    let refused: ApiError | undefined
    let inFlight: Promise<T> | undefined
    const sendOnce = async (): Promise<T> => {
      let askedAgain = false
      for (let checks = 0; ; ) {
        try {
          const value = await this.write(path, { ...body, request_id: request }, 'reload', (token) => {
            // A different request can discover a restart while session() is awaited,
            // before this attempt has ever sent a mutation and established sentTo.
            if (preparedSession !== null && preparedSession !== this.token) throw restarted(401)
            if (sentTo !== undefined && sentTo !== token) throw restarted(401)
            sentTo = token
          })
          const parsed = parse(value, request)
          uncertain = false
          settled = parsed
          return parsed
        } catch (failure) {
          if (!(failure instanceof ApiError)) throw failure
          if (failure instanceof ConsentRequired) {
            // A validated consent refusal confirms that this immutable request did
            // not commit, even when an earlier transport reply was lost. Retain
            // the refusal so accepting losses requires a new explicit attempt.
            uncertain = false
            refused = failure
            throw failure
          }
          if (failure.code === 'service_restarted') {
            restartedAttempt = true
            throw new ApiError(`clipboard_${kind}_restarted`, `The service restarted before this ${action} was confirmed. Reload the drawing to reconcile it; this attempt will not be resent.`, failure.status)
          }
          if (outcomeOpen(failure) || failure.code === 'incompatible_response') uncertain = sentTo !== undefined
          if (failure.status === 0 && !askedAgain) { askedAgain = true; continue }
          if (failure.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
            checks += 1
            await pause(OUTCOME_CHECK_INTERVAL_MS)
            continue
          }
          // A definitive revision refusal proves this request is no longer running, but
          // its earlier commit may have outlived the replay record: reread, never repeat.
          if (uncertain && failure.code === 'revision_conflict') {
            refused = new ApiError(`clipboard_${kind}_reconcile`, `The original ${action} can no longer be confirmed by replay. It may already have completed. Reload the drawing before making another change.`, failure.status)
            throw refused
          }
          if (uncertain) throw new ApiError(`clipboard_${kind}_unconfirmed`, `The service has not confirmed this ${action}. Check again asks about the same attempt without repeating it.`, failure.status)
          refused = failure
          throw failure
        }
      }
    }
    return {
      send: () => {
        if (settled !== undefined) return Promise.resolve(settled)
        if (restartedAttempt) return Promise.reject(new ApiError(`clipboard_${kind}_restarted`, 'The service restarted; reload before another editor mutation.', 401))
        if (refused) return Promise.reject(refused)
        inFlight ??= sendOnce().finally(() => { inFlight = undefined })
        return inFlight
      },
    }
  }

  /** Validate native file bytes without reading/writing the OS clipboard or changing a drawing. */
  async inspectTemplate(payload: string): Promise<TemplateInspection> {
    if (!clipboardTextFits(payload)) throw new ApiError('template_byte_limit', 'Template files must be at most 64 KiB.', 0)
    const request = requestId()
    const { result, revision, losses } = operationDataReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.template_inspect', parameters: { payload },
    }, 'reload'), request, 'document.template_inspect')
    if (revision !== null || result.payload !== payload || losses.length !== 0 || !Array.isArray(result.atoms) || result.atoms.length === 0) throw clipboardIncompatible()
    const atoms: TemplateInspection['atoms'] = []
    for (const atom of result.atoms) {
      if (!isRecord(atom) || !clipboardId(atom.id, 'a') || typeof atom.element !== 'string' ||
          typeof atom.x !== 'number' || !Number.isFinite(atom.x) || typeof atom.y !== 'number' || !Number.isFinite(atom.y) || typeof atom.supported !== 'boolean') throw clipboardIncompatible()
      atoms.push({ id: atom.id, element: atom.element, x: atom.x, y: atom.y, supported: atom.supported })
    }
    if (new Set(atoms.map((atom) => atom.id)).size !== atoms.length || !sameClipboardIds(atoms.map((atom) => atom.id), clipboardPayloadIds(payload).atoms)) throw clipboardIncompatible()
    return { payload, atoms }
  }

  private replacementMapping(before: DocumentView, result: Record<string, unknown>, input: TemplateReplacementParameters, preview: DocumentResponse) {
    const target = before.atoms.find((atom) => atom.id === input.atom)
    const incident = before.bonds.filter((bond) => bond.begin === input.atom || bond.end === input.atom)
    if (!target || incident.length !== 1 || incident[0].order !== 'single' || incident[0].query !== null ||
        result.boundary !== incident[0].id || !isRecord(result.mapping)) throw clipboardIncompatible()
    const source = clipboardPayloadIds(input.payload)
    const oldAtoms = new Set(before.atoms.map((atom) => atom.id))
    const oldBonds = new Set(before.bonds.map((bond) => bond.id))
    const atoms = clipboardMappings(result.mapping.atoms, source.atoms, 'a', [input.atom,
      ...preview.document.atoms.filter((atom) => !oldAtoms.has(atom.id)).map((atom) => atom.id)])
    const bonds = clipboardMappings(result.mapping.bonds, source.bonds.map((bond) => bond.id), 'b',
      preview.document.bonds.filter((bond) => !oldBonds.has(bond.id)).map((bond) => bond.id))
    if (!atoms.some((entry) => entry.source === input.source_anchor && entry.target === input.atom)) throw clipboardIncompatible()
    return { mapping: { atoms, bonds }, boundary: incident[0].id }
  }

  /** Bind one existing terminal identity in a read-only preview; no IDs are reserved. */
  async previewTemplateReplacement(destination: DocumentView, parameters: TemplateReplacementParameters): Promise<TemplateReplacementOffer> {
    const input = structuredClone(parameters)
    const generation = this.generation
    const before = structuredClone(withDocumentStates(destination))
    if (before.id !== input.document_id || before.revision !== input.expected_revision || before.scene?.instances.length !== 1 ||
        before.scene.instances[0].id !== input.instance || !clipboardTextFits(input.payload)) throw clipboardIncompatible()
    const request = requestId()
    const { result, revision, losses } = clipboardReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.template_replace_plan', parameters: input,
    }, 'reload'), request, 'document.template_replace_plan')
    if (generation !== this.generation || !isRecord(result.source) || !isRecord(result.snapshot)) throw clipboardIncompatible()
    const source = DocumentClient.document({ api_version: API_VERSION, ...result.source })
    const preview = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
    snapshotOrigins.delete(preview.document)
    if (!sameJson(withDocumentStates(source.document), before) || revision !== input.expected_revision || losses.length || preview.notes?.length) throw clipboardIncompatible()
    const checked = this.replacementMapping(before, result, input, preview)
    this.validateTemplatePlan(before, preview, input, checked.mapping, checked.boundary)
    snapshotOrigins.set(source.document, { client: this, generation })
    return { parameters: input, source, preview, ...checked }
  }

  /** A replacement confirmation replays only the original body and exact bound preview. */
  createTemplateReplacement(offer: TemplateReplacementOffer): NativeClipboardPasteAttempt {
    if (!this.isCurrentSnapshot(offer.source)) throw restarted(0)
    const frozen = structuredClone(offer)
    return this.clipboardAttempt('paste', '/api/v1/operations', {
      contract: 1, profile: 'preserve-semantics', operation: 'document.template_replace', parameters: frozen.parameters,
    }, (value, request) => {
      const { result, revision, losses } = clipboardReply(value, request, 'document.template_replace')
      if (!isRecord(result.snapshot)) throw clipboardIncompatible()
      const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
      const checked = this.replacementMapping(frozen.source.document, result, frozen.parameters, snapshot)
      if (revision !== frozen.preview.document.revision || !sameJson(snapshot.document, frozen.preview.document) || losses.length || snapshot.notes?.length ||
          !sameJson(checked.mapping, frozen.mapping) || checked.boundary !== frozen.boundary) throw clipboardIncompatible()
      return { snapshot, mapping: checked.mapping }
    })
  }

  /** Freeze a full read-only preview; it reserves no identifiers and cannot be adopted as a live snapshot. */
  async previewTemplateAttachment(destination: DocumentView, parameters: TemplateParameters): Promise<TemplateAttachmentOffer> {
    const input = structuredClone(parameters)
    const generation = this.generation
    const before = structuredClone(withDocumentStates(destination))
    if (before.id !== input.document_id || before.revision !== input.expected_revision || before.scene?.instances.length !== 1 || before.scene.instances[0].id !== input.instance || !before.atoms.some((atom) => atom.id === input.atom)) {
      throw new ApiError('template_destination_invalid', 'Choose an existing target in the only destination placement.', 0)
    }
    if (!clipboardTextFits(input.payload)) throw new ApiError('template_byte_limit', 'Template files must be at most 64 KiB.', 0)
    if (!Number.isFinite(input.direction.dx) || !Number.isFinite(input.direction.dy) || (input.direction.dx === 0 && input.direction.dy === 0)) {
      throw new ApiError('template_direction_invalid', 'Attachment direction must be finite and nonzero.', 0)
    }
    const request = requestId()
    const { result, revision, losses } = clipboardReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.template_plan', parameters: input,
    }, 'reload'), request, 'document.template_plan')
    if (generation !== this.generation || !isRecord(result.source) || !isRecord(result.snapshot)) throw clipboardIncompatible()
    const source = DocumentClient.document({ api_version: API_VERSION, ...result.source })
    const preview = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
    snapshotOrigins.delete(preview.document)
    if (!sameJson(withDocumentStates(source.document), before) || source.document.id !== input.document_id || source.document.revision !== input.expected_revision || revision !== input.expected_revision ||
        preview.document.id !== input.document_id || preview.document.revision !== input.expected_revision + 1 || !sameJson(losses, preview.notes ?? [])) throw clipboardIncompatible()
    const checked = this.templateMapping(result, input, preview)
    this.validateTemplatePlan(before, preview, input, checked.mapping, checked.connection)
    snapshotOrigins.set(source.document, { client: this, generation })
    return { parameters: input, source, preview, ...checked }
  }

  private templateMapping(result: Record<string, unknown>, input: TemplateParameters, snapshot: DocumentResponse) {
    const selection = snapshot.document.selection
    if (!selection || selection.objects.length !== 0 || !isRecord(result.mapping) || typeof result.connection !== 'string') throw clipboardIncompatible()
    const source = clipboardPayloadIds(input.payload)
    const atoms = clipboardMappings(result.mapping.atoms, source.atoms, 'a', selection.atoms)
    const bonds = clipboardMappings(result.mapping.bonds, source.bonds.map((bond) => bond.id), 'b', selection.bonds)
    const mapped = new Map(atoms.map((entry) => [entry.source, entry.target]))
    const mappedBonds = new Map(bonds.map((entry) => [entry.source, entry.target]))
    const connector = snapshot.document.bonds.find((bond) => bond.id === result.connection)
    if (!connector || connector.begin !== input.atom || connector.end !== mapped.get(input.source_anchor) || connector.order !== 'single' || connector.mark !== null || connector.query !== null ||
        bonds.some((entry) => entry.target === connector.id) || source.bonds.some((bond) => {
          const actual = snapshot.document.bonds.find((entry) => entry.id === mappedBonds.get(bond.id))
          return !actual || actual.begin !== mapped.get(bond.begin) || actual.end !== mapped.get(bond.end)
        })) throw clipboardIncompatible()
    return { mapping: { atoms, bonds }, connection: result.connection }
  }

  private validateTemplatePlan(before: DocumentView, preview: DocumentResponse, input: TemplateParameters | TemplateReplacementParameters, mapping: NativeClipboardPaste['mapping'], connection: string) {
    const after = preview.document
    const replacing = !('direction' in input)
    const freshAtoms = new Set(mapping.atoms.filter((entry) => !replacing || entry.source !== input.source_anchor).map((entry) => entry.target))
    const freshBonds = new Set([...mapping.bonds.map((entry) => entry.target), ...(replacing ? [] : [connection])])
    if (after.atoms.length !== before.atoms.length + freshAtoms.size || after.bonds.length !== before.bonds.length + freshBonds.size ||
        before.atoms.some((atom) => freshAtoms.has(atom.id)) || before.bonds.some((bond) => freshBonds.has(bond.id))) throw clipboardIncompatible()
    const original = stereoOf(before)
    const touchedBonds = new Set(before.bonds.filter((bond) => bond.begin === input.atom || bond.end === input.atom).map((bond) => bond.id))
    const expectedOld = replacing ? original : { atoms: original.atoms.filter((record) => record.atom !== input.atom),
      bonds: original.bonds.filter((record) => !touchedBonds.has(record.bond)),
      groups: (original.groups ?? []).map((group) => ({ ...group, members: group.members.filter((atom) => atom !== input.atom) })).filter((group) => group.members.length > 0) }
    const actual = stereoOf(after)
    if (!sameJson(actual.atoms.filter((record) => !freshAtoms.has(record.atom)), expectedOld.atoms) ||
        !sameJson(actual.bonds.filter((record) => !freshBonds.has(record.bond)), expectedOld.bonds) || !sameJson(actual.groups ?? [], expectedOld.groups)) throw clipboardIncompatible()
    const group = original.groups?.find((group) => group.members.includes(input.atom))
    const expectedNotes = replacing ? [] : [...original.atoms.filter((record) => record.atom === input.atom).map((record) => `stereo_configuration:atoms[${record.atom}]`),
      ...original.bonds.filter((record) => touchedBonds.has(record.bond)).map((record) => `stereo_configuration:bonds[${record.bond}]`),
      ...(group ? [`stereo_group_member:atoms[${input.atom}]`, ...(group.members.length === 1 ? [`stereo_group:groups[${group.id}]`] : [])] : [])]
    const notes = preview.notes ?? []
    const keys = notes.map((note) => `${note.feature}:${note.source_path}`)
    if (notes.length !== expectedNotes.length || new Set(keys).size !== keys.length || expectedNotes.some((key) => !keys.includes(key)) || notes.some((note) => note.disposition !== 'dropped')) throw clipboardIncompatible()
    const raw: unknown = JSON.parse(input.payload)
    if (!isRecord(raw) || !Array.isArray(raw.atoms) || !Array.isArray(raw.bonds) || !Array.isArray(raw.stereo)) throw clipboardIncompatible()
    const atomIds = new Map(mapping.atoms.map((entry) => [entry.source, entry.target]))
    const bondIds = new Map(mapping.bonds.map((entry) => [entry.source, entry.target]))
    const sourceAtoms: unknown[] = raw.atoms
    const sourceBonds: unknown[] = raw.bonds
    const sourceStereo: unknown[] = raw.stereo
    const sourceAnchor = sourceAtoms.find((atom: unknown) => isRecord(atom) && atom.id === input.source_anchor)
    const target = before.atoms.find((atom) => atom.id === input.atom)
    if (!isRecord(sourceAnchor) || !isRecord(sourceAnchor.position) || !isFiniteNumber(sourceAnchor.position.x) || !isFiniteNumber(sourceAnchor.position.y) || !target) throw clipboardIncompatible()
    let offset = { x: target.position.x - sourceAnchor.position.x, y: target.position.y - sourceAnchor.position.y }
    if ('direction' in input) {
      const scale = Math.max(Math.abs(input.direction.dx), Math.abs(input.direction.dy))
      if (!Number.isFinite(scale) || scale === 0) throw clipboardIncompatible()
      const dx = input.direction.dx / scale
      const dy = input.direction.dy / scale
      const norm = Math.hypot(dx, dy)
      const length = settingsOf(before).bond_length_pt
      offset = { x: target.position.x + length * dx / norm - sourceAnchor.position.x, y: target.position.y + length * dy / norm - sourceAnchor.position.y }
    }
    for (const atom of sourceAtoms) {
      if (!isRecord(atom) || typeof atom.id !== 'string' || !isRecord(atom.kind) || !isRecord(atom.position) || !isFiniteNumber(atom.position.x) || !isFiniteNumber(atom.position.y) || Math.abs(atom.position.x) > 100_000 || Math.abs(atom.position.y) > 100_000) throw clipboardIncompatible()
      const copiedId = atomIds.get(atom.id)
      const copied = after.atoms.find((entry) => entry.id === copiedId)
      const position = replacing && atom.id === input.source_anchor ? target.position : { x: atom.position.x + offset.x, y: atom.position.y + offset.y }
      if (!copied || copied.kind !== atom.kind.type || (atom.kind.type === 'element' ? copied.element !== atom.kind.symbol || copied.query !== null : !sameJson(copied.query, atom.kind.query)) ||
          Math.abs(copied.position.x - position.x) > 1e-10 || Math.abs(copied.position.y - position.y) > 1e-10 ||
          !sameJson([copied.charge, copied.isotope, copied.hydrogens, copied.radical, copied.attachment_points], [atom.charge, atom.isotope, atom.hydrogens, atom.radical, atom.attachment_points])) throw clipboardIncompatible()
    }
    for (const bond of sourceBonds) {
      if (!isRecord(bond) || typeof bond.id !== 'string') throw clipboardIncompatible()
      const copiedId = bondIds.get(bond.id)
      const copied = after.bonds.find((entry) => entry.id === copiedId)
      if (!copied || copied.begin !== atomIds.get(String(bond.begin)) || copied.end !== atomIds.get(String(bond.end)) || !sameJson([copied.order, copied.query, copied.mark], [bond.order, bond.query, bond.mark])) throw clipboardIncompatible()
    }
    const centers: string[] = []
    const doubles: string[] = []
    for (const record of sourceStereo) {
      if (!isRecord(record)) throw clipboardIncompatible()
      if (record.type === 'atom' && typeof record.atom === 'string') {
        const id = atomIds.get(record.atom)
        if (!id || record.atom === input.source_anchor) throw clipboardIncompatible()
        const configuration = record.configuration
        if (!isRecord(configuration) || !Array.isArray(configuration.neighbors) || configuration.neighbors.length !== 4) throw clipboardIncompatible()
        const slots: string[] = []
        for (const slot of configuration.neighbors as unknown[]) {
          if (typeof slot !== 'string') throw clipboardIncompatible()
          const mapped = atomIds.get(slot) ?? (slot === 'implicit_hydrogen' || slot === 'lone_pair' ? slot : undefined)
          if (!mapped) throw clipboardIncompatible()
          slots.push(mapped)
        }
        // Native IDs are relabeled: canonical slot reordering flips the stated turn only for odd permutations.
        const rank = (slot: string) => slot === 'implicit_hydrogen' ? 0x1_0000_0000 : slot === 'lone_pair' ? 0x1_0000_0001 : Number(slot.slice(1))
        let inversions = 0
        for (let first = 0; first < slots.length; first += 1) {
          for (let second = first + 1; second < slots.length; second += 1) {
            if (rank(slots[first]) > rank(slots[second])) inversions += 1
          }
        }
        const neighbors = [...slots].sort((left, right) => rank(left) - rank(right))
        let expected: unknown
        if (configuration.kind === 'unknown') expected = { kind: 'unknown', neighbors }
        else if (configuration.kind === 'tetrahedral' && (configuration.turn === 'clockwise' || configuration.turn === 'anticlockwise')) {
          const opposite = configuration.turn === 'clockwise' ? 'anticlockwise' : 'clockwise'
          expected = { kind: 'tetrahedral', neighbors, turn: inversions % 2 === 0 ? configuration.turn : opposite }
        } else throw clipboardIncompatible()
        if (!sameJson(actual.atoms.find((center) => center.atom === id)?.configuration, expected)) throw clipboardIncompatible()
        centers.push(id)
      } else if (record.type === 'bond' && typeof record.bond === 'string') {
        const id = bondIds.get(record.bond)
        const bond = sourceBonds.find((bond: unknown) => isRecord(bond) && bond.id === record.bond)
        if (!id || !isRecord(bond) || bond.begin === input.source_anchor || bond.end === input.source_anchor) throw clipboardIncompatible()
        const configuration = record.configuration
        if (!isRecord(configuration)) throw clipboardIncompatible()
        let expected: unknown
        if (configuration.kind === 'unknown') expected = { kind: 'unknown' }
        else if (configuration.kind === 'relative' && typeof bond.begin === 'string' && typeof bond.end === 'string' &&
            typeof configuration.begin_neighbor === 'string' && typeof configuration.end_neighbor === 'string' &&
            (configuration.relation === 'same_side' || configuration.relation === 'opposite_sides')) {
          const mappedBegin = atomIds.get(bond.begin)
          const mappedEnd = atomIds.get(bond.end)
          if (!mappedBegin || !mappedEnd) throw clipboardIncompatible()
          const reference = (own: string, partner: string) => {
            const others = after.bonds.flatMap((edge) => edge.begin === own && edge.end !== partner ? [edge.end]
              : edge.end === own && edge.begin !== partner ? [edge.begin] : [])
            others.sort((left, right) => Number(left.slice(1)) - Number(right.slice(1)))
            return others[0] ?? 'implicit_hydrogen'
          }
          const begin = reference(mappedBegin, mappedEnd)
          const end = reference(mappedEnd, mappedBegin)
          const oldBegin = atomIds.get(configuration.begin_neighbor) ?? (configuration.begin_neighbor === 'implicit_hydrogen' ? 'implicit_hydrogen' : undefined)
          const oldEnd = atomIds.get(configuration.end_neighbor) ?? (configuration.end_neighbor === 'implicit_hydrogen' ? 'implicit_hydrogen' : undefined)
          if (!oldBegin || !oldEnd) throw clipboardIncompatible()
          // Each changed reference names the complementary substituent position, flipping the relation.
          const flips = Number(begin !== oldBegin) + Number(end !== oldEnd)
          const opposite = configuration.relation === 'same_side' ? 'opposite_sides' : 'same_side'
          expected = { kind: 'relative', begin_neighbor: begin, end_neighbor: end, relation: flips % 2 === 0 ? configuration.relation : opposite }
        } else throw clipboardIncompatible()
        if (!sameJson(actual.bonds.find((item) => item.bond === id)?.configuration, expected)) throw clipboardIncompatible()
        doubles.push(id)
      } else throw clipboardIncompatible()
    }
    if (!sameClipboardIds(actual.atoms.filter((record) => freshAtoms.has(record.atom)).map((record) => record.atom), centers) ||
        !sameClipboardIds(actual.bonds.filter((record) => freshBonds.has(record.bond)).map((record) => record.bond), doubles)) throw clipboardIncompatible()
    const retained = { ...after, revision: before.revision, selection: before.selection, stereo: before.stereo,
      atoms: after.atoms.filter((atom) => !freshAtoms.has(atom.id)).map((atom) => replacing && atom.id === input.atom ? target : atom), bonds: after.bonds.filter((bond) => !freshBonds.has(bond.id)),
      scene: after.scene && { ...after.scene, instances: after.scene.instances.map((instance) => ({ ...instance, positions: instance.positions.filter((position) => !freshAtoms.has(position.atom)) })) } }
    if (!sameJson(retained, before)) throw clipboardIncompatible()
    if (replacing) {
      const unchanged = sameJson({ ...after, revision: before.revision, selection: before.selection }, before)
      if (after.revision !== before.revision + Number(!unchanged) || !sameJson(after.selection, unchanged ? before.selection : {
        objects: [], atoms: mapping.atoms.map((entry) => entry.target).sort((a, b) => Number(a.slice(1)) - Number(b.slice(1))),
        bonds: mapping.bonds.map((entry) => entry.target).sort((a, b) => Number(a.slice(1)) - Number(b.slice(1))),
      })) throw clipboardIncompatible()
    }
  }

  /** Retain exactly this request after a lost response; preview and later UI changes cannot retarget it. */
  createTemplateAttachment(offer: TemplateAttachmentOffer): NativeClipboardPasteAttempt {
    if (!this.isCurrentSnapshot(offer.source)) throw restarted(0)
    const frozen = structuredClone(offer)
    return this.clipboardAttempt('paste', '/api/v1/operations', {
      contract: 1, profile: 'preserve-semantics', operation: 'document.template_attach', parameters: frozen.parameters,
    }, (value, request) => {
      const { result, revision, losses } = clipboardReply(value, request, 'document.template_attach')
      if (!isRecord(result.snapshot)) throw clipboardIncompatible()
      const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
      const checked = this.templateMapping(result, frozen.parameters, snapshot)
      if (revision !== frozen.parameters.expected_revision + 1 || !sameJson(snapshot.document, frozen.preview.document) ||
          !sameJson(losses, frozen.preview.notes ?? []) || !sameJson(snapshot.notes ?? [], frozen.preview.notes ?? []) ||
          !sameJson(checked.mapping, frozen.mapping) || checked.connection !== frozen.connection) throw clipboardIncompatible()
      return { snapshot, mapping: checked.mapping }
    })
  }

  /** Freeze one paste body and request ID; every send checks only that attempt's outcome. */
  createClipboardPaste(id: string, expectedRevision: number, payload: string, offset: Position, selectionBefore: SelectionView): NativeClipboardPasteAttempt {
    if (!clipboardTextFits(payload)) throw new ApiError('invalid_command', 'Native clipboard text exceeds 64 KiB.', 400, { stateReason: 'clipboard_byte_limit' })
    const parameters = { document_id: id, expected_revision: expectedRevision, payload,
      offset: { x: offset.x, y: offset.y }, selection_before: clipboardSelection(selectionBefore) }
    return this.clipboardAttempt('paste', '/api/v1/operations', {
      contract: 1, profile: 'preserve-semantics', operation: 'document.clipboard_paste', parameters,
    }, (value, request) => {
      const { result, revision, losses } = clipboardReply(value, request, 'document.clipboard_paste')
      if (!isRecord(result.snapshot) || !isRecord(result.mapping)) throw clipboardIncompatible()
      const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
      const selection = snapshot.document.selection
      if (snapshot.document.id !== id || revision !== expectedRevision + 1 || snapshot.document.revision !== revision ||
          !selection || selection.objects.length !== 0 || !isStereoNotes(losses) || !sameJson(losses, snapshot.notes ?? [])) throw clipboardIncompatible()
      const source = clipboardPayloadIds(payload)
      const atoms = clipboardMappings(result.mapping.atoms, source.atoms, 'a', selection.atoms)
      const bonds = clipboardMappings(result.mapping.bonds, source.bonds.map((bond) => bond.id), 'b', selection.bonds)
      const mappedAtom = new Map(atoms.map((entry) => [entry.source, entry.target]))
      const mappedBond = new Map(bonds.map((entry) => [entry.source, entry.target]))
      if (atoms.some((entry) => !snapshot.document.atoms.some((atom) => atom.id === entry.target)) ||
          source.bonds.some((sourceBond) => {
            const bond = snapshot.document.bonds.find((bond) => bond.id === mappedBond.get(sourceBond.id))
            return !bond || bond.begin !== mappedAtom.get(sourceBond.begin) || bond.end !== mappedAtom.get(sourceBond.end)
          })) throw clipboardIncompatible()
      return { snapshot, mapping: { atoms, bonds } }
    })
  }

  /** Freeze exact graphics text and destination content; retries ask only about this attempt. */
  createClipboardScenePaste(
    id: string, expectedRevision: number, payload: string, offset: Position, selectionBefore: SelectionView, destination: DocumentView,
  ): NativeSceneClipboardPasteAttempt {
    if (!clipboardTextFits(payload)) throw new ApiError('invalid_command', 'Native clipboard text exceeds 64 KiB.', 400, { stateReason: 'clipboard_byte_limit' })
    if (!Number.isFinite(offset.x) || !Number.isFinite(offset.y)) {
      throw new ApiError('invalid_command', 'Graphics paste offsets must be finite numbers in points.', 400)
    }
    if (destination.id !== id || destination.revision !== expectedRevision) throw clipboardIncompatible()
    const before = structuredClone(withDocumentStates(destination))
    const parameters = { document_id: id, expected_revision: expectedRevision, payload,
      offset: { x: offset.x, y: offset.y }, selection_before: clipboardSelection(selectionBefore) }
    return this.clipboardAttempt('scene_paste', '/api/v1/operations', {
      contract: 1, profile: 'preserve-semantics', operation: 'document.clipboard_scene_paste', parameters,
    }, (value, request) => {
      const { result, revision, losses } = clipboardReply(value, request, 'document.clipboard_scene_paste')
      if (!isRecord(result.snapshot) || losses.length !== 0) throw clipboardIncompatible()
      const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot })
      if (snapshot.document.id !== id || revision !== expectedRevision + 1 || snapshot.document.revision !== revision || snapshot.notes?.length) {
        throw clipboardIncompatible()
      }
      const objects = sceneClipboardPasteMapping(payload, result.mapping, parameters.offset, before, snapshot.document, settingsOf(before))
      if (!objects) throw clipboardIncompatible()
      return { snapshot, mapping: { objects } }
    })
  }

  /** Called only after clipboard writing succeeds; deletes the exact prepared graphic roots. */
  createSceneClipboardCut(copy: NativeSceneClipboardCopy, selectionBefore: SelectionView): NativeClipboardCutAttempt {
    const frozen = structuredClone(copy)
    const selection = clipboardSelection(selectionBefore)
    const source = frozen.snapshot.document
    if (!sceneClipboardCopyMatches(frozen.payload, frozen.copied, frozen.roots, selection, source, settingsOf(source)) ||
        sceneClipboardCutHasIncoming(source, frozen.copied.objects)) {
      throw new ApiError('clipboard_scene_cut_invalid', 'The graphics Cut source is invalid or has incoming references. Prepare Cut again, or use Copy.', 0)
    }
    return this.clipboardAttempt('scene_cut', `${documentPath(source.id)}/transactions`, {
      expected_revision: source.revision,
      commands: [{ op: 'delete_objects', objects: frozen.roots, mode: 'subtree' }],
      selection_before: selection,
      selection_after: { objects: [], atoms: [], bonds: [] },
    }, (value) => {
      const snapshot = DocumentClient.document(value)
      if (snapshot.document.revision !== source.revision + 1 || snapshot.notes?.length ||
          !sceneClipboardCutMatches(source, snapshot.document, frozen.copied.objects)) throw clipboardIncompatible()
      return snapshot
    })
  }

  /** Called only after clipboard write succeeds; sends exactly one frozen atom deletion. */
  createClipboardCut(id: string, expectedRevision: number, atoms: string[], selectionBefore: SelectionView): NativeClipboardCutAttempt {
    const removed = [...atoms]
    if (removed.length === 0 || new Set(removed).size !== removed.length || !removed.every((atom) => clipboardId(atom, 'a'))) {
      throw new ApiError('invalid_command', 'A cut needs distinct existing atom identifiers.', 400)
    }
    return this.clipboardAttempt('cut', `${documentPath(id)}/transactions`, {
      expected_revision: expectedRevision,
      commands: [{ op: 'delete_atoms', atoms: removed }],
      selection_before: clipboardSelection(selectionBefore),
      selection_after: { objects: [], atoms: [], bonds: [] },
    }, (value) => {
      const snapshot = DocumentClient.document(value)
      const selection = snapshot.document.selection
      if (snapshot.document.id !== id || snapshot.document.revision !== expectedRevision + 1 || !selection ||
          selection.objects.length !== 0 || selection.atoms.length !== 0 || selection.bonds.length !== 0 ||
          snapshot.document.atoms.some((atom) => removed.includes(atom.id))) throw clipboardIncompatible()
      // document() validates every note; return them all, including normal removal of
      // copied stereo records. Eligibility and user-visible reporting belong to the hook.
      return snapshot
    })
  }

  /** Create only after the user accepts the inspected clipboard MOL preview and its losses. */
  createClipboardMolImport(input: CtImport): NativeClipboardMolImportAttempt {
    if (input.format !== 'mol' || input.record !== 1 || !isCount(input.revision) || !/^[a-f0-9]{64}$/.test(input.digest)) {
      throw new ApiError('invalid_request', 'Clipboard MOL import requires the inspected first MOL record and its consent digest.', 400)
    }
    if (new TextEncoder().encode(input.source).byteLength > MAX_CLIPBOARD_MOL_BYTES) {
      throw new ApiError('invalid_request', 'Clipboard MOL text exceeds 200 KiB.', 400)
    }
    const documentId = input.documentId
    const revision = input.revision
    const parameters = {
      document_id: documentId, expected_revision: revision, format: 'mol', record: 1,
      source: input.source, preview_digest: input.digest, accept_loss: true,
    }
    return this.clipboardAttempt('mol_import', '/api/v1/operations', {
      contract: 1, profile: 'preserve-semantics', operation: 'document.import', parameters,
    }, (value, request) => {
      if (!isRecord(value) || value.contract !== 1 || value.envelope !== 'operation_result' || value.request_id !== request ||
          value.operation !== 'document.import' || value.profile !== 'preserve-semantics' ||
          !Array.isArray(value.diagnostics) || !Array.isArray(value.loss_report)) throw clipboardIncompatible()
      if (value.outcome === 'error' || value.outcome === 'unsupported') {
        const diagnostic: unknown = value.diagnostics[0]
        if (value.result !== null || value.loss_report.length !== 0 || !isRecord(diagnostic) ||
            typeof diagnostic.code !== 'string' || typeof diagnostic.message !== 'string' || !isRecord(diagnostic.details)) throw clipboardIncompatible()
      } else if ((value.outcome !== 'ok' && value.outcome !== 'partial') || value.diagnostics.length !== 0 ||
          (value.outcome === 'ok') !== (value.loss_report.length === 0) || !value.loss_report.every(isDrawingExportLoss)) {
        throw clipboardIncompatible()
      }
      const { result } = ctReply(value, 'import')
      const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result })
      if (snapshot.document.id !== documentId || !isCount(value.document_revision) || snapshot.document.revision !== value.document_revision ||
          (snapshot.document.revision !== revision && snapshot.document.revision !== revision + 1)) throw clipboardIncompatible()
      return snapshot
    })
  }

  /** The server returns the actual inspected snapshot, not just its potentially reused revision. */
  async inspectValence(id: string): Promise<ValenceInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics',
      operation: 'chem.validate', parameters: { document_id: id },
    }, 'reload')
    if (!isRecord(value) || value.outcome !== 'ok' || typeof value.model_id !== 'string' ||
        !isRecord(value.result) || !isRecord(value.result.snapshot) || !Array.isArray(value.result.atoms)) {
      throw new ApiError('analysis_unavailable', 'The local service could not inspect the drawing.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const atoms = value.result.atoms
    if (snapshot.id !== id || snapshot.revision !== value.document_revision ||
        !atoms.every((a): a is ValenceAtom => isRecord(a) && typeof a.atom_id === 'string' &&
          (a.bond_order_sum === null || isCount(a.bond_order_sum)) && (a.hydrogens === null || isCount(a.hydrogens)) &&
          (a.status === 'valid' || a.status === 'invalid' || a.status === 'unsupported') &&
          (a.reason === null || typeof a.reason === 'string')) ||
        atoms.length !== snapshot.atoms.length || new Set(atoms.map((a) => a.atom_id)).size !== atoms.length ||
        atoms.some((a) => !snapshot.atoms.some((atom) => atom.id === a.atom_id))) {
      throw new ApiError('analysis_unavailable', 'The local service returned an incompatible inspection.', 200)
    }
    return { model: value.model_id, snapshot, atoms }
  }

  async readInterchange(source: string, format: InterchangeFormat): Promise<InterchangeChoice[]> {
    const { result } = interchangeReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.import',
      // Only the choices are used; compact bytes keep an unknown CDX property's reply a quarter the size.
      parameters: { mode: 'read', format, source, compact_bytes: true },
    }, 'reload'))
    if (!Array.isArray(result.choices) || !result.choices.every((choice): choice is InterchangeChoice =>
      isRecord(choice) && isCount(choice.page_id) && choice.page_id > 0 &&
      isCount(choice.fragment_id) && choice.fragment_id > 0 &&
      typeof choice.page_path === 'string' && typeof choice.fragment_path === 'string')) {
      throw new ApiError('incompatible_response', 'The service returned invalid page choices.', 200)
    }
    return result.choices
  }

  async previewInterchange(input: InterchangeInput): Promise<InterchangePreview> {
    const { result, losses } = interchangeReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.import',
      parameters: { mode: 'preview', ...input },
    }, 'reload'))
    const document = result.document
    if (typeof result.preview_digest !== 'string' || !/^[a-f0-9]{64}$/.test(result.preview_digest) ||
        !isRecord(document) || !Array.isArray(document.atoms) || !Array.isArray(document.bonds) ||
        !document.atoms.every((atom: unknown) => isRecord(atom) && typeof atom.element === 'string') ||
        !isInterchangeMapping(result.mapping) || result.mapping.atoms.length !== document.atoms.length ||
        result.mapping.bonds.length !== document.bonds.length) {
      throw new ApiError('incompatible_response', 'The service returned an invalid import preview.', 200)
    }
    return { digest: result.preview_digest, atomLabels: document.atoms.map((atom: { element: string }) => atom.element),
      bonds: document.bonds.length, mapping: result.mapping, losses }
  }

  /** Reviewed browser inputs use `reload` to prevent even a first-send restart retry. */
  async createInterchange(input: InterchangeInput, digest: string, onInitialRestart: 'retry' | 'reload' = 'retry'): Promise<DocumentResponse> {
    const key = `interchange:${JSON.stringify([input, digest])}`
    try {
      return await this.createOnce(
      key,
      '/api/v1/operations',
      { contract: 1, profile: 'preserve-semantics', operation: 'document.import',
        parameters: { mode: 'create', ...input, preview_digest: digest, accept_loss: true } },
      new ApiError('create_unconfirmed', 'The service restarted before confirming the imported drawing. Check Saved documents before creating another.', 401),
      async (value, replayed) => {
        const result = interchangeReply(value).result
        const created = DocumentClient.document({ api_version: API_VERSION, ...result })
        return replayed ? this.get(created.document.id) : created
      },
      onInitialRestart,
      )
    } catch (error) {
      // A structured refusal is a definitive non-commit; an invalid or lost reply may hide a commit.
      if (error instanceof ApiError && error.status === 200 && error.code !== 'incompatible_response') {
        this.unconfirmed.delete(key)
      }
      throw error
    }
  }

  /** Explicit Saved-documents reconciliation ends tracking; it never resends or rolls back creation. */
  reconcileInterchangeCreation(input: InterchangeInput, digest: string): void {
    this.unconfirmed.delete(`interchange:${JSON.stringify([input, digest])}`)
  }

  async previewCt(documentId: string, revision: number, format: CtFormat, source: string, record: number): Promise<CtPreview> {
    const reply = ctReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.import',
      parameters: { document_id: documentId, expected_revision: revision, format, source, record, preview: true },
    }, 'reload'), 'import')
    const { result } = reply
    if (typeof result.preview_digest !== 'string' || !/^[a-f0-9]{64}$/.test(result.preview_digest) ||
        result.selected_record !== record || !Array.isArray(result.records) ||
        !result.records.every(isCtSourceRecord) || !isRecord(result.snapshot) ||
        !isRecord(result.document) || result.document.id !== documentId || !Array.isArray(result.document.atoms) ||
        !Array.isArray(result.document.bonds)) {
      throw new ApiError('incompatible_response', 'The service returned an incompatible import preview.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    if (snapshot.id !== documentId || snapshot.revision !== revision) {
      throw new ApiError('incompatible_response', 'The import preview used a different drawing or revision.', 200)
    }
    const records = result.records.map((item) => ({
      title: item.title, atoms: item.atoms.length, bonds: item.bonds.length, fields: item.fields,
    }))
    return { digest: result.preview_digest, snapshot, records, losses: reply.losses }
  }

  /**
   * Commits an import. An unanswered request might have committed, so its request ID is reused
   * only against the same service, and `parse` checks what a reply says it did.
   */
  private async commitImport(
    input: { documentId: string },
    parameters: Record<string, unknown>,
    parse: (value: unknown) => { result: Record<string, unknown> },
  ): Promise<DocumentResponse> {
    const key = JSON.stringify(parameters)
    const pending = this.unconfirmedImports.get(key) ?? { requestId: requestId() }
    this.unconfirmedImports.set(key, pending)
    let sentTo: string | undefined
    try {
      const reply = parse(await this.write('/api/v1/operations', {
        contract: 1, request_id: pending.requestId, profile: 'preserve-semantics', operation: 'document.import',
        parameters,
      }, 'reload', (token) => {
        if (pending.sentTo !== undefined && pending.sentTo !== token) throw restarted(401)
        sentTo = token
      }))
      const response = DocumentClient.document({ api_version: API_VERSION, ...reply.result })
      if (response.document.id !== input.documentId) {
        throw new ApiError('incompatible_response', 'The import returned a different drawing.', 200)
      }
      this.unconfirmedImports.delete(key)
      return response
    } catch (error) {
      if (error instanceof ApiError && outcomeOpen(error) && sentTo !== undefined) {
        pending.sentTo = sentTo
        throw new ApiError(error.code, `${error.message} The import may have completed; try again to check its result.`, error.status)
      }
      this.unconfirmedImports.delete(key)
      throw error
    }
  }

  importCt(input: CtImport): Promise<DocumentResponse> {
    return this.commitImport(input, {
      document_id: input.documentId, expected_revision: input.revision, format: input.format,
      source: input.source, record: input.record, preview_digest: input.digest, accept_loss: true,
    }, (value) => ctReply(value, 'import'))
  }

  async previewSmiles(documentId: string, revision: number, source: string): Promise<SmilesPreview> {
    const reply = smilesReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.import',
      parameters: { document_id: documentId, expected_revision: revision, format: 'smiles', source, preview: true },
    }, 'reload'))
    const { result } = reply
    if (typeof result.preview_digest !== 'string' || !/^[a-f0-9]{64}$/.test(result.preview_digest) ||
        !Array.isArray(result.atoms) || !Array.isArray(result.bonds) || !isRecord(result.snapshot) ||
        !isRecord(result.document) || result.document.id !== documentId ||
        typeof result.valence_model !== 'string' || typeof result.aromaticity_model !== 'string' ||
        (result.ignored_text !== null && !isSmilesSpan(result.ignored_text))) {
      throw new ApiError('incompatible_response', 'The service returned an incompatible SMILES preview.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    if (snapshot.id !== documentId || snapshot.revision !== revision) {
      throw new ApiError('incompatible_response', 'The SMILES preview used a different drawing or revision.', 200)
    }
    return {
      digest: result.preview_digest, snapshot, atoms: result.atoms.length, bonds: result.bonds.length,
      ignoredText: result.ignored_text, losses: reply.losses,
      valenceModel: result.valence_model, aromaticityModel: result.aromaticity_model,
    }
  }

  /** Freeze an inspected clipboard import; retain this attempt for every outcome check. */
  createClipboardSmilesImport(input: SmilesImport): SmilesClipboardImportAttempt {
    if (typeof input.documentId !== 'string' || input.documentId.length === 0 || !isCount(input.revision) ||
        typeof input.source !== 'string' || typeof input.digest !== 'string' || !/^[a-f0-9]{64}$/.test(input.digest) ||
        typeof input.acceptLoss !== 'boolean') {
      throw new ApiError('invalid_request', 'Clipboard SMILES import requires the inspected source, revision and consent digest.', 400)
    }
    if (new TextEncoder().encode(input.source).byteLength > MAX_CLIPBOARD_SMILES_BYTES) {
      throw new ApiError('invalid_request', 'Clipboard SMILES text exceeds 16 KiB.', 400)
    }
    const documentId = input.documentId
    const revision = input.revision
    const parameters = {
      document_id: documentId, expected_revision: revision, format: 'smiles',
      source: input.source, preview_digest: input.digest, accept_loss: input.acceptLoss,
    }
    return this.clipboardAttempt('smiles_import', '/api/v1/operations', {
      contract: 1, profile: 'preserve-semantics', operation: 'document.import', parameters,
    }, (value, request) => {
      if (!isRecord(value) || value.contract !== 1 || value.envelope !== 'operation_result' || value.request_id !== request ||
          value.operation !== 'document.import' || value.profile !== 'preserve-semantics' ||
          !Array.isArray(value.diagnostics) || !Array.isArray(value.loss_report)) throw clipboardIncompatible()
      if (value.outcome === 'error' || value.outcome === 'unsupported') {
        const diagnostic: unknown = value.diagnostics[0]
        if (value.result !== null || value.loss_report.length !== 0 || !isRecord(diagnostic) ||
            typeof diagnostic.code !== 'string' || typeof diagnostic.message !== 'string' || !isRecord(diagnostic.details)) throw clipboardIncompatible()
      } else if ((value.outcome !== 'ok' && value.outcome !== 'partial') || value.diagnostics.length !== 0 ||
          (value.outcome === 'ok') !== (value.loss_report.length === 0) || !value.loss_report.every(isDrawingExportLoss)) {
        throw clipboardIncompatible()
      }
      const { result, model } = smilesReply(value)
      const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result })
      if (model !== 'chemifree-smiles-1' || snapshot.document.id !== documentId || !isCount(value.document_revision) ||
          snapshot.document.revision !== value.document_revision || snapshot.document.revision !== revision + 1) throw clipboardIncompatible()
      return snapshot
    })
  }

  /** Commits a previewed string. A refusal that asks for consent is a `ConsentRequired`. */
  importSmiles(input: SmilesImport): Promise<DocumentResponse> {
    return this.commitImport(input, {
      document_id: input.documentId, expected_revision: input.revision, format: 'smiles',
      source: input.source, preview_digest: input.digest, accept_loss: input.acceptLoss,
    }, smilesReply)
  }

  async exportSmiles(documentId: string, style: SmilesStyle, acceptedLosses?: string): Promise<SmilesExport> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.export',
      parameters: {
        format: 'smiles', document_id: documentId, style,
        ...(acceptedLosses === undefined ? {} : { accepted_losses: acceptedLosses }),
      },
    }, 'reload')
    throwIfStopped(value, { models: [style === 'canonical' ? 'chemifree-smiles-canonical-1' : 'chemifree-smiles-1'], revision: 'count' })
    const reply = smilesReply(value)
    const { result } = reply
    if (typeof result.content !== 'string' || result.style !== style || !isRecord(result.snapshot)) {
      throw new ApiError('incompatible_response', 'The service returned invalid SMILES export content.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    if (snapshot.id !== documentId) throw new ApiError('incompatible_response', 'The export used a different drawing.', 200)
    return { content: result.content, style, snapshot, losses: reply.losses, model: reply.model }
  }

  /** Writes the editable drawing as bounded CDXML text or canonical-base64 CDX bytes. */
  async exportInterchange(
    documentId: string,
    format: InterchangeFormat,
    acceptedLosses?: string,
  ): Promise<InterchangeExport> {
    if (acceptedLosses !== undefined && !/^[a-f0-9]{64}$/.test(acceptedLosses)) {
      throw new ApiError('invalid_request', 'The export loss consent digest is invalid.', 0)
    }
    const request = requestId()
    const reply = drawingExportReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.export',
      parameters: {
        format, document_id: documentId,
        ...(acceptedLosses === undefined ? {} : { accepted_losses: acceptedLosses }),
      },
    }, 'reload'), request)
    const { result } = reply
    if (typeof result.content !== 'string' || result.content.length === 0 || !isRecord(result.snapshot) ||
        (format === 'cdx' && !isCanonicalBase64(result.content))) {
      throw new ApiError('incompatible_response', 'The service returned invalid drawing export content.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    if (snapshot.id !== documentId || snapshot.revision !== reply.revision) {
      throw new ApiError('incompatible_response', 'The export used a different drawing or revision.', 200)
    }
    return { content: result.content, format, snapshot, losses: reply.losses }
  }

  /** Writes the editable drawing as a bounded SVG picture. Its graph is not machine-readable, and it cannot be imported back. */
  async exportSvg(documentId: string, acceptedLosses?: string): Promise<SvgExport> {
    if (acceptedLosses !== undefined && !/^[a-f0-9]{64}$/.test(acceptedLosses)) {
      throw new ApiError('invalid_request', 'The export loss consent digest is invalid.', 0)
    }
    const request = requestId()
    const reply = drawingExportReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: request, profile: 'preserve-semantics', operation: 'document.export',
      parameters: {
        format: 'svg', document_id: documentId,
        ...(acceptedLosses === undefined ? {} : { accepted_losses: acceptedLosses }),
      },
    }, 'reload'), request)
    const { result, model } = reply
    const viewBox = svgViewBox(result.view_box)
    if (typeof result.content !== 'string' || result.content.length === 0 || result.media_type !== 'image/svg+xml' ||
        viewBox === undefined || typeof result.font_table !== 'string' || result.font_table.length === 0 ||
        model === undefined || model.length === 0 || !isRecord(result.snapshot)) {
      throw new ApiError('incompatible_response', 'The service returned an invalid SVG drawing export.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    if (snapshot.id !== documentId || snapshot.revision !== reply.revision) {
      throw new ApiError('incompatible_response', 'The export used a different drawing or revision.', 200)
    }
    return {
      content: result.content, media_type: 'image/svg+xml', view_box: viewBox,
      font_table: result.font_table, model, snapshot, losses: reply.losses,
    }
  }

  async exportCt(
    documentId: string,
    format: CtFormat,
    ctab: CtabChoice = DEFAULT_CTAB,
    acceptedLosses?: string,
  ): Promise<{ content: string; losses: CtLoss[]; snapshot: DocumentView; ctab: 'v2000' | 'v3000' }> {
    const reply = ctReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.export',
      parameters: { format, records: [{ document_id: documentId }], ...ctabParameters(ctab, acceptedLosses) },
    }, 'reload'), 'export')
    if (typeof reply.result.content !== 'string' || !Array.isArray(reply.result.records) ||
        reply.result.records.length !== 1 || !isRecord(reply.result.records[0]) || !isRecord(reply.result.records[0].snapshot) ||
        !writtenDialectMatches(ctab, reply.result.ctab)) {
      throw new ApiError('incompatible_response', 'The service returned invalid export content.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...reply.result.records[0].snapshot }).document
    if (snapshot.id !== documentId) throw new ApiError('incompatible_response', 'The export used a different drawing.', 200)
    return { content: reply.result.content, losses: reply.losses, snapshot, ctab: reply.result.ctab }
  }

  /** Read-only properties are tied to the returned document snapshot and source profile. */
  async inspectProperties(
    id: string,
    scope: PropertiesScope,
    property: PropertyKind,
    selection: SelectionView | null,
    dataRegistrationId: string | null,
  ): Promise<PropertiesInspection> {
    const parameters = {
      document_id: id, scope, property,
      ...(selection === null ? {} : { selection }),
      ...(dataRegistrationId === null ? {} : { data_registration_id: dataRegistrationId }),
    }
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.properties', parameters,
    }, 'reload')
    throwIfStopped(value, { models: ['chemifree-properties-1'], revision: 'count' })
    if (!isRecord(value) || (value.outcome !== 'ok' && value.outcome !== 'partial')) {
      const first = isRecord(value) && Array.isArray(value.diagnostics) ? value.diagnostics[0] : null
      const message = isRecord(first) && typeof first.message === 'string' ? first.message : 'The local service could not calculate these properties.'
      throw new ApiError('properties_unavailable', message, 200)
    }
    if (value.model_id !== 'chemifree-properties-1' || !isRecord(value.result) || !Array.isArray(value.loss_report)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible properties.', 200)
    }
    const result = value.result
    if (!isRecord(result.snapshot) || !Array.isArray(result.components) || !Array.isArray(result.valence_warnings)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible properties.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    const validOmitted = (rows: unknown) => Array.isArray(rows) && rows.every((row) => isRecord(row) &&
      typeof row.element === 'string' && isCount(row.isotope) && row.isotope > 0)
    const validMass = (mass: unknown) => isRecord(mass) && isFiniteNumber(mass.average_u) &&
      isFiniteNumber(mass.exact_u) && typeof mass.average_display === 'string' && typeof mass.exact_display === 'string' &&
      Array.isArray(mass.exact_natural_weight_fallbacks) && mass.exact_natural_weight_fallbacks.every((item) => typeof item === 'string') &&
      Array.isArray(mass.average_bases) && mass.average_bases.every((row) => isRecord(row) && typeof row.element === 'string' && typeof row.basis === 'string') &&
      Array.isArray(mass.exact_selections) && mass.exact_selections.every((row) => isRecord(row) && typeof row.element === 'string' && isCount(row.isotope) && row.isotope > 0 && typeof row.basis === 'string') &&
      validOmitted(mass.average_omitted_abundances) && validOmitted(mass.exact_omitted_abundances)
    const validPeaks = (peaks: unknown) => Array.isArray(peaks) && peaks.every((peak) => isRecord(peak) &&
      isFiniteNumber(peak.mass_u) && typeof peak.mass_display === 'string' && isFiniteNumber(peak.probability) &&
      peak.probability >= 0 && peak.probability <= 1)
    const components = result.components
    const componentValid = (component: unknown): component is PropertyComponent => (
      isRecord(component) && typeof component.formula === 'string' &&
      Number.isSafeInteger(component.formal_charge) && Array.isArray(component.atoms) &&
      component.atoms.length > 0 && component.atoms.every((atom: unknown) => typeof atom === 'string' && snapshot.atoms.some((shown) => shown.id === atom)) &&
      validOmitted(component.omitted_abundances) && Array.isArray(component.abundance_sums) &&
      component.abundance_sums.every((row) => isRecord(row) && typeof row.element === 'string' && isFiniteNumber(row.input_sum)) &&
      (property === 'formula' ? component.mass === null && component.isotope_distribution === null :
        validMass(component.mass) && (property === 'distribution' ? validPeaks(component.isotope_distribution) : component.isotope_distribution === null))
    )
    const data = result.data
    const dataValid = property === 'formula' ? data === null : isRecord(data) &&
      typeof data.registration_id === 'string' && isCount(data.generation) && typeof data.profile_id === 'string' &&
      typeof data.manifest_sha256 === 'string' &&
      (data.source_schema_id === undefined || data.source_schema_id === null || typeof data.source_schema_id === 'string') &&
      isFiniteNumber(data.charge_correction_u_per_e) &&
      typeof data.dataset_id === 'string' && typeof data.dataset_version === 'string' &&
      typeof data.calculation_profile === 'string' && typeof data.charge_convention === 'string' &&
      typeof data.abundance_policy === 'string' && typeof data.uncertainty === 'string' && result.mass_unit === 'u'
    const losses = value.loss_report
    const valence = result.valence_warnings
    const atomIds: unknown[] = components.flatMap((component) => isRecord(component) && Array.isArray(component.atoms) ? component.atoms : [])
    if (snapshot.id !== id || snapshot.revision !== value.document_revision || result.scope !== scope || result.property !== property ||
        !(result.selection === null || isSelection(result.selection)) || JSON.stringify(result.selection) !== JSON.stringify(selection) ||
        !isCount(result.display_decimals) || result.display_decimals > 8 || typeof result.valence_model !== 'string' || typeof result.arithmetic !== 'string' ||
        components.length === 0 || !components.every(componentValid) || new Set(atomIds).size !== atomIds.length || !dataValid ||
        !losses.every((loss) => isRecord(loss) && typeof loss.reason === 'string') ||
        (value.outcome === 'partial' && losses.length === 0) ||
        !valence.every((warning) => isRecord(warning) && typeof warning.atom === 'string' &&
          (warning.status === 'invalid' || warning.status === 'unsupported') && (warning.reason === null || typeof warning.reason === 'string'))) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible properties.', 200)
    }
    return {
      model: value.model_id, snapshot, scope, property, selection, display_decimals: result.display_decimals,
      valence_model: result.valence_model, arithmetic: result.arithmetic, data: data as PropertiesInspection['data'], components: components as PropertyComponent[],
      warnings: [
        ...losses.map((loss) => (loss as { reason: string }).reason),
        ...valence.map((warning) => `${(warning as { atom: string }).atom}: valence ${(warning as { status: string }).status}`),
      ],
    }
  }

  async inspectAromaticity(id: string): Promise<AromaticityInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics',
      operation: 'chem.aromaticity', parameters: { document_id: id },
    }, 'reload')
    throwIfStopped(value, { models: ['chemifree-aromaticity-2'], revision: 'count' })
    if (!isRecord(value) || value.outcome !== 'ok' || value.model_id !== 'chemifree-aromaticity-2' ||
        !isRecord(value.result) || !isRecord(value.result.snapshot) || !Array.isArray(value.result.systems)) {
      throw new ApiError('analysis_unavailable', 'The local service could not inspect aromaticity.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const systems = value.result.systems
    if (snapshot.id !== id || snapshot.revision !== value.document_revision ||
        !systems.every((item): item is AromaticitySystem => isRecord(item) &&
          Array.isArray(item.atoms) && item.atoms.length >= 3 && new Set(item.atoms).size === item.atoms.length &&
          item.atoms.every((atom: unknown) => typeof atom === 'string' && snapshot.atoms.some((shown) => shown.id === atom)) &&
          Array.isArray(item.bonds) && item.bonds.length >= 3 && new Set(item.bonds).size === item.bonds.length &&
          item.bonds.every((bond: unknown) => typeof bond === 'string' && snapshot.bonds.some((shown) => shown.id === bond)) &&
          isCount(item.cycle_rank) && item.cycle_rank > 0 &&
          (item.classification === 'aromatic' || item.classification === 'non_aromatic' || item.classification === 'unsupported') &&
          (item.electrons === null || isCount(item.electrons)) && (item.reason === null || typeof item.reason === 'string'))) {
      throw new ApiError('analysis_unavailable', 'The local service returned an incompatible aromaticity inspection.', 200)
    }
    return { model: value.model_id, snapshot, systems }
  }

  /**
   * Compares every stereo item of a drawing with each instance's picture (`chem.stereo`, every
   * instance). The reply is used only if it is exactly what the service sends for the snapshot
   * it names. A time limit or a cancel is a `ComputeStopped`; any other refusal, a resource limit
   * included, is an `ApiError` carrying the service's message.
   */
  async inspectStereo(id: string): Promise<StereoInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics',
      operation: 'chem.stereo', parameters: { document_id: id },
    }, 'reload')
    throwIfStopped(value, { models: ['chemifree-stereo-1'], revision: 'count' })
    if (!isRecord(value) || value.outcome !== 'ok') {
      throw new ApiError('stereo_unavailable', firstDiagnostic(value).message ?? 'The local service could not compare the stereo of the drawing.', 200)
    }
    const incompatible = () => new ApiError('incompatible_response', 'The local service returned an incompatible stereo inspection.', 200)
    if (value.model_id !== 'chemifree-stereo-1' || !isRecord(value.result) || !isRecord(value.result.snapshot) ||
        (value.loss_report !== undefined && !(Array.isArray(value.loss_report) && value.loss_report.length === 0))) {
      throw incompatible()
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const diagnostics = readStereoDiagnostics(value.diagnostics)
    const body = snapshot.id === id && snapshot.revision === value.document_revision && diagnostics !== null
      ? readStereoInspection(value.result, {
        atoms: snapshot.atoms,
        bonds: snapshot.bonds,
        stereo: stereoOf(snapshot),
        instances: snapshot.scene?.instances.map((instance) => instance.id) ?? [],
      })
      : null
    if (body === null || diagnostics === null) throw incompatible()
    return { model: 'chemifree-stereo-1', snapshot, ...body, diagnostics }
  }

  /**
   * Asks for the CIP labels of the stored configurations (`chem.cip`). The reply is used only if
   * it is exactly what the service sends for the snapshot it names and the scope asked for; an
   * error reply (a limit included) is an `ApiError` carrying the service's message, and a reply
   * that is neither is `incompatible_response`. The labels are read-only: nothing is stored.
   */
  async inspectCip(id: string, params: CipParameters = {}): Promise<CipInspection> {
    // The scope the reply must say is sent explicitly, so what is expected never depends on a default.
    const scope: CipScope = params.scope ?? (params.atoms !== undefined || params.bonds !== undefined ? 'named' : 'stored')
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics',
      operation: 'chem.cip', parameters: { document_id: id, ...params, scope },
    }, 'reload')
    throwIfStopped(value, { models: [CIP_MODEL], revision: 'count' })
    const incompatible = () => new ApiError('incompatible_response', 'The local service returned an incompatible CIP reply.', 200)
    if (!isRecord(value)) throw incompatible()
    if (value.outcome === 'unsupported') {
      throw new ApiError('cip_unavailable', firstDiagnostic(value).message ?? 'The local service does not offer CIP labels.', 200)
    }
    if (value.outcome === 'error') {
      const failure = readCipFailure(value)
      if (failure === null) throw incompatible()
      throw new ApiError('cip_unavailable', failure.message, 200)
    }
    if (!isRecord(value.result) || !isRecord(value.result.snapshot)) throw incompatible()
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const body = snapshot.id === id && snapshot.revision === value.document_revision
      ? readCipReply(value, { atoms: snapshot.atoms, bonds: snapshot.bonds, stereo: stereoOf(snapshot) }, scope, { atoms: params.atoms ?? [], bonds: params.bonds ?? [] })
      : null
    if (body === null) throw incompatible()
    return { ...body, snapshot }
  }

  /** Canonical identity and optional comparison are bound to the returned graph snapshots. */
  async inspectIdentity(id: string, mode: IdentityMode, otherId: string | null): Promise<IdentityInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.identity',
      parameters: { document_id: id, mode, ...(otherId === null ? {} : { other_document_id: otherId }) },
    }, 'reload')
    const expectedModel = mode === 'recorded' ? 'chemifree-recorded-graph-1' : 'chemifree-chemical-graph-2'
    throwIfStopped(value, { models: [expectedModel], revision: 'count' })
    if (!isRecord(value) || (value.outcome !== 'ok' && value.outcome !== 'partial')) {
      const diagnostic = firstDiagnostic(value)
      const message = diagnostic.message ?? 'The local service could not compare these graphs.'
      const details = diagnostic.details
      const reason = typeof details.reason === 'string' ? details.reason.replaceAll('_', ' ') : null
      if (reason && (details.side === 'source' || details.side === 'target')) {
        if (!isRecord(value) || value.model_id !== expectedModel || value.result !== null || !isRecord(details.snapshot)) {
          throw new ApiError('incompatible_response', 'The local service returned an incompatible identity failure.', 200)
        }
        const snapshot = snapshotOf(details.snapshot)
        const target = details.side === 'target'
        if (snapshot.id !== (target ? otherId : id) || (!target && snapshot.revision !== value.document_revision)) {
          throw new ApiError('incompatible_response', 'The local service returned an incompatible identity failure.', 200)
        }
        const side = target ? 'Comparison' : 'Current drawing'
        throw new ApiError('identity_unavailable', `${message} ${side} snapshot ${snapshot.id}, revision ${snapshot.revision}: ${reason}. Inspect again to include later edits.`, 200)
      }
      throw new ApiError('identity_unavailable', message, 200)
    }
    if (value.model_id !== expectedModel || !isRecord(value.result) || !isRecord(value.result.snapshot)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible graph identity.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const validIdentity = (item: unknown, document: DocumentView): item is CanonicalIdentity => {
      if (!isRecord(item) || typeof item.code !== 'string' || !Array.isArray(item.numbering) ||
          !Array.isArray(item.symmetry) || !Array.isArray(item.folded_hydrogens) ||
          !item.numbering.every((atom: unknown) => typeof atom === 'string')) return false
      const numbered = new Set<string>(item.numbering as string[])
      const atoms = new Map(document.atoms.map((atom) => [atom.id, atom]))
      if (numbered.size !== item.numbering.length || [...numbered].some((id) => !atoms.has(id))) return false
      const folded = new Set<string>()
      for (const row of item.folded_hydrogens) {
        if (!isRecord(row) || typeof row.hydrogen !== 'string' || typeof row.attached_to !== 'string' ||
            folded.has(row.hydrogen) || numbered.has(row.hydrogen) || !numbered.has(row.attached_to) ||
            atoms.get(row.hydrogen)?.element !== 'H') return false
        folded.add(row.hydrogen)
      }
      if (numbered.size + folded.size !== atoms.size || (mode === 'recorded' && folded.size !== 0)) return false
      const members: string[] = []
      for (const orbit of item.symmetry) {
        if (!Array.isArray(orbit) || orbit.length === 0 ||
            !orbit.every((atom: unknown) => typeof atom === 'string' && atoms.has(atom))) return false
        members.push(...orbit)
      }
      return members.length === atoms.size && new Set(members).size === atoms.size
    }
    const sourceIdentity = value.result.identity
    if (snapshot.id !== id || snapshot.revision !== value.document_revision || !validIdentity(sourceIdentity, snapshot)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible graph identity.', 200)
    }
    const other = value.result.comparison
    let comparison: IdentityInspection['comparison'] = null
    if (otherId === null) {
      if (other !== null) throw new ApiError('incompatible_response', 'The local service returned an unexpected comparison.', 200)
    } else {
      if (!isRecord(other) || !isRecord(other.snapshot) || typeof other.equivalent !== 'boolean') {
        throw new ApiError('incompatible_response', 'The local service returned an incompatible comparison.', 200)
      }
      const otherResponse = DocumentClient.document({ api_version: API_VERSION, ...other.snapshot })
      const otherSnapshot = otherResponse.document
      if (otherSnapshot.id !== otherId || !validIdentity(other.identity, otherSnapshot) ||
          other.equivalent !== (sourceIdentity.code === other.identity.code) ||
          (otherId === id && !sameInspectionSnapshot(snapshot, otherSnapshot))) {
        throw new ApiError('incompatible_response', 'The local service returned an incompatible comparison.', 200)
      }
      comparison = { snapshot: otherSnapshot, recovery: otherResponse.recovery, identity: other.identity, equivalent: other.equivalent }
    }
    const stereo = readStereoReport(value, value.result, [
      { key: 'source_present', path: 'snapshot', snapshot },
      { key: 'other_present', path: 'comparison.snapshot', snapshot: comparison?.snapshot ?? null },
    ])
    if (stereo === null) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible statement about stereo.', 200)
    }
    return {
      model: expectedModel, snapshot, identity: sourceIdentity, comparison,
      stereo: { sourcePresent: stereo.source_present === true, otherPresent: stereo.other_present },
    }
  }

  /**
   * Searches `queryId` in `targetId` (they may be one document). The reply names the exact
   * snapshots it searched, and every mapping is checked here against them for its structure only:
   * a pair for each query atom and bond, distinct target atoms, and each bond between its atoms'
   * images. This is not a chemical re-check: that the target atoms' elements and the target
   * bonds' kinds satisfy the query is the service's answer, taken as given.
   */
  async searchSubstructure(queryId: string, targetId: string): Promise<SubstructureSearch> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.substructure',
      parameters: { query_document_id: queryId, target_document_id: targetId },
    }, 'reload')
    throwIfStopped(value, { models: ['chemifree-substructure-1'], revision: 'count' })
    const incompatible = () => new ApiError('incompatible_response', 'The local service returned an incompatible substructure search.', 200)
    if (!isRecord(value) || !Array.isArray(value.diagnostics)) throw incompatible()
    // The two snapshots a reply is about, which must be the drawings asked for and the revision it reports.
    const bound = (rawQuery: Record<string, unknown>, rawTarget: Record<string, unknown>) => {
      const query = snapshotOf(rawQuery)
      const target = snapshotOf(rawTarget)
      if (query.id !== queryId || target.id !== targetId || query.revision !== value.document_revision ||
          (queryId === targetId && !sameConversionSnapshot(query, target))) throw incompatible()
      return { query, target }
    }
    if (value.outcome !== 'ok' && value.outcome !== 'partial') {
      const { code, message, details } = firstDiagnostic(value)
      if (code === null || message === null) {
        throw new ApiError('analysis_unavailable', 'The local service could not search the drawings.', 200)
      }
      if (!SUBSTRUCTURE_OUTCOMES.has(code)) throw new ApiError(code, message, 200)
      if (SUBSTRUCTURE_OUTCOMES.get(code) !== value.outcome || value.result !== null || value.model_id !== 'chemifree-substructure-1' ||
          !isRecord(details.query_snapshot) || !isRecord(details.target_snapshot)) throw incompatible()
      const { query, target } = bound(details.query_snapshot, details.target_snapshot)
      const words = (key: string) => typeof details[key] === 'string' ? details[key].replaceAll('_', ' ') : null
      const subject = [words('item'), typeof details.id === 'string' ? details.id : null].filter((part) => part !== null).join(' ')
      const where = typeof details.side === 'string'
        ? ` The ${details.side} drawing${subject === '' ? '' : `, ${subject}`}: ${words('reason') ?? 'not readable'}.`
        : ''
      const limit = typeof details.limit_kind === 'string' ? ` Limit: ${words('limit_kind')} (${String(details.limit)}).` : ''
      throw new SubstructureRefusal(code, `${message}${where}${limit}`, query, target)
    }
    const result = value.result
    if (value.model_id !== 'chemifree-substructure-1' || value.diagnostics.length !== 0 || !Array.isArray(value.loss_report) ||
        !isRecord(result) || !isRecord(result.query) || !isRecord(result.target) || !isRecord(result.limits) ||
        result.aromaticity_model !== 'chemifree-aromaticity-2' || !isCount(result.steps) || !Array.isArray(result.mappings)) throw incompatible()
    const { query, target } = bound(result.query, result.target)
    const limits = result.limits
    if (!isCount(limits.query_atoms) || !isCount(limits.search_steps) || !isCount(limits.scanned_candidates) || !isCount(limits.mappings) ||
        result.steps > limits.search_steps || result.mappings.length > limits.mappings) throw incompatible()
    const stereo = readStereoReport(value, result, [
      { key: 'query_present', path: 'query', snapshot: query },
      { key: 'target_present', path: 'target', snapshot: target },
    ])
    if (stereo === null) throw incompatible()
    const queryAtoms = new Set(query.atoms.map((atom) => atom.id))
    const targetAtoms = new Set(target.atoms.map((atom) => atom.id))
    const queryBonds = new Map(query.bonds.map((bond) => [bond.id, bond]))
    const targetBonds = new Map(target.bonds.map((bond) => [bond.id, bond]))
    const pairs = (item: unknown): MatchPair[] | null => Array.isArray(item) && item.every((pair: unknown) =>
      isRecord(pair) && typeof pair.query === 'string' && typeof pair.target === 'string') ? item as MatchPair[] : null
    const mappings: SubstructureMapping[] = []
    for (const mapping of result.mappings as unknown[]) {
      const atoms = isRecord(mapping) ? pairs(mapping.atoms) : null
      const bonds = isRecord(mapping) ? pairs(mapping.bonds) : null
      if (!atoms || !bonds || atoms.length !== queryAtoms.size || bonds.length !== queryBonds.size ||
          new Set(atoms.map((pair) => pair.query)).size !== atoms.length || new Set(atoms.map((pair) => pair.target)).size !== atoms.length ||
          !atoms.every((pair) => queryAtoms.has(pair.query) && targetAtoms.has(pair.target)) ||
          new Set(bonds.map((pair) => pair.query)).size !== bonds.length ||
          !bonds.every((pair) => queryBonds.has(pair.query) && targetBonds.has(pair.target))) throw incompatible()
      const image = new Map(atoms.map((pair) => [pair.query, pair.target]))
      for (const pair of bonds) {
        const from = queryBonds.get(pair.query)
        const to = targetBonds.get(pair.target)
        if (!from || !to) throw incompatible()
        const ends = [image.get(from.begin), image.get(from.end)]
        if (!((ends[0] === to.begin && ends[1] === to.end) || (ends[0] === to.end && ends[1] === to.begin))) throw incompatible()
      }
      mappings.push({ atoms, bonds })
    }
    return {
      model: value.model_id, aromaticityModel: result.aromaticity_model, query, target,
      limits: {
        query_atoms: limits.query_atoms, search_steps: limits.search_steps,
        scanned_candidates: limits.scanned_candidates, mappings: limits.mappings,
      },
      steps: result.steps, mappings,
      stereo: { queryPresent: stereo.query_present === true, targetPresent: stereo.target_present === true },
    }
  }

  /** Apply one model conversion under the revision the user inspected, with history selection. */
  async convertAromaticity(id: string, expectedRevision: number, to: 'aromatic' | 'kekule', selection: SelectionView) {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.aromaticity_convert',
      parameters: { document_id: id, expected_revision: expectedRevision, to, selection_before: selection, selection_after: selection },
    }, 'reload')
    throwIfStopped(value, { models: ['chemifree-aromaticity-2'], revision: 'count_or_null' })
    if (!isRecord(value) || value.outcome !== 'ok' || !isRecord(value.result)) {
      const { code, message, details } = firstDiagnostic(value)
      throw new ApiError(code ?? 'analysis_unavailable', message ?? 'The local service could not convert aromaticity.', 200, {
        currentRevision: isCount(details.current_revision) ? details.current_revision : undefined,
        stateReason: typeof details.reason === 'string' ? details.reason : undefined,
      })
    }
    if (value.model_id !== 'chemifree-aromaticity-2') {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible conversion.', 200)
    }
    const response = DocumentClient.document({ api_version: API_VERSION, ...value.result })
    if (response.document.id !== id || response.document.revision !== value.document_revision) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible conversion.', 200)
    }
    return response
  }

  async list(query?: ListQuery): Promise<DocumentListResponse> {
    const params = new URLSearchParams()
    if (query?.query.trim()) params.set('query', query.query.trim())
    if (query && query.sort !== 'title') params.set('sort', query.sort)
    const search = params.size ? `?${params}` : ''
    const value = await this.request('GET', `/api/v1/documents${search}`)
    if (!isList(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible list.', 200)
    }
    return value
  }

  async trash(): Promise<TrashListResponse> {
    const value = await this.request('GET', '/api/v1/trash')
    if (!isTrashList(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible trash list.', 200)
    }
    return value
  }

  /** Commits a new title for a document without unsaved changes. */
  async rename(id: string, expectedRevision: number, title: string) {
    return DocumentClient.document(
      await this.guarded(`${documentPath(id)}/rename`, { expected_revision: expectedRevision, title }, 'reload'),
    )
  }

  /** Moves a document without unsaved changes to the trash; `expectedRevision` is the one the user confirmed. */
  async delete(id: string, expectedRevision: number) {
    return DocumentClient.moved(
      await this.guarded(`${documentPath(id)}/delete`, { expected_revision: expectedRevision }, 'reload'),
    )
  }

  /** Restoring again after a restart is safe: a restored document is simply no longer in the trash. */
  async restore(id: string) {
    return DocumentClient.moved(await this.guarded(`/api/v1/trash/${encodeURIComponent(id)}/restore`, {}, 'retry'))
  }

  /**
   * A write that its own precondition (a revision, or the document being in the trash) keeps
   * from taking effect twice. A lost reply is asked about once more with the same request ID,
   * and while the service reports the request still running it is asked again, so the page
   * learns what happened instead of meeting its own first attempt as a conflict. Any later
   * repeat needs no record: the precondition refuses it. As for saves, an attempt that may
   * have run is asked about only in the service process it was sent to.
   */
  private async guarded(
    path: string,
    body: Record<string, unknown>,
    onRestart: 'retry' | 'reload',
    method: 'POST' | 'PUT' = 'POST',
  ) {
    const request = { ...body, request_id: requestId() }
    let sentTo: string | undefined
    let token: string | undefined
    const sending = (current: string) => {
      if (sentTo !== undefined && sentTo !== current) throw restarted(401)
      token = current
    }
    let askedAgain = false
    for (let checks = 0; ; ) {
      try {
        return await this.write(path, request, onRestart, sending, method)
      } catch (error) {
        if (!(error instanceof ApiError)) throw error
        if (outcomeOpen(error) && token !== undefined) sentTo ??= token
        if (error.status === 0 && !askedAgain) {
          askedAgain = true
          continue
        }
        if (error.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
          checks += 1
          await pause(OUTCOME_CHECK_INTERVAL_MS)
          continue
        }
        throw error
      }
    }
  }

  private static moved(value: unknown) {
    if (!isMoved(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible reply.', 200)
    }
    return value
  }

  /**
   * Explicitly reads the current process's document. A cached token may describe an older
   * process even before any write has been refused, so verify the token around the read.
   * A process change during the read refuses the snapshot rather than guessing its origin.
   */
  async get(id: string) {
    const previous = this.token === null ? null : await this.session()
    const observed = DocumentClient.sessionOf(await this.request('GET', '/api/v1/session'))
    const alreadyNotified = this.replaced
    this.token = Promise.resolve(observed)
    this.replaced = false
    if (previous !== null && previous.token !== observed.token && !alreadyNotified) {
      this.notifyRestart()
    }
    const generation = this.generation
    const value = await this.request('GET', documentPath(id), undefined, {}, generation)
    const after = DocumentClient.sessionOf(await this.request('GET', '/api/v1/session'))
    if (observed.token !== after.token) {
      this.token = Promise.resolve(after)
      if (this.generation === generation) this.notifyRestart()
      throw restarted(200)
    }
    if (this.generation !== generation) throw restarted(200)
    return DocumentClient.document(value)
  }

  /**
   * Reads a document again to see what the service holds now, from the service process this
   * client writes to. A restarted process has lost the working copy the page shows and would
   * answer with the stored document only, so a restart is reported instead of that reply
   * (`service_restarted`, also to the `onRestart` listeners). Reads carry no token, so the
   * process is compared by its session token after the read, which catches a restart at any
   * point before the reply. Until the page reads the document again with `get`, the old token
   * is kept, so a write is refused and meets the restart as writes do, rather than reaching
   * the new process as if nothing happened.
   */
  async reread(id: string) {
    const generation = this.generation
    const { token } = await this.session()
    const value = await this.request('GET', documentPath(id), undefined, {}, generation)
    if (DocumentClient.sessionOf(await this.request('GET', '/api/v1/session')).token !== token) {
      this.replaced = true
      this.notifyRestart()
      throw restarted(200)
    }
    return DocumentClient.document(value)
  }

  /**
   * Sends a write that creates a document at most once. A restarted service has forgotten the
   * request_id of an earlier unanswered attempt, so that attempt is not resent: if it was
   * committed, a resend would create a second document. `settle` turns the reply into the
   * result; `replayed` says the reply may be the service's record of an earlier attempt,
   * which describes the document as it was then. The request ID is kept until `settle` is
   * done, so a failure there leaves the request to be asked about again.
   */
  private async createOnce<T>(
    key: string,
    path: string,
    body: Record<string, unknown>,
    unknown: ApiError,
    settle: (value: unknown, replayed: boolean) => Promise<T>,
    onInitialRestart: 'retry' | 'reload' = 'retry',
  ): Promise<T> {
    const pending = this.unconfirmed.get(key) ?? { requestId: requestId() }
    this.unconfirmed.set(key, pending)
    let token: string | undefined
    const sending = (current: string) => {
      // Another request found that the service restarted since the unknown attempt.
      if (pending.sentTo !== undefined && pending.sentTo !== current) throw restarted(401)
      token = current
    }
    for (let checks = 0; ; checks += 1) {
      let value: unknown
      try {
        value = await this.write(
          path,
          { request_id: pending.requestId, ...body },
          pending.sentTo === undefined ? onInitialRestart : 'reload',
          sending,
        )
      } catch (error) {
        if (!(error instanceof ApiError)) throw error
        if (error.code === 'service_restarted') {
          // The restarted service has forgotten the ID, so a later attempt is a new request.
          this.unconfirmed.delete(key)
          throw unknown
        }
        // Only an attempt that was sent can have taken effect.
        if (outcomeOpen(error) && token !== undefined) pending.sentTo = token
        if (error.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
          await pause(OUTCOME_CHECK_INTERVAL_MS)
          continue
        }
        if (!outcomeOpen(error)) {
          this.unconfirmed.delete(key)
          throw error
        }
        // The ID is kept, so asking again finds out without creating twice.
        throw new ApiError(
          error.code,
          `${error.message} It may have been carried out; trying again finds out without repeating it.`,
          error.status,
        )
      }
      const replayed = pending.sentTo !== undefined
      // The service answered, so the document exists. Until it is shown, the ID is kept, and
      // trying again asks about it instead of creating another.
      pending.sentTo = token
      const result = await settle(value, replayed)
      this.unconfirmed.delete(key)
      return result
    }
  }

  /**
   * Saves the working copy at `expectedRevision`. When no reply arrives the save may have
   * committed, so the client asks again with the same request_id, at once and then while the
   * service reports it in progress; the service returns the committed reply rather than
   * saving twice. If it stays unanswered, the save is reported as `save_unconfirmed`, and
   * saving the same revision again asks about the same request; saving a later revision is a
   * new save, which also commits the earlier changes.
   */
  async save(id: string, expectedRevision: number) {
    const key = `${id}:${expectedRevision}`
    const pending = this.unconfirmedSaves.get(key) ?? { requestId: requestId() }
    this.unconfirmedSaves.set(key, pending)
    const request = { request_id: pending.requestId, expected_revision: expectedRevision }
    let token: string | undefined
    const sending = (current: string) => {
      // Another request found that the service restarted since the unknown attempt.
      if (pending.sentTo !== undefined && pending.sentTo !== current) throw restarted(401)
      token = current
    }
    let askedAgain = false
    for (let checks = 0; ; ) {
      try {
        const value = DocumentClient.document(
          await this.write(`${documentPath(id)}/save`, request, 'reload', sending),
        )
        // This commit covers every earlier revision of the document, so their checks are moot.
        for (const pending of this.unconfirmedSaves.keys()) {
          if (pending.startsWith(`${id}:`)) this.unconfirmedSaves.delete(pending)
        }
        return value
      } catch (error) {
        if (!(error instanceof ApiError)) throw error
        // Only an attempt that was sent can have taken effect.
        if (outcomeOpen(error) && token !== undefined) pending.sentTo = token
        if (error.status === 0 && !askedAgain) {
          askedAgain = true
          continue
        }
        if (error.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
          checks += 1
          await pause(OUTCOME_CHECK_INTERVAL_MS)
          continue
        }
        if (!outcomeOpen(error)) {
          this.unconfirmedSaves.delete(key)
          throw error
        }
        throw new ApiError(
          'save_unconfirmed',
          'The local service has not confirmed this save, so it may or may not be saved. Check again to ask.',
          error.status,
        )
      }
    }
  }

  async create(title?: string) {
    return this.createOnce(
      `create:${JSON.stringify(title ?? null)}`,
      '/api/v1/documents',
      { title },
      new ApiError(
        'create_unconfirmed',
        'The local service restarted before confirming the earlier new document, so it may already exist. Check Saved documents before creating another.',
        401,
      ),
      async (value, replayed) => {
        const created = DocumentClient.document(value)
        return replayed ? this.get(created.document.id) : created
      },
    )
  }

  private static copy(value: unknown): CopyResponse {
    if (!isCopy(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible copy.', 200)
    }
    return { ...withStates(value), source: value.source }
  }

  private static copyUnconfirmed() {
    return new ApiError(
      'copy_unconfirmed',
      'The local service restarted before confirming the copy, so it may already exist. Check Saved documents before copying again.',
      401,
    )
  }

  /** The copy as it is now, with what became of its source when it was made. */
  private async settleCopy(value: unknown, replayed: boolean): Promise<CopyResponse> {
    const copied = DocumentClient.copy(value)
    return replayed ? { ...(await this.get(copied.document.id)), source: copied.source } : copied
  }

  /** Saves the working copy as a new document; the source returns to its last save. */
  async saveAs(id: string, expectedRevision: number, title?: string) {
    return this.createOnce(
      `save-as:${id}:${expectedRevision}:${JSON.stringify(title ?? null)}`,
      `${documentPath(id)}/save-as`,
      { expected_revision: expectedRevision, title },
      DocumentClient.copyUnconfirmed(),
      (value, replayed) => this.settleCopy(value, replayed),
    )
  }

  async recoverAsCopy(id: string, title?: string) {
    return this.createOnce(
      `recover-as-copy:${id}:${JSON.stringify(title ?? null)}`,
      `${documentPath(id)}/recover-as-copy`,
      { title },
      DocumentClient.copyUnconfirmed(),
      (value, replayed) => this.settleCopy(value, replayed),
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

  async transact(id: string, expectedRevision: number, commands: Command[], selection?: SelectionSnapshots) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/transactions`,
        {
          request_id: requestId(),
          expected_revision: expectedRevision,
          commands,
          selection_before: selection?.before,
          selection_after: selection?.after,
        },
        'reload',
      ),
    )
  }

  async revisionAction(id: string, action: 'undo' | 'redo', expectedRevision: number) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/${action}`,
        { request_id: requestId(), expected_revision: expectedRevision },
        'reload',
      ),
    )
  }

  /**
   * Replaces the document's settings with the built-in defaults or the preferred new-document
   * settings, as one undoable edit.
   */
  async resetSettings(id: string, expectedRevision: number, to: SettingsSource, selection?: SelectionSnapshots) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/settings/reset`,
        { request_id: requestId(), expected_revision: expectedRevision, to, selection_before: selection?.before, selection_after: selection?.after },
        'reload',
      ),
    )
  }

  /** Applies the settings of document `source` to document `id`, as one undoable edit. */
  async importSettings(id: string, expectedRevision: number, source: string, selection?: SelectionSnapshots) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/settings/import`,
        { request_id: requestId(), expected_revision: expectedRevision, source, selection_before: selection?.before, selection_after: selection?.after },
        'reload',
      ),
    )
  }

  private static preferencesOf(value: unknown): PreferencesResponse {
    if (!isPreferencesResponse(value)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible preferences.', 200)
    }
    return value
  }

  async preferences() {
    return DocumentClient.preferencesOf(await this.request('GET', PREFERENCES_PATH))
  }

  /**
   * Preference writes are stored at once and guarded by their revision, so a lost reply is
   * asked about and a restart retried, as for a rename.
   */
  async setPreferences(expectedRevision: number, change: PreferencesChange) {
    return DocumentClient.preferencesOf(
      await this.guarded(PREFERENCES_PATH, { expected_revision: expectedRevision, ...change }, 'retry', 'PUT'),
    )
  }

  async resetPreferences(expectedRevision: number) {
    return DocumentClient.preferencesOf(
      await this.guarded(`${PREFERENCES_PATH}/reset`, { expected_revision: expectedRevision }, 'retry'),
    )
  }

  /** Makes new documents start from every setting of `document` (as the service holds it now). */
  async preferencesFromDocument(expectedRevision: number, document: string) {
    return DocumentClient.preferencesOf(
      await this.guarded(`${PREFERENCES_PATH}/import`, { expected_revision: expectedRevision, document }, 'retry'),
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
