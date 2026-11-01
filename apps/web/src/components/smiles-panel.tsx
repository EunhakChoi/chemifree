import { useEffect, useMemo, useRef, useState } from 'react'

import { LossList } from '@/components/loss-list'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import {
  ConsentRequired,
  describeSpan,
  type CtLoss,
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
  drawing: string
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
  const alive = useRef(true)
  const previewRequest = useRef(0)
  const exportRequest = useRef(0)
  // How many requests of each kind are running, so an edit can say it replaced one.
  const previewing = useRef(0)
  const exporting = useRef(0)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      previewRequest.current += 1
      exportRequest.current += 1
    }
  }, [])

  // The drawing as text, built when it changes and compared as often as the panel renders.
  const drawing = useMemo(() => JSON.stringify(document), [document])
  const generation = editor.serviceGeneration
  const current = (stamp: Stamp) => stamp.drawing === drawing && stamp.generation === generation

  const livePreview = prepared && prepared.sourceVersion === sourceVersion && current(prepared) ? prepared : null
  const liveImportOffer = livePreview && importOffer && importOffer.preview === livePreview.data.digest && current(importOffer)
    ? importOffer : null
  const liveWritten = written && written.styleVersion === styleVersion && current(written) ? written : null
  const liveExportOffer = exportOffer && exportOffer.styleVersion === styleVersion && current(exportOffer) ? exportOffer : null
  const stale = (prepared && !livePreview) || (written && !liveWritten)
  const busy = pending > 0 || editor.busy

  function changeSource(next: string) {
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
    const sequence = ++previewRequest.current
    const asked = { source, sourceVersion, generation }
    previewing.current += 1
    setPending((count) => count + 1)
    setError(null)
    setNotice(null)
    setPrepared(null)
    setImportOffer(null)
    try {
      const data = await editor.previewSmiles(document.id, document.revision, asked.source)
      const returned = JSON.stringify(data.snapshot)
      // The service must have previewed the very drawing the request named.
      if (returned !== drawing) throw new Error('The service previewed a different drawing. Reload and preview again.')
      if (alive.current && previewRequest.current === sequence) {
        setPrepared({ ...asked, data, drawing: returned })
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
    if (!ready) return
    setPending((count) => count + 1)
    setError(null)
    setNotice(null)
    const result = await editor.importSmiles({
      documentId: document.id, revision: document.revision, source: ready.source,
      digest: ready.data.digest, acceptLoss,
    }, ready.data.snapshot, ready.generation)
    if (alive.current) {
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
        // Any other refusal leaves nothing to commit: the preview and its offer are void.
        setPrepared(null)
        setImportOffer(null)
        setError(result.failure?.message ?? 'The import failed.')
      }
      setPending((count) => count - 1)
    }
  }

  async function write(accepted?: string) {
    const sequence = ++exportRequest.current
    const asked = { style, styleVersion, generation, drawing }
    exporting.current += 1
    setPending((count) => count + 1)
    setError(null)
    setNotice(null)
    setWritten(null)
    if (accepted === undefined) setExportOffer(null)
    try {
      const data = await editor.exportSmiles(document.id, asked.style, accepted)
      const returned = JSON.stringify(data.snapshot)
      if (returned !== asked.drawing) throw new Error('The service wrote a different drawing. Reload and write again.')
      if (alive.current && exportRequest.current === sequence) {
        setWritten({ styleVersion: asked.styleVersion, generation: asked.generation, drawing: returned, data })
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

  return <aside aria-label="SMILES" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">SMILES</h3>
    <p className="leading-relaxed text-muted-foreground">
      An OpenSMILES subset. Stereo, query and reaction SMILES are refused; positions are generated, not read.
    </p>
    <Field>
      <FieldLabel htmlFor="smiles-source">SMILES text</FieldLabel>
      <Textarea id="smiles-source" rows={3} spellCheck={false} autoComplete="off" autoCapitalize="off"
        className="font-mono text-xs" value={source} disabled={editor.busy}
        onChange={(event) => changeSource(event.currentTarget.value)}
        onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)} />
      <FieldDescription>
        Line breaks are kept: text after the first whitespace is not read, and is reported as a loss.
      </FieldDescription>
    </Field>
    <Button size="sm" variant="outline" disabled={source === '' || busy || composing} onClick={() => void previewImport()}>
      Preview SMILES import
    </Button>
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {stale && <p>The text, the drawing or the service changed since the last result. Run it again.</p>}
    {livePreview && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p data-testid="smiles-preview-summary">
        {livePreview.data.atoms} atom(s), {livePreview.data.bonds} bond(s) would be added.
      </p>
      {livePreview.data.ignoredText && <p>
        Not read: characters {livePreview.data.ignoredText.character_offset + 1} to{' '}
        {livePreview.data.ignoredText.character_offset + livePreview.data.ignoredText.character_length}
        {' '}({describeSpan(livePreview.data.ignoredText)}).
      </p>}
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
      <Textarea aria-label="Written SMILES" readOnly rows={2} className="font-mono text-xs" value={liveWritten.data.content} />
      <LossList losses={liveWritten.data.losses} />
    </div>}
  </aside>
}
