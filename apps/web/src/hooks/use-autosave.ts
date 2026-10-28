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
/** How long after the service's own retry is due the page reads the document to show its outcome. */
const RECHECK_AFTER_RETRY_MS = 1000

export type AutosaveState =
  | { kind: 'clean' }
  | { kind: 'waiting' }
  | { kind: 'saving' }
  /** `at` is known when this page wrote the draft. */
  | { kind: 'saved'; revision: number; at: number | null }
  /**
   * Drafting keeps failing. With `by: 'service'` the service has recorded the failure
   * (`session.draft_failure`, or the page's own failed attempt until that record is read): it
   * retries on its own schedule and counts every attempt, the page's included. A service from
   * before that record leaves the page to keep its own.
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
   * Re-reads the document: the page's view of earlier drafts is out of date. Settles once the
   * read has ended; a failed read leaves the view as it was.
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
 * A failure the service reports for the shown document is shown as it reports it, and the
 * page follows the service's retry instead of keeping a schedule of its own: it reads the
 * document again once that retry is due. Like the draft revision, the failure is read from
 * the view, so a save, a recovery, a restart, or another document never shows it.
 */
export function useAutosave(client: DocumentClient, current: DocumentResponse | null, host: AutosaveHost) {
  const [state, setState] = useState<AutosaveState>({ kind: 'clean' })
  const hostRef = useRef(host)
  /** The document the bookkeeping below describes. */
  const owner = useRef<string | null>(null)
  const firstPending = useRef<number | null>(null)
  const attempts = useRef(0)
  /**
   * The page's own failed attempt, with the view it was made from. `recorded` when the service
   * recorded it as its own draft failure; then `missedReads` counts reads of that record that
   * left the view on screen.
   */
  const failure = useRef<{
    message: string
    reason: StorageReason | null
    retryAt: number
    view: DocumentResponse
    recorded: boolean
    missedReads: number
  } | null>(null)
  /**
   * When the service's reported retry is due, fixed when the view reporting it is first shown,
   * and how many reads of its outcome since then left that view on screen.
   */
  const reported = useRef<{ view: DocumentResponse; retryAt: number; missedReads: number } | null>(null)
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
      failure.current = { message, reason, retryAt: Date.now() + delay, view, recorded, missedReads: 0 }
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
          fail('The service holds a different state than this page shows; reloading it.', null, view, false)
          void hostRef.current.refresh(id)
        } catch (thrown) {
          if (owner.current !== id) return
          const error = thrown instanceof ApiError ? thrown : new ApiError('unexpected', 'Autosave failed.', 0)
          if (error.code === 'service_restarted') {
            stoppedOn.current = { view: hostRef.current.shown() ?? view, message: error.message }
            hostRef.current.onRestart(error)
            return
          }
          // A service that reports its drafting failures has recorded this one with the retry
          // it will make; read it, so the page shows and follows that one record.
          const recorded = view.session.draft_failure !== undefined
          fail(error.message, error.reason ?? null, view, recorded)
          if (STALE_VIEW.has(error.code) || recorded) void hostRef.current.refresh(id)
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
      // Reads the document once a retry at `retryAt` is due, and again with the page's backoff
      // while reads fail and leave `view` on screen, so a stale failure is not left showing.
      const readOutcome = (retryAt: number, reads: { missedReads: number }, current: () => boolean) => {
        const wait =
          reads.missedReads === 0
            ? Math.max(0, retryAt - now) + RECHECK_AFTER_RETRY_MS
            : RETRY_DELAYS_MS[Math.min(reads.missedReads - 1, RETRY_DELAYS_MS.length - 1)]
        timer.current = setTimeout(() => {
          void hostRef.current.refresh(document.id).then(() => {
            if (!current() || hostRef.current.shown() !== view) return
            reads.missedReads += 1
            evaluate.current()
          })
        }, wait)
      }
      const report = session.draft_failure
      if (report) {
        if (reported.current?.view !== view) reported.current = { view, retryAt: now + report.retry_in_ms, missedReads: 0 }
        const outcome = reported.current
        const { retryAt } = outcome
        setState({
          kind: 'failed',
          by: 'service',
          attempts: report.attempts,
          message: report.message,
          reason: report.reason ?? null,
          retryAt,
        })
        readOutcome(retryAt, outcome, () => reported.current === outcome)
        return
      }
      const failed = failure.current
      if (failed && report === null && failed.view !== view) {
        // A newer view from a service that reports failures shows none for this work.
        failure.current = null
        attempts.current = 0
      } else if (failed) {
        // A service that reports failures recorded this one and retries it on its own schedule,
        // which the page's matches; the page reads that record rather than drafting beside it.
        // Otherwise the page retries, and edits meanwhile do not push its retry back.
        setState({
          kind: 'failed',
          by: failed.recorded ? 'service' : 'page',
          attempts: attempts.current,
          message: failed.message,
          reason: failed.reason,
          retryAt: failed.retryAt,
        })
        if (failed.recorded) readOutcome(failed.retryAt, failed, () => failure.current === failed)
        else timer.current = setTimeout(() => void save.current(), Math.max(0, failed.retryAt - now))
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
      reported.current = null
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
