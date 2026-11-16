import { useEffect, useId, useMemo, useRef, useState, type ChangeEvent, type DragEvent } from 'react'

import { InterchangeCreationNotice, InterchangeImportFlow } from '@/components/cdxml-import'
import { CtImportFlow, MolTextImportFlow } from '@/components/ctfile-panel'
import { SmilesImportFlow } from '@/components/smiles-panel'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView } from '@/lib/document-contract'
import {
  decodeImportFile,
  detectImportFile,
  detectImportText,
  detectionSentence,
  formatLabel,
  MAX_IMPORT_FILE_BYTES,
  type ImportFlowHandle,
  type ImportFormat,
} from '@/lib/import-detect'

const FORMATS: readonly ImportFormat[] = ['smiles', 'mol', 'sdf', 'cdxml', 'cdx']
const TOO_LARGE = 'This web form reads files up to 200 KiB (SMILES 16 KiB, CDXML and CDX 120 KiB). The operation API and CLI have their own limits.'
const CLIPBOARD_DISCARDED = 'The text, drawing or service changed while reading clipboard. The clipboard result was not used.'
const CLIPBOARD_FAILED = 'Clipboard reading was unavailable, denied or failed. Paste the text manually.'

interface PickedFile {
  name: string
  size: number
  bytes: Uint8Array
}

/** The Import card: one entry for SMILES, MOL, SDF, CDXML and CDX text or files. It detects the format, the user can override it, and the matching flow previews and commits the source. */
export function ImportCard({ document, editor }: { document: DocumentView | null; editor: DocumentEditor }) {
  const [text, setText] = useState('')
  const [file, setFile] = useState<PickedFile | null>(null)
  const [fileEpoch, setFileEpoch] = useState(0)
  const [sourceVersion, setSourceVersion] = useState(0)
  const [override, setOverride] = useState<ImportFormat | null>(null)
  const [reading, setReading] = useState<'file' | 'clipboard' | null>(null)
  const [cardError, setCardError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [composing, setComposing] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [checking, setChecking] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)
  const flow = useRef<ImportFlowHandle | null>(null)
  const fileRequest = useRef(0)
  const clipboardRequest = useRef(0)
  const textEpoch = useRef(0)
  const alive = useRef(true)
  const titleId = useId()

  useEffect(() => {
    alive.current = true
    // Unmounting ends every request that is still in flight.
    return () => {
      alive.current = false
      fileRequest.current += 1
      clipboardRequest.current += 1
    }
  }, [])
  useEffect(() => {
    // A service restart keeps the picked name, so clear it: the same file can then be picked again.
    if (fileInput.current) fileInput.current.value = ''
  }, [editor.serviceGeneration])

  const origin = file ? 'file' : 'text'
  const detection = useMemo(() => (file ? detectImportFile(file.name, file.bytes) : detectImportText(text)), [file, text])
  const effective = override ?? (detection?.kind === 'detected' ? detection.format : null)
  const decoded = useMemo(() => (file && effective ? decodeImportFile(file.bytes, effective) : null), [file, effective])
  const locked = editor.interchangeCreation !== null
  const idle = !file && text.trim() === ''
  const suggestedTitle = file ? (file.name.replace(/\.cdx(ml)?$/i, '') || 'Imported drawing') : 'Imported drawing'

  function editText(value: string, fromClipboard: boolean) {
    textEpoch.current += 1
    clipboardRequest.current += 1
    fileRequest.current += 1
    if (file) {
      setFile(null)
      setFileEpoch((epoch) => epoch + 1)
      if (fileInput.current) fileInput.current.value = ''
    }
    setText(value)
    setSourceVersion((version) => version + 1)
    setCardError(null)
    setNotice(null)
    setReading(null)
    if (value === '' || fromClipboard || override === 'cdx') setOverride(null)
    flow.current?.reset()
  }

  async function chooseFile(selected: File | undefined) {
    const request = ++fileRequest.current
    setCardError(null)
    setNotice(null)
    setText('')
    setOverride(null)
    const drop = (message: string | null) => {
      setFile(null)
      setFileEpoch((epoch) => epoch + 1)
      if (fileInput.current) fileInput.current.value = ''
      setCardError(message)
    }
    if (!selected) {
      drop(null)
      return
    }
    if (selected.size === 0) {
      drop('The file is empty.')
      return
    }
    if (selected.size > MAX_IMPORT_FILE_BYTES) {
      drop(TOO_LARGE)
      return
    }
    setReading('file')
    try {
      const bytes = new Uint8Array(await selected.arrayBuffer())
      if (alive.current && fileRequest.current === request) {
        setFile({ name: selected.name, size: selected.size, bytes })
        setFileEpoch((epoch) => epoch + 1)
        setSourceVersion((version) => version + 1)
        flow.current?.reset()
      }
    } catch {
      if (alive.current && fileRequest.current === request) drop('The file could not be read.')
    } finally {
      if (alive.current && fileRequest.current === request) setReading(null)
    }
  }

  async function readClipboard() {
    if (reading !== null || editor.busy || locked) return
    const sequence = ++clipboardRequest.current
    const askedEpoch = textEpoch.current
    const askedGeneration = editor.serviceGeneration
    const drawing = document
    setReading('clipboard')
    setCardError(null)
    setNotice(null)
    try {
      if (typeof navigator.clipboard?.readText !== 'function') throw new Error('Browser clipboard reading is unavailable.')
      const value = await navigator.clipboard.readText()
      if (!alive.current) return
      if (clipboardRequest.current !== sequence || editor.currentServiceGeneration() !== askedGeneration ||
          textEpoch.current !== askedEpoch || (drawing !== null && !editor.isCurrentConversion(drawing, askedGeneration))) {
        setNotice(CLIPBOARD_DISCARDED)
        return
      }
      editText(value, true)
    } catch {
      if (alive.current && clipboardRequest.current === sequence && editor.currentServiceGeneration() === askedGeneration) setCardError(CLIPBOARD_FAILED)
    } finally {
      if (alive.current && clipboardRequest.current === sequence) setReading(null)
    }
  }

  function clear() {
    clipboardRequest.current += 1
    fileRequest.current += 1
    textEpoch.current += 1
    setText('')
    setFile(null)
    setFileEpoch((epoch) => epoch + 1)
    if (fileInput.current) fileInput.current.value = ''
    setOverride(null)
    setCardError(null)
    setNotice(null)
    setReading(null)
    flow.current?.reset()
  }

  function chooseFormat(event: ChangeEvent<HTMLSelectElement>) {
    const next = FORMATS.find((format) => format === event.currentTarget.value)
    if (next) {
      setOverride(next)
      setCardError(null)
    }
  }

  function carriesFiles(event: DragEvent<HTMLElement>) {
    return event.dataTransfer.types.includes('Files')
  }

  function dragOver(event: DragEvent<HTMLElement>) {
    if (!carriesFiles(event)) return
    event.preventDefault()
    setDragging(true)
  }

  function dragLeave(event: DragEvent<HTMLElement>) {
    if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setDragging(false)
  }

  function drop(event: DragEvent<HTMLElement>) {
    if (!carriesFiles(event)) return
    event.preventDefault()
    setDragging(false)
    if (locked || editor.busy || reading !== null) {
      setCardError('Drop the file again when the current request has finished.')
      return
    }
    if (event.dataTransfer.files.length !== 1) {
      setCardError('Drop one file at a time.')
      return
    }
    void chooseFile(event.dataTransfer.files[0])
  }

  async function checkOutcome(kind: 'smiles' | 'mol') {
    if (checking) return
    const sequence = ++clipboardRequest.current
    const generation = editor.serviceGeneration
    setChecking(true)
    setCardError(null)
    setNotice(null)
    const current = () => alive.current && clipboardRequest.current === sequence && editor.currentServiceGeneration() === generation
    const fallback = kind === 'smiles' ? 'The SMILES import outcome could not be checked.' : 'MOL import outcome could not be checked.'
    try {
      if (editor.clipboardNeedsReload) await editor.reconcileClipboardPaste()
      else await editor.checkClipboardPaste()
      if (current()) {
        if (kind === 'smiles') {
          flow.current?.reset()
          setNotice('SMILES import outcome reconciled with the document.')
        } else {
          clear()
        }
      }
    } catch (failure: unknown) {
      if (current()) setCardError(failure instanceof Error ? failure.message : fallback)
    } finally {
      // clear() bumps clipboardRequest, so the busy flag is cleared whenever this card is still mounted.
      if (alive.current) setChecking(false)
    }
  }

  function renderRoute() {
    if (idle) return null
    // A file being read replaces the current source: the old flow is unmounted, so its preview cannot be imported meanwhile.
    if (reading === 'file') return null
    if (effective === null) return <p role="status" data-testid="import-ambiguous">{detection ? `${detectionSentence(detection, null)} ` : ''}Choose the format; nothing is sent until you do.</p>
    if (file && decoded && !decoded.ok) return <p role="alert" className="text-destructive">{decoded.message}</p>
    const source = file ? (decoded?.ok ? decoded.source : '') : text
    const key = `${effective}:${origin}:${fileEpoch}`
    if (effective === 'cdxml' || effective === 'cdx') {
      return <>
        {document && <p>This creates a new drawing and switches to it, like New document. Unsaved changes to this drawing are written to its recovery draft first.</p>}
        <InterchangeImportFlow key={key} ref={flow} editor={editor} source={source} origin={origin} format={effective} suggestedTitle={suggestedTitle} byteLength={file?.bytes.length} onDone={clear} />
      </>
    }
    if (document === null) return <p role="status" data-testid="import-needs-document">{formatLabel(effective)} is added to an open drawing. Create or open a document, then import it there. Nothing was sent.</p>
    if (effective === 'smiles') return <SmilesImportFlow key={key} ref={flow} document={document} editor={editor} source={source} origin={origin} sourceVersion={sourceVersion} composing={composing} />
    if (effective === 'mol' && origin === 'text') return <MolTextImportFlow key={key} ref={flow} document={document} editor={editor} source={source} origin={origin} onDone={clear} />
    if (origin === 'text' && new TextEncoder().encode(text).byteLength > MAX_IMPORT_FILE_BYTES) {
      return <p role="alert" className="text-destructive">SDF text exceeds 200 KiB of UTF-8, the limit of an SDF file in this web form. Nothing was sent.</p>
    }
    return <CtImportFlow key={key} ref={flow} document={document} editor={editor} source={source} origin={origin} fileName={file?.name} format={effective} />
  }

  return (
    <section
      aria-labelledby={titleId}
      data-testid="import-card"
      data-dragging={dragging || undefined}
      className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs data-dragging:border-dashed data-dragging:border-ring data-dragging:bg-muted/50"
      onDragEnter={dragOver}
      onDragOver={dragOver}
      onDragLeave={dragLeave}
      onDrop={drop}
    >
      <h3 id={titleId} className="text-sm font-medium">Import</h3>
      <p className="text-muted-foreground">{document ? 'SMILES, MOL and SDF are added to this drawing. CDXML and CDX create a new drawing and switch to it.' : 'CDXML and CDX create a new drawing. SMILES, MOL and SDF are added to an open drawing.'}</p>
      <p className="text-muted-foreground">Choose a file, drop one on this card, or paste text.</p>
      <Field>
        <FieldLabel htmlFor="import-file">Import file</FieldLabel>
        <Input ref={fileInput} id="import-file" type="file" accept=".smi,.smiles,.mol,.sdf,.sd,.cdxml,.cdx,.xml,.txt,chemical/x-mdl-molfile,chemical/x-mdl-sdfile,application/xml,text/xml,text/plain" disabled={locked || editor.busy || reading !== null} onChange={(event) => void chooseFile(event.currentTarget.files?.[0])} />
        <FieldDescription>{file ? `${file.name} (${(file.size / 1024).toFixed(1)} KiB)` : 'No file chosen.'}</FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor="import-text">Text to import</FieldLabel>
        <Textarea id="import-text" rows={5} spellCheck={false} autoComplete="off" autoCapitalize="off" className="field-sizing-fixed max-h-48 resize-y font-mono text-xs" value={text} disabled={locked} onChange={(event) => editText(event.currentTarget.value, false)} onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)} />
        <FieldDescription>Kept exactly as entered. Reading the clipboard only fills this field; nothing is sent until you press a preview or read button.</FieldDescription>
      </Field>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={reading !== null || editor.busy || locked} onClick={() => void readClipboard()}>Read clipboard</Button>
        <Button size="sm" variant="outline" disabled={locked} onClick={clear}>Clear</Button>
      </div>
      <Field>
        <FieldLabel htmlFor="import-format">Format</FieldLabel>
        <select id="import-format" value={effective ?? ''} disabled={idle || editor.busy || locked} className="h-8 rounded-lg border border-input bg-transparent px-2 text-xs" onChange={chooseFormat}>
          <option value="" disabled>Choose a format</option>
          <option value="smiles">SMILES</option>
          <option value="mol">MOL</option>
          <option value="sdf">SDF</option>
          <option value="cdxml">CDXML</option>
          <option value="cdx" disabled={origin === 'text'} title={origin === 'text' ? 'CDX is binary: choose or drop the .cdx file.' : undefined}>CDX</option>
        </select>
        {detection && <p data-testid="import-detected">{detectionSentence(detection, override)}</p>}
      </Field>
      {reading && <p role="status">{reading === 'file' ? 'Reading the file…' : 'Reading the clipboard…'}</p>}
      {cardError && <p role="alert" data-testid="import-error" className="text-destructive">{cardError}</p>}
      {notice && <p role="status">{notice}</p>}
      {document && editor.clipboardPending && editor.clipboardAction === 'smiles_import' && <Alert>
        <AlertDescription className="flex flex-col gap-2">
          <p>{editor.clipboardNeedsReload
            ? 'The original SMILES import can no longer be confirmed by replay. It may already be present. Reload the document to reconcile its outcome.'
            : 'The SMILES import outcome is not confirmed. Check the original attempt before importing again. Editing this source text does not change that attempt.'}</p>
          <Button size="sm" variant="outline" disabled={checking || reading !== null} onClick={() => void checkOutcome('smiles')}>
            {editor.clipboardNeedsReload ? 'Reload document to reconcile' : 'Check SMILES import outcome'}
          </Button>
        </AlertDescription>
      </Alert>}
      {document && editor.clipboardPending && editor.clipboardAction === 'mol_import' && <Alert>
        <AlertDescription className="flex flex-col gap-2">
          <p>{editor.clipboardNeedsReload
            ? 'The original MOL import can no longer be confirmed by replay. It may already be present. Reload the document to reconcile its outcome.'
            : 'The MOL import outcome is not confirmed. Check the original attempt before importing again.'}</p>
          <Button size="sm" variant="outline" disabled={checking || reading !== null} onClick={() => void checkOutcome('mol')}>
            {editor.clipboardNeedsReload ? 'Reload document to reconcile' : 'Check MOL import outcome'}
          </Button>
        </AlertDescription>
      </Alert>}
      {document === null && <InterchangeCreationNotice editor={editor} onClear={clear} />}
      {renderRoute()}
    </section>
  )
}
