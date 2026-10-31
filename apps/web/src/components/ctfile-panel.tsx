import { useEffect, useRef, useState } from 'react'

import { Button, buttonVariants } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { CtFormat, CtImport, CtLoss, CtPreview, DocumentView } from '@/lib/document-contract'

interface SourceFile {
  name: string
  text: string
  format: CtFormat
}
interface PreparedImport {
  input: CtImport
  snapshot: DocumentView
  preview: CtPreview
}
interface PreparedExport {
  format: CtFormat
  snapshot: DocumentView
  url: string
  losses: CtLoss[]
}

function sameDrawing(a: DocumentView, b: DocumentView): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

function Losses({ losses }: { losses: CtLoss[] }) {
  if (losses.length === 0) return <p>No conversion losses were reported.</p>
  return <ul className="list-disc space-y-1 pl-4">
    {losses.map((loss, index) => <li key={`${loss.source_path}-${loss.feature}-${index}`}>
      {loss.source_path}: {loss.reason} ({loss.semantic_impact}, {loss.disposition})
    </li>)}
  </ul>
}

/** The editor uses the same preview, loss consent and operation contract as other clients. */
export function CtfilePanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [file, setFile] = useState<SourceFile | null>(null)
  const [record, setRecord] = useState('1')
  const [prepared, setPrepared] = useState<PreparedImport | null>(null)
  const [exported, setExported] = useState<PreparedExport | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false }
  }, [])
  useEffect(() => () => { if (exported) URL.revokeObjectURL(exported.url) }, [exported])

  const currentImport = prepared && sameDrawing(document, prepared.snapshot) ? prepared : null
  const currentExport = exported && sameDrawing(document, exported.snapshot) ? exported : null

  async function readFile(selected: File | undefined) {
    setFile(null)
    setPrepared(null)
    setError(null)
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
    setPending(true)
    try {
      const text = new TextDecoder('utf-8', { fatal: true }).decode(await selected.arrayBuffer())
      if (alive.current) setFile({ name: selected.name, text, format: extension })
    } catch {
      if (alive.current) setError('The selected file is not valid UTF-8.')
    } finally {
      if (alive.current) setPending(false)
    }
  }

  async function previewImport() {
    const number = Number(record)
    if (!file || !Number.isSafeInteger(number) || number < 1) {
      setError('Choose a file and enter a record number starting at 1.')
      return
    }
    const snapshot = document
    setPending(true)
    setError(null)
    setPrepared(null)
    try {
      const preview = await editor.previewCt(snapshot.id, snapshot.revision, file.format, file.text, number)
      if (alive.current) setPrepared({ snapshot, preview, input: {
        documentId: snapshot.id, revision: snapshot.revision, format: file.format,
        source: file.text, record: number, digest: preview.digest,
      } })
    } catch (cause) {
      if (alive.current) setError(cause instanceof Error ? cause.message : 'Import preview failed.')
    } finally {
      if (alive.current) setPending(false)
    }
  }

  async function commitImport() {
    if (!currentImport) return
    setPending(true)
    const committed = await editor.importCt(currentImport.input)
    if (alive.current) {
      if (committed) setPrepared(null)
      setPending(false)
    }
  }

  async function previewExport(format: CtFormat) {
    const snapshot = document
    setPending(true)
    setError(null)
    try {
      const result = await editor.exportCt(snapshot.id, format)
      if (alive.current) {
        const url = URL.createObjectURL(new Blob([result.content], { type: 'chemical/x-mdl-molfile;charset=utf-8' }))
        setExported({ format, snapshot: result.snapshot, url, losses: result.losses })
      }
    } catch (cause) {
      if (alive.current) setError(cause instanceof Error ? cause.message : 'Export failed.')
    } finally {
      if (alive.current) setPending(false)
    }
  }

  return <aside aria-label="MOL and SDF" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3 text-xs">
    <h3 className="text-sm font-medium">MOL and SDF</h3>
    <p className="leading-relaxed text-muted-foreground">V2000 basic structures only. Stereo, queries and advanced objects may be refused or reported as losses.</p>
    <Field>
      <FieldLabel htmlFor="ctfile-input">Import file</FieldLabel>
      <Input id="ctfile-input" type="file" accept=".mol,.sdf" disabled={pending || editor.busy}
        onChange={(event) => void readFile(event.currentTarget.files?.[0])} />
      <FieldDescription>{file ? file.name : 'Choose a UTF-8 MOL or SDF file.'}</FieldDescription>
    </Field>
    <Field>
      <FieldLabel htmlFor="ctfile-record">Record number</FieldLabel>
      <Input id="ctfile-record" type="number" min={1} step={1} value={record} disabled={pending || editor.busy}
        onChange={(event) => { setRecord(event.currentTarget.value); setPrepared(null) }} />
    </Field>
    <Button size="sm" variant="outline" disabled={!file || pending || editor.busy} onClick={() => void previewImport()}>
      Preview import
    </Button>
    {error && <p role="alert">{error}</p>}
    {prepared && !currentImport && <p>The drawing changed. Preview the import again.</p>}
    {currentImport && <div aria-live="polite" className="flex max-h-64 flex-col gap-2 overflow-auto">
      <p>{currentImport.preview.records.length} record(s); importing record {currentImport.input.record}.</p>
      {currentImport.preview.records.map((item, index) => <div key={`${index}-${item.title}`}>
        <p>{index + 1}. {item.title || '(untitled)'}: {item.atoms} atoms, {item.bonds} bonds, {item.fields.length} fields</p>
        {item.fields.map((field, fieldIndex) => <p className="break-all whitespace-pre-wrap" key={`${field.name}-${fieldIndex}`}>
          {field.name}: {field.value || '(empty)'}
        </p>)}
      </div>)}
      <Losses losses={currentImport.preview.losses} />
      <Button size="sm" disabled={pending || editor.busy} onClick={() => void commitImport()}>
        Accept losses and import
      </Button>
    </div>}
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="outline" disabled={pending || editor.busy} onClick={() => void previewExport('mol')}>Preview MOL export</Button>
      <Button size="sm" variant="outline" disabled={pending || editor.busy} onClick={() => void previewExport('sdf')}>Preview SDF export</Button>
    </div>
    {exported && !currentExport && <p>The drawing changed. Preview the export again.</p>}
    {currentExport && <div className="flex max-h-64 flex-col gap-2 overflow-auto" aria-live="polite">
      <Losses losses={currentExport.losses} />
      <a className={buttonVariants({ size: 'sm' })} href={currentExport.url} download={`chemifree.${currentExport.format}`}>
        Download {currentExport.format.toUpperCase()}
      </a>
    </div>}
  </aside>
}
