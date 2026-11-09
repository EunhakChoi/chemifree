import { useRef, useState } from 'react'

import { LossList } from '@/components/loss-list'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Spinner } from '@/components/ui/spinner'
import { Textarea } from '@/components/ui/textarea'
import { type PreparedExport, useConsentedExport } from '@/hooks/use-consented-export'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView, InterchangeExport, InterchangeFormat } from '@/lib/document-contract'

type PreparedInterchange = PreparedExport<InterchangeFormat, InterchangeExport>

interface ClipboardWrite {
  item: PreparedInterchange
  status: 'pending' | 'copied' | 'failed'
  message: string
}

interface InterchangeExportPanelProps {
  document: DocumentView
  editor: DocumentEditor
}

function exportBlob(result: InterchangeExport): Blob {
  if (result.format === 'cdxml') {
    return new Blob([result.content], { type: 'chemical/x-cdxml;charset=utf-8' })
  }
  // CDX is binary. A Blob of the base64 string would download the transport encoding.
  const decoded = atob(result.content)
  const bytes = new Uint8Array(decoded.length)
  for (let index = 0; index < decoded.length; index += 1) {
    bytes[index] = decoded.charCodeAt(index)
  }
  return new Blob([bytes], { type: 'chemical/x-cdx' })
}

/** Offers files and explicit CDXML text copying under the shared export loss consent. */
export function InterchangeExportPanel({ document, editor }: InterchangeExportPanelProps) {
  const {
    exported,
    current: currentExport,
    offer,
    currentOffer,
    currentUnacceptable,
    pending,
    error,
    busy: exportBusy,
    preview,
    download,
    claim,
    discard,
  } = useConsentedExport<InterchangeFormat, InterchangeExport>({
    document,
    editor,
    run: (id, kind, accepted) => editor.exportInterchange(id, kind, accepted),
    blob: (result) => exportBlob(result),
    fileName: (kind) => `chemifree.${kind}`,
  })
  const [clipboardWrite, setClipboardWrite] = useState<ClipboardWrite | null>(null)
  const text = useRef<HTMLTextAreaElement>(null)
  const currentWrite = clipboardWrite?.item === currentExport ? clipboardWrite : null
  const busy = exportBusy || currentWrite?.status === 'pending'

  function previewExport(format: InterchangeFormat, acceptedStart?: string, changedStart?: boolean) {
    // The copy status belongs to the prepared export that this preview replaces.
    setClipboardWrite(null)
    return preview(format, acceptedStart, changedStart)
  }

  async function copyCdxml(item: PreparedInterchange) {
    if (item.kind !== 'cdxml' || !editor.isCurrentConversion(item.snapshot, item.generation)) {
      discard('The drawing or service changed. Preview the export again.')
      return
    }
    const current = claim()
    setClipboardWrite({ item, status: 'pending', message: 'Copying CDXML text…' })
    try {
      if (!navigator.clipboard?.writeText) throw new Error('Clipboard writing is unavailable.')
      // Use the already reviewed bytes directly in this click, before any await.
      // Unlike downloading, this cannot re-export first without losing user activation.
      await navigator.clipboard.writeText(item.result.content)
      if (current() && editor.isCurrentConversion(item.snapshot, item.generation)) {
        setClipboardWrite({ item, status: 'copied', message: 'CDXML text copied.' })
      }
    } catch (cause: unknown) {
      if (current() && editor.isCurrentConversion(item.snapshot, item.generation)) {
        setClipboardWrite({ item, status: 'failed', message: `${cause instanceof Error ? cause.message : 'Clipboard writing failed.'} Select the CDXML text and copy it manually.` })
      }
    }
  }

  return <aside aria-label="CDXML and CDX export" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">CDXML and CDX export</h3>
    <p className="leading-relaxed text-muted-foreground" data-testid="interchange-export-note">Writes the first molecule placement as one page and fragment, including wedge, hash, wavy and crossed marks. Configurations are carried through the drawn marks and written positions; the exporter does not add wedges. Supported enhanced stereo groups are written, with group numbers outside 1–127 renumbered separately for And and Or. If a member’s written depiction cannot retain its stored tetrahedral configuration, its whole group is omitted with a loss to accept. Stored double-bond configurations and ungrouped stereo uncertainty are also reported. Query atoms and unsupported bond types are refused.</p>
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void previewExport('cdxml')}>Preview CDXML export</Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void previewExport('cdx')}>Preview CDX export</Button>
    </div>
    {pending > 0 && <p role="status" className="flex items-center gap-2"><Spinner />Checking export…</p>}
    {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
    {currentUnacceptable && <div role="group" aria-label="Losses that need consent" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>These losses ({currentUnacceptable.features.join(', ')}) need consent, but this reply has no consent digest to accept. No download is available.</p>
      <LossList losses={currentUnacceptable.losses} />
    </div>}
    {offer && !currentOffer && <p>The drawing or service changed. Preview the export again.</p>}
    {currentOffer && <div role="group" aria-label="Losses to accept before writing" className="flex max-h-64 flex-col gap-2 overflow-auto" data-testid="interchange-export-offer">
      {currentOffer.changed && <p>The losses changed since you accepted them. Review them again.</p>}
      <p>Writing this {currentOffer.kind.toUpperCase()} needs these losses accepted: {currentOffer.features.join(', ')}.</p>
      <LossList losses={currentOffer.losses} />
      <Button size="sm" disabled={busy} onClick={() => void previewExport(currentOffer.kind, currentOffer.digest, currentOffer.changed)}>
        Accept these losses and preview {currentOffer.kind.toUpperCase()}
      </Button>
    </div>}
    {exported && !currentExport && <p>The drawing or service changed. Preview the export again.</p>}
    {currentExport && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite" data-testid="interchange-export-written">
      <p>Prepared {currentExport.kind.toUpperCase()} for this drawing.</p>
      <LossList losses={currentExport.result.losses} />
      <Button size="sm" disabled={busy} onClick={() => void download(currentExport)}>Download {currentExport.kind.toUpperCase()}</Button>
      {currentExport.kind === 'cdxml' && <>
        <p>Copy the prepared whole-drawing export as plain CDXML text. It uses the first molecule placement, not the current selection. Other applications may not accept this text as a chemical object.</p>
        <Field>
          <FieldLabel htmlFor="interchange-cdxml-text">Prepared CDXML text</FieldLabel>
          <Textarea id="interchange-cdxml-text" ref={text} readOnly value={currentExport.result.content}
            className="h-36 min-h-16 max-w-full resize-y field-sizing-fixed font-mono text-xs"
            onFocus={(event) => event.currentTarget.select()} />
          <FieldDescription>Select and copy manually if browser clipboard access is unavailable or denied.</FieldDescription>
        </Field>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" disabled={busy} onClick={() => { text.current?.focus(); text.current?.select() }}>Select CDXML text</Button>
          <Button size="sm" disabled={busy} onClick={() => void copyCdxml(currentExport)}>
            {currentWrite?.status === 'pending' && <Spinner data-icon="inline-start" />}Copy CDXML to clipboard
          </Button>
        </div>
        {currentWrite && (currentWrite.status === 'failed'
          ? <Alert variant="destructive"><AlertDescription>{currentWrite.message}</AlertDescription></Alert>
          : <p role="status">{currentWrite.message}</p>)}
      </>}
    </div>}
  </aside>
}
