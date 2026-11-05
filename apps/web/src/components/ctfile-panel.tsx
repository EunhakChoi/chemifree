import { useEffect, useId, useRef, useState } from 'react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { LossList } from '@/components/loss-list'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { afterDownloadFailure, afterExportFailure, CTAB_OPTIONS, DEFAULT_CTAB, MAX_CONSENT_RESTARTS, parseCtabChoice, type CtabChoice } from '@/lib/ct-conversion'
import { ApiError, ConsentRequired, MAX_CLIPBOARD_MOL_BYTES, sameConversionSnapshot, type CtFormat, type CtImport, type CtLoss, type CtPreview, type DocumentView } from '@/lib/document-contract'

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
  const [molMode, setMolMode] = useState<'import' | 'export' | null>(null)
  const [molText, setMolText] = useState('')
  const [molPrepared, setMolPrepared] = useState<PreparedImport | null>(null)
  const [molExport, setMolExport] = useState<{ item: PreparedExport; choiceVersion: number } | null>(null)
  const [molPending, setMolPending] = useState<{ generation: number; action: 'read' | 'preview' | 'import' | 'write' | 'check' } | null>(null)
  const [molError, setMolError] = useState<string | null>(null)
  const [molStatus, setMolStatus] = useState<string | null>(null)
  const molRequest = useRef(0)
  const molExportText = useRef<HTMLTextAreaElement>(null)
  const molId = useId()
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
      molRequest.current += 1
    }
  }, [])
  useEffect(() => () => { if (exported) URL.revokeObjectURL(exported.url) }, [exported])

  const currentImport = prepared && prepared.generation === editor.serviceGeneration && sameConversionSnapshot(document, prepared.snapshot) ? prepared : null
  const currentExport = exported && exported.generation === editor.serviceGeneration && sameConversionSnapshot(document, exported.snapshot) ? exported : null

  const molBusy = molPending !== null && molPending.generation === editor.serviceGeneration
  const molBytes = new TextEncoder().encode(molText).byteLength
  const molFits = molBytes <= MAX_CLIPBOARD_MOL_BYTES
  const currentMolImport = molPrepared && molPrepared.generation === editor.serviceGeneration &&
    molPrepared.input.source === molText && sameConversionSnapshot(document, molPrepared.snapshot) ? molPrepared : null
  const currentMolExport = molExport && molExport.choiceVersion === choiceVersion && molExport.item.choice === choice &&
    molExport.item.generation === editor.serviceGeneration && sameConversionSnapshot(document, molExport.item.snapshot) ? molExport.item : null

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

  function molCurrent(sequence: number, generation: number): boolean {
    return alive.current && molRequest.current === sequence && editor.currentServiceGeneration() === generation
  }

  function closeMolDialog() {
    molRequest.current += 1
    setMolMode(null)
    setMolText('')
    setMolPrepared(null)
    setMolExport(null)
    setMolPending(null)
    setMolError(null)
    setMolStatus(null)
  }

  function openMolDialog(item?: PreparedExport) {
    molRequest.current += 1
    setMolMode(item ? 'export' : 'import')
    setMolText('')
    setMolPrepared(null)
    setMolExport(item ? { item, choiceVersion } : null)
    setMolPending(null)
    setMolError(null)
    setMolStatus(null)
  }

  async function readMolClipboard() {
    if (molBusy || editor.busy) return
    const sequence = ++molRequest.current
    const generation = editor.serviceGeneration
    setMolPending({ generation, action: 'read' })
    setMolError(null)
    setMolPrepared(null)
    try {
      if (typeof navigator.clipboard?.readText !== 'function') {
        throw new Error('Browser clipboard reading is unavailable. Paste MOL text manually.')
      }
      const text = await navigator.clipboard.readText()
      if (molCurrent(sequence, generation)) setMolText(text)
    } catch {
      if (molCurrent(sequence, generation)) setMolError('Clipboard reading was unavailable, denied or failed. Paste MOL text manually.')
    } finally {
      if (molCurrent(sequence, generation)) setMolPending(null)
    }
  }

  async function previewMolText() {
    if (molBusy || editor.busy || !molText.trim() || !molFits) return
    const sequence = ++molRequest.current
    const generation = editor.serviceGeneration
    const snapshot = document
    const source = molText
    setMolPending({ generation, action: 'preview' })
    setMolPrepared(null)
    setMolError(null)
    try {
      const preview = await editor.previewCt(snapshot.id, snapshot.revision, 'mol', source, 1)
      if (!molCurrent(sequence, generation)) return
      if (!editor.isCurrentConversion(snapshot, generation) || !sameConversionSnapshot(snapshot, preview.snapshot)) {
        throw new Error('The drawing or service changed. Preview the MOL text again.')
      }
      setMolPrepared({ snapshot: preview.snapshot, preview, generation, input: {
        documentId: snapshot.id, revision: snapshot.revision, format: 'mol', source, record: 1, digest: preview.digest,
      } })
    } catch (cause: unknown) {
      if (molCurrent(sequence, generation)) setMolError(cause instanceof Error ? cause.message : 'MOL text preview failed.')
    } finally {
      if (molCurrent(sequence, generation)) setMolPending(null)
    }
  }

  async function importMolText() {
    if (!currentMolImport || molBusy || editor.busy) return
    const sequence = ++molRequest.current
    const item = currentMolImport
    setMolPending({ generation: item.generation, action: 'import' })
    setMolError(null)
    try {
      await editor.importClipboardMol(item.input, item.snapshot, item.generation)
      if (molCurrent(sequence, item.generation)) closeMolDialog()
    } catch (cause: unknown) {
      if (molCurrent(sequence, item.generation)) setMolError(cause instanceof Error ? cause.message : 'MOL text import failed.')
    } finally {
      if (molCurrent(sequence, item.generation)) setMolPending(null)
    }
  }

  async function writeMolClipboard() {
    const item = currentMolExport
    if (!item || molBusy || editor.busy) return
    if (!editor.isCurrentConversion(item.snapshot, item.generation)) {
      setMolError('The drawing or service changed. Preview the MOL export again.')
      return
    }
    const sequence = ++molRequest.current
    setMolPending({ generation: item.generation, action: 'write' })
    setMolError(null)
    setMolStatus(null)
    try {
      if (typeof navigator.clipboard?.writeText !== 'function') {
        throw new Error('Browser clipboard writing is unavailable.')
      }
      // Prepared text is ready before the click: no backend call or await precedes writing.
      await navigator.clipboard.writeText(item.content)
      if (molCurrent(sequence, item.generation)) setMolStatus('MOL text copied to clipboard.')
    } catch {
      if (molCurrent(sequence, item.generation)) setMolError('Clipboard writing was unavailable, denied or failed. Select and copy the MOL text manually.')
    } finally {
      if (molCurrent(sequence, item.generation)) setMolPending(null)
    }
  }

  async function recoverMolImport() {
    if (molBusy) return
    const sequence = ++molRequest.current
    const generation = editor.serviceGeneration
    setMolPending({ generation, action: 'check' })
    setMolError(null)
    try {
      if (editor.clipboardNeedsReload) await editor.reconcileClipboardPaste()
      else await editor.checkClipboardPaste()
      if (molCurrent(sequence, generation)) closeMolDialog()
    } catch (cause: unknown) {
      if (molCurrent(sequence, generation)) setMolError(cause instanceof Error ? cause.message : 'MOL import outcome could not be checked.')
    } finally {
      if (molCurrent(sequence, generation)) setMolPending(null)
    }
  }

  const molRecovery = editor.clipboardPending && editor.clipboardAction === 'mol_import' && <Alert>
    <AlertDescription className="flex flex-col gap-2">
      <p>{editor.clipboardNeedsReload
        ? 'The original MOL import can no longer be confirmed by replay. It may already be present. Reload the document to reconcile its outcome.'
        : 'The MOL import outcome is not confirmed. Check the original attempt before importing again.'}</p>
      <Button size="sm" variant="outline" disabled={molBusy} onClick={() => void recoverMolImport()}>
        {editor.clipboardNeedsReload ? 'Reload document to reconcile' : 'Check MOL import outcome'}
      </Button>
    </AlertDescription>
  </Alert>

  return <Dialog open={molMode !== null} onOpenChange={(open) => { if (!open) closeMolDialog() }}><aside aria-label="MOL and SDF" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">MOL and SDF</h3>
    <p className="leading-relaxed text-muted-foreground" data-testid="ctfile-note">Reads and writes bounded V2000 and V3000 MOL and SDF: atoms, coordinates, charges, isotopes, radicals, single, double and triple bonds (V3000 also aromatic), wedge, hash and wavy marks, and (V3000 only) enhanced stereo groups as COLLECTION entries. Query atoms and bonds, S-groups, R-groups, 3D and reactions are refused, never ignored. Stereo that a version cannot carry is listed as a loss to accept before anything is written.</p>
    <DialogTrigger render={<Button size="sm" variant="outline" disabled={pending > 0 || molBusy || editor.busy} />} onClick={() => openMolDialog()}>Import MOL text…</DialogTrigger>
    {molMode !== 'import' && molRecovery}
    {molMode === null && molError && <Alert variant="destructive"><AlertDescription>{molError}</AlertDescription></Alert>}
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
      {currentExport.format === 'mol' && <DialogTrigger render={<Button size="sm" variant="outline" disabled={pending > 0 || molBusy || editor.busy} />} onClick={() => openMolDialog(currentExport)}>MOL clipboard text…</DialogTrigger>}
    </div>}
    <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col overflow-hidden transition-none data-open:animate-none data-closed:animate-none sm:max-w-lg">
      <DialogHeader className="shrink-0 pr-8">
        <DialogTitle>{molMode === 'import' ? 'Import MOL text' : 'Copy whole drawing as MOL text'}</DialogTitle>
      </DialogHeader>
      <div className="-mx-4 flex min-h-0 flex-col gap-4 overflow-x-hidden overflow-y-auto px-4 pb-4">
        {molMode === 'import' ? <>
          <DialogDescription>Paste MOL text manually or explicitly read the clipboard. Only Preview MOL text sends it to the local service, as MOL record 1. Review every reported loss before importing.</DialogDescription>
          {molMode === 'import' && molRecovery}
          <FieldGroup><Field>
            <FieldLabel htmlFor={`${molId}-import`}>MOL text to import</FieldLabel>
            <Textarea id={`${molId}-import`} className="field-sizing-fixed max-h-48 resize-y" rows={7} spellCheck={false} value={molText} disabled={molBusy || editor.busy}
              onChange={(event) => { molRequest.current += 1; setMolText(event.target.value); setMolPrepared(null); setMolError(null) }} />
            <FieldDescription>UTF-8 MOL text, up to 200 KiB. SDF, SMILES and other clipboard formats are not accepted here.</FieldDescription>
          </Field></FieldGroup>
          {!molFits && <Alert variant="destructive"><AlertDescription>This web form accepts MOL text up to 200 KiB of UTF-8.</AlertDescription></Alert>}
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={molBusy || editor.busy} onClick={() => void readMolClipboard()}>Read clipboard</Button>
            <Button variant="outline" disabled={molBusy || editor.busy || !molText.trim() || !molFits} onClick={() => void previewMolText()}>Preview MOL text</Button>
          </div>
          {molPrepared && !currentMolImport && <p>The drawing, source text or service changed. Preview the MOL text again.</p>}
          {currentMolImport && <div className="flex flex-col gap-2" aria-live="polite">
            <p>{currentMolImport.preview.records.length} record(s); importing MOL record 1.</p>
            {currentMolImport.preview.records.map((item, index) => <p key={`${index}-${item.title}`}>{index + 1}. {item.title || '(untitled)'}: {item.atoms} atoms, {item.bonds} bonds, {item.fields.length} fields</p>)}
            <LossList losses={currentMolImport.preview.losses} />
            {currentMolImport.preview.losses.length > 0 && <p>Importing accepts every loss listed above.</p>}
            <Button disabled={molBusy || editor.busy} onClick={() => void importMolText()}>Import MOL text</Button>
          </div>}
        </> : <>
          <DialogDescription>This is the entire drawing export under the existing first-placement rules, regardless of selected atoms or bonds. It is not a selected-fragment copy. The prepared MOL version and accepted losses are the same as the export preview.</DialogDescription>
          {!currentMolExport && <p>The drawing, MOL version or service changed. Close this dialog and preview MOL export again.</p>}
          {currentMolExport && <><p>Written as MOL {currentMolExport.written === 'v3000' ? 'V3000' : 'V2000'}{currentMolExport.choice === 'auto' ? ' (chosen by Auto)' : ''}.</p><LossList losses={currentMolExport.losses} /></>}
          <FieldGroup><Field>
            <FieldLabel htmlFor={`${molId}-export`}>Whole drawing MOL text</FieldLabel>
            <Textarea ref={molExportText} id={`${molId}-export`} className="field-sizing-fixed max-h-48 resize-y" rows={7} spellCheck={false} readOnly value={currentMolExport?.content ?? ''} onFocus={(event) => event.currentTarget.select()} />
          </Field></FieldGroup>
          <DialogFooter className="mx-0 mb-0">
            <Button variant="outline" disabled={molBusy || editor.busy || !currentMolExport} onClick={() => { molExportText.current?.focus(); molExportText.current?.select() }}>Select MOL text</Button>
            <Button disabled={molBusy || editor.busy || !currentMolExport} onClick={() => void writeMolClipboard()}>Copy to clipboard</Button>
          </DialogFooter>
        </>}
        {molBusy && <p role="status">{molPending?.action === 'preview' ? 'Previewing MOL text…' : 'Working…'}</p>}
        {molError && <Alert variant="destructive"><AlertDescription>{molError}</AlertDescription></Alert>}
        {molStatus && <p role="status">{molStatus}</p>}
      </div>
    </DialogContent>
  </aside></Dialog>
}
