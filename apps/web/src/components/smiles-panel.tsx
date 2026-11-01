import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import {
  sameConversionSnapshot, sameDrawingContent, type CtLoss, type DocumentView, type SmilesImport, type SmilesMode,
  type SmilesPreview, type SmilesWrite,
} from '@/lib/document-contract'

interface PreparedImport {
  input: SmilesImport
  snapshot: DocumentView
  preview: SmilesPreview
  generation: number
}

interface WriteResult {
  result: SmilesWrite
  generation: number
}

function Losses({ losses }: { losses: CtLoss[] }) {
  if (losses.length === 0) return <p>No conversion losses were reported.</p>
  return <ul className="list-disc space-y-1 pl-4">
    {losses.map((loss, index) => <li key={`${loss.source_path}-${loss.feature}-${index}`}>
      {loss.source_path}: {loss.reason} ({loss.semantic_impact}, {loss.disposition})
    </li>)}
  </ul>
}

/**
 * SMILES import (preview, loss consent, append) and write (recorded or canonical). A write
 * describes the drawing content it was made from; selecting atoms does not invalidate it.
 */
export function SmilesPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [source, setSource] = useState('')
  const [prepared, setPrepared] = useState<PreparedImport | null>(null)
  const [written, setWritten] = useState<WriteResult | null>(null)
  const [pending, setPending] = useState(0)
  const [importError, setImportError] = useState<string | null>(null)
  const [writeError, setWriteError] = useState<string | null>(null)
  const alive = useRef(true)
  const previewRequest = useRef(0)
  const writeRequest = useRef(0)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      previewRequest.current += 1
      writeRequest.current += 1
    }
  }, [])

  const currentImport = prepared && prepared.generation === editor.serviceGeneration &&
    prepared.input.source === source && sameConversionSnapshot(document, prepared.snapshot) ? prepared : null
  const currentWrite = written && written.generation === editor.serviceGeneration &&
    written.result.snapshot.revision === document.revision && sameDrawingContent(document, written.result.snapshot) ? written : null

  async function previewImport() {
    const snapshot = document
    const generation = editor.serviceGeneration
    const sequence = ++previewRequest.current
    setPending((count) => count + 1)
    setImportError(null)
    setPrepared(null)
    try {
      const preview = await editor.previewSmiles(snapshot.id, snapshot.revision, source)
      if (!sameConversionSnapshot(snapshot, preview.snapshot)) throw new Error('The service previewed a different drawing. Reload and preview again.')
      if (alive.current && previewRequest.current === sequence) setPrepared({ snapshot: preview.snapshot, preview, generation, input: {
        documentId: snapshot.id, revision: snapshot.revision, source, digest: preview.digest,
      } })
    } catch (cause) {
      if (alive.current && previewRequest.current === sequence) setImportError(cause instanceof Error ? cause.message : 'Import preview failed.')
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function commitImport() {
    if (!currentImport) return
    setPending((count) => count + 1)
    const committed = await editor.importSmiles(currentImport.input, currentImport.snapshot, currentImport.generation)
    if (alive.current) {
      if (committed) {
        setPrepared(null)
        setWritten(null)
      }
      setPending((count) => count - 1)
    }
  }

  async function write(mode: SmilesMode) {
    const generation = editor.serviceGeneration
    const sequence = ++writeRequest.current
    setPending((count) => count + 1)
    setWriteError(null)
    try {
      const result = await editor.exportSmiles(document.id, mode)
      if (alive.current && writeRequest.current === sequence) setWritten({ result, generation })
    } catch (cause) {
      if (alive.current && writeRequest.current === sequence) {
        setWritten(null)
        setWriteError(cause instanceof Error ? cause.message : 'Writing SMILES failed.')
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  const locked = pending > 0 || editor.busy
  return <aside aria-label="SMILES" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SMILES</h3>
    <p className="leading-relaxed text-muted-foreground">
      Connectivity only: branches, rings, components, isotopes, charges, hydrogens and aromatic atoms.
      Stereo, query and reaction SMILES are refused. Canonical text is Chemifree&apos;s own form and is not
      compatible with other toolkits. Imported atoms are placed on a placeholder grid.
    </p>
    <Field>
      <FieldLabel htmlFor="smiles-input">SMILES text</FieldLabel>
      <Input id="smiles-input" value={source} spellCheck={false} autoComplete="off" disabled={locked}
        onChange={(event) => { previewRequest.current += 1; setSource(event.currentTarget.value); setPrepared(null); setImportError(null) }} />
      <FieldDescription>Whitespace ends the SMILES; later text is reported as ignored.</FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={source === '' || locked} onClick={() => void previewImport()}>
      Preview SMILES import
    </Button>
    {importError && <p role="alert">{importError}</p>}
    {prepared && !currentImport && <p>The drawing or text changed. Preview the import again.</p>}
    {currentImport && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>{currentImport.preview.atoms} atoms, {currentImport.preview.bonds} bonds will be appended.</p>
      <Losses losses={currentImport.preview.losses} />
      <Button size="sm" disabled={locked} onClick={() => void commitImport()}>
        Accept losses and import
      </Button>
    </div>}
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={locked} onClick={() => void write('recorded')}>Write recorded SMILES</Button>
      <Button size="sm" variant="outline" disabled={locked} onClick={() => void write('canonical')}>Write canonical SMILES</Button>
    </div>
    {writeError && <p role="alert">{writeError}</p>}
    {written && !currentWrite && <p>The drawing changed. Write the SMILES again.</p>}
    {currentWrite && <div aria-live="polite" className="flex flex-col gap-2">
      <p className="break-all font-mono" data-testid="smiles-output">{currentWrite.result.smiles}</p>
      <p>{currentWrite.result.mode} by {currentWrite.result.implementation} {currentWrite.result.version} (model {currentWrite.result.modelId}).</p>
      <Losses losses={currentWrite.result.losses} />
    </div>}
  </aside>
}
