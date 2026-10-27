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
    (call: (document: DocumentResponse) => Promise<DocumentResponse> | null) =>
      run(async () => {
        const document = currentRef.current
        if (!document) return
        const request = call(document)
        if (!request) return
        const next = await request
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
        return mutate((document) => {
          if (typeof edit === 'function') {
            const commands = edit(document.document)
            return commands?.length ? client.transact(id(), revision(document), commands) : null
          }
          if (!edit.length || shown?.id !== document.document.id) return null
          return client.transact(shown.id, shown.revision, edit)
        })
      },
      [mutate],
    ),
    undo: useCallback(() => mutate((document) => client.revisionAction(id(), 'undo', revision(document))), [mutate]),
    redo: useCallback(() => mutate((document) => client.revisionAction(id(), 'redo', revision(document))), [mutate]),
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
