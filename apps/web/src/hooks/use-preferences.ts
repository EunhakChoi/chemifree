import { useCallback, useEffect, useRef, useState } from 'react'

import { ApiError, DocumentClient, type PreferencesChange, type PreferencesResponse } from '@/lib/document-contract'
import { applyTheme } from '@/lib/theme'

export type PreferencesState =
  | { kind: 'loading' }
  | { kind: 'ready'; preferences: PreferencesResponse }
  | { kind: 'error'; message: string }

/**
 * Builds a write from the latest preferences when the queued request runs, with their
 * revision; returning null sends nothing (the change no longer changes anything).
 */
export type PreferencesWrite = (latest: PreferencesResponse) => Promise<PreferencesResponse> | null

const client = new DocumentClient()

function asApiError(error: unknown) {
  return error instanceof ApiError ? error : new ApiError('unexpected', 'The request failed unexpectedly.', 0)
}

/**
 * The service's preferences: the theme the page uses and the settings new documents start
 * from. Writes run one at a time, each built from the latest preferences; a revision conflict
 * (another client or process wrote first) re-reads them and is reported.
 */
export function usePreferences() {
  const [state, setState] = useState<PreferencesState>({ kind: 'loading' })
  const [error, setError] = useState<ApiError | null>(null)
  const [pending, setPending] = useState(0)
  const latest = useRef<PreferencesResponse | null>(null)
  const queue = useRef<Promise<void>>(Promise.resolve())

  const show = useCallback((preferences: PreferencesResponse) => {
    latest.current = preferences
    setState({ kind: 'ready', preferences })
    applyTheme(preferences.ui.theme)
  }, [])

  /** Reads the preferences; a failure after they were shown keeps them and is reported. */
  const load = useCallback(async () => {
    try {
      show(await client.preferences())
    } catch (failure) {
      if (latest.current === null) setState({ kind: 'error', message: asApiError(failure).message })
      else setError(asApiError(failure))
    }
  }, [show])

  useEffect(() => {
    let active = true
    client.preferences().then(
      (preferences) => active && show(preferences),
      (failure: unknown) => active && setState({ kind: 'error', message: asApiError(failure).message }),
    )
    return () => {
      active = false
    }
  }, [show])

  const write = useCallback(
    (make: PreferencesWrite) => {
      setPending((count) => count + 1)
      const next = queue.current
        .then(async () => {
          const current = latest.current
          const request = current && make(current)
          if (!request) return
          try {
            show(await request)
            setError(null)
          } catch (failure) {
            const reported = asApiError(failure)
            setError(reported)
            if (reported.code === 'revision_conflict') await load()
          }
        })
        .catch((failure: unknown) => setError(asApiError(failure)))
        .finally(() => setPending((count) => count - 1))
      queue.current = next
      return next
    },
    [load, show],
  )

  return {
    state,
    error,
    busy: pending > 0,
    dismissError: useCallback(() => setError(null), []),
    /** Replaces the preferences with what `change` makes of the latest ones (null: nothing). */
    set: useCallback(
      (change: (latest: PreferencesResponse) => PreferencesChange | null) =>
        write((current) => {
          const next = change(current)
          return next && client.setPreferences(current.revision, next)
        }),
      [write],
    ),
    reset: useCallback(() => write((current) => client.resetPreferences(current.revision)), [write]),
    /** New documents start from every setting of `documentId` as the service holds it. */
    fromDocument: useCallback(
      (documentId: string, documentReady: Promise<void>) => write(async (current) => {
        await documentReady
        return client.preferencesFromDocument(current.revision, documentId)
      }),
      [write],
    ),
  }
}

export type PreferencesEditor = ReturnType<typeof usePreferences>
