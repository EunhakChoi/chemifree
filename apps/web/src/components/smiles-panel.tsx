import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { LossList } from '@/components/loss-list'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { currentSmilesExport, type WrittenSmiles } from '@/lib/smiles-state'
import {
  ApiError,
  sameConversionSnapshot,
  type CtLoss,
  type DocumentView,
  type SmilesForm,
  type SmilesImport,
  type SmilesPreview,
} from '@/lib/document-contract'

interface PreparedImport {
  /** The text that was previewed. The input may have changed since. */
  source: string
  snapshot: DocumentView
  preview: SmilesPreview
  generation: number
}

/** A refusal that needs the losses accepted: the export is repeated with this digest. */
interface Consent {
  form: SmilesForm
  losses: CtLoss[]
  digest: string
}

const FORMS: Record<SmilesForm, string> = {
  recorded: 'Recorded order',
  canonical: 'Canonical (Chemifree)',
}

/**
 * SMILES import (preview, losses, consent, commit) and export. Stereo, queries and reactions
 * are refused by the service with a specific code; this panel shows the service's words.
 */
export function SmilesPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [text, setText] = useState('')
  const [form, setForm] = useState<SmilesForm>('canonical')
  const [prepared, setPrepared] = useState<PreparedImport | null>(null)
  const [written, setWritten] = useState<WrittenSmiles | null>(null)
  const [consent, setConsent] = useState<Consent | null>(null)
  const [pending, setPending] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const alive = useRef(true)
  const previewRequest = useRef(0)
  const exportRequest = useRef(0)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      previewRequest.current += 1
      exportRequest.current += 1
    }
  }, [])

  // Editing the text never discards a preview or a request in flight: the result stays on
  // screen, marked as being for the earlier text, and only a newer request replaces it.
  const sameDrawing = prepared !== null && prepared.generation === editor.serviceGeneration &&
    sameConversionSnapshot(document, prepared.snapshot)
  const sameText = prepared !== null && prepared.source === text
  const importable = prepared !== null && sameDrawing && sameText ? prepared : null
  const exportCurrent = currentSmilesExport(written, form, editor.serviceGeneration, document)

  async function previewImport() {
    const snapshot = document
    const generation = editor.serviceGeneration
    const source = text
    const sequence = ++previewRequest.current
    setPending((count) => count + 1)
    setError(null)
    try {
      const preview = await editor.previewSmiles(snapshot.id, snapshot.revision, source)
      if (!sameConversionSnapshot(snapshot, preview.snapshot)) throw new Error('The service previewed a different drawing. Reload and preview again.')
      if (alive.current && previewRequest.current === sequence) setPrepared({ source, snapshot: preview.snapshot, preview, generation })
    } catch (cause) {
      if (alive.current && previewRequest.current === sequence) setError(cause instanceof Error ? cause.message : 'Import preview failed.')
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function commitImport() {
    if (!importable) return
    const input: SmilesImport = {
      documentId: importable.snapshot.id, revision: importable.snapshot.revision,
      source: importable.source, digest: importable.preview.digest,
    }
    setPending((count) => count + 1)
    setError(null)
    try {
      await editor.importSmiles(input, importable.snapshot, importable.generation)
      if (alive.current) setPrepared(null)
    } catch (cause) {
      // preview_changed, missing consent and revision conflicts are shown here.
      if (alive.current) setError(cause instanceof ApiError ? `${cause.message} (${cause.code})` : 'The import failed.')
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function write(accepted?: Consent) {
    const snapshot = document
    const generation = editor.serviceGeneration
    const sequence = ++exportRequest.current
    setPending((count) => count + 1)
    setError(null)
    if (!accepted) setConsent(null)
    try {
      const result = await editor.exportSmiles(snapshot.id, form, accepted?.digest)
      if (alive.current && exportRequest.current === sequence) {
        setConsent(null)
        setWritten({ result, generation })
      }
    } catch (cause) {
      if (alive.current && exportRequest.current === sequence) {
        if (cause instanceof ApiError && cause.consent) {
          setConsent({ form, losses: cause.consent.losses, digest: cause.consent.digest })
        }
        setError(cause instanceof Error ? cause.message : 'Writing SMILES failed.')
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  const busy = pending > 0 || editor.busy
  return <aside aria-label="SMILES" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SMILES</h3>
    <p className="leading-relaxed text-muted-foreground">
      Basic SMILES only. Stereo (@, /, \), queries, reactions and some aromatic systems are refused with a code.
      Atoms are placed on a grid: there is no layout.
    </p>
    <Field>
      <FieldLabel htmlFor="smiles-input">SMILES</FieldLabel>
      <Input id="smiles-input" value={text} spellCheck={false} autoComplete="off"
        onChange={(event) => setText(event.currentTarget.value)} />
      <FieldDescription>Whitespace ends the SMILES; later text is reported and not stored.</FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={busy || text === ''} onClick={() => void previewImport()}>
      Preview import
    </Button>
    {error && <p role="alert" data-testid="smiles-error" className="break-words">{error}</p>}
    {prepared && !sameDrawing && <p>The drawing changed. Preview the import again.</p>}
    {prepared && sameDrawing && !sameText && <p data-testid="smiles-stale">
      The text changed after this preview of <code className="break-all">{prepared.source}</code>. Preview again to import the new text.
    </p>}
    {prepared && sameDrawing && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto" data-testid="smiles-preview">
      <p>
        {prepared.preview.document.atoms.length - prepared.snapshot.atoms.length} atom(s) and{' '}
        {prepared.preview.document.bonds.length - prepared.snapshot.bonds.length} bond(s) in {prepared.preview.components} part(s).
      </p>
      <LossList losses={prepared.preview.losses} />
      <Button size="sm" disabled={busy || !importable} onClick={() => void commitImport()}>
        Accept losses and import
      </Button>
    </div>}
    <Field>
      <FieldLabel htmlFor="smiles-form">Write as</FieldLabel>
      <Select items={FORMS} value={form} disabled={busy}
        onValueChange={(value) => { if (value === 'recorded' || value === 'canonical') { setForm(value); setConsent(null) } }}>
        <SelectTrigger id="smiles-form"><SelectValue /></SelectTrigger>
        <SelectContent>{Object.entries(FORMS).map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
      </Select>
      <FieldDescription>Canonical is Chemifree&apos;s own form, not a toolkit&apos;s.</FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={busy} onClick={() => void write()}>Write SMILES</Button>
    {consent && <div className="flex flex-col gap-2" data-testid="smiles-consent">
      <p>Writing drops content that needs your agreement:</p>
      <LossList losses={consent.losses} />
      <Button size="sm" disabled={busy} onClick={() => void write(consent)}>Accept these losses and write</Button>
    </div>}
    {written && !exportCurrent && <p>The drawing, service or form changed. Write the SMILES again.</p>}
    {exportCurrent && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite" data-testid="smiles-output">
      <Input readOnly aria-label="Written SMILES" value={exportCurrent.result.content} />
      <p>
        {exportCurrent.result.canonical
          ? `${exportCurrent.result.canonical.implementation}, from ${exportCurrent.result.canonical.graph_model}.`
          : `Recorded order (${exportCurrent.result.modelId}).`}
        {' '}Revision {exportCurrent.result.snapshot.revision}.
      </p>
      {exportCurrent.result.foldedHydrogens.length > 0 &&
        <p>Folded hydrogens: {exportCurrent.result.foldedHydrogens.map((row) => `${row.hydrogen}→${row.attached_to}`).join(', ')}</p>}
      <LossList losses={exportCurrent.result.losses} />
    </div>}
  </aside>
}
