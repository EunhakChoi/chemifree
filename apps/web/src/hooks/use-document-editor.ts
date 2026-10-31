import { useCallback, useEffect, useRef, useState } from 'react'

import { useAutosave } from '@/hooks/use-autosave'
import { mergeGroupSelection, selectionAfterGroup, selectionIds, selectionSnapshot } from '@/editor/scene-selection'
import {
  ApiError,
  DocumentClient,
  type Command,
  type CopyResponse,
  type DocumentListResponse,
  type DocumentResponse,
  type DocumentView,
  type ListQuery,
  type SettingsSource,
  type TrashListResponse,
} from '@/lib/document-contract'

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

/** Grouping replaces the members selected at the press with the newly created group. */
export type EditSelection = { kind: 'group'; objects: string[] }

const client = new DocumentClient()

/** Same title, graph, scene, and settings, whatever revision number names them. */
function sameContent(a: DocumentView, b: DocumentView) {
  return (
    a.id === b.id &&
    a.title === b.title &&
    JSON.stringify(a.atoms) === JSON.stringify(b.atoms) &&
    JSON.stringify(a.bonds) === JSON.stringify(b.bonds) &&
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
  const [serviceGeneration, setServiceGeneration] = useState(0)
  const [selection, setSelectionState] = useState<Set<string>>(new Set())
  const selectionRef = useRef<Set<string>>(new Set())
  const selectionVersion = useRef(0)
  const currentRef = useRef<DocumentResponse | null>(null)
  const queue = useRef<Promise<void>>(Promise.resolve())
  const failures = useRef(0)

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
    setServiceGeneration((generation) => generation + 1)
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
   * when this task was queued, never one from a request that failed while it waited.
   */
  const run = useCallback((task: () => Promise<void>) => {
    setPending((count) => count + 1)
    const failuresSeen = failures.current
    const next = queue.current
      .then(task)
      .then(
        () => {
          if (failures.current === failuresSeen) setError(null)
        },
        (failure: unknown) => {
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
        const same = shown?.document.revision === response.document.revision && sameContent(shown.document, response.document)
        if (same) setCurrent(response)
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
              if (currentRef.current?.document.id === documentId) setCurrent(fresh)
              resolve()
            } catch (failure) {
              reject(failure)
            }
          })
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
    inspectProperties: useCallback((...args: Parameters<DocumentClient['inspectProperties']>) => client.inspectProperties(...args), []),
    selection,
    /** A snapshot for a command made now, including changes not rendered yet. */
    selectionAtPress: useCallback(() => new Set(selectionRef.current), []),
    setSelection,
    /** Holds later document edits until a dependent preference write has finished. */
    reserveStyle: useCallback(() => {
      let reached = () => {}
      let release = () => {}
      const ready = new Promise<void>((resolve) => { reached = () => resolve() })
      const released = new Promise<void>((resolve) => { release = () => resolve() })
      void run(async () => {
        reached()
        await released
      })
      return { ready, release }
    }, [run]),
    busy: pending > 0,
    inspectValence: useCallback((id: string) => client.inspectValence(id), []),
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
        const pressed = new Set(selectionRef.current)
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
        })
      },
      [mutate, adoptSelection],
    ),
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
