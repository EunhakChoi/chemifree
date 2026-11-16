import { useEffect, useImperativeHandle, useRef, useState, type Ref } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { InterchangeChoice, InterchangeFormat, InterchangeInput, InterchangePreview } from '@/lib/document-contract'
import type { ImportFlowHandle } from '@/lib/import-detect'

/** This form's cap on the file itself; CDX grows by a third as base64 and must fit the 256 KiB request body. */
const MAX_FILE_BYTES = 120 * 1024

interface Source {
  format: InterchangeFormat
  text: string
  choices: InterchangeChoice[]
  generation: number
}

interface Prepared {
  input: InterchangeInput
  preview: InterchangePreview
  generation: number
}

/** CDXML text or a CDXML/CDX file from the Import card, read into pages and molecules only on an explicit click. */
export function InterchangeImportFlow({ editor, source, origin, format, suggestedTitle, byteLength, onDone, ref }: {
  editor: DocumentEditor
  source: string
  origin: 'text' | 'file'
  format: InterchangeFormat
  suggestedTitle: string
  byteLength?: number
  onDone: () => void
  ref: Ref<ImportFlowHandle>
}) {
  const [inspected, setInspected] = useState<Source | null>(null)
  const [title, setTitle] = useState(suggestedTitle)
  const [choice, setChoice] = useState(0)
  const [prepared, setPrepared] = useState<Prepared | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const creation = editor.interchangeCreation
  const sequence = useRef(0)
  const creating = useRef(false)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false; sequence.current += 1 }
  }, [])
  useImperativeHandle(ref, () => ({
    reset() {
      sequence.current += 1
      setInspected(null)
      setPrepared(null)
      setChoice(0)
      setError(null)
      setPending(false)
    },
  }))

  const current = prepared?.generation === editor.serviceGeneration ? prepared : null
  const liveInspected = inspected?.generation === editor.serviceGeneration ? inspected : null
  // The card always passes a file's size; a missing size fails closed rather than reading as 0.
  const fits = origin === 'file'
    ? byteLength !== undefined && byteLength <= MAX_FILE_BYTES
    : new TextEncoder().encode(source).byteLength <= MAX_FILE_BYTES
  const locked = creation !== null
  const label = format === 'cdx' ? 'CDX' : 'CDXML'

  async function readChoices() {
    if (!source.trim() || !fits || pending || locked || editor.busy) return
    const request = ++sequence.current
    const generation = editor.currentServiceGeneration()
    const asked = source
    const failure = origin === 'file' ? 'Could not read this file.' : 'Could not read CDXML text.'
    setPending(true)
    setInspected(null)
    setPrepared(null)
    setChoice(0)
    setError(null)
    try {
      const choices = await editor.readInterchange(asked, format)
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) {
        setInspected({ format, text: asked, choices, generation })
      }
    } catch (cause) {
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) setError(cause instanceof Error ? cause.message : failure)
    } finally {
      if (alive.current && sequence.current === request) setPending(false)
    }
  }

  async function preview() {
    const selected = liveInspected?.choices[choice]
    if (!liveInspected || !selected || !title.trim() || locked) return
    const input: InterchangeInput = { format: liveInspected.format, source: liveInspected.text, title: title.trim(), page_id: selected.page_id, fragment_id: selected.fragment_id }
    const request = ++sequence.current
    const generation = editor.currentServiceGeneration()
    setPending(true)
    setError(null)
    setPrepared(null)
    try {
      const result = await editor.previewInterchange(input)
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) {
        setPrepared({ input, preview: result, generation })
      }
    } catch (cause) {
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) setError(cause instanceof Error ? cause.message : 'Preview failed.')
    } finally {
      if (alive.current && sequence.current === request) setPending(false)
    }
  }

  async function create() {
    const ready = current
    if (!ready || creating.current || pending || editor.busy || locked) return
    creating.current = true
    setPending(true)
    setError(null)
    try {
      await editor.createInterchange(ready.input, ready.preview.digest, { generation: ready.generation })
      if (alive.current) onDone()
    } catch (cause) {
      if (alive.current) setError(cause instanceof Error ? cause.message : 'The creation outcome is unknown. Check the original attempt.')
    } finally {
      creating.current = false
      if (alive.current) setPending(false)
    }
  }

  return <div className="flex min-w-0 flex-col gap-3">
    <p className="text-muted-foreground">Choose a page and molecule. The editor supports one molecule; review the other content that will be left out before creating a new drawing. External resources are never fetched.</p>
    {!fits && <p role="alert" className="text-sm text-destructive">{origin === 'file' ? 'This web form accepts CDXML and CDX files up to 120 KiB. The operation API and CLI have their own limits.' : 'CDXML text exceeds 120 KiB of UTF-8. Shorten it before reading pages and molecules.'}</p>}
    <Button variant="outline" disabled={pending || locked || editor.busy || !source.trim() || !fits} onClick={() => void readChoices()}>{`Read ${label} pages and molecules`}</Button>
    {liveInspected && liveInspected.choices.length === 0 && <p role="status">No page and top-level molecule with IDs can be projected into the editor.</p>}
    {liveInspected && liveInspected.choices.length > 0 && <>
      <Field>
        <FieldLabel htmlFor="cdxml-choice">Page and molecule</FieldLabel>
        <select id="cdxml-choice" className="w-full min-w-0 max-w-full rounded-md border bg-background p-2 text-sm" value={choice} disabled={pending || locked || editor.busy}
          onChange={(event) => { setChoice(Number(event.currentTarget.value)); setPrepared(null) }}>
          {liveInspected.choices.map((item, index) => <option key={`${item.page_id}:${item.fragment_id}`} value={index}>
            Page {item.page_id}, molecule {item.fragment_id} ({item.fragment_path})
          </option>)}
        </select>
      </Field>
      <Field>
        <FieldLabel htmlFor="cdxml-title">New drawing title</FieldLabel>
        <Input id="cdxml-title" value={title} maxLength={200} disabled={pending || locked || editor.busy}
          onChange={(event) => { setTitle(event.currentTarget.value); setPrepared(null) }} />
      </Field>
      <Button variant="outline" disabled={pending || locked || editor.busy || !title.trim()} onClick={() => void preview()}>Preview import</Button>
    </>}
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    {current && <div className="space-y-3 text-sm" data-testid="cdxml-preview">
      <p>Preview: {current.preview.atomLabels.length} {current.preview.atomLabels.length === 1 ? 'atom' : 'atoms'} ({current.preview.atomLabels.join(', ') || 'none'}), {current.preview.bonds} {current.preview.bonds === 1 ? 'bond' : 'bonds'}.</p>
      <p className="font-medium">Source-to-editor IDs</p>
      <ul className="list-disc pl-5">
        {[current.preview.mapping.page, current.preview.mapping.fragment, ...current.preview.mapping.atoms, ...current.preview.mapping.bonds].map((item, index) =>
          <li className="break-all" key={`${item.source_path}:${index}`}>{item.source_path} {item.source_id === null ? '(no ID)' : `#${item.source_id}`} → {item.target_id}</li>)}
      </ul>
      <p className="font-medium">Content changed or left out</p>
      {current.preview.losses.length === 0 ? <p>No loss reported.</p> : <ul className="list-disc space-y-1 pl-5">
        {current.preview.losses.map((loss, index) => <li className="break-words" key={`${loss.source_path}:${index}`}>
          {loss.source_path}: {loss.reason} ({loss.disposition}, {loss.semantic_impact})
        </li>)}
      </ul>}
      <Button disabled={pending || locked || editor.busy} onClick={() => void create()}>
        Accept losses and create drawing
      </Button>
    </div>}
  </div>
}

/** Recovery stays visible while inspecting Saved documents, outside the conditional picker. */
export function InterchangeCreationNotice({ editor, onClear }: { editor: DocumentEditor; onClear?: () => void }) {
  const creation = editor.interchangeCreation
  if (!creation) return null
  return <div role="status" className="flex flex-col gap-2 text-sm">
    <p>{creation.message ?? (creation.status === 'sending' ? 'Checking the reviewed drawing creation.' : 'Creation may have completed. Check the original attempt before preparing another drawing.')}</p>
    {creation.status === 'reconcile'
      ? <>
        <p>Inspect Saved documents before explicitly clearing this attempt. This does not mean the earlier creation failed.</p>
        <Button variant="outline" disabled={editor.busy} onClick={() => editor.refreshList()}>Refresh Saved documents</Button>
        <Button variant="outline" disabled={editor.busy} onClick={() => { editor.clearInterchangeCreation(); onClear?.() }}>I checked Saved documents; clear this attempt</Button>
      </>
      : <Button variant="outline" disabled={editor.busy || creation.status === 'sending'} onClick={() => void editor.checkInterchangeCreation().catch(() => {})}>Check CDXML creation outcome</Button>}
  </div>
}
