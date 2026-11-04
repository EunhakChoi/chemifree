import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { cipAsk, cipView, type CipAsked, type CipOutcome, type CipView } from '@/editor/cip-labels'
import { CheckScheduler } from '@/editor/stereo-agreement'
import type { CipInspection, DocumentView } from '@/lib/document-contract'

interface Source {
  /** The drawing on screen. */
  document: DocumentView
  /** Changes when the service process does: a reply from another process is not used. */
  serviceGeneration: number
  /** True while a pointer gesture is in progress; no request starts until it ends. */
  dragging: boolean
  /** Whether a label switch is on and the drawing stores something it covers (`wantsCip`). */
  wanted: boolean
  /** Asks `chem.cip` with `scope: "stored"`; the editor never asks for any other scope. */
  inspect: (documentId: string) => Promise<CipInspection>
}

const browserTimers = {
  set: (task: () => void, milliseconds: number) => window.setTimeout(task, milliseconds),
  clear: (handle: unknown) => window.clearTimeout(handle as number),
}

/**
 * Asks the service for the CIP labels of the stored configurations, only while labels are
 * wanted (a switch is on): with both switches off no request is ever sent, because `chem.cip`
 * has no time limit and a dense graph can be slow. The scheduling is the agreement indicator's
 * (`CheckScheduler`): a wait after the last change, one request in flight, one more for changes
 * made meanwhile, none while dragging. What it returns is shown only for exactly the drawing on
 * screen (see `cipView`), so an out-of-date answer is never drawn.
 */
export function useCipLabels(source: Source): { view: CipView; checkNow: () => void } {
  const [outcome, setOutcome] = useState<CipOutcome | null>(null)
  const [running, setRunning] = useState(false)
  const latest = useRef(source)
  const scheduler = useRef<CheckScheduler | null>(null)
  const asked = useRef<CipAsked | null>(null)
  // Declared first, so the other effects of a render see the source of that render.
  useEffect(() => {
    latest.current = source
  })

  useEffect(() => {
    let mounted = true
    // What it asks about is the drawing on screen when the request starts, not when it was scheduled.
    const check = async () => {
      const { document, serviceGeneration, inspect, wanted } = latest.current
      if (!wanted) return
      setRunning(true)
      try {
        const inspection = await inspect(document.id)
        if (mounted) setOutcome({ kind: 'ok', generation: serviceGeneration, inspection })
      } catch (failure) {
        const message = failure instanceof Error ? failure.message : 'The CIP request failed.'
        if (mounted) setOutcome({ kind: 'failed', generation: serviceGeneration, snapshot: document, message })
      } finally {
        if (mounted) setRunning(false)
      }
    }
    const created = new CheckScheduler(check, browserTimers)
    scheduler.current = created
    // A new scheduler knows nothing yet (a development remount makes one): the drawing is asked about again.
    asked.current = null
    return () => {
      mounted = false
      created.stop()
      scheduler.current = null
    }
  }, [])

  const { document, serviceGeneration, wanted, dragging } = source
  useEffect(() => {
    const next = { document, generation: serviceGeneration, wanted }
    const decision = cipAsk(asked.current, next)
    asked.current = next
    if (decision === 'stop') scheduler.current?.stop()
    else if (decision === 'ask') scheduler.current?.changed()
  }, [document, serviceGeneration, wanted])
  useEffect(() => {
    scheduler.current?.pause(dragging)
  }, [dragging])

  const view = useMemo(
    () => cipView(document, outcome, serviceGeneration, running, wanted),
    [document, outcome, serviceGeneration, running, wanted],
  )
  const checkNow = useCallback(() => scheduler.current?.now(), [])
  return { view, checkNow }
}
