import { useEffect, useState } from 'react'

import { readRuntime, type RuntimeSnapshot } from '@/lib/contract'

export type RuntimeState =
  | { kind: 'loading' }
  | { kind: 'ready'; snapshot: RuntimeSnapshot }
  | { kind: 'error'; message: string }

export function useRuntime() {
  const [state, setState] = useState<RuntimeState>({ kind: 'loading' })
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    const controller = new AbortController()
    void readRuntime(controller.signal)
      .then((snapshot) => {
        if (!controller.signal.aborted) setState({ kind: 'ready', snapshot })
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          const message = error instanceof Error && error.name === 'TimeoutError'
            ? 'The local service did not respond within 5 seconds.'
            : error instanceof TypeError
              ? 'The local service could not be reached.'
              : error instanceof Error ? error.message : 'The local service could not be reached.'
          setState({ kind: 'error', message })
        }
      })
    return () => controller.abort()
  }, [attempt])

  function refresh() {
    setState({ kind: 'loading' })
    setAttempt((previous) => previous + 1)
  }

  return { state, refresh }
}
