import { useCallback, useEffect, useRef, useState } from 'react'

import { ApiError, type DocumentClient, type DocumentResponse } from '@/lib/document-contract'

/** A recovery draft is written this long after the last change… */
export const AUTOSAVE_DELAY_MS = 1000
/** …and at least this often while changes keep coming. */
export const AUTOSAVE_MAX_WAIT_MS = 5000
const RETRY_DELAYS_MS = [2000, 5000, 15_000, 30_000]

export type AutosaveState =
  | { kind: 'clean' }
  | { kind: 'waiting' }
  | { kind: 'saving' }
  | { kind: 'saved'; revision: number; at: number }
  | { kind: 'failed'; message: string; retryAt: number }
  /** Needs the user: an earlier draft must be recovered or discarded first. */
  | { kind: 'paused'; message: string }

const PAUSING = new Set(['recovery_pending', 'draft_in_use'])

/**
 * Keeps the open document's recovery draft current. It runs beside the edit queue, so drafts
 * never hold up drawing; the server serializes them with edits and drafts whatever its working
 * copy holds. Failures retry with backoff; `retryNow` skips the wait.
 */
export function useAutosave(
  client: DocumentClient,
  current: DocumentResponse | null,
  onRestart: (error: ApiError) => void,
) {
  const [state, setState] = useState<AutosaveState>({ kind: 'clean' })
  const latest = useRef(current)
  const covered = useRef<{ id: string; revision: number } | null>(null)
  const firstPending = useRef<number | null>(null)
  const attempts = useRef(0)
  const saving = useRef(false)
  /** Why autosave waits for the user, and what lifts it: the earlier draft being resolved,
   * or any newer view of the document (after a restart, the page must reload or resend). */
  const paused = useRef<{ until: 'recovery' | 'change' } | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const evaluate = useRef<() => void>(() => {})

  const save = useCallback(async () => {
    const document = latest.current
    if (!document || saving.current) return
    const id = document.document.id
    saving.current = true
    setState({ kind: 'saving' })
    try {
      const response = await client.autosave(id)
      attempts.current = 0
      const revision = response.session.draft_revision
      // Revisions only grow, so a draft at or past the page's revision is the newest work. A
      // service with nothing unsaved (another client saved) needs no draft either.
      covered.current = { id, revision: revision ?? response.document.revision }
      setState(revision === null ? { kind: 'clean' } : { kind: 'saved', revision, at: Date.now() })
    } catch (failure) {
      const error = failure instanceof ApiError ? failure : new ApiError('unexpected', 'Autosave failed.', 0)
      if (PAUSING.has(error.code)) {
        paused.current = { until: 'recovery' }
        setState({ kind: 'paused', message: error.message })
      } else if (error.code === 'service_restarted') {
        paused.current = { until: 'change' }
        setState({ kind: 'paused', message: error.message })
        onRestart(error)
      } else {
        const delay = RETRY_DELAYS_MS[Math.min(attempts.current, RETRY_DELAYS_MS.length - 1)]
        attempts.current += 1
        setState({ kind: 'failed', message: error.message, retryAt: Date.now() + delay })
      }
    } finally {
      saving.current = false
      evaluate.current()
    }
  }, [client, onRestart])

  useEffect(() => {
    evaluate.current = () => {
      clearTimeout(timer.current)
      const document = latest.current
      if (!document || !document.session.unsaved_changes) {
        firstPending.current = null
        attempts.current = 0
        if (!saving.current) setState({ kind: 'clean' })
        return
      }
      const { id, revision } = document.document
      if (covered.current?.id === id && covered.current.revision >= revision) {
        firstPending.current = null
        setState((previous) => (previous.kind === 'saved' ? previous : { kind: 'saved', revision, at: Date.now() }))
        return
      }
      if (paused.current !== null || saving.current) return
      const now = Date.now()
      firstPending.current ??= now
      const retrying = attempts.current > 0
      const wait = retrying
        ? RETRY_DELAYS_MS[Math.min(attempts.current - 1, RETRY_DELAYS_MS.length - 1)]
        : Math.min(AUTOSAVE_DELAY_MS, Math.max(0, firstPending.current + AUTOSAVE_MAX_WAIT_MS - now))
      if (!retrying) setState({ kind: 'waiting' })
      timer.current = setTimeout(() => void save(), wait)
    }
  }, [save])

  const id = current?.document.id
  const recoveryState = current?.recovery?.state
  useEffect(() => {
    // A different document starts over; the draft it already has counts as covered.
    if (latest.current?.document.id !== id || paused.current?.until === 'change') {
      firstPending.current = null
      attempts.current = 0
      paused.current = null
    }
    latest.current = current
    const draft = current?.session.draft_revision
    if (current && draft !== null && draft !== undefined) {
      covered.current = { id: current.document.id, revision: draft }
    } else if (covered.current?.id !== id) {
      covered.current = null
    }
    evaluate.current()
  }, [current, id])

  // Recovering or discarding the earlier draft unblocks autosave.
  useEffect(() => {
    if (recoveryState === undefined && paused.current?.until === 'recovery') {
      paused.current = null
      evaluate.current()
    }
  }, [recoveryState])

  useEffect(() => () => clearTimeout(timer.current), [])

  const retryNow = useCallback(() => {
    clearTimeout(timer.current)
    paused.current = null
    void save()
  }, [save])

  return { state, retryNow }
}
