import { useMemo } from 'react'

import { cipView, wantsCip, type CipView } from '@/editor/cip-labels'
import { useScheduledInspection } from '@/hooks/use-scheduled-inspection'
import type { CipInspection, DocumentView } from '@/lib/document-contract'

interface Source {
  /** The drawing on screen. */
  document: DocumentView
  /** Changes when the service process does: a reply from another process is not used. */
  serviceGeneration: number
  /** True while a pointer gesture is in progress; no request starts until it ends. */
  dragging: boolean
  /** The two label switches. */
  atomLabels: boolean
  bondLabels: boolean
  /** Asks `chem.cip` with `scope: "stored"`; the editor never asks for any other scope. */
  inspect: (documentId: string) => Promise<CipInspection>
}

/**
 * Asks the service for the CIP labels of the stored configurations, only while labels are
 * wanted (a switch is on and the drawing stores something it covers, `wantsCip`): with both
 * switches off no request is ever sent. The service bounds each request with its time limit
 * (5 seconds by default), the web client gives up after its own 10 seconds, and only stored
 * configurations are asked for, because a dense graph's scan has no lower node cap yet (issue
 * #250). The scheduling is the agreement indicator's (`InspectionRunner`). Switching
 * off clears the last outcome and discards a request still in flight, so turning labels on again
 * never shows an earlier failure or answer. What it returns is shown only for exactly the drawing
 * on screen (see `cipView`), so an out-of-date answer is never drawn.
 */
export function useCipLabels(source: Source): { view: CipView; checkNow: () => void } {
  const { document, serviceGeneration, dragging, atomLabels, bondLabels, inspect } = source
  const wanted = wantsCip(document, atomLabels, bondLabels)
  const { outcome, running, checkNow } = useScheduledInspection(
    { document, generation: serviceGeneration, dragging, active: wanted, allowed: wanted, inspect },
    true,
  )
  const view = useMemo(
    () => cipView(document, outcome, serviceGeneration, running, atomLabels, bondLabels),
    [document, outcome, serviceGeneration, running, atomLabels, bondLabels],
  )
  return { view, checkNow }
}
