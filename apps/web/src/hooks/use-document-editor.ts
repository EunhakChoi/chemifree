import { useCallback, useEffect, useRef, useState } from 'react'

import { useAutosave } from '@/hooks/use-autosave'
import { mergeGroupSelection, selectionAfterGroup, selectionIds, selectionSnapshot } from '@/editor/scene-selection'
import {
  ApiError,
  DocumentClient,
  sameConversionSnapshot,
  type Command,
  type CopyResponse,
  type CtImport,
  type SmilesImport,
  type SmilesClipboardImportAttempt,
  type SmilesStyle,
  type DocumentListResponse,
  type DocumentResponse,
  type DocumentView,
  type InterchangeFormat,
  type InterchangeInput,
  type ListQuery,
  type NativeClipboardCopy,
  type NativeClipboardCutAttempt,
  type NativeClipboardMolImportAttempt,
  type NativeClipboardPasteAttempt,
  type NativeSceneClipboardCopy,
  type NativeSceneClipboardPasteAttempt,
  type SelectionView,
  type SettingsSource,
  type TrashListResponse,
} from '@/lib/document-contract'
import { sendMark, type DeriveOutcome, type MarkGesture } from '@/editor/mark-gestures'
import type { CtabChoice } from '@/lib/ct-conversion'
import { resolutionCommands, resolutionStatus, type Resolution } from '@/editor/stereo-agreement'
import { groupStatus, planGroupRequest, type GroupRequest } from '@/editor/stereo-groups'
import { describeStereoChange } from '@/editor/stereo-status'
import { holdsStereo, stereoOf } from '@/lib/stereo-contract'

/**
 * What the last stereo gesture did, in words. It says what happened at one revision of one
 * document, so it is shown only while the page shows that revision.
 */
export interface StereoStatus {
  documentId: string
  revision: number
  text: string
}

/** What became of the original when the copy was made. */
export type CopiedFrom =
  /** A recovered draft became the copy; the original's saved document was not changed. */
  | { kind: 'recovered' }
  /** The original's session closed; any unsaved changes it had went only into the copy. */
  | { kind: 'closed'; discardedChanges: boolean }
  /**
   * The original's recovery draft could not be removed, so its session was left open with its
   * unsaved changes (which the copy also has) instead of leaving a draft behind.
   */
  | { kind: 'left_open' }

/**
 * What the page is editing right after a copy: a new document beside the original. It
 * describes the moment of the copy, so it is dropped when the page shows another document or
 * learns that the service restarted. The page learns that only when one of its writes is
 * refused, so a page that sends no writes keeps the notice until then.
 */
export interface CopyNotice {
  copyId: string
  sourceId: string
  sourceTitle: string
  source: CopiedFrom
}

export type ListState =
  | { kind: 'loading' }
  | { kind: 'ready'; list: DocumentListResponse }
  | { kind: 'error'; message: string }

export type TrashState =
  | { kind: 'loading' }
  | { kind: 'ready'; trash: TrashListResponse }
  | { kind: 'error'; message: string }

/**
 * Commands for one transaction. A list is built from the document on screen and is sent with
 * that revision, so if an earlier queued request changed the document first, the server
 * rejects it as a revision conflict. Edits relative to the latest state (such as "one more
 * charge") pass a function instead, evaluated when the queued request runs. Returning null
 * skips the request.
 */
export type Edit = Command[] | ((document: DocumentView) => Command[] | null)

/** What committing an import came to; a refusal is the caller's to show. */
export type ImportResult = { committed: true } | { committed: false; failure: ApiError | null }

/** Grouping replaces selected scene members; a drag supplies its own press-time history selection. */
export type EditSelection = { kind: 'group'; objects: string[] } | { kind: 'gesture'; before: ReadonlySet<string> }

/** A prepared Cut belongs to this exact source snapshot and service process. */
export interface NativeClipboardCutPlan {
  copy: NativeClipboardCopy
  selection: SelectionView
  generation: number
}

type ClipboardMutation = {
  documentId: string
  generation: number
  terminalConflict?: boolean
} & ({ kind: 'paste'; attempt: NativeClipboardPasteAttempt } | { kind: 'cut'; attempt: NativeClipboardCutAttempt } |
  { kind: 'mol_import'; attempt: NativeClipboardMolImportAttempt } | { kind: 'scene_paste'; attempt: NativeSceneClipboardPasteAttempt } |
  { kind: 'smiles_import'; attempt: SmilesClipboardImportAttempt })

function requireCutEligible(copy: NativeClipboardCopy) {
  const source = copy.snapshot.document
  if (source.scene?.instances.length !== 1) {
    throw new ApiError('clipboard_cut_placements', 'Cut requires exactly one source placement. Copy remains available.', 0)
  }
  if (copy.boundaryBonds.length !== 0) {
    throw new ApiError('clipboard_cut_boundary', 'Cut requires a fragment with no crossing bonds. Select the complete connected fragment, or use Copy.', 0)
  }
  const atoms = new Set(copy.copied.atoms)
  const bonds = new Set(copy.copied.bonds)
  if (source.scene.references?.some(({ target }) =>
    target.kind === 'atom' && atoms.has(target.id) || target.kind === 'bond' && bonds.has(target.id))) {
    throw new ApiError('clipboard_cut_references', 'Scene references target this fragment. Remove those references before Cut, or use Copy.', 0)
  }
}

const client = new DocumentClient()

/**
 * The view to show when an autosave reply describes the revision already on screen. The reply
 * brings the new session state (the draft revision) but states no notes, so the notes of the
 * edit that produced this revision stay until another reply replaces them.
 */
export function adoptedView(shown: DocumentResponse, reply: DocumentResponse): DocumentResponse {
  return { ...reply, notes: reply.notes?.length ? reply.notes : (shown.notes ?? []) }
}

/** Whether `reply` describes the revision and content already shown. */
export function describesShown(shown: DocumentResponse | null, reply: DocumentResponse) {
  return shown !== null && shown.document.revision === reply.document.revision && sameContent(shown.document, reply.document)
}

/** The view after a read or draft reply: the shown one's notes survive a reply for the same revision and content; anything else replaces the view. */
export function viewAfterRead(shown: DocumentResponse | null, reply: DocumentResponse): DocumentResponse {
  return shown !== null && describesShown(shown, reply) ? adoptedView(shown, reply) : reply
}

/**
 * Same title, graph, stereo table, scene, and settings, whatever revision number names them. An
 * omitted stereo table is the empty one, so a service that states none equals one that states
 * an empty table, but not one that states configurations.
 */
export function sameContent(a: DocumentView, b: DocumentView) {
  return (
    a.id === b.id &&
    a.title === b.title &&
    JSON.stringify(a.atoms) === JSON.stringify(b.atoms) &&
    JSON.stringify(a.bonds) === JSON.stringify(b.bonds) &&
    JSON.stringify(stereoOf(a)) === JSON.stringify(stereoOf(b)) &&
    JSON.stringify(a.scene) === JSON.stringify(b.scene) &&
    JSON.stringify(a.settings) === JSON.stringify(b.settings)
  )
}

/**
 * Builds a request from the latest document. `restartedFrom` is set on the one retry after
 * the service restarted: the document it showed before, which the restarted service
 * reopened with the same content. Returning null sends nothing (or gives up the retry).
 */
type Call = (document: DocumentResponse, restartedFrom?: DocumentResponse) => Promise<DocumentResponse> | null

function asApiError(error: unknown) {
  return error instanceof ApiError ? error : new ApiError('unexpected', 'The request failed unexpectedly.', 0)
}

/**
 * Editor state backed by the local API. The server is authoritative: every edit is a
 * request carrying the revision it is based on, requests run one at a time, and the view
 * changes only when a response arrives.
 */
export function useDocumentEditor() {
  const [list, setList] = useState<ListState>({ kind: 'loading' })
  const [current, setCurrentState] = useState<DocumentResponse | null>(null)
  const [pending, setPending] = useState(0)
  const [error, setError] = useState<ApiError | null>(null)
  const [copyNotice, setCopyNotice] = useState<CopyNotice | null>(null)
  const [stereoStatus, setStereoStatus] = useState<StereoStatus | null>(null)
  const [serviceGeneration, setServiceGeneration] = useState(0)
  const serviceGenerationRef = useRef(0)
  const [selection, setSelectionState] = useState<Set<string>>(new Set())
  const selectionRef = useRef<Set<string>>(new Set())
  const selectionVersion = useRef(0)
  const currentRef = useRef<DocumentResponse | null>(null)
  const queue = useRef<Promise<void>>(Promise.resolve())
  const failures = useRef(0)
  // An unanswered clipboard mutation is checked with its original request, never resubmitted.
  const clipboardAttempt = useRef<ClipboardMutation | null>(null)
  const [clipboardPending, setClipboardPending] = useState(false)
  const [clipboardNeedsReload, setClipboardNeedsReload] = useState(false)
  const [clipboardAction, setClipboardAction] = useState<'paste' | 'cut' | 'mol_import' | 'scene_paste' | 'smiles_import' | null>(null)

  const adoptSelection = useCallback((next: Set<string>) => {
    selectionRef.current = next
    setSelectionState(next)
  }, [])
  const setSelection = useCallback((update: Set<string> | ((current: ReadonlySet<string>) => Set<string>)) => {
    selectionVersion.current += 1
    adoptSelection(typeof update === 'function' ? update(selectionRef.current) : update)
  }, [adoptSelection])

  const setCurrent = useCallback((document: DocumentResponse | null) => {
    if (currentRef.current?.document.id !== document?.document.id) {
      adoptSelection(selectionIds(document?.document.selection))
    }
    currentRef.current = document
    setCurrentState(document)
    // Showing anything but the copy ends the notice; returning later would show a stale one.
    setCopyNotice((notice) => (notice?.copyId === document?.document.id ? notice : null))
  }, [adoptSelection])

  // A restarted service no longer holds the session the notice describes.
  useEffect(() => client.onRestart(() => {
    setCopyNotice(null)
    serviceGenerationRef.current += 1
    setServiceGeneration(serviceGenerationRef.current)
  }), [])

  const [listRequest, setListRequest] = useState(0)
  const [listQuery, setListQuery] = useState<ListQuery>({ query: '', sort: 'title' })
  const [trash, setTrash] = useState<TrashState>({ kind: 'loading' })

  useEffect(() => {
    let active = true
    client.list(listQuery).then(
      (value) => active && setList({ kind: 'ready', list: value }),
      (failure: unknown) => active && setList({ kind: 'error', message: asApiError(failure).message }),
    )
    client.trash().then(
      (value) => active && setTrash({ kind: 'ready', trash: value }),
      (failure: unknown) => active && setTrash({ kind: 'error', message: asApiError(failure).message }),
    )
    return () => {
      active = false
    }
  }, [listRequest, listQuery])

  const refreshList = useCallback(() => {
    setList({ kind: 'loading' })
    setListRequest((count) => count + 1)
  }, [])

  /**
   * Queues a task behind earlier ones. A success clears an error the user had already seen
   * when this task was queued, never one from a request that failed while it waited. A task
   * given `ownFailure` reports its failure there instead of to the page.
   */
  const run = useCallback((task: () => Promise<void>, ownFailure?: (failure: ApiError) => void, checkClipboard = false) => {
    setPending((count) => count + 1)
    const failuresSeen = failures.current
    const next = queue.current
      .then(async () => {
        if (clipboardAttempt.current && !checkClipboard) {
          throw new ApiError(`clipboard_${clipboardAttempt.current.kind}_unconfirmed`, 'Check the pending clipboard operation before making another change.', 0)
        }
        await task()
      })
      .then(
        () => {
          if (failures.current === failuresSeen) setError(null)
        },
        (failure: unknown) => {
          // A caller with its own place to show the failure keeps it off the page's error.
          if (ownFailure) {
            ownFailure(asApiError(failure))
            return
          }
          failures.current += 1
          setError(asApiError(failure))
        },
      )
      .finally(() => setPending((count) => count - 1))
    queue.current = next
    return next
  }, [])

  const mutate = useCallback(
    (call: Call, onReply?: (response: DocumentResponse) => void) =>
      run(async () => {
        const document = currentRef.current
        if (!document) return
        const request = call(document)
        if (!request) return
        let next: DocumentResponse
        try {
          next = await request
        } catch (failure) {
          // A restarted service lost its working copies. If it reopened exactly what this page
          // shows, the edit still means the same thing; otherwise the page must reload.
          if (!(failure instanceof ApiError && failure.code === 'service_restarted')) throw failure
          const fresh = await client.get(document.document.id)
          const retry = sameContent(fresh.document, document.document) ? call(fresh, document) : null
          if (!retry) throw failure
          setCurrent(fresh)
          next = await retry
        }
        if (currentRef.current?.document.id === next.document.id) {
          setCurrent(next)
          onReply?.(next)
        }
      }),
    [run, setCurrent],
  )

  const clipboardTask = useCallback(<T,>(task: () => Promise<T>, checking = false): Promise<T> =>
    new Promise<T>((resolve, reject) => {
      void run(async () => { resolve(await task()) }, reject, checking)
    }), [run])

  const finishClipboardMutation = useCallback(async () => {
    const pendingPaste = clipboardAttempt.current
    if (!pendingPaste) return
    try {
      const snapshot = pendingPaste.kind === 'paste' || pendingPaste.kind === 'scene_paste'
        ? (await pendingPaste.attempt.send()).snapshot : await pendingPaste.attempt.send()
      // Another request may discover a restart while this old-process reply is held.
      // A successful old reply cannot confirm what the new process currently contains.
      if (pendingPaste.generation !== serviceGenerationRef.current) {
        throw new ApiError(`clipboard_${pendingPaste.kind}_restarted`, 'The service changed before this clipboard operation was confirmed. Reload the drawing to reconcile its outcome.', 0)
      }
      clipboardAttempt.current = null
      setClipboardPending(false)
      setClipboardNeedsReload(false)
      setClipboardAction(null)
      if (currentRef.current?.document.id === pendingPaste.documentId) {
        setCurrent(snapshot)
        adoptSelection(pendingPaste.kind === 'mol_import' || pendingPaste.kind === 'smiles_import'
          ? selectionIds(selectionSnapshot(selectionRef.current, snapshot.document))
          : selectionIds(snapshot.document.selection))
      }
    } catch (failure) {
      const error = asApiError(failure)
      const prefix = `clipboard_${pendingPaste.kind}_`
      const uncertain = error.code === `${prefix}unconfirmed` || error.code === `${prefix}restarted` || error.code === `${prefix}reconcile`
      if (uncertain) {
        pendingPaste.terminalConflict = error.code === `${prefix}reconcile`
        setClipboardPending(true)
        setClipboardNeedsReload(error.code !== `${prefix}unconfirmed`)
      } else {
        clipboardAttempt.current = null
        setClipboardPending(false)
        setClipboardNeedsReload(false)
        setClipboardAction(null)
        if (pendingPaste.kind === 'cut') {
          throw new ApiError(error.code, `The fragment was copied, but Cut deletion was refused. ${error.message}`, error.status, {
            currentRevision: error.currentRevision,
            commandIndex: error.commandIndex,
            reason: error.reason,
            stateReason: error.stateReason,
          })
        }
      }
      throw error
    }
  }, [setCurrent, adoptSelection])

  const readClipboard = useCallback(async (documentId: string, instance: string | null, pressed: ReadonlySet<string>): Promise<NativeClipboardCutPlan> => {
    const shown = currentRef.current?.document
    if (!shown || shown.id !== documentId || !instance ||
        !shown.scene?.instances.some((placement) => placement.id === instance)) {
      throw new ApiError('clipboard_instance_invalid', 'Choose an existing source placement and copy again.', 0)
    }
    const selected = selectionSnapshot(pressed, shown)
    if (selectionIds(selected).size !== pressed.size) {
      throw new ApiError('clipboard_selection_unknown', 'The captured selection changed. Select the fragment again.', 0)
    }
    const generation = serviceGenerationRef.current
    const result = await client.clipboardCopy(documentId, selected, instance)
    if (serviceGenerationRef.current !== generation || currentRef.current?.document.id !== documentId ||
        !sameConversionSnapshot(shown, result.snapshot.document)) {
      throw new ApiError('clipboard_source_changed', 'The drawing or service changed. Prepare the fragment again.', 0)
    }
    return { copy: result, selection: selected, generation }
  }, [])

  const id = () => currentRef.current?.document.id ?? ''
  const revision = (document: DocumentResponse) => document.document.revision

  const report = useCallback((failure: ApiError) => {
    failures.current += 1
    setError(failure)
  }, [])
  const autosave = useAutosave(client, current, {
    shown: useCallback(() => currentRef.current, []),
    adopt: useCallback(
      (response: DocumentResponse) => {
        const shown = currentRef.current
        const same = describesShown(shown, response)
        if (same) setCurrent(viewAfterRead(shown, response))
        return same
      },
      [setCurrent],
    ),
    // Queued with edits so an older read cannot replace a newer edit's view. Its failure goes
    // to autosave, which retries or stops, rather than to the page's error.
    refresh: useCallback(
      (documentId: string) =>
        new Promise<void>((resolve, reject) => {
          void run(async () => {
            try {
              const fresh = await client.reread(documentId)
              if (currentRef.current?.document.id === documentId) setCurrent(viewAfterRead(currentRef.current, fresh))
              resolve()
            } catch (failure) {
              reject(failure)
            }
          }, reject)
        }),
      [run, setCurrent],
    ),
    onRestart: report,
  })
  const { flush } = autosave

  /**
   * Shows another document. The one on screen is drafted first, so pending work it leaves
   * open in the service survives a crash; if that fails, the switch still happens and says so.
   */
  const show = useCallback(
    async (next: () => Promise<DocumentResponse>) => {
      const left = currentRef.current?.document.title
      const unwritten = await flush()
      setCurrent(await next())
      if (unwritten !== null) {
        throw new ApiError(
          'draft_not_written',
          `Unsaved changes to “${left}” could not be autosaved (${unwritten}); they stay open in the service until you return to it.`,
          0,
        )
      }
    },
    [flush, setCurrent],
  )

  /** Runs a copy, then shows the copy as the document being edited. */
  const copy = useCallback(
    (make: (shown: DocumentResponse) => Promise<CopyResponse>, recovered: boolean) => {
      const shown = currentRef.current
      if (!shown) return Promise.resolve()
      return run(async () => {
        try {
          const copied = await make(shown)
          let source: CopiedFrom = { kind: 'recovered' }
          if (!recovered) {
            source = copied.source.closed
              ? { kind: 'closed', discardedChanges: copied.source.discarded_changes }
              : { kind: 'left_open' }
          }
          setCurrent(copied)
          setCopyNotice({ copyId: copied.document.id, sourceId: copied.source.id, sourceTitle: shown.document.title, source })
          refreshList()
        } catch (failure) {
          if (failure instanceof ApiError && failure.code === 'copy_unconfirmed') refreshList()
          throw failure
        }
      })
    },
    [run, setCurrent, refreshList],
  )

  /**
   * Commits an import for a preview made of `snapshot` under `generation`. It sends only if the
   * page still shows exactly that drawing, and shows the drawing it returns. Resolves true when
   * it committed; a failure goes to `ownFailure`, else to the page.
   */
  const importDocument = useCallback(async (
    target: { documentId: string; revision: number },
    snapshot: DocumentView,
    generation: number,
    send: () => Promise<DocumentResponse>,
    ownFailure?: (failure: ApiError) => void,
  ) => {
    let committed = false
    await run(async () => {
      const shown = currentRef.current
      if (!shown || serviceGenerationRef.current !== generation ||
          !sameConversionSnapshot(shown.document, snapshot) ||
          shown.document.id !== target.documentId || shown.document.revision !== target.revision) {
        throw new ApiError('preview_changed', 'The drawing changed. Preview the import again.', 200)
      }
      const response = await send()
      if (currentRef.current?.document.id === target.documentId) setCurrent(response)
      committed = true
    }, ownFailure)
    return committed
  }, [run, setCurrent])

  return {
    list,
    listQuery,
    /** Searches or re-sorts the list; the service applies the query. */
    queryList: useCallback((query: ListQuery) => {
      setList({ kind: 'loading' })
      setListQuery(query)
    }, []),
    trash,
    current,
    serviceGeneration,
    clipboardPending,
    clipboardNeedsReload,
    clipboardAction,
    copyFragment: useCallback((documentId: string, instance: string | null): Promise<NativeClipboardCopy> => {
      const pressed = new Set(selectionRef.current)
      return clipboardTask(async () => (await readClipboard(documentId, instance, pressed)).copy)
    }, [clipboardTask, readClipboard]),
    copySceneGraphics: useCallback((documentId: string): Promise<NativeSceneClipboardCopy> => {
      const pressed = new Set(selectionRef.current)
      const generation = serviceGenerationRef.current
      return clipboardTask(async () => {
        const shown = currentRef.current?.document
        if (!shown || shown.id !== documentId || generation !== serviceGenerationRef.current) {
          throw new ApiError('clipboard_scene_source_changed', 'The drawing or service changed. Copy the graphics again.', 0)
        }
        const selected = selectionSnapshot(pressed, shown)
        if (selectionIds(selected).size !== pressed.size) {
          throw new ApiError('clipboard_scene_selection_unknown', 'The captured selection changed. Select the graphics again.', 0)
        }
        const result = await client.clipboardSceneCopy(documentId, selected)
        if (generation !== serviceGenerationRef.current || currentRef.current?.document.id !== documentId ||
            !sameConversionSnapshot(shown, result.snapshot.document)) {
          throw new ApiError('clipboard_scene_source_changed', 'The drawing or service changed. Copy the graphics again.', 0)
        }
        return result
      })
    }, [clipboardTask]),
    prepareCut: useCallback((documentId: string, instance: string | null): Promise<NativeClipboardCutPlan> => {
      const pressed = new Set(selectionRef.current)
      return clipboardTask(async () => {
        const plan = await readClipboard(documentId, instance, pressed)
        requireCutEligible(plan.copy)
        return plan
      })
    }, [clipboardTask, readClipboard]),
    cutFragment: useCallback((plan: NativeClipboardCutPlan, write: Promise<void>): Promise<void> => {
      // Observe a denied write immediately even if this task waits behind another action.
      // The browser write itself has already started directly in the user's click.
      const written = write.then(() => ({ ok: true as const }), () => ({ ok: false as const }))
      return clipboardTask(async () => {
        if (!(await written).ok) {
          throw new ApiError('clipboard_write_failed', 'Clipboard writing failed. The original was retained; use the native text for manual copying.', 0)
        }
        const shown = currentRef.current?.document
        const source = plan.copy.snapshot.document
        if (!shown || plan.generation !== serviceGenerationRef.current || !sameConversionSnapshot(shown, source)) {
          throw new ApiError('clipboard_cut_source_changed', 'The fragment was copied, but the drawing or service changed. No Cut deletion was sent. Prepare Cut again.', 0)
        }
        requireCutEligible(plan.copy)
        clipboardAttempt.current = {
          kind: 'cut',
          attempt: client.createClipboardCut(source.id, source.revision, plan.copy.copied.atoms, plan.selection),
          documentId: source.id,
          generation: plan.generation,
        }
        setClipboardAction('cut')
        await finishClipboardMutation()
      })
    }, [clipboardTask, finishClipboardMutation]),
    pasteFragment: useCallback((documentId: string, payload: string, offset: { x: number; y: number }): Promise<void> => {
      const pressed = new Set(selectionRef.current)
      const capturedOffset = { ...offset }
      return clipboardTask(async () => {
        const shown = currentRef.current?.document
        if (!shown || shown.id !== documentId) {
          throw new ApiError('clipboard_destination_changed', 'The destination changed. Open Paste fragment again.', 0)
        }
        const before = selectionSnapshot(pressed, shown)
        if (selectionIds(before).size !== pressed.size) {
          throw new ApiError('clipboard_selection_unknown', 'The captured selection changed. Open Paste fragment again.', 0)
        }
        clipboardAttempt.current = {
          kind: 'paste',
          attempt: client.createClipboardPaste(documentId, shown.revision, payload, capturedOffset, before),
          documentId,
          generation: serviceGenerationRef.current,
        }
        setClipboardAction('paste')
        await finishClipboardMutation()
      })
    }, [clipboardTask, finishClipboardMutation]),
    pasteSceneGraphics: useCallback((documentId: string, payload: string, offset: { x: number; y: number }): Promise<void> => {
      const pressed = new Set(selectionRef.current)
      const capturedOffset = { ...offset }
      const generation = serviceGenerationRef.current
      return clipboardTask(async () => {
        const shown = currentRef.current?.document
        if (!shown || shown.id !== documentId || generation !== serviceGenerationRef.current) {
          throw new ApiError('clipboard_scene_destination_changed', 'The destination or service changed. Open Paste graphics again.', 0)
        }
        const before = selectionSnapshot(pressed, shown)
        if (selectionIds(before).size !== pressed.size) {
          throw new ApiError('clipboard_scene_selection_unknown', 'The captured selection changed. Open Paste graphics again.', 0)
        }
        clipboardAttempt.current = {
          kind: 'scene_paste',
          attempt: client.createClipboardScenePaste(documentId, shown.revision, payload, capturedOffset, before, shown),
          documentId,
          generation,
        }
        setClipboardAction('scene_paste')
        await finishClipboardMutation()
      })
    }, [clipboardTask, finishClipboardMutation]),
    importClipboardMol: useCallback((input: CtImport, snapshot: DocumentView, generation: number): Promise<void> => {
      const frozen = { ...input }
      return clipboardTask(async () => {
        const shown = currentRef.current?.document
        if (frozen.format !== 'mol' || frozen.record !== 1 || frozen.documentId !== snapshot.id || frozen.revision !== snapshot.revision ||
            !shown || generation !== serviceGenerationRef.current || !sameConversionSnapshot(shown, snapshot)) {
          throw new ApiError('preview_changed', 'The drawing or service changed. Preview the MOL text again before importing.', 0)
        }
        clipboardAttempt.current = {
          kind: 'mol_import',
          attempt: client.createClipboardMolImport(frozen),
          documentId: frozen.documentId,
          generation,
        }
        setClipboardAction('mol_import')
        await finishClipboardMutation()
      })
    }, [clipboardTask, finishClipboardMutation]),
    checkClipboardPaste: useCallback(() => clipboardTask(finishClipboardMutation, true), [clipboardTask, finishClipboardMutation]),
    reconcileClipboardPaste: useCallback(() => clipboardTask(async () => {
      const unresolved = clipboardAttempt.current
      if (!unresolved) return
      // A read in the old process cannot prove that a request still running there will not
      // commit later. A confirmed new process or terminal revision conflict permits a read
      // to reconcile the drawing without pretending that the original paste failed.
      if (!unresolved.terminalConflict && serviceGenerationRef.current === unresolved.generation) {
        throw new ApiError(`clipboard_${unresolved.kind}_unconfirmed`, 'Check the clipboard operation outcome in the original service first.', 0)
      }
      const fresh = await client.get(unresolved.documentId)
      if (currentRef.current?.document.id === unresolved.documentId) setCurrent(fresh)
      clipboardAttempt.current = null
      setClipboardPending(false)
      setClipboardNeedsReload(false)
      setClipboardAction(null)
      if (currentRef.current?.document.id === unresolved.documentId) {
        adoptSelection(unresolved.kind === 'mol_import' || unresolved.kind === 'smiles_import'
          ? selectionIds(selectionSnapshot(selectionRef.current, fresh.document))
          : selectionIds(fresh.document.selection))
      }
    }, true), [clipboardTask, setCurrent, adoptSelection]),
    inspectProperties: useCallback((...args: Parameters<DocumentClient['inspectProperties']>) => client.inspectProperties(...args), []),
    inspectIdentity: useCallback((...args: Parameters<DocumentClient['inspectIdentity']>) => client.inspectIdentity(...args), []),
    searchSubstructure: useCallback((...args: Parameters<DocumentClient['searchSubstructure']>) => client.searchSubstructure(...args), []),
    selection,
    /** A snapshot for a command made now, including changes not rendered yet. */
    selectionAtPress: useCallback(() => new Set(selectionRef.current), []),
    setSelection,
    /** Holds later document edits until a dependent preference write has finished. */
    reserveStyle: useCallback(() => {
      let reached = () => {}
      let refused: (failure: ApiError) => void = () => {}
      let release = () => {}
      const ready = new Promise<void>((resolve, reject) => {
        reached = () => resolve()
        refused = reject
      })
      const released = new Promise<void>((resolve) => { release = () => resolve() })
      void run(async () => {
        reached()
        await released
      }, refused)
      return { ready, release }
    }, [run]),
    busy: pending > 0 || clipboardPending,
    currentServiceGeneration: useCallback(() => serviceGenerationRef.current, []),
    readInterchange: useCallback((source: string, format: InterchangeFormat) => client.readInterchange(source, format), []),
    previewInterchange: useCallback((input: InterchangeInput) => client.previewInterchange(input), []),
    createInterchange: useCallback((input: InterchangeInput, digest: string, context?: { generation: number; checking: boolean }) => {
      const frozen = { ...input }
      const generation = context?.generation
      const checking = context?.checking ?? false
      return clipboardTask(async () => {
        let confirmed = false
        try {
          await show(async () => {
            if (generation !== undefined && !checking && generation !== serviceGenerationRef.current) {
              throw new ApiError('preview_changed', 'The service changed. Read and preview the source again before creating a drawing.', 400)
            }
            const created = await client.createInterchange(frozen, digest)
            if (generation !== undefined && generation !== serviceGenerationRef.current) {
              throw new ApiError('create_unconfirmed', 'The service changed before this creation was confirmed. Check Saved documents before creating another drawing.', 401)
            }
            confirmed = true
            return created
          })
          refreshList()
        } catch (failure) {
          if (confirmed && failure instanceof ApiError && failure.code === 'draft_not_written') {
            // Creation is already confirmed. Report the old drawing's draft issue
            // without inviting a second creation with a fresh request ID.
            report(failure)
            refreshList()
            return
          }
          if (failure instanceof ApiError && failure.code === 'create_unconfirmed') refreshList()
          throw failure
        }
      })
    }, [clipboardTask, show, refreshList, report]),
    isCurrentConversion: useCallback((snapshot: DocumentView, generation: number) =>
      serviceGenerationRef.current === generation && currentRef.current !== null &&
      sameConversionSnapshot(currentRef.current.document, snapshot), []),
    inspectValence: useCallback((id: string) => client.inspectValence(id), []),
    inspectStereo: useCallback((id: string) => client.inspectStereo(id), []),
    // The editor asks for the stored configurations only: `stereogenic` would scan every candidate, and chem.cip has no time limit.
    inspectCip: useCallback((id: string) => client.inspectCip(id, { scope: 'stored' }), []),
    previewCt: useCallback((id: string, revision: number, format: CtImport['format'], source: string, record: number) =>
      client.previewCt(id, revision, format, source, record), []),
    exportCt: useCallback((id: string, format: CtImport['format'], ctab: CtabChoice, acceptedLosses?: string) =>
      client.exportCt(id, format, ctab, acceptedLosses), []),
    exportInterchange: useCallback(async (id: string, format: InterchangeFormat, acceptedLosses?: string) => {
      const drawing = currentRef.current?.document
      const generation = serviceGenerationRef.current
      const stillShown = () => drawing !== undefined && currentRef.current !== null &&
        serviceGenerationRef.current === generation && sameConversionSnapshot(currentRef.current.document, drawing)
      const stale = () => new ApiError('preview_changed', 'The drawing or service changed. Write the export again.', 200)
      if (drawing?.id !== id || !stillShown()) throw stale()
      try {
        const reply = await client.exportInterchange(id, format, acceptedLosses)
        if (!stillShown() || !sameConversionSnapshot(drawing, reply.snapshot)) throw stale()
        return reply
      } catch (failure) {
        // Consent for a drawing that is no longer shown must not become an active offer.
        if (!stillShown()) throw stale()
        throw failure
      }
    }, []),
    importCt: useCallback((input: CtImport, snapshot: DocumentView, generation: number) =>
      importDocument(input, snapshot, generation, () => client.importCt(input)), [importDocument]),
    previewSmiles: useCallback((id: string, revision: number, source: string) =>
      client.previewSmiles(id, revision, source), []),
    exportSmiles: useCallback((id: string, style: SmilesStyle, acceptedLosses?: string) =>
      client.exportSmiles(id, style, acceptedLosses), []),
    /** Typed and clipboard SMILES share one immutable, process-bound import attempt. */
    importSmiles: useCallback(async (input: SmilesImport, snapshot: DocumentView, generation: number): Promise<ImportResult> => {
      const frozen = { ...input }
      try {
        await clipboardTask(async () => {
          const shown = currentRef.current?.document
          if (frozen.documentId !== snapshot.id || frozen.revision !== snapshot.revision ||
              !shown || generation !== serviceGenerationRef.current || !sameConversionSnapshot(shown, snapshot)) {
            throw new ApiError('preview_changed', 'The drawing or service changed. Preview the SMILES text again before importing.', 0)
          }
          clipboardAttempt.current = {
            kind: 'smiles_import',
            attempt: client.createClipboardSmilesImport(frozen),
            documentId: frozen.documentId,
            generation,
          }
          setClipboardAction('smiles_import')
          await finishClipboardMutation()
        })
        return { committed: true }
      } catch (failure) {
        return { committed: false, failure: asApiError(failure) }
      }
    }, [clipboardTask, finishClipboardMutation]),
    inspectAromaticity: useCallback((id: string) => client.inspectAromaticity(id), []),
    convertAromaticity: useCallback((to: 'aromatic' | 'kekule', inspected: DocumentView, generation: number) => {
      const pressed = new Set(selectionRef.current)
      return mutate((document, restartedFrom) => {
        if (restartedFrom || generation !== serviceGenerationRef.current ||
            !sameContent(inspected, document.document) || inspected.revision !== document.document.revision) return null
        const selected = selectionSnapshot(pressed, document.document)
        return client.convertAromaticity(inspected.id, inspected.revision, to, selected)
      })
    }, [mutate]),
    error,
    autosave,
    copyNotice,
    dismissError: useCallback(() => setError(null), []),
    refreshList,
    create: useCallback(
      (title?: string) =>
        run(async () => {
          try {
            await show(() => client.create(title))
          } catch (failure) {
            // The list shows whether the unconfirmed document exists.
            if (failure instanceof ApiError && failure.code === 'create_unconfirmed') refreshList()
            throw failure
          }
        }),
      [run, show, refreshList],
    ),
    open: useCallback((documentId: string) => run(() => show(() => client.get(documentId))), [run, show]),
    saveAs: useCallback(
      (title?: string) => copy((shown) => client.saveAs(shown.document.id, shown.document.revision, title), false),
      [copy],
    ),
    /** Makes the abandoned draft the working copy, unsaved, with its undo history. */
    recover: useCallback(() => run(async () => {
      const recovered = await client.recover(id())
      setCurrent(recovered)
      adoptSelection(selectionIds(recovered.document.selection))
    }), [run, setCurrent, adoptSelection]),
    recoverAsCopy: useCallback(() => copy((shown) => client.recoverAsCopy(shown.document.id), true), [copy]),
    discardDraft: useCallback(
      () =>
        run(async () => {
          const shownId = id()
          await client.discardDraft(shownId)
          if (currentRef.current?.document.id === shownId) setCurrent(await client.get(shownId))
          refreshList()
        }),
      [run, setCurrent, refreshList],
    ),
    /** Commits a new title for a listed document; refused while it has unsaved changes. */
    rename: useCallback(
      (documentId: string, expectedRevision: number, title: string) =>
        run(async () => {
          try {
            await client.rename(documentId, expectedRevision, title)
          } finally {
            // Refused or not, the list shows the document's current revision and title.
            refreshList()
          }
        }),
      [run, refreshList],
    ),
    /** Moves a listed document to the trash at the revision the user confirmed. */
    remove: useCallback(
      (documentId: string, expectedRevision: number) =>
        run(async () => {
          try {
            await client.delete(documentId, expectedRevision)
          } finally {
            // Refused or not, the list shows the document's current state.
            refreshList()
          }
        }),
      [run, refreshList],
    ),
    restore: useCallback(
      (documentId: string) =>
        run(async () => {
          try {
            await client.restore(documentId)
          } finally {
            // Refused or not (restored elsewhere, say), the lists show where the document is.
            refreshList()
          }
        }),
      [run, refreshList],
    ),
    /** Re-reads the list, and the working copy if one is shown, e.g. after a revision conflict. */
    reload: useCallback(
      () =>
        run(async () => {
          refreshList()
          const shown = currentRef.current
          if (shown) setCurrent(await client.get(shown.document.id))
        }),
      [run, setCurrent, refreshList],
    ),
    apply: useCallback(
      (edit: Edit, selectionEdit?: EditSelection) => {
        const shown = currentRef.current?.document
        // Capture at the press, before a caller can clear selection and before the queue runs.
        const pressed = new Set(selectionEdit?.kind === 'gesture' ? selectionEdit.before : selectionRef.current)
        let groupSource: DocumentView | undefined
        const snapshots = (document: DocumentView) => {
          const before = selectionSnapshot(pressed, document)
          groupSource = document
          return {
            before,
            ...(selectionEdit?.kind === 'group' ? { after: selectionAfterGroup(before, selectionEdit.objects) } : {}),
          }
        }
        return mutate((document, restartedFrom) => {
          if (typeof edit === 'function') {
            const commands = edit(document.document)
            return commands?.length ? client.transact(id(), revision(document), commands, snapshots(document.document)) : null
          }
          if (!edit.length || shown?.id !== document.document.id) return null
          if (restartedFrom) {
            // Built on an older view than the one the restarted service matched: it lapses.
            if (restartedFrom.document.revision !== shown.revision) return null
            return client.transact(shown.id, revision(document), edit, snapshots(document.document))
          }
          return client.transact(shown.id, shown.revision, edit, snapshots(document.document))
        }, (response) => {
          if (selectionEdit?.kind === 'group' && groupSource) {
            adoptSelection(mergeGroupSelection(selectionRef.current, selectionEdit.objects, groupSource, response.document))
          } else {
            adoptSelection(selectionIds(selectionSnapshot(selectionRef.current, response.document)))
          }
          // An edit that cleared a mark or a stored configuration so that the change could apply
          // (a bond's order, say) says what it removed; the reply's notes list only the cascade.
          if (groupSource && holdsStereo(groupSource)) {
            const text = describeStereoChange(groupSource, response.document, { kind: 'none', why: null })
            if (text) setStereoStatus({ documentId: response.document.id, revision: response.document.revision, text })
          }
        })
      },
      [mutate, adoptSelection],
    ),
    stereoStatus,
    /**
     * Sends a mark gesture built from the latest document (null: it no longer applies): the mark
     * and the configuration the drawing shows in one transaction. If the service will not
     * derive the configuration, the mark is sent alone and the reason is the status. A gesture
     * the editor refused sends nothing and only says why.
     */
    markBond: useCallback((build: (latest: DocumentView) => MarkGesture | null) => {
      const pressed = new Set(selectionRef.current)
      let before: DocumentView | null = null
      let outcome: DeriveOutcome = { kind: 'none', why: null }
      return mutate((document) => {
        before = null
        const gesture = build(document.document)
        if (!gesture) return null
        if (gesture.kind === 'refused') {
          setStereoStatus({ documentId: document.document.id, revision: document.document.revision, text: gesture.text })
          return null
        }
        const shown = document.document
        const selection = { before: selectionSnapshot(pressed, shown) }
        return sendMark((commands) => client.transact(shown.id, shown.revision, commands, selection), gesture).then((sent) => {
          before = shown
          outcome = sent.outcome
          return sent.response
        })
      }, (response) => {
        adoptSelection(selectionIds(selectionSnapshot(selectionRef.current, response.document)))
        const text = before && describeStereoChange(before, response.document, outcome)
        if (text) setStereoStatus({ documentId: response.document.id, revision: response.document.revision, text })
      })
    }, [mutate, adoptSelection]),
    /**
     * Resolves one row of the stereo comparison, or all its disagreements, built from the latest
     * document (null: what it names is gone, so nothing is sent and the status says so). One
     * transaction, so one undo step; the status names the stated value that was replaced.
     */
    resolveStereo: useCallback((resolution: Resolution) => {
      const pressed = new Set(selectionRef.current)
      let before: DocumentView | null = null
      return mutate((document) => {
        before = null
        const commands = resolutionCommands(document.document, resolution)
        if (!commands) {
          const { id, revision: shownRevision } = document.document
          setStereoStatus({ documentId: id, revision: shownRevision, text: 'The drawing changed, so nothing was changed. Check the comparison again.' })
          return null
        }
        const shown = document.document
        before = shown
        return client.transact(shown.id, shown.revision, commands, { before: selectionSnapshot(pressed, shown) })
      }, (response) => {
        adoptSelection(selectionIds(selectionSnapshot(selectionRef.current, response.document)))
        const text = before && resolutionStatus(before, response.document, resolution)
        if (text) setStereoStatus({ documentId: response.document.id, revision: response.document.revision, text })
      })
    }, [mutate, adoptSelection]),
    /**
     * Edits enhanced stereo groups for one click, built from the latest document and the atoms
     * selected at the press (`planGroupRequest` decides, so a click that no longer applies sends
     * nothing and says why). One transaction, so one undo step; the page's status names the result
     * and a refusal reaches the error notice with nothing changed.
     */
    editGroups: useCallback((request: GroupRequest) => {
      const pressed = new Set(selectionRef.current)
      let before: DocumentView | null = null
      return mutate((document) => {
        before = null
        const shown = document.document
        const plan = planGroupRequest(shown, [...pressed], request)
        if (!plan.ok) {
          setStereoStatus({ documentId: shown.id, revision: shown.revision, text: `Nothing was changed: ${plan.why}` })
          return null
        }
        before = shown
        return client.transact(shown.id, shown.revision, plan.commands, { before: selectionSnapshot(pressed, shown) })
      }, (response) => {
        adoptSelection(selectionIds(selectionSnapshot(selectionRef.current, response.document)))
        if (before) setStereoStatus({ documentId: response.document.id, revision: response.document.revision, text: groupStatus(before, response.document, request) })
      })
    }, [mutate, adoptSelection]),
    // Undo and redo are not retried after a restart: the restarted service's history can
    // differ from the one this page saw even when the drawing matches.
    resetSettings: useCallback((to: SettingsSource) => {
      const pressed = new Set(selectionRef.current)
      const shown = currentRef.current?.document.id
      return mutate((document) => {
        if (shown !== document.document.id) return null
        const selection = selectionSnapshot(pressed, document.document)
        return client.resetSettings(shown, revision(document), to, { before: selection, after: selection })
      })
    }, [mutate]),
    importSettings: useCallback((source: string) => {
      const pressed = new Set(selectionRef.current)
      const shown = currentRef.current?.document.id
      return mutate((document) => {
        if (shown !== document.document.id || shown === source) return null
        const selection = selectionSnapshot(pressed, document.document)
        return client.importSettings(shown, revision(document), source, { before: selection, after: selection })
      })
    }, [mutate]),
    undo: useCallback(
      () => {
        const version = selectionVersion.current
        return mutate((document, restartedFrom) => (restartedFrom ? null : client.revisionAction(id(), 'undo', revision(document))), (response) => {
          const next = selectionVersion.current === version && response.document.selection
            ? selectionIds(response.document.selection)
            : selectionIds(selectionSnapshot(selectionRef.current, response.document))
          adoptSelection(next)
        })
      },
      [mutate, adoptSelection],
    ),
    redo: useCallback(
      () => {
        const version = selectionVersion.current
        return mutate((document, restartedFrom) => (restartedFrom ? null : client.revisionAction(id(), 'redo', revision(document))), (response) => {
          const next = selectionVersion.current === version && response.document.selection
            ? selectionIds(response.document.selection)
            : selectionIds(selectionSnapshot(selectionRef.current, response.document))
          adoptSelection(next)
        })
      },
      [mutate, adoptSelection],
    ),
    save: useCallback(() => mutate((document) => client.save(id(), revision(document))), [mutate]),
    close: useCallback(
      (discardUnsaved: boolean) => {
        // The revision on screen when the user confirmed: a discard never drops edits that
        // another client made after it.
        const shown = currentRef.current?.document.revision
        return run(async () => {
          await client.close(id(), discardUnsaved, shown)
          setCurrent(null)
          refreshList()
        })
      },
      [run, setCurrent, refreshList],
    ),
  }
}

export type DocumentEditor = ReturnType<typeof useDocumentEditor>
