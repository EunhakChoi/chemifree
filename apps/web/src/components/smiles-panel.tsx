import { useEffect, useMemo, useRef, useState } from 'react'

import { LossList } from '@/components/loss-list'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import {
  ConsentRequired,
  conversionSnapshotMatcher,
  describeSpan,
  MAX_CLIPBOARD_SMILES_BYTES,
  type CtLoss,
  sameConversionSnapshot,
  type DocumentView,
  type SmilesExport,
  type SmilesPreview,
  type SmilesStyle,
} from '@/lib/document-contract'

/**
 * What a result was made for. A result is shown, and its button works, only while the form,
 * the drawing and the service are still the ones it was made for.
 */
interface Stamp {
  /** The drawing as the service returned it with the result. */
  drawing: DocumentView
  generation: number
}

interface Prepared extends Stamp {
  source: string
  sourceVersion: number
  data: SmilesPreview
}

interface Written extends Stamp {
  styleVersion: number
  data: SmilesExport
}

/** Losses a person must accept before the service goes on, tied to the result they belong to. */
interface Offer extends Stamp {
  /** The preview digest of an import, or the consent digest of an export. */
  digest: string
  features: string[]
  losses: CtLoss[]
}

const MESSAGE_TEXT_CHANGED = 'The text changed while a request was running; its result was not used. Run it again.'
const MESSAGE_FORM_CHANGED = 'The output form changed while a request was running; its result was not used. Run it again.'

function message(failure: unknown, fallback: string) {
  return failure instanceof Error ? failure.message : fallback
}

/** SMILES text in and out of the drawing, with the same preview, losses and consent as other formats. */
export function SmilesPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [source, setSource] = useState('')
  const [style, setStyle] = useState<SmilesStyle>('recorded')
  const [sourceVersion, setSourceVersion] = useState(0)
  const [styleVersion, setStyleVersion] = useState(0)
  const [prepared, setPrepared] = useState<Prepared | null>(null)
  const [importOffer, setImportOffer] = useState<(Offer & { preview: string }) | null>(null)
  const [written, setWritten] = useState<Written | null>(null)
  const [exportOffer, setExportOffer] = useState<(Offer & { styleVersion: number }) | null>(null)
  const [pending, setPending] = useState(0)
  const [composing, setComposing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [transport, setTransport] = useState<{ generation: number; action: 'read' | 'write' | 'check' } | null>(null)
  const writtenText = useRef<HTMLTextAreaElement>(null)
  const sourceEpoch = useRef(0)
  const clipboardRequest = useRef(0)
  const alive = useRef(true)
  const previewRequest = useRef(0)
  const exportRequest = useRef(0)
  // How many requests of each kind are running, so an edit can say it replaced one.
  const previewing = useRef(0)
  const exporting = useRef(0)
  const committing = useRef(false)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      previewRequest.current += 1
      exportRequest.current += 1
      clipboardRequest.current += 1
    }
  }, [])

  const generation = editor.serviceGeneration
  // Whether a result's drawing is the one shown. The matcher keeps one answer per snapshot
  // object, and it is made anew when the drawing changes, so each snapshot is compared with
  // the drawing once per change however many results and offers hold it.
  const matchesShown = useMemo(() => conversionSnapshotMatcher(document), [document])
  const current = (stamp: Stamp) => stamp.generation === generation && matchesShown(stamp.drawing)

  const livePreview = prepared && prepared.sourceVersion === sourceVersion && current(prepared) ? prepared : null
  const liveImportOffer = livePreview && importOffer && importOffer.preview === livePreview.data.digest && current(importOffer)
    ? importOffer : null
  const liveWritten = written && written.styleVersion === styleVersion && current(written) ? written : null
  const liveExportOffer = exportOffer && exportOffer.styleVersion === styleVersion && current(exportOffer) ? exportOffer : null
  const stale = (prepared && !livePreview) || (written && !liveWritten)
  const transportBusy = transport !== null && transport.generation === generation
  const sourceFits = new TextEncoder().encode(source).byteLength <= MAX_CLIPBOARD_SMILES_BYTES
  const busy = pending > 0 || transportBusy || editor.busy

  function changeSource(next: string) {
    sourceEpoch.current += 1
    setSource(next)
    setSourceVersion((version) => version + 1)
    setPrepared(null)
    setImportOffer(null)
    setError(null)
    if (previewing.current > 0) {
      previewRequest.current += 1
      setNotice(MESSAGE_TEXT_CHANGED)
    } else {
      setNotice(null)
    }
  }

  function changeStyle(next: SmilesStyle) {
    setStyle(next)
    setStyleVersion((version) => version + 1)
    setWritten(null)
    setExportOffer(null)
    setError(null)
    if (exporting.current > 0) {
      exportRequest.current += 1
      setNotice(MESSAGE_FORM_CHANGED)
    } else {
      setNotice(null)
    }
  }

  async function previewImport() {
    if (busy || composing || source === '' || !sourceFits) return
    const sequence = ++previewRequest.current
    const asked = { source, sourceVersion, generation, drawing: document }
    previewing.current += 1
    setPending((count) => count + 1)
    setError(null)
    setNotice(null)
    setPrepared(null)
    setImportOffer(null)
    try {
      const data = await editor.previewSmiles(document.id, document.revision, asked.source)
      // The service must have previewed the very drawing the request named.
      if (!sameConversionSnapshot(asked.drawing, data.snapshot)) {
        throw new Error('The service previewed a different drawing. Reload and preview again.')
      }
      if (alive.current && previewRequest.current === sequence) {
        setPrepared({ ...asked, data, drawing: data.snapshot })
        setImportOffer(null)
      }
    } catch (failure) {
      if (alive.current && previewRequest.current === sequence) setError(message(failure, 'Preview failed.'))
    } finally {
      previewing.current -= 1
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function commitImport(acceptLoss: boolean) {
    const ready = livePreview
    // A second click before the first reply would find the drawing changed by the first.
    if (!ready || committing.current || busy) return
    committing.current = true
    setPending((count) => count + 1)
    setError(null)
    setNotice(null)
    try {
      const result = await editor.importSmiles({
        documentId: document.id, revision: document.revision, source: ready.source,
        digest: ready.data.digest, acceptLoss,
      }, ready.data.snapshot, ready.generation)
      if (!alive.current) return
      if (result.committed) {
        setPrepared(null)
        setImportOffer(null)
        setNotice('Imported. The drawing now holds the new atoms; undo reverts the import.')
      } else if (result.failure instanceof ConsentRequired) {
        // A new refusal to proceed without consent replaces any earlier offer for this preview.
        setImportOffer({
          preview: ready.data.digest, drawing: ready.drawing, generation: ready.generation, digest: ready.data.digest,
          features: result.failure.features, losses: result.failure.losses,
        })
      } else {
        // Discard only the local preview. The hook retains any unanswered immutable attempt.
        setPrepared(null)
        setImportOffer(null)
        setError(result.failure?.message ?? 'The import failed.')
      }
    } catch (failure) {
      // Whatever went wrong, the panel is usable again and says so.
      if (alive.current) {
        setPrepared(null)
        setImportOffer(null)
        setError(message(failure, 'The import failed.'))
      }
    } finally {
      committing.current = false
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function write(accepted?: string) {
    const sequence = ++exportRequest.current
    const asked = { style, styleVersion, generation, drawing: document }
    exporting.current += 1
    setPending((count) => count + 1)
    setError(null)
    setNotice(null)
    setWritten(null)
    if (accepted === undefined) setExportOffer(null)
    try {
      const data = await editor.exportSmiles(document.id, asked.style, accepted)
      if (!sameConversionSnapshot(asked.drawing, data.snapshot)) {
        throw new Error('The service wrote a different drawing. Reload and write again.')
      }
      if (alive.current && exportRequest.current === sequence) {
        setWritten({ styleVersion: asked.styleVersion, generation: asked.generation, drawing: data.snapshot, data })
        setExportOffer(null)
      }
    } catch (failure) {
      if (alive.current && exportRequest.current === sequence) {
        if (failure instanceof ConsentRequired && failure.digest !== undefined) {
          setExportOffer({
            styleVersion: asked.styleVersion, generation: asked.generation, drawing: asked.drawing,
            digest: failure.digest, features: failure.features, losses: failure.losses,
          })
        } else {
          setExportOffer(null)
          setError(message(failure, 'Writing SMILES failed.'))
        }
      }
    } finally {
      exporting.current -= 1
      if (alive.current) setPending((count) => count - 1)
    }
  }

  function clipboardCurrent(sequence: number, askedGeneration: number): boolean {
    return alive.current && clipboardRequest.current === sequence && editor.currentServiceGeneration() === askedGeneration
  }

  async function readClipboard() {
    if (busy) return
    const sequence = ++clipboardRequest.current
    const askedGeneration = generation
    const askedEpoch = sourceEpoch.current
    const drawing = document
    setTransport({ generation: askedGeneration, action: 'read' })
    setError(null)
    setNotice(null)
    try {
      if (typeof navigator.clipboard?.readText !== 'function') {
        throw new Error('Browser clipboard reading is unavailable.')
      }
      // This optional read starts directly in the click. The source is passed unchanged.
      const value = await navigator.clipboard.readText()
      if (!clipboardCurrent(sequence, askedGeneration)) return
      if (sourceEpoch.current !== askedEpoch || !editor.isCurrentConversion(drawing, askedGeneration)) {
        setNotice('The text, drawing or service changed while reading clipboard. The clipboard result was not used.')
        return
      }
      changeSource(value)
    } catch {
      if (clipboardCurrent(sequence, askedGeneration)) setError('Clipboard reading was unavailable, denied or failed. Paste SMILES text manually.')
    } finally {
      if (alive.current && clipboardRequest.current === sequence) setTransport(null)
    }
  }

  async function copyWritten() {
    const ready = liveWritten
    if (!ready || busy) return
    if (!editor.isCurrentConversion(ready.drawing, ready.generation)) {
      setError('The drawing or service changed. Write SMILES again before copying.')
      return
    }
    const sequence = ++clipboardRequest.current
    setTransport({ generation: ready.generation, action: 'write' })
    setError(null)
    setNotice(null)
    try {
      if (typeof navigator.clipboard?.writeText !== 'function') {
        throw new Error('Browser clipboard writing is unavailable.')
      }
      // Written content is already prepared. No backend request or await precedes this call.
      await navigator.clipboard.writeText(ready.data.content)
      if (clipboardCurrent(sequence, ready.generation)) setNotice('Written SMILES copied to clipboard.')
    } catch {
      if (clipboardCurrent(sequence, ready.generation)) setError('Clipboard writing was unavailable, denied or failed. Select and copy the written SMILES manually.')
    } finally {
      if (alive.current && clipboardRequest.current === sequence) setTransport(null)
    }
  }

  async function checkImportOutcome() {
    if (pending > 0 || transportBusy) return
    const sequence = ++clipboardRequest.current
    const askedGeneration = generation
    setTransport({ generation: askedGeneration, action: 'check' })
    setError(null)
    setNotice(null)
    try {
      if (editor.clipboardNeedsReload) await editor.reconcileClipboardPaste()
      else await editor.checkClipboardPaste()
      if (clipboardCurrent(sequence, askedGeneration)) {
        setPrepared(null)
        setImportOffer(null)
        setNotice('SMILES import outcome reconciled with the document.')
      }
    } catch (failure: unknown) {
      if (clipboardCurrent(sequence, askedGeneration)) setError(message(failure, 'The SMILES import outcome could not be checked.'))
    } finally {
      if (alive.current && clipboardRequest.current === sequence) setTransport(null)
    }
  }

  return <aside aria-label="SMILES" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SMILES</h3>
    <p className="leading-relaxed text-muted-foreground">
      An OpenSMILES subset. Reading takes the tetrahedral marks @ and @@ and CXSMILES stereo groups (a:, o1:, &1:) as stored configurations and groups, with no wedges drawn; it refuses directional bonds / and \, other stereo classes and CXSMILES fields, query and reaction SMILES. Recorded output writes supported tetrahedral configurations and groups; unsupported stereo requires loss consent. Canonical output does not retain stereo. Positions are generated, not read.
    </p>
    <Field>
      <FieldLabel htmlFor="smiles-source">SMILES text</FieldLabel>
      <Textarea id="smiles-source" rows={3} spellCheck={false} autoComplete="off" autoCapitalize="off"
        className="font-mono text-xs" value={source}
        onChange={(event) => changeSource(event.currentTarget.value)}
        onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)} />
      <FieldDescription>
        Line breaks and trailing text are kept unchanged. The reader handles its supported CXSMILES stereo block after the structure; other text after the first whitespace is not read and is reported as a loss.
      </FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={busy || composing} onClick={() => void readClipboard()}>Read clipboard</Button>
    <p className="text-muted-foreground">Reading the clipboard keeps text here. Preview SMILES import sends it to the local service; importing sends the same inspected source after confirmation. The UTF-8 limit is 16 KiB.</p>
    {!sourceFits && <Alert variant="destructive"><AlertDescription>This SMILES text exceeds 16 KiB of UTF-8. Shorten it before previewing.</AlertDescription></Alert>}
    <Button size="sm" variant="outline" disabled={source === '' || !sourceFits || busy || composing} onClick={() => void previewImport()}>
      Preview SMILES import
    </Button>
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {editor.clipboardPending && editor.clipboardAction === 'smiles_import' && <Alert>
      <AlertDescription className="flex flex-col gap-2">
        <p>{editor.clipboardNeedsReload
          ? 'The original SMILES import can no longer be confirmed by replay. It may already be present. Reload the document to reconcile its outcome.'
          : 'The SMILES import outcome is not confirmed. Check the original attempt before importing again. Editing this source text does not change that attempt.'}</p>
        <Button size="sm" variant="outline" disabled={pending > 0 || transportBusy} onClick={() => void checkImportOutcome()}>
          {editor.clipboardNeedsReload ? 'Reload document to reconcile' : 'Check SMILES import outcome'}
        </Button>
      </AlertDescription>
    </Alert>}
    {stale && <p>The text, the drawing or the service changed since the last result. Run it again.</p>}
    {livePreview && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p data-testid="smiles-preview-summary">
        {livePreview.data.atoms} atom(s), {livePreview.data.bonds} bond(s) would be added.
      </p>
      {livePreview.data.ignoredText && <p>Not read: {describeSpan(livePreview.data.ignoredText)}.</p>}
      <LossList losses={livePreview.data.losses} />
      <p className="text-muted-foreground">
        Valence model {livePreview.data.valenceModel}; aromaticity model {livePreview.data.aromaticityModel}.
      </p>
      {!liveImportOffer && <Button size="sm" disabled={busy} onClick={() => void commitImport(false)}>
        Import SMILES
      </Button>}
      {liveImportOffer && <div role="group" aria-label="Losses to accept" className="flex flex-col gap-2">
        <p>These losses change the chemistry or drop text and must be accepted: {liveImportOffer.features.join(', ')}.</p>
        <LossList losses={liveImportOffer.losses} />
        <Button size="sm" disabled={busy} onClick={() => void commitImport(true)}>Accept these losses and import SMILES</Button>
      </div>}
    </div>}
    <Field>
      <FieldLabel htmlFor="smiles-style">Write the drawing as</FieldLabel>
      <select id="smiles-style" value={style} disabled={busy}
        className="h-8 rounded-lg border border-input bg-transparent px-2 text-xs"
        onChange={(event) => changeStyle(event.currentTarget.value === 'canonical' ? 'canonical' : 'recorded')}>
        <option value="recorded">Recorded: depth-first from the first atom</option>
        <option value="canonical">Canonical: one text per chemical graph</option>
      </select>
    </Field>
    <Button size="sm" variant="outline" disabled={busy} onClick={() => void write()}>Write SMILES</Button>
    {liveExportOffer && <div role="group" aria-label="Losses to accept before writing" className="flex flex-col gap-2">
      <p>Writing this needs these assumptions accepted: {liveExportOffer.features.join(', ')}.</p>
      <LossList losses={liveExportOffer.losses} />
      <Button size="sm" disabled={busy} onClick={() => void write(liveExportOffer.digest)}>
        Accept these losses and write SMILES
      </Button>
    </div>}
    {liveWritten && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>{liveWritten.data.style === 'canonical' ? 'Canonical' : 'Recorded'} SMILES ({liveWritten.data.model}):</p>
      <p>The entire chemical graph is written, regardless of selected atoms, bonds or scene objects. This is not a selected-fragment copy.</p>
      <Textarea ref={writtenText} aria-label="Written SMILES" readOnly rows={2} className="field-sizing-fixed max-h-48 resize-y font-mono text-xs" value={liveWritten.data.content} />
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={busy} onClick={() => { writtenText.current?.focus(); writtenText.current?.select() }}>Select written SMILES</Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void copyWritten()}>Copy to clipboard</Button>
      </div>
      <LossList losses={liveWritten.data.losses} />
    </div>}
  </aside>
}
