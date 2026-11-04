import { useMemo } from 'react'

import { agreementView, type AgreementView } from '@/editor/stereo-agreement'
import { useScheduledInspection } from '@/hooks/use-scheduled-inspection'
import { type DocumentView, type StereoInspection } from '@/lib/document-contract'
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

/**
 * Asks the service how each stored configuration compares with the drawing, for a drawing that
 * holds a mark or a stored configuration, and nothing else: a drawing without either sends no
 * request. The question is asked after each committed change that alters the drawing, once the
 * changes stop for a moment; see `InspectionRunner`. What it returns is shown only for exactly the
 * drawing on screen (see `agreementView`), so an out-of-date answer is never drawn.
 */
export function useStereoAgreement(source: Source): { view: AgreementView; checkNow: () => void } {
  const { document, serviceGeneration, dragging, automatic, inspect } = source
  const holds = holdsStereo(document)
  const { outcome, running, checkNow } = useScheduledInspection(
    { document, generation: serviceGeneration, dragging, active: automatic, allowed: holds, inspect },
    false,
  )
  const view = useMemo(
    () => agreementView(document, outcome, serviceGeneration, running, automatic),
    [document, outcome, serviceGeneration, running, automatic],
  )
  return { view, checkNow }
}
