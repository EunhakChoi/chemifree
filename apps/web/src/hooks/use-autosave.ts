import { useCallback, useEffect, useRef, useState } from 'react'

import {
  ApiError,
  type DocumentClient,
  type DocumentResponse,
  type RecoveryState,
  type StorageReason,
} from '@/lib/document-contract'

/** A recovery draft is written this long after the last change… */
export const AUTOSAVE_DELAY_MS = 1000
/** …and at least this often while changes keep coming. */
export const AUTOSAVE_MAX_WAIT_MS = 5000
const RETRY_DELAYS_MS = [2000, 5000, 15_000, 30_000]
/** How long after the service's retry is due the page reads the document to see its outcome. */
const OUTCOME_READ_AFTER_MS = 1000

export type AutosaveState =
  | { kind: 'clean' }
  | { kind: 'waiting' }
  | { kind: 'saving' }
  /** `at` is known when this page wrote the draft. */
  | { kind: 'saved'; revision: number; at: number | null }
  /**
   * Drafting keeps failing. `by: 'service'`: the service recorded the failure (as
   * `session.draft_failure`) and retries it on its own schedule, counting every attempt, the
   * page's included; the page only reads the outcome. `by: 'page'`: the page retries, as with
   * a service from before that record. `reason` is the storage failure's, when known.
   */
  | {
      kind: 'failed'
      by: 'service' | 'page'
      attempts: number
      message: string
      reason: StorageReason | null
      retryAt: number
    }
  /** An earlier session's draft must be recovered or discarded (or its process end) first. */
  | { kind: 'blocked'; recovery: RecoveryState }
  /** The service restarted; nothing can be drafted until the page shows its new state. */
  | { kind: 'stopped'; message: string }

export interface AutosaveHost {
  /** The document on screen now, ahead of the rendered `current` while updates are pending. */
  shown: () => DocumentResponse | null
  /** Shows an autosave reply if it describes the document on screen; says whether it did. */
  adopt: (response: DocumentResponse) => boolean
  /**
   * Re-reads the document from the service process the page writes to: the page's view of
   * its drafts is out of date. Settles once the read has ended, and rejects with its error (a
   * restarted service is `service_restarted`), leaving the view as it was.
   */
  refresh: (id: string) => Promise<void>
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
 *
 * A service that records its drafting failures retries them itself, so the page does not
 * draft beside it: it shows the record and reads the document again once the service's retry
 * is due, and again with backoff while those reads fail. Like the draft revision, the record
 * is read from the view, so a save, a recovery, or another document never shows it, and a
 * read that finds the service restarted stops drafting rather than showing what the new
 * process holds.
 */
export function useAutosave(client: DocumentClient, current: DocumentResponse | null, host: AutosaveHost) {
  const [state, setState] = useState<AutosaveState>({ kind: 'clean' })
  const hostRef = useRef(host)
  /** The document the bookkeeping below describes. */
  const owner = useRef<string | null>(null)
  const firstPending = useRef<number | null>(null)
  const attempts = useRef(0)
  /**
   * The page's own failed attempt, made from `view`. `recorded` when the service records
   * drafting failures, so it has recorded this one and will retry it.
   */
  const failure = useRef<{
    message: string
    reason: StorageReason | null
    retryAt: number
    view: DocumentResponse
    recorded: boolean
  } | null>(null)
  /**
   * The service's retry the page waits on while `view` is shown: when it is due (fixed when
   * first seen) and how many reads since then failed.
   */
  const following = useRef<{ view: DocumentResponse; retryAt: number; missedReads: number } | null>(null)
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
    const fail = (message: string, reason: StorageReason | null, view: DocumentResponse, recorded: boolean) => {
      const delay = RETRY_DELAYS_MS[Math.min(attempts.current, RETRY_DELAYS_MS.length - 1)]
      attempts.current += 1
      failure.current = { message, reason, retryAt: Date.now() + delay, view, recorded }
    }
    const stop = (error: ApiError, view: DocumentResponse) => {
      stoppedOn.current = { view: hostRef.current.shown() ?? view, message: error.message }
      hostRef.current.onRestart(error)
    }
    /** Reads the document again; says whether the read landed. */
    const reread = (id: string, view: DocumentResponse) =>
      hostRef.current.refresh(id).then(
        () => true,
        (thrown: unknown) => {
          if (owner.current === id && thrown instanceof ApiError && thrown.code === 'service_restarted') {
            stop(thrown, view)
            evaluate.current()
          }
          return false
        },
      )

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
          // session): re-read it rather than draft again. Nothing failed on the service's side.
          fail('The service holds a different state than this page shows; reloading it.', null, view, false)
          void reread(id, view)
        } catch (thrown) {
          if (owner.current !== id) return
          const error = thrown instanceof ApiError ? thrown : new ApiError('unexpected', 'Autosave failed.', 0)
          if (error.code === 'service_restarted') {
            stop(error, view)
            return
          }
          // A service that reports drafting failures has recorded this one with its retry.
          const recorded = view.session.draft_failure !== undefined
          fail(error.message, error.reason ?? null, view, recorded)
          if (recorded || STALE_VIEW.has(error.code)) void reread(id, view)
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
      /** Waits on the service's retry at `retryAt`, then reads what it did. */
      const follow = (retryAt: number) => {
        if (following.current?.view !== view) following.current = { view, retryAt, missedReads: 0 }
        const reads = following.current
        const wait =
          reads.missedReads === 0
            ? Math.max(0, reads.retryAt - now) + OUTCOME_READ_AFTER_MS
            : RETRY_DELAYS_MS[Math.min(reads.missedReads - 1, RETRY_DELAYS_MS.length - 1)]
        timer.current = setTimeout(() => {
          void reread(document.id, view).then((landed) => {
            if (landed || following.current !== reads || hostRef.current.shown() !== view) return
            reads.missedReads += 1
            evaluate.current()
          })
        }, wait)
        return reads.retryAt
      }
      const report = session.draft_failure
      if (report) {
        setState({
          kind: 'failed',
          by: 'service',
          attempts: report.attempts,
          message: report.message,
          reason: report.reason ?? null,
          retryAt: follow(now + report.retry_in_ms),
        })
        return
      }
      const failed = failure.current
      if (failed && report === null && failed.view !== view) {
        // A newer view from a service that records failures shows none for this work.
        failure.current = null
        attempts.current = 0
      } else if (failed?.recorded) {
        setState({
          kind: 'failed',
          by: 'service',
          attempts: attempts.current,
          message: failed.message,
          reason: failed.reason,
          retryAt: follow(failed.retryAt),
        })
        return
      } else if (failed) {
        // The retry keeps its time; edits meanwhile do not push it back.
        const { message, reason, retryAt } = failed
        setState({ kind: 'failed', by: 'page', attempts: attempts.current, message, reason, retryAt })
        timer.current = setTimeout(() => void save.current(), Math.max(0, retryAt - now))
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
      following.current = null
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
