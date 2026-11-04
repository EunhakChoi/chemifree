import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { LossList } from '@/components/loss-list'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { afterDownloadFailure, afterExportFailure, CTAB_OPTIONS, DEFAULT_CTAB, MAX_CONSENT_RESTARTS, parseCtabChoice, type CtabChoice } from '@/lib/ct-conversion'
import { ApiError, ConsentRequired, sameConversionSnapshot, type CtFormat, type CtImport, type CtLoss, type CtPreview, type DocumentView } from '@/lib/document-contract'

interface SourceFile {
  name: string
  text: string
  format: CtFormat
}
interface PreparedImport {
  input: CtImport
  snapshot: DocumentView
  preview: CtPreview
  generation: number
}
interface PreparedExport {
  format: CtFormat
  snapshot: DocumentView
  url: string
  content: string
  losses: CtLoss[]
  generation: number
  /** The version asked for, the version the service says it wrote, and the digest accepted (if any). */
  choice: CtabChoice
  written: 'v2000' | 'v3000'
  accepted?: string
}
/** Losses the service says need consent, for a request this page has no way to accept. */
interface Unacceptable {
  features: string[]
  losses: CtLoss[]
}
/** Losses the person must accept before this export is written, for the drawing and choice asked about. */
interface ExportOffer {
  format: CtFormat
  choiceVersion: number
  digest: string
  features: string[]
  losses: CtLoss[]
  /** True when an accepted digest no longer matched and the losses are shown again. */
  changed: boolean
  drawing: DocumentView
  generation: number
}

/** The editor uses the same preview, loss consent and operation contract as other clients. */
export function CtfilePanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [file, setFile] = useState<SourceFile | null>(null)
  const [record, setRecord] = useState('1')
  const [prepared, setPrepared] = useState<PreparedImport | null>(null)
  const [exported, setExported] = useState<PreparedExport | null>(null)
  const [choice, setChoice] = useState<CtabChoice>(DEFAULT_CTAB)
  const [choiceVersion, setChoiceVersion] = useState(0)
  const [offer, setOffer] = useState<ExportOffer | null>(null)
  const [unacceptable, setUnacceptable] = useState<Unacceptable | null>(null)
  const [pending, setPending] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const alive = useRef(true)
  const fileRequest = useRef(0)
  const previewRequest = useRef(0)
  const exportRequest = useRef(0)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      fileRequest.current += 1
      previewRequest.current += 1
      exportRequest.current += 1
    }
  }, [])
  useEffect(() => () => { if (exported) URL.revokeObjectURL(exported.url) }, [exported])

  const currentImport = prepared && prepared.generation === editor.serviceGeneration && sameConversionSnapshot(document, prepared.snapshot) ? prepared : null
  const currentExport = exported && exported.generation === editor.serviceGeneration && sameConversionSnapshot(document, exported.snapshot) ? exported : null

  const currentOffer = offer && offer.choiceVersion === choiceVersion && offer.generation === editor.serviceGeneration &&
    sameConversionSnapshot(document, offer.drawing) ? offer : null

  function clearMessages() {
    setError(null)
    setUnacceptable(null)
  }

  function changeChoice(next: CtabChoice) {
    exportRequest.current += 1
    setChoice(next)
    setChoiceVersion((version) => version + 1)
    setExported(null)
    setOffer(null)
    clearMessages()
  }

  async function readFile(selected: File | undefined) {
    const sequence = ++fileRequest.current
    previewRequest.current += 1
    setFile(null)
    setPrepared(null)
    clearMessages()
    if (!selected) return
    const extension = selected.name.toLowerCase().split('.').pop()
    if (extension !== 'mol' && extension !== 'sdf') {
      setError('Choose a .mol or .sdf file.')
      return
    }
    if (selected.size > 200 * 1024) {
      setError('This web form accepts files up to 200 KiB. The operation API and CLI have their own request limits.')
      return
    }
    setPending((count) => count + 1)
    try {
      const text = new TextDecoder('utf-8', { fatal: true }).decode(await selected.arrayBuffer())
      if (alive.current && fileRequest.current === sequence) setFile({ name: selected.name, text, format: extension })
    } catch {
      if (alive.current && fileRequest.current === sequence) setError('The selected file is not valid UTF-8.')
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function previewImport() {
    const number = Number(record)
    if (!file || !Number.isSafeInteger(number) || number < 1) {
      setError('Choose a file and enter a record number starting at 1.')
      return
    }
    const snapshot = document
    const generation = editor.serviceGeneration
    const sequence = ++previewRequest.current
    setPending((count) => count + 1)
    clearMessages()
    setPrepared(null)
    try {
      const preview = await editor.previewCt(snapshot.id, snapshot.revision, file.format, file.text, number)
      if (!sameConversionSnapshot(snapshot, preview.snapshot)) throw new Error('The service previewed a different drawing. Reload and preview again.')
      if (alive.current && previewRequest.current === sequence) setPrepared({ snapshot: preview.snapshot, preview, generation, input: {
        documentId: snapshot.id, revision: snapshot.revision, format: file.format,
        source: file.text, record: number, digest: preview.digest,
      } })
    } catch (cause) {
      if (alive.current && previewRequest.current === sequence) {
        if (cause instanceof ConsentRequired) setUnacceptable({ features: cause.features, losses: cause.losses })
        setError(cause instanceof Error ? cause.message : 'Import preview failed.')
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  async function commitImport() {
    if (!currentImport) return
    setPending((count) => count + 1)
    const committed = await editor.importCt(currentImport.input, currentImport.snapshot, currentImport.generation)
    if (alive.current) {
      if (committed) setPrepared(null)
      setPending((count) => count - 1)
    }
  }

  async function previewExport(format: CtFormat, acceptedStart?: string, changedStart = false) {
    const snapshot = document
    const generation = editor.serviceGeneration
    const asked = choice
    const askedVersion = choiceVersion
    const sequence = ++exportRequest.current
    setPending((count) => count + 1)
    clearMessages()
    setExported(null)
    setOffer(null)
    try {
      let accepted = acceptedStart
      let changed = changedStart
      // An accepted digest that went stale is dropped and the export asked again, a few times at most.
      for (let restarts = 0; ; restarts += 1) {
        try {
          const result = await editor.exportCt(snapshot.id, format, asked, accepted)
          if (alive.current && exportRequest.current === sequence) {
            const url = URL.createObjectURL(new Blob([result.content], { type: 'chemical/x-mdl-molfile;charset=utf-8' }))
            setExported({ format, snapshot: result.snapshot, url, content: result.content, losses: result.losses, generation,
              choice: asked, written: result.ctab, accepted })
          }
          return
        } catch (cause) {
          // Another request, a change of version or an unmount replaced this one: say nothing.
          if (!alive.current || exportRequest.current !== sequence) return
          const step = cause instanceof ApiError
            ? afterExportFailure(cause.code, cause instanceof ConsentRequired ? cause.digest : undefined, accepted)
            : { kind: 'error' as const }
          if (step.kind === 'offer' && cause instanceof ConsentRequired) {
            setOffer({ format, choiceVersion: askedVersion, digest: step.digest, features: cause.features,
              losses: cause.losses, changed: changed || step.changed, drawing: snapshot, generation })
            return
          }
          if (step.kind === 'ask-again' && restarts < MAX_CONSENT_RESTARTS) {
            accepted = undefined
            changed = true
            continue
          }
          if (step.kind === 'no-accept' && cause instanceof ConsentRequired) setUnacceptable({ features: cause.features, losses: cause.losses })
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
    setPending((count) => count + 1)
    clearMessages()
    try {
      // Recheck at the click: a service restart may have occurred while the page was idle.
      const fresh = await editor.exportCt(item.snapshot.id, item.format, item.choice, item.accepted)
      if (!alive.current || !editor.isCurrentConversion(item.snapshot, item.generation) ||
          !sameConversionSnapshot(item.snapshot, fresh.snapshot) || fresh.content !== item.content || fresh.ctab !== item.written ||
          JSON.stringify(fresh.losses) !== JSON.stringify(item.losses)) {
        throw new Error('The export preview changed. Preview the export again.')
      }
      const anchor = window.document.createElement('a')
      anchor.href = item.url
      anchor.download = `chemifree.${item.format}`
      anchor.click()
    } catch (cause) {
      if (!alive.current) return
      if (cause instanceof ApiError && afterDownloadFailure(cause.code, item.accepted) === 'restart') {
        // The consent no longer fits what would be written: drop the stale preview and show the new losses.
        setExported(null)
        void previewExport(item.format, undefined, item.accepted !== undefined)
      } else {
        setError(cause instanceof Error ? cause.message : 'Export verification failed.')
      }
    } finally {
      if (alive.current) setPending((count) => count - 1)
    }
  }

  return <aside aria-label="MOL and SDF" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">MOL and SDF</h3>
    <p className="leading-relaxed text-muted-foreground" data-testid="ctfile-note">Reads and writes bounded V2000 and V3000 MOL and SDF: atoms, coordinates, charges, isotopes, radicals, single, double and triple bonds (V3000 also aromatic), wedge, hash and wavy marks, and (V3000 only) enhanced stereo groups as COLLECTION entries. Query atoms and bonds, S-groups, R-groups, 3D and reactions are refused, never ignored. Stereo that a version cannot carry is listed as a loss to accept before anything is written.</p>
    <Field>
      <FieldLabel htmlFor="ctfile-input">Import file</FieldLabel>
      <Input id="ctfile-input" type="file" accept=".mol,.sdf" disabled={pending > 0 || editor.busy}
        onChange={(event) => void readFile(event.currentTarget.files?.[0])} />
      <FieldDescription>{file ? file.name : 'Choose a UTF-8 MOL or SDF file.'}</FieldDescription>
    </Field>
    <Field>
      <FieldLabel htmlFor="ctfile-record">Record number</FieldLabel>
      <Input id="ctfile-record" type="number" min={1} step={1} value={record} disabled={pending > 0 || editor.busy}
        onChange={(event) => { previewRequest.current += 1; setRecord(event.currentTarget.value); setPrepared(null) }} />
    </Field>
    <Button size="sm" variant="outline" disabled={!file || pending > 0 || editor.busy} onClick={() => void previewImport()}>
      Preview import
    </Button>
    {error && <p role="alert">{error}</p>}
    {unacceptable && <div role="group" aria-label="Losses that need consent" className="flex max-h-64 flex-col gap-2 overflow-auto" data-testid="ctfile-unacceptable">
      <p>These losses ({unacceptable.features.join(', ')}) need consent, but this page cannot accept them for this request. Nothing was changed.</p>
      <LossList losses={unacceptable.losses} />
    </div>}
    {prepared && !currentImport && <p>The drawing changed. Preview the import again.</p>}
    {currentImport && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>{currentImport.preview.records.length} record(s); importing record {currentImport.input.record}.</p>
      {currentImport.preview.records.map((item, index) => <div key={`${index}-${item.title}`}>
        <p>{index + 1}. {item.title || '(untitled)'}: {item.atoms} atoms, {item.bonds} bonds, {item.fields.length} fields</p>
        {item.fields.map((field, fieldIndex) => <p className="break-all whitespace-pre-wrap" key={`${field.name}-${fieldIndex}`}>
          {field.name}: {field.value || '(empty)'}
        </p>)}
      </div>)}
      <LossList losses={currentImport.preview.losses} />
      <Button size="sm" disabled={pending > 0 || editor.busy} onClick={() => void commitImport()}>
        Accept losses and import
      </Button>
    </div>}
    <Field>
      <FieldLabel htmlFor="ctfile-version">MOL version to write</FieldLabel>
      <select id="ctfile-version" value={choice} disabled={pending > 0 || editor.busy}
        className="h-8 rounded-lg border border-input bg-transparent px-2 text-xs"
        onChange={(event) => changeChoice(parseCtabChoice(event.currentTarget.value))}>
        {CTAB_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
      </select>
      <FieldDescription>Your choice for this page only; it is not saved in the drawing.</FieldDescription>
    </Field>
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={pending > 0 || editor.busy} onClick={() => void previewExport('mol')}>Preview MOL export</Button>
      <Button size="sm" variant="outline" disabled={pending > 0 || editor.busy} onClick={() => void previewExport('sdf')}>Preview SDF export</Button>
    </div>
    {offer && !currentOffer && <p>The drawing or the version changed. Preview the export again.</p>}
    {currentOffer && <div role="group" aria-label="Losses to accept before writing" className="flex max-h-64 flex-col gap-2 overflow-auto" data-testid="ctfile-offer">
      {currentOffer.changed && <p>The losses changed since you accepted them. Review them again.</p>}
      <p>Writing this {currentOffer.format.toUpperCase()} needs these losses accepted: {currentOffer.features.join(', ')}.</p>
      <LossList losses={currentOffer.losses} />
      <Button size="sm" disabled={pending > 0 || editor.busy} onClick={() => void previewExport(currentOffer.format, currentOffer.digest, currentOffer.changed)}>
        Accept these losses and preview {currentOffer.format.toUpperCase()}
      </Button>
    </div>}
    {exported && !currentExport && <p>The drawing changed. Preview the export again.</p>}
    {currentExport && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite">
      <p data-testid="ctfile-written">Written as {currentExport.format.toUpperCase()} {currentExport.written === 'v3000' ? 'V3000' : 'V2000'}{currentExport.choice === 'auto' ? ' (chosen by Auto)' : ''}.</p>
      <LossList losses={currentExport.losses} />
      <Button size="sm" disabled={pending > 0 || editor.busy} onClick={() => void downloadExport(currentExport)}>
        Download {currentExport.format.toUpperCase()}
      </Button>
    </div>}
  </aside>
}
