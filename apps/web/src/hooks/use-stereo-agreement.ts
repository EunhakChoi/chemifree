import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { agreementView, CheckScheduler, type AgreementView, type CheckOutcome } from '@/editor/stereo-agreement'
import { samePropertySnapshot, type DocumentView, type StereoInspection } from '@/lib/document-contract'
import { holdsStereo } from '@/lib/stereo-contract'

interface Source {
  /** The drawing on screen. */
  document: DocumentView
  /** Changes when the service process does: a reply from another process is not used. */
  serviceGeneration: number
  /** True while a pointer gesture is in progress; no request starts until it ends. */
  dragging: boolean
  /** Whether a change is checked on its own (else only `checkNow` asks). */
  automatic: boolean
  inspect: (documentId: string) => Promise<StereoInspection>
}

const browserTimers = {
  set: (task: () => void, milliseconds: number) => window.setTimeout(task, milliseconds),
  clear: (handle: unknown) => window.clearTimeout(handle as number),
}

/**
 * Asks the service how each stored configuration compares with the drawing, for a drawing that
 * holds a mark or a stored configuration, and nothing else: a drawing without either sends no
 * request. The question is asked after each committed change that alters the drawing, once the
 * changes stop for a moment; see `CheckScheduler`. What it returns is shown only for exactly the
 * drawing on screen (see `agreementView`), so an out-of-date answer is never drawn.
 */
export function useStereoAgreement(source: Source): { view: AgreementView; checkNow: () => void } {
  const [outcome, setOutcome] = useState<CheckOutcome | null>(null)
  const [running, setRunning] = useState(false)
  const latest = useRef(source)
  const scheduler = useRef<CheckScheduler | null>(null)
  const asked = useRef<{ document: DocumentView; generation: number; automatic: boolean } | null>(null)
  // Declared first, so the other effects of a render see the source of that render.
  useEffect(() => {
    latest.current = source
  })

  useEffect(() => {
    let mounted = true
    // What it asks about is the drawing on screen when the request starts, not when it was scheduled.
    const check = async () => {
      const { document, serviceGeneration, inspect } = latest.current
      if (!holdsStereo(document)) return
      setRunning(true)
      try {
        const inspection = await inspect(document.id)
        if (mounted) setOutcome({ kind: 'ok', generation: serviceGeneration, inspection })
      } catch (failure) {
        const message = failure instanceof Error ? failure.message : 'The comparison failed.'
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

  // A change of what is drawn, of the service process, or turning checking on asks again. A
  // document with nothing to compare asks nothing, and a repeat of the content already asked
  // about (a reread, say) is not asked twice.
  const { document, serviceGeneration, automatic, dragging } = source
  useEffect(() => {
    const before = asked.current
    asked.current = { document, generation: serviceGeneration, automatic }
    if (!automatic || !holdsStereo(document)) {
      scheduler.current?.stop()
      return
    }
    const repeat = before !== null && before.automatic && before.generation === serviceGeneration && samePropertySnapshot(before.document, document)
    if (!repeat) scheduler.current?.changed()
  }, [document, serviceGeneration, automatic])
  useEffect(() => {
    scheduler.current?.pause(dragging)
  }, [dragging])

  const view = useMemo(
    () => agreementView(document, outcome, serviceGeneration, running, automatic),
    [document, outcome, serviceGeneration, running, automatic],
  )
  const checkNow = useCallback(() => scheduler.current?.now(), [])
  return { view, checkNow }
}
