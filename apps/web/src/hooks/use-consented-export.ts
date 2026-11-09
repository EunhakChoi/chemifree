import { useEffect, useRef, useState } from 'react'

import type { DocumentEditor } from '@/hooks/use-document-editor'
import { afterDownloadFailure, afterExportFailure, type ExportStep, MAX_CONSENT_RESTARTS } from '@/lib/ct-conversion'
import { ApiError, ConsentRequired, sameConversionSnapshot, type CtLoss, type DocumentView } from '@/lib/document-contract'

/** The fields the consent flow reads from a prepared export, whatever its format. */
interface ConsentedExportResult {
  content: string
  snapshot: DocumentView
  losses: CtLoss[]
}

/** The drawing and the service process a result or offer was made for. It is shown only while both still hold. */
interface ExportStamp {
  snapshot: DocumentView
  generation: number
}

/** A prepared export: its result, the object URL of its blob, and the consent it was made with, if any. */
export interface PreparedExport<K extends string, R extends ConsentedExportResult> extends ExportStamp {
  kind: K
  result: R
  url: string
  accepted?: string
}

/** Losses that need consent, with the digest that accepts them. */
interface ExportOffer<K extends string> extends ExportStamp {
  kind: K
  /** Sending this digest again accepts exactly these losses. */
  digest: string
  features: string[]
  losses: CtLoss[]
  /** True when the losses differ from the ones the person accepted before. */
  changed: boolean
}

/** Losses that need consent when the reply carries no digest to accept them with. */
interface UnacceptableExport extends ExportStamp {
  features: string[]
  losses: CtLoss[]
}

interface ConsentedExportOptions<K extends string, R extends ConsentedExportResult> {
  /** The drawing on screen. */
  document: DocumentView
  /** Supplies the service generation and the check that the drawing is still the one shown. */
  editor: Pick<DocumentEditor, 'busy' | 'isCurrentConversion' | 'serviceGeneration'>
  /** One export request for `kind`. `accepted` is the digest of losses the person accepted. */
  run: (id: string, kind: K, accepted?: string) => Promise<R>
  /** The bytes that a download saves for a result. */
  blob: (result: R, kind: K) => Blob
  /** The file name that a download saves as. */
  fileName: (kind: K) => string
}

interface ConsentedExport<K extends string, R extends ConsentedExportResult> {
  /** The last prepared export. It may belong to a drawing that is no longer shown. */
  exported: PreparedExport<K, R> | null
  /** The prepared export, when it still belongs to the drawing and service on screen. */
  current: PreparedExport<K, R> | null
  /** The last loss offer. It may belong to a drawing that is no longer shown. */
  offer: ExportOffer<K> | null
  /** The loss offer, when it still belongs to the drawing and service on screen. */
  currentOffer: ExportOffer<K> | null
  /** The losses without a digest to accept, when they still belong to the drawing and service on screen. */
  currentUnacceptable: UnacceptableExport | null
  /** Requests in flight. */
  pending: number
  /** The message to show, or null. */
  error: string | null
  /** True while a request runs or the editor is busy. */
  busy: boolean
  /**
   * Asks the service for `kind`, then prepares the result or offers the losses that need accepting.
   * `acceptedStart` is a digest the person accepted before; `changedStart` marks that the losses changed since.
   */
  preview: (kind: K, acceptedStart?: string, changedStart?: boolean) => Promise<void>
  /** Re-checks a prepared export at the click, then saves it. */
  download: (item: PreparedExport<K, R>) => Promise<void>
  /**
   * Claims the next request for work that is not a preview or a download, such as a clipboard write.
   * The returned check is true only while no later request has started and the component is still mounted.
   */
  claim: () => () => boolean
  /** Drops the prepared export and shows `message`. */
  discard: (message: string) => void
}

/**
 * The consent flow of one drawing export, shared by the drawing exports that can report losses.
 * A preview asks the service, offers the losses that need accepting, and prepares the file. A
 * download re-checks the export at the click before it saves the file. Results and offers are shown
 * only while the drawing and the service that made them still hold. Failures are messages in
 * `error`, not exceptions.
 */
export function useConsentedExport<K extends string, R extends ConsentedExportResult>(
  options: ConsentedExportOptions<K, R>,
): ConsentedExport<K, R> {
  const { document, editor, run, blob, fileName } = options
  const [exported, setExported] = useState<PreparedExport<K, R> | null>(null)
  const [offer, setOffer] = useState<ExportOffer<K> | null>(null)
  const [unacceptable, setUnacceptable] = useState<UnacceptableExport | null>(null)
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

  const matchesShown = (stamp: ExportStamp) => stamp.generation === editor.serviceGeneration && sameConversionSnapshot(document, stamp.snapshot)
  const current = exported && matchesShown(exported) ? exported : null
  const currentOffer = offer && matchesShown(offer) ? offer : null
  const currentUnacceptable = unacceptable && matchesShown(unacceptable) ? unacceptable : null

  const clearMessages = () => {
    setError(null)
    setUnacceptable(null)
  }

  const preview = async (kind: K, acceptedStart?: string, changedStart = false) => {
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
          const result = await run(snapshot.id, kind, accepted)
          if (!currentRequest()) return
          if (!editor.isCurrentConversion(snapshot, generation) || !sameConversionSnapshot(snapshot, result.snapshot)) {
            throw new Error('The drawing or service changed. Preview the export again.')
          }
          const url = URL.createObjectURL(blob(result, kind))
          setExported({ kind, snapshot: result.snapshot, generation, result, url, accepted })
          return
        } catch (cause: unknown) {
          if (!currentRequest()) return
          if (!editor.isCurrentConversion(snapshot, generation)) {
            setError('The drawing or service changed. Preview the export again.')
            return
          }
          const step: ExportStep = cause instanceof ApiError
            ? afterExportFailure(cause.code, cause instanceof ConsentRequired ? cause.digest : undefined, accepted)
            : { kind: 'error' }
          if (step.kind === 'offer' && cause instanceof ConsentRequired) {
            setOffer({ kind, snapshot, generation, digest: step.digest, features: cause.features, losses: cause.losses, changed: changed || step.changed })
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

  const download = async (item: PreparedExport<K, R>) => {
    if (!editor.isCurrentConversion(item.snapshot, item.generation)) {
      setExported(null)
      setError('The drawing or service changed. Preview the export again.')
      return
    }
    const sequence = ++request.current
    setPending((count) => count + 1)
    clearMessages()
    try {
      // Recheck at the click: an idle page may not yet know that the service restarted. The same
      // kind is requested again, so the format of the fresh result cannot differ from the prepared one.
      const fresh = await run(item.snapshot.id, item.kind, item.accepted)
      if (!alive.current || request.current !== sequence) return
      if (!editor.isCurrentConversion(item.snapshot, item.generation) ||
          !sameConversionSnapshot(item.snapshot, fresh.snapshot) ||
          fresh.content !== item.result.content || JSON.stringify(fresh.losses) !== JSON.stringify(item.result.losses)) {
        setExported(null)
        throw new Error('The export preview changed. Preview the export again.')
      }
      const anchor = window.document.createElement('a')
      anchor.href = item.url
      anchor.download = fileName(item.kind)
      anchor.click()
    } catch (cause: unknown) {
      if (!alive.current || request.current !== sequence) return
      if (!editor.isCurrentConversion(item.snapshot, item.generation)) {
        setExported(null)
        setError('The drawing or service changed. Preview the export again.')
      } else if (cause instanceof ApiError && afterDownloadFailure(cause.code, item.accepted) === 'restart') {
        setExported(null)
        await preview(item.kind, undefined, item.accepted !== undefined)
      } else {
        setExported(null)
        setError(cause instanceof Error ? cause.message : 'Export verification failed.')
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  const claim = () => {
    const sequence = ++request.current
    return () => alive.current && request.current === sequence
  }

  const discard = (message: string) => {
    setExported(null)
    setError(message)
  }

  return {
    exported,
    current,
    offer,
    currentOffer,
    currentUnacceptable,
    pending,
    error,
    busy: pending > 0 || editor.busy,
    preview,
    download,
    claim,
    discard,
  }
}
