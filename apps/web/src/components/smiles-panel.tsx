import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { sameConversionSnapshot, type CtLoss, type DocumentView, type SmilesImport, type SmilesPreview } from '@/lib/document-contract'

interface Prepared {
  input: SmilesImport
  preview: SmilesPreview
  generation: number
}
interface Written {
  text: string
  model: string
  losses: CtLoss[]
  snapshot: DocumentView
  generation: number
}

function Losses({ losses }: { losses: CtLoss[] }) {
  return <ul className="list-disc space-y-1 pl-4">{losses.map((loss, index) =>
    <li key={`${loss.feature}-${index}`}>{loss.reason} ({loss.semantic_impact}, {loss.disposition})</li>,
  )}</ul>
}

export function SmilesPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [source, setSource] = useState('')
  const [prepared, setPrepared] = useState<Prepared | null>(null)
  const [written, setWritten] = useState<Written | null>(null)
  const [pending, setPending] = useState(0)
  const [error, setError] = useState<{ message: string; snapshot: DocumentView; generation: number } | null>(null)
  const alive = useRef(true)
  const sequence = useRef(0)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false; sequence.current += 1 }
  }, [])

  const current = (snapshot: DocumentView, generation: number) =>
    generation === editor.serviceGeneration && sameConversionSnapshot(document, snapshot)
  const currentPrepared = prepared && current(prepared.preview.snapshot, prepared.generation) ? prepared : null
  const currentWritten = written && current(written.snapshot, written.generation) ? written : null

  async function preview() {
    const request = ++sequence.current
    const snapshot = document
    const generation = editor.serviceGeneration
    setPrepared(null)
    setError(null)
    setPending((count) => count + 1)
    try {
      const result = await editor.previewSmiles(snapshot.id, snapshot.revision, source)
      if (!sameConversionSnapshot(snapshot, result.snapshot)) throw new Error('The service previewed a different drawing. Preview again.')
      if (alive.current && sequence.current === request) setPrepared({
        input: { documentId: snapshot.id, revision: snapshot.revision, source, digest: result.digest },
        preview: result, generation,
      })
    } catch (cause) {
      if (alive.current && sequence.current === request) setError({ message: cause instanceof Error ? cause.message : 'SMILES preview failed.', snapshot, generation })
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function commit() {
    if (!currentPrepared) return
    setPending((count) => count + 1)
    const committed = await editor.importSmiles(currentPrepared.input, currentPrepared.preview.snapshot, currentPrepared.generation)
    if (alive.current) {
      if (committed) setPrepared(null)
      setPending((count) => count - 1)
    }
  }

  async function write(mode: 'recorded' | 'canonical') {
    const request = ++sequence.current
    const snapshot = document
    const generation = editor.serviceGeneration
    setWritten(null)
    setError(null)
    setPending((count) => count + 1)
    try {
      const result = await editor.writeSmiles(snapshot.id, mode)
      if (!sameConversionSnapshot(snapshot, result.snapshot)) throw new Error('The service used a different drawing. Write again.')
      if (alive.current && sequence.current === request) setWritten({ ...result, generation })
    } catch (cause) {
      if (alive.current && sequence.current === request) setError({ message: cause instanceof Error ? cause.message : 'SMILES output failed.', snapshot, generation })
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  return <aside aria-label="SMILES" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SMILES</h3>
    <p className="leading-relaxed text-muted-foreground">Basic structures, branches, rings and disconnected components are supported. Stereo and query extensions are refused.</p>
    <Field>
      <FieldLabel htmlFor="smiles-source">SMILES input</FieldLabel>
      <Input id="smiles-source" value={source} disabled={editor.busy} onChange={(event) => {
        sequence.current += 1
        setSource(event.currentTarget.value)
        setPrepared(null)
        setError(null)
      }} />
      <FieldDescription>Imports append to the current drawing after a preview.</FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={!source || pending > 0 || editor.busy} onClick={() => void preview()}>Preview SMILES</Button>
    {error && current(error.snapshot, error.generation) && <p role="alert">{error.message}</p>}
    {prepared && !currentPrepared && <p>The drawing changed. Preview the SMILES again.</p>}
    {currentPrepared && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite">
      <p>{currentPrepared.preview.atoms} {currentPrepared.preview.atoms === 1 ? 'atom' : 'atoms'}, {currentPrepared.preview.bonds} {currentPrepared.preview.bonds === 1 ? 'bond' : 'bonds'}.</p>
      <Losses losses={currentPrepared.preview.losses} />
      <Button size="sm" disabled={pending > 0 || editor.busy} onClick={() => void commit()}>Accept and import SMILES</Button>
    </div>}
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={pending > 0 || editor.busy} onClick={() => void write('recorded')}>Write recorded SMILES</Button>
      <Button size="sm" variant="outline" disabled={pending > 0 || editor.busy} onClick={() => void write('canonical')}>Write canonical SMILES</Button>
    </div>
    {written && !currentWritten && <p>The drawing changed. Write the SMILES again.</p>}
    {currentWritten && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite">
      <p>Model: {currentWritten.model}</p>
      <code className="break-all select-all">{currentWritten.text}</code>
      <Losses losses={currentWritten.losses} />
    </div>}
  </aside>
}
