import { useState } from 'react'

import { Button } from '@/components/ui/button'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { sameInspectionSnapshot, type AromaticityInspection, type DocumentView } from '@/lib/document-contract'

/** The interpretation is separate from the recorded bonds until a conversion is requested. */
export function AromaticityPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [pending, setPending] = useState(false)
  const [answer, setAnswer] = useState<{ generation: number; inspection: AromaticityInspection } | null>(null)
  const [failure, setFailure] = useState<{ generation: number; snapshot: DocumentView; message: string } | null>(null)
  const current = answer && answer.generation === editor.serviceGeneration &&
    sameInspectionSnapshot(document, answer.inspection.snapshot) ? answer.inspection : null
  const currentFailure = failure && failure.generation === editor.serviceGeneration &&
    sameInspectionSnapshot(document, failure.snapshot) ? failure.message : null

  async function inspect() {
    const requested = document
    const generation = editor.serviceGeneration
    setPending(true)
    setAnswer(null)
    setFailure(null)
    try {
      setAnswer({ generation, inspection: await editor.inspectAromaticity(requested.id) })
    } catch (error) {
      setFailure({ generation, snapshot: requested, message: error instanceof Error ? error.message : 'Inspection failed.' })
    } finally {
      setPending(false)
    }
  }

  return <section aria-label="Rings and aromaticity" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
    <h3 className="text-sm font-medium">Rings and aromaticity</h3>
    <p className="text-xs text-muted-foreground">Inspect the recorded graph without changing it. Conversion is a separate undoable edit under this bounded model.</p>
    <Button size="sm" variant="outline" disabled={pending || editor.busy} onClick={() => void inspect()}>
      {pending ? 'Inspecting…' : 'Inspect rings'}
    </Button>
    {currentFailure && <p role="alert" className="text-xs">{currentFailure}</p>}
    {answer && !current && <p className="text-xs">The drawing or service changed. Inspect again.</p>}
    {current && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto text-xs" data-testid="aromaticity-result">
      <p>Model: {current.model}</p>
      {current.systems.length === 0 && <p>No cyclic systems.</p>}
      {current.systems.map((system) => <p key={system.bonds.join(',')}>
        {system.atoms.join(', ')}: {system.classification.replaceAll('_', ' ')}; cycle rank {system.cycle_rank}
        {system.electrons !== null && `; ${system.electrons} π electrons`}
        {system.reason && ` (${system.reason.replaceAll('_', ' ')})`}
      </p>)}
    </div>}
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={!current || pending || editor.busy} onClick={() => current && answer && void editor.convertAromaticity('aromatic', current.snapshot, answer.generation)}>Convert to aromatic</Button>
      <Button size="sm" variant="outline" disabled={!current || pending || editor.busy} onClick={() => current && answer && void editor.convertAromaticity('kekule', current.snapshot, answer.generation)}>Convert to Kekulé</Button>
    </div>
  </section>
}
