import { useCallback, useEffect, useRef, useState } from 'react'

import { ApiError, type DocumentClient, type DocumentResponse, type RecoveryState } from '@/lib/document-contract'

/** A recovery draft is written this long after the last change… */
export const AUTOSAVE_DELAY_MS = 1000
/** …and at least this often while changes keep coming. */
export const AUTOSAVE_MAX_WAIT_MS = 5000
const RETRY_DELAYS_MS = [2000, 5000, 15_000, 30_000]

export type AutosaveState =
  | { kind: 'clean' }
  | { kind: 'waiting' }
  | { kind: 'saving' }
  /** `at` is known when this page wrote the draft. */
  | { kind: 'saved'; revision: number; at: number | null }
  | { kind: 'failed'; message: string; retryAt: number }
  /** An earlier session's draft must be recovered or discarded (or its process end) first. */
  | { kind: 'blocked'; recovery: RecoveryState }
  /** The service restarted; nothing can be drafted until the page shows its new state. */
  | { kind: 'stopped'; message: string }

export interface AutosaveHost {
  /** The document on screen now, ahead of the rendered `current` while updates are pending. */
  shown: () => DocumentResponse | null
  /** Shows an autosave reply if it describes the document on screen; says whether it did. */
  adopt: (response: DocumentResponse) => boolean
  /** Re-reads the document: the page's view of earlier drafts is out of date. */
  refresh: (id: string) => void
  onRestart: (error: ApiError) => void
}

/** Refusals over a draft that the page's view does not show yet. */
const STALE_VIEW = new Set(['recovery_pending', 'draft_in_use'])

/**
 * Keeps the open document's recovery draft current.
 *
 * Whether a draft covers the work on screen is read from the service's own view
 * (`session.draft_revision`) rather than remembered, so neither a restarted service nor
 * another document can pass for drafted work, and an earlier session's draft (`recovery`)
 * shows as blocking for as long as the view shows it. The pending window and retries
 * belong to one document and start over when another is shown. Drafts run beside the edit
 * queue; the service serializes them with edits.
 */
export function useAutosave(client: DocumentClient, current: DocumentResponse | null, host: AutosaveHost) {
  const [state, setState] = useState<AutosaveState>({ kind: 'clean' })
  const hostRef = useRef(host)
  /** The document the bookkeeping below describes. */
  const owner = useRef<string | null>(null)
  const firstPending = useRef<number | null>(null)
  const attempts = useRef(0)
  const failure = useRef<{ message: string; retryAt: number } | null>(null)
  /** The view on which a restart was noticed; any newer view lifts the stop. */
  const stoppedOn = useRef<{ view: DocumentResponse; message: string } | null>(null)
  const written = useRef<{ revision: number; at: number } | null>(null)
  /** A view the service has already moved past and drafted; the newer view is on its way. */
  const passed = useRef<{ view: DocumentResponse; revision: number } | null>(null)
  const inFlight = useRef<{ id: string; done: Promise<void> } | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const evaluate = useRef<() => void>(() => {})
  const save = useRef<() => Promise<void>>(() => Promise.resolve())

  useEffect(() => {
    hostRef.current = host
  })

  useEffect(() => {
    const settle = () => {
      firstPending.current = null
      attempts.current = 0
      failure.current = null
    }
    const fail = (message: string) => {
      const delay = RETRY_DELAYS_MS[Math.min(attempts.current, RETRY_DELAYS_MS.length - 1)]
      attempts.current += 1
      failure.current = { message, retryAt: Date.now() + delay }
    }

    save.current = () => {
      const view = hostRef.current.shown()
      if (!view) return Promise.resolve()
      const id = view.document.id
      if (inFlight.current?.id === id) return inFlight.current.done
      clearTimeout(timer.current)
      // The draft takes all the service holds now; later edits open a new window.
      firstPending.current = null
      setState({ kind: 'saving' })
      const attempt = { id, done: Promise.resolve() }
      inFlight.current = attempt
      attempt.done = (async () => {
        try {
          const response = await client.autosave(id)
          if (owner.current !== id) return
          const revision = response.session.draft_revision
          written.current = revision === null ? null : { revision, at: Date.now() }
          settle()
          if (hostRef.current.adopt(response) || hostRef.current.shown() !== view) return
          if (revision !== null && response.document.revision > view.document.revision) {
            // An edit still in flight (or another tab) moved the session on, and the draft
            // holds that newer state. Wait for the newer view instead of drafting again.
            passed.current = { view, revision }
            return
          }
          // The service went back or holds other content (a restart seen elsewhere, a closed
          // session): re-read it rather than draft again.
          fail('The service holds a different state than this page shows; reloading it.')
          hostRef.current.refresh(id)
        } catch (thrown) {
          if (owner.current !== id) return
          const error = thrown instanceof ApiError ? thrown : new ApiError('unexpected', 'Autosave failed.', 0)
          if (error.code === 'service_restarted') {
            stoppedOn.current = { view: hostRef.current.shown() ?? view, message: error.message }
            hostRef.current.onRestart(error)
            return
          }
          fail(error.message)
          if (STALE_VIEW.has(error.code)) hostRef.current.refresh(id)
        } finally {
          if (inFlight.current === attempt) inFlight.current = null
          evaluate.current()
        }
      })()
      return attempt.done
    }

    evaluate.current = () => {
      clearTimeout(timer.current)
      const view = hostRef.current.shown()
      if (!view) {
        setState({ kind: 'clean' })
        return
      }
      const stopped = stoppedOn.current
      if (stopped?.view === view) {
        setState({ kind: 'stopped', message: stopped.message })
        return
      }
      stoppedOn.current = null
      const ahead = passed.current
      if (ahead?.view === view) {
        const at = written.current?.revision === ahead.revision ? written.current.at : null
        setState({ kind: 'saved', revision: ahead.revision, at })
        return
      }
      passed.current = null
      const { session, recovery, document } = view
      const drafted = session.draft_revision
      if (!session.unsaved_changes || (drafted !== null && drafted >= document.revision)) {
        settle()
        const at = drafted !== null && written.current?.revision === drafted ? written.current.at : null
        setState(session.unsaved_changes && drafted !== null ? { kind: 'saved', revision: drafted, at } : { kind: 'clean' })
        return
      }
      if (recovery) {
        settle()
        setState({ kind: 'blocked', recovery: recovery.state })
        return
      }
      if (inFlight.current?.id === document.id) {
        setState({ kind: 'saving' })
        return
      }
      const now = Date.now()
      const failed = failure.current
      if (failed) {
        // The retry keeps its time; edits meanwhile do not push it back.
        setState({ kind: 'failed', ...failed })
        timer.current = setTimeout(() => void save.current(), Math.max(0, failed.retryAt - now))
        return
      }
      firstPending.current ??= now
      setState({ kind: 'waiting' })
      const wait = Math.min(AUTOSAVE_DELAY_MS, Math.max(0, firstPending.current + AUTOSAVE_MAX_WAIT_MS - now))
      timer.current = setTimeout(() => void save.current(), wait)
    }
  }, [client])

  useEffect(() => {
    const id = current?.document.id ?? null
    if (owner.current !== id) {
      owner.current = id
      firstPending.current = null
      attempts.current = 0
      failure.current = null
      stoppedOn.current = null
      written.current = null
      passed.current = null
    }
    evaluate.current()
  }, [current])

  useEffect(() => () => clearTimeout(timer.current), [])

  /** Tries now: retries a failure (keeping the backoff) or checks a blocking draft again. */
  const retryNow = useCallback(() => {
    failure.current = null
    void save.current()
  }, [])

  /**
   * Drafts the shown document's pending work before the page leaves it. Resolves to why
   * work was left undrafted, or null when there was none a draft could take.
   */
  const flush = useCallback(async () => {
    const view = hostRef.current.shown()
    if (!view) return null
    const pending = inFlight.current
    if (pending?.id === view.document.id) await pending.done
    const shown = hostRef.current.shown()
    if (shown?.document.id !== view.document.id) return null
    const { session, recovery, document } = shown
    const drafted = session.draft_revision
    if (!session.unsaved_changes || (drafted !== null && drafted >= document.revision)) return null
    if (recovery || stoppedOn.current?.view === shown) return null
    await save.current()
    return failure.current?.message ?? stoppedOn.current?.message ?? null
  }, [])

  return { state, retryNow, flush }
}
