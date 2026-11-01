import { useEffect, useRef, useState } from 'react'

import { LossList } from '@/components/loss-list'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import {
  ApiError, sameConversionSnapshot,
  type ConsentOffer, type DocumentView, type SmilesExport, type SmilesPreview, type SmilesStyle,
} from '@/lib/document-contract'

/** Everything a result is tied to: the form it was made from, the drawing and the service. */
interface Tied {
  snapshot: DocumentView
  generation: number
}
interface PreparedImport extends Tied {
  source: string
  preview: SmilesPreview
  /** Set when the commit was refused for losses that need the caller's consent. */
  offer: ConsentOffer | null
}
interface Written extends Tied {
  style: SmilesStyle
  result: SmilesExport
}
interface ExportOffer extends Tied {
  style: SmilesStyle
  offer: ConsentOffer
}

/**
 * SMILES import and export over the operation contract. A result or consent offer is shown
 * only while the text, the style, the drawing and the service are the ones it was made for.
 */
export function SmilesPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [source, setSource] = useState('')
  const [style, setStyle] = useState<SmilesStyle>('canonical')
  const [prepared, setPrepared] = useState<PreparedImport | null>(null)
  const [written, setWritten] = useState<Written | null>(null)
  const [exportOffer, setExportOffer] = useState<ExportOffer | null>(null)
  const [importing, setImporting] = useState(0)
  const [exporting, setExporting] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const alive = useRef(true)
  const importRequest = useRef(0)
  const exportRequest = useRef(0)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      importRequest.current += 1
      exportRequest.current += 1
    }
  }, [])

  const tiedToNow = (item: Tied) => item.generation === editor.serviceGeneration && sameConversionSnapshot(document, item.snapshot)
  const currentImport = prepared && prepared.source === source && tiedToNow(prepared) ? prepared : null
  const currentWritten = written && written.style === style && tiedToNow(written) ? written : null
  const currentOffer = exportOffer && exportOffer.style === style && tiedToNow(exportOffer) ? exportOffer : null
  const stale = (prepared !== null && !currentImport) || (written !== null && !currentWritten) || (exportOffer !== null && !currentOffer)

  /** Drops what was made for another form, and says so when it was in flight or on screen. */
  function discard(reason: string, imports: boolean, exports: boolean) {
    if (imports) {
      const inFlight = importing > 0
      importRequest.current += 1
      if (prepared || inFlight) setNotice(reason)
      setPrepared(null)
    }
    if (exports) {
      const inFlight = exporting > 0
      exportRequest.current += 1
      if (written || exportOffer || inFlight) setNotice(reason)
      setWritten(null)
      setExportOffer(null)
    }
    setError(null)
  }

  function fail(cause: unknown, fallback: string) {
    setError(cause instanceof Error ? cause.message : fallback)
    setPrepared(null)
    setWritten(null)
    setExportOffer(null)
  }

  async function previewImport() {
    const snapshot = document
    const generation = editor.serviceGeneration
    const text = source
    const sequence = ++importRequest.current
    setImporting((count) => count + 1)
    setError(null)
    setNotice(null)
    setPrepared(null)
    try {
      const preview = await editor.previewSmiles(snapshot.id, snapshot.revision, text)
      if (alive.current && importRequest.current === sequence) setPrepared({ source: text, snapshot: preview.snapshot, generation, preview, offer: null })
    } catch (cause) {
      if (alive.current && importRequest.current === sequence) fail(cause, 'SMILES preview failed.')
    } finally {
      if (alive.current) setImporting((count) => count - 1)
    }
  }

  async function commitImport(item: PreparedImport, acceptLoss: boolean) {
    const sequence = ++importRequest.current
    setImporting((count) => count + 1)
    setError(null)
    const failure = await editor.importSmiles({
      documentId: item.snapshot.id, revision: item.snapshot.revision, source: item.source,
      digest: item.preview.digest, acceptLoss,
    }, item.snapshot, item.generation)
    if (!alive.current) return
    setImporting((count) => count - 1)
    if (failure === null) {
      // The input is locked while a commit is in flight, but a success is reported whatever
      // else happened, so the user always learns that the drawing changed.
      setPrepared(null)
      setNotice('Imported. Undo restores the drawing before the import.')
    } else if (importRequest.current !== sequence) {
      return
    } else if (failure instanceof ApiError && failure.code === 'loss_consent_required' && failure.consent) {
      // The only failure that keeps the preview: the same preview, plus what must be accepted.
      setPrepared({ ...item, offer: failure.consent })
      setError(null)
    } else {
      fail(failure, 'SMILES import failed.')
    }
  }

  async function write(acceptedLosses?: string) {
    const snapshot = document
    const generation = editor.serviceGeneration
    const wanted = style
    const sequence = ++exportRequest.current
    setExporting((count) => count + 1)
    setError(null)
    setNotice(null)
    setWritten(null)
    if (acceptedLosses === undefined) setExportOffer(null)
    try {
      const result = await editor.exportSmiles(snapshot.id, wanted, acceptedLosses)
      if (alive.current && exportRequest.current === sequence) {
        setExportOffer(null)
        setWritten({ style: wanted, snapshot: result.snapshot, generation, result })
      }
    } catch (cause) {
      if (alive.current && exportRequest.current === sequence) {
        if (cause instanceof ApiError && (cause.code === 'loss_consent_required' || cause.code === 'loss_consent_changed') && cause.consent?.digest) {
          setExportOffer({ style: wanted, snapshot, generation, offer: cause.consent })
          setError(cause.code === 'loss_consent_changed' ? 'The losses changed; review the new list before accepting.' : null)
        } else {
          fail(cause, 'SMILES output failed.')
        }
      }
    } finally {
      if (alive.current) setExporting((count) => count - 1)
    }
  }

  const busy = importing > 0 || exporting > 0 || editor.busy
  return <aside aria-label="SMILES" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SMILES</h3>
    <p className="leading-relaxed text-muted-foreground">
      No stereochemistry, queries or reactions. The canonical form is Chemifree&apos;s own, not a toolkit&apos;s.
    </p>
    <Field>
      <FieldLabel htmlFor="smiles-input">SMILES to import</FieldLabel>
      <Input id="smiles-input" value={source} spellCheck={false} autoComplete="off" disabled={busy}
        onChange={(event) => {
          setSource(event.currentTarget.value)
          discard('The text changed, so the earlier SMILES preview was discarded.', true, false)
        }} />
      <FieldDescription>Text after the first space is not part of the SMILES.</FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={busy || source.length === 0} onClick={() => void previewImport()}>
      Preview SMILES import
    </Button>
    {currentImport && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>{currentImport.preview.atoms} atoms, {currentImport.preview.bonds} bonds, {currentImport.preview.parts} connected part(s).</p>
      <LossList losses={currentImport.preview.losses} />
      {currentImport.offer && <div role="group" aria-label="Losses that need consent" className="flex flex-col gap-2">
        <p>These losses need your consent before importing:</p>
        <LossList losses={currentImport.offer.losses} />
        <Button size="sm" disabled={busy} onClick={() => void commitImport(currentImport, true)}>Accept these losses and import</Button>
      </div>}
      {!currentImport.offer && <Button size="sm" disabled={busy} onClick={() => void commitImport(currentImport, false)}>
        Import SMILES
      </Button>}
    </div>}
    <Field>
      <FieldLabel htmlFor="smiles-style">Write the drawing as</FieldLabel>
      <select id="smiles-style" className="h-8 rounded-md border bg-background px-2" value={style} disabled={busy}
        onChange={(event) => {
          setStyle(event.currentTarget.value === 'recorded' ? 'recorded' : 'canonical')
          discard('The style changed, so the earlier SMILES output was discarded.', false, true)
        }}>
        <option value="canonical">Canonical (chemifree-smiles-canonical-1)</option>
        <option value="recorded">Recorded order</option>
      </select>
    </Field>
    <Button size="sm" variant="outline" disabled={busy} onClick={() => void write()}>Write SMILES</Button>
    {currentOffer && <div role="group" aria-label="Losses that need consent" className="flex flex-col gap-2">
      <p>The output needs your consent to these losses:</p>
      <LossList losses={currentOffer.offer.losses} />
      <Button size="sm" disabled={busy} onClick={() => void write(currentOffer.offer.digest)}>Accept these losses and write</Button>
    </div>}
    {currentWritten && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite">
      <pre aria-label="SMILES output" className="break-all whitespace-pre-wrap">{currentWritten.result.content}</pre>
      <p>{currentWritten.result.style} output by {currentWritten.result.modelId}{currentWritten.result.foldedHydrogens.length > 0 ? `; ${currentWritten.result.foldedHydrogens.length} explicit hydrogen(s) folded` : ''}.</p>
      <LossList losses={currentWritten.result.losses} />
    </div>}
    {stale && <p>The drawing or service changed. Preview or write again.</p>}
    {notice && <p aria-live="polite">{notice}</p>}
    {error && <p role="alert">{error}</p>}
  </aside>
}
