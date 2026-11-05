import { useEffect, useRef, useState } from 'react'

import { LossList } from '@/components/loss-list'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { afterDownloadFailure, afterExportFailure, MAX_CONSENT_RESTARTS } from '@/lib/ct-conversion'
import { ApiError, ConsentRequired, sameConversionSnapshot, type CtLoss, type DocumentView, type InterchangeExport, type InterchangeFormat } from '@/lib/document-contract'

interface Stamp {
  snapshot: DocumentView
  generation: number
}

interface PreparedExport extends Stamp {
  result: InterchangeExport
  url: string
  accepted?: string
}

interface ExportOffer extends Stamp {
  format: InterchangeFormat
  digest: string
  features: string[]
  losses: CtLoss[]
  changed: boolean
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

/** Downloads the current drawing with the same loss consent used by the HTTP and CLI clients. */
export function InterchangeExportPanel({ document, editor }: InterchangeExportPanelProps) {
  const [exported, setExported] = useState<PreparedExport | null>(null)
  const [offer, setOffer] = useState<ExportOffer | null>(null)
  const [unacceptable, setUnacceptable] = useState<(Stamp & { features: string[]; losses: CtLoss[] }) | null>(null)
  const [pending, setPending] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const alive = useRef(true)
  const request = useRef(0)

  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      request.current += 1
    }
  }, [])
  useEffect(() => () => {
    if (exported) URL.revokeObjectURL(exported.url)
  }, [exported])

  const matchesShown = (stamp: Stamp) => stamp.generation === editor.serviceGeneration && sameConversionSnapshot(document, stamp.snapshot)
  const currentExport = exported && matchesShown(exported) ? exported : null
  const currentOffer = offer && matchesShown(offer) ? offer : null
  const currentUnacceptable = unacceptable && matchesShown(unacceptable) ? unacceptable : null
  const busy = pending > 0 || editor.busy

  function clearMessages() {
    setError(null)
    setUnacceptable(null)
  }

  async function previewExport(format: InterchangeFormat, acceptedStart?: string, changedStart = false) {
    const snapshot = document
    const generation = editor.serviceGeneration
    if (!editor.isCurrentConversion(snapshot, generation)) {
      setError('The drawing or service changed. Reload and preview the export again.')
      return
    }
    const sequence = ++request.current
    const currentRequest = () => alive.current && request.current === sequence
    setPending((count) => count + 1)
    clearMessages()
    setExported(null)
    setOffer(null)
    try {
      let accepted = acceptedStart
      let changed = changedStart
      for (let restarts = 0; ; restarts += 1) {
        if (!currentRequest()) return
        if (!editor.isCurrentConversion(snapshot, generation)) {
          setError('The drawing or service changed. Preview the export again.')
          return
        }
        try {
          const result = await editor.exportInterchange(snapshot.id, format, accepted)
          if (!currentRequest()) return
          if (!editor.isCurrentConversion(snapshot, generation) || !sameConversionSnapshot(snapshot, result.snapshot)) {
            throw new Error('The drawing or service changed. Preview the export again.')
          }
          const url = URL.createObjectURL(exportBlob(result))
          setExported({ snapshot: result.snapshot, generation, result, url, accepted })
          return
        } catch (cause) {
          if (!currentRequest()) return
          if (!editor.isCurrentConversion(snapshot, generation)) {
            setError('The drawing or service changed. Preview the export again.')
            return
          }
          const step = cause instanceof ApiError
            ? afterExportFailure(cause.code, cause instanceof ConsentRequired ? cause.digest : undefined, accepted)
            : { kind: 'error' as const }
          if (step.kind === 'offer' && cause instanceof ConsentRequired) {
            setOffer({ snapshot, generation, format, digest: step.digest, features: cause.features, losses: cause.losses, changed: changed || step.changed })
            return
          }
          if (step.kind === 'ask-again' && restarts < MAX_CONSENT_RESTARTS) {
            accepted = undefined
            changed = true
            continue
          }
          if (step.kind === 'no-accept' && cause instanceof ConsentRequired) {
            setUnacceptable({ snapshot, generation, features: cause.features, losses: cause.losses })
          }
          setError(step.kind === 'ask-again'
            ? 'The losses of this export kept changing. Preview the export again.'
            : cause instanceof Error ? cause.message : 'Export failed.')
          return
        }
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function downloadExport(item: PreparedExport) {
    if (!editor.isCurrentConversion(item.snapshot, item.generation)) {
      setExported(null)
      setError('The drawing or service changed. Preview the export again.')
      return
    }
    const sequence = ++request.current
    setPending((count) => count + 1)
    clearMessages()
    try {
      // Recheck at the click: an idle page may not yet know that the service restarted.
      const fresh = await editor.exportInterchange(item.snapshot.id, item.result.format, item.accepted)
      if (!alive.current || request.current !== sequence) return
      if (!editor.isCurrentConversion(item.snapshot, item.generation) ||
          !sameConversionSnapshot(item.snapshot, fresh.snapshot) || fresh.format !== item.result.format ||
          fresh.content !== item.result.content || JSON.stringify(fresh.losses) !== JSON.stringify(item.result.losses)) {
        setExported(null)
        throw new Error('The export preview changed. Preview the export again.')
      }
      const anchor = window.document.createElement('a')
      anchor.href = item.url
      anchor.download = `chemifree.${item.result.format}`
      anchor.click()
    } catch (cause) {
      if (!alive.current || request.current !== sequence) return
      if (!editor.isCurrentConversion(item.snapshot, item.generation)) {
        setExported(null)
        setError('The drawing or service changed. Preview the export again.')
      } else if (cause instanceof ApiError && afterDownloadFailure(cause.code, item.accepted) === 'restart') {
        setExported(null)
        await previewExport(item.result.format, undefined, item.accepted !== undefined)
      } else {
        setExported(null)
        setError(cause instanceof Error ? cause.message : 'Export verification failed.')
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
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
      <p>Writing this {currentOffer.format.toUpperCase()} needs these losses accepted: {currentOffer.features.join(', ')}.</p>
      <LossList losses={currentOffer.losses} />
      <Button size="sm" disabled={busy} onClick={() => void previewExport(currentOffer.format, currentOffer.digest, currentOffer.changed)}>
        Accept these losses and preview {currentOffer.format.toUpperCase()}
      </Button>
    </div>}
    {exported && !currentExport && <p>The drawing or service changed. Preview the export again.</p>}
    {currentExport && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite" data-testid="interchange-export-written">
      <p>Prepared {currentExport.result.format.toUpperCase()} for this drawing.</p>
      <LossList losses={currentExport.result.losses} />
      <Button size="sm" disabled={busy} onClick={() => void downloadExport(currentExport)}>Download {currentExport.result.format.toUpperCase()}</Button>
    </div>}
  </aside>
}
