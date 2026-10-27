import { useCallback, useEffect, useRef, useState } from 'react'

import {
  ApiError,
  DocumentClient,
  type Command,
  type DocumentListResponse,
  type DocumentResponse,
  type DocumentView,
} from '@/lib/document-contract'

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

  return {
    list,
    current,
    busy: pending > 0,
    error,
    dismissError: useCallback(() => setError(null), []),
    refreshList,
    create: useCallback(
      (title?: string) => run(async () => setCurrent(await client.create(title))),
      [run, setCurrent],
    ),
    open: useCallback((documentId: string) => run(async () => setCurrent(await client.get(documentId))), [run, setCurrent]),
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
      (discardUnsaved: boolean) =>
        run(async () => {
          await client.close(id(), discardUnsaved)
          setCurrent(null)
          refreshList()
        }),
      [run, setCurrent, refreshList],
    ),
  }
}

export type DocumentEditor = ReturnType<typeof useDocumentEditor>
