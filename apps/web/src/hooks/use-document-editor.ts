import { useCallback, useEffect, useRef, useState } from 'react'

import { useAutosave } from '@/hooks/use-autosave'
import {
  ApiError,
  DocumentClient,
  type Command,
  type CopyResponse,
  type DocumentListResponse,
  type DocumentResponse,
  type DocumentView,
} from '@/lib/document-contract'

/** What became of the source document when the page moved to a copy of it. */
export type CopiedFrom =
  /** A recovered draft became the copy; the source's saved document is unchanged. */
  | { kind: 'recovered' }
  /** The source's session closed; its unsaved changes, if any, live only in the copy now. */
  | { kind: 'closed'; discardedChanges: boolean }
  /**
   * The copy is committed, but the source's recovery draft could not be removed, so its
   * session stays open with its unsaved changes rather than leave that draft behind.
   */
  | { kind: 'left_open' }

/** What the page is editing after a copy: a new document beside the original. */
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

/**
 * Commands for one transaction. A list is built from the document on screen and is sent with
 * that revision, so if an earlier queued request changed the document first, the server
 * rejects it as a revision conflict. Edits relative to the latest state (such as "one more
 * charge") pass a function instead, evaluated when the queued request runs. Returning null
 * skips the request.
 */
export type Edit = Command[] | ((document: DocumentView) => Command[] | null)

const client = new DocumentClient()

/** Same title and drawing, whatever revision number names them. */
function sameContent(a: DocumentView, b: DocumentView) {
  return (
    a.id === b.id &&
    a.title === b.title &&
    JSON.stringify(a.atoms) === JSON.stringify(b.atoms) &&
    JSON.stringify(a.bonds) === JSON.stringify(b.bonds)
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
  const currentRef = useRef<DocumentResponse | null>(null)
  const queue = useRef<Promise<void>>(Promise.resolve())
  const failures = useRef(0)

  const setCurrent = useCallback((document: DocumentResponse | null) => {
    currentRef.current = document
    setCurrentState(document)
  }, [])

  const [listRequest, setListRequest] = useState(0)

  useEffect(() => {
    let active = true
    client.list().then(
      (value) => active && setList({ kind: 'ready', list: value }),
      (failure: unknown) => active && setList({ kind: 'error', message: asApiError(failure).message }),
    )
    return () => {
      active = false
    }
  }, [listRequest])

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
    (call: Call) =>
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
        if (currentRef.current?.document.id === next.document.id) setCurrent(next)
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
    refresh: useCallback(
      (documentId: string) =>
        void run(async () => {
          const fresh = await client.get(documentId)
          if (currentRef.current?.document.id === documentId) setCurrent(fresh)
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
          setCurrent(copied)
          let source: CopiedFrom = { kind: 'recovered' }
          if (!recovered) {
            source = copied.source.closed
              ? { kind: 'closed', discardedChanges: copied.source.discarded_changes }
              : { kind: 'left_open' }
          }
          setCopyNotice({
            copyId: copied.document.id,
            sourceId: copied.source.id,
            sourceTitle: shown.document.title,
            source,
          })
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
    current,
    busy: pending > 0,
    error,
    autosave,
    /** Shown while the copy it describes is the open document. */
    copyNotice: copyNotice && current?.document.id === copyNotice.copyId ? copyNotice : null,
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
    recover: useCallback(() => run(async () => setCurrent(await client.recover(id()))), [run, setCurrent]),
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
    /** Re-reads the working copy, e.g. after a revision conflict. */
    reload: useCallback(() => run(async () => setCurrent(await client.get(id()))), [run, setCurrent]),
    apply: useCallback(
      (edit: Edit) => {
        const shown = currentRef.current?.document
        return mutate((document, restartedFrom) => {
          if (typeof edit === 'function') {
            const commands = edit(document.document)
            return commands?.length ? client.transact(id(), revision(document), commands) : null
          }
          if (!edit.length || shown?.id !== document.document.id) return null
          if (restartedFrom) {
            // Built on an older view than the one the restarted service matched: it lapses.
            if (restartedFrom.document.revision !== shown.revision) return null
            return client.transact(shown.id, revision(document), edit)
          }
          return client.transact(shown.id, shown.revision, edit)
        })
      },
      [mutate],
    ),
    // Undo and redo are not retried after a restart: the restarted service's history can
    // differ from the one this page saw even when the drawing matches.
    undo: useCallback(
      () => mutate((document, restartedFrom) => (restartedFrom ? null : client.revisionAction(id(), 'undo', revision(document)))),
      [mutate],
    ),
    redo: useCallback(
      () => mutate((document, restartedFrom) => (restartedFrom ? null : client.revisionAction(id(), 'redo', revision(document)))),
      [mutate],
    ),
    save: useCallback(() => mutate((document) => client.revisionAction(id(), 'save', revision(document))), [mutate]),
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
