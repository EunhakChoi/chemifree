import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { ApiError, outcomeOpen, type InterchangeChoice, type InterchangeFormat, type InterchangeInput, type InterchangePreview } from '@/lib/document-contract'

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

/** Canonical padded base64, which is what the operation API requires for CDX. */
function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000))
  }
  return btoa(binary)
}

/** A rich file is projected only after its particular losses have been reviewed. */
export function CdxmlImport({ editor }: { editor: DocumentEditor }) {
  const [source, setSource] = useState<Source | null>(null)
  const [title, setTitle] = useState('Imported drawing')
  const [choice, setChoice] = useState(0)
  const [prepared, setPrepared] = useState<Prepared | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [text, setText] = useState('')
  const [reading, setReading] = useState(false)
  const [creation, setCreation] = useState<{ prepared: Prepared; reconcile: boolean } | null>(null)
  const sequence = useRef(0)
  const creating = useRef(false)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false; sequence.current += 1 }
  }, [])

  const current = prepared?.generation === editor.serviceGeneration ? prepared : null
  const liveSource = source?.generation === editor.serviceGeneration ? source : null
  const fits = new TextEncoder().encode(text).byteLength <= MAX_FILE_BYTES
  const locked = creation !== null

  function editText(value: string) {
    sequence.current += 1
    setText(value)
    setSource(null)
    setPrepared(null)
    setChoice(0)
    setError(null)
    setPending(false)
    setReading(false)
  }

  async function readClipboard() {
    if (pending || reading || locked || editor.busy) return
    const request = ++sequence.current
    const generation = editor.currentServiceGeneration()
    setReading(true)
    setError(null)
    try {
      if (typeof navigator.clipboard?.readText !== 'function') throw new Error('Clipboard reading is unavailable. Paste CDXML text manually.')
      const value = await navigator.clipboard.readText()
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) editText(value)
    } catch (cause) {
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) {
        setError(cause instanceof Error ? `${cause.message} Paste CDXML text manually.` : 'Clipboard reading failed. Paste CDXML text manually.')
      }
    } finally {
      if (alive.current && sequence.current === request) setReading(false)
    }
  }

  async function readText() {
    if (!text.trim() || !fits || pending || locked || editor.busy) return
    const request = ++sequence.current
    const generation = editor.currentServiceGeneration()
    const asked = text
    setPending(true)
    setSource(null)
    setPrepared(null)
    setChoice(0)
    setError(null)
    try {
      const choices = await editor.readInterchange(asked, 'cdxml')
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) {
        setSource({ format: 'cdxml', text: asked, choices, generation })
      }
    } catch (cause) {
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) setError(cause instanceof Error ? cause.message : 'Could not read CDXML text.')
    } finally {
      if (alive.current && sequence.current === request) setPending(false)
    }
  }

  async function chooseFile(file?: File) {
    const request = ++sequence.current
    setPrepared(null)
    setSource(null)
    setChoice(0)
    setError(null)
    setText('')
    if (!file) return
    const name = file.name.toLowerCase()
    const format: InterchangeFormat | null = name.endsWith('.cdxml') ? 'cdxml' : name.endsWith('.cdx') ? 'cdx' : null
    if (!format) {
      setError('Choose a .cdxml or .cdx file.')
      return
    }
    if (file.size > MAX_FILE_BYTES) {
      setError('This web form accepts CDXML and CDX files up to 120 KiB. The operation API and CLI have their own limits.')
      return
    }
    setPending(true)
    const generation = editor.currentServiceGeneration()
    try {
      const buffer = await file.arrayBuffer()
      // CDXML preserves a UTF-8 BOM while rejecting invalid input rather than replacing bytes.
      const text = format === 'cdx'
        ? toBase64(new Uint8Array(buffer))
        : new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(buffer)
      const choices = await editor.readInterchange(text, format)
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) {
        setSource({ format, text, choices, generation })
        setTitle(file.name.replace(/\.cdx(ml)?$/i, '') || 'Imported drawing')
      }
    } catch (cause) {
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) setError(cause instanceof Error ? cause.message : 'Could not read this file.')
    } finally {
      if (alive.current && sequence.current === request) setPending(false)
    }
  }

  async function preview() {
    const selected = liveSource?.choices[choice]
    if (!liveSource || !selected || !title.trim() || locked) return
    const input: InterchangeInput = { format: liveSource.format, source: liveSource.text, title: title.trim(), page_id: selected.page_id, fragment_id: selected.fragment_id }
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

  async function create(checking = false) {
    const ready = checking ? creation?.prepared : current
    if (!ready || creating.current || pending || editor.busy || creation?.reconcile) return
    creating.current = true
    setPending(true)
    setError(null)
    setCreation({ prepared: ready, reconcile: false })
    try {
      await editor.createInterchange(ready.input, ready.preview.digest, { generation: ready.generation, checking })
      if (alive.current) {
        setCreation(null)
        editText('')
      }
    } catch (cause) {
      if (!alive.current) return
      if (cause instanceof ApiError && cause.code === 'create_unconfirmed') {
        setCreation({ prepared: ready, reconcile: true })
        editor.refreshList()
      } else if (!(cause instanceof ApiError) || outcomeOpen(cause) || cause.code === 'incompatible_response') {
        setCreation({ prepared: ready, reconcile: false })
      } else {
        setCreation(null)
      }
      setError(cause instanceof Error ? cause.message : 'The creation outcome is unknown. Check the original attempt.')
    } finally {
      creating.current = false
      if (alive.current) setPending(false)
    }
  }

  return <section aria-labelledby="cdxml-import" className="space-y-3 rounded-lg border p-4">
    <h3 id="cdxml-import" className="text-sm font-medium">Create a drawing from CDXML or CDX</h3>
    <p className="text-sm text-muted-foreground">Choose a page and molecule. The editor supports one molecule; review the other content that will be left out before creating a new drawing.</p>
    <Field>
      <FieldLabel htmlFor="cdxml-file">CDXML or CDX file</FieldLabel>
      <Input id="cdxml-file" type="file" accept=".cdxml,.cdx,application/xml,text/xml" disabled={pending || reading || locked || editor.busy}
        onChange={(event) => void chooseFile(event.currentTarget.files?.[0])} />
      <FieldDescription>External resources are never fetched. Files up to 120 KiB; CDX is sent as base64.</FieldDescription>
    </Field>
    <Field data-invalid={!fits || undefined}>
      <FieldLabel htmlFor="cdxml-text">CDXML source text</FieldLabel>
      <Textarea id="cdxml-text" rows={5} value={text} disabled={locked} spellCheck={false} autoComplete="off" autoCapitalize="off" aria-invalid={!fits}
        className="field-sizing-fixed max-h-48 resize-y font-mono text-xs" onChange={(event) => editText(event.currentTarget.value)} />
      <FieldDescription>CDXML only, up to 120 KiB of UTF-8. Reading the clipboard only fills this field. Read pages and molecules sends the exact text to the local service; no format is guessed.</FieldDescription>
    </Field>
    {!fits && <p role="alert" className="text-sm text-destructive">CDXML text exceeds 120 KiB of UTF-8. Shorten it before reading pages and molecules.</p>}
    <div className="flex flex-wrap gap-2">
      <Button variant="outline" disabled={pending || reading || locked || editor.busy} onClick={() => void readClipboard()}>Read CDXML clipboard</Button>
      <Button variant="outline" disabled={pending || reading || locked || editor.busy || !text.trim() || !fits} onClick={() => void readText()}>Read CDXML pages and molecules</Button>
    </div>
    {creation && <div role="status" className="flex flex-col gap-2 text-sm">
      <p>{creation.reconcile ? 'The service changed before creation was confirmed. Check Saved documents before creating another drawing; this form will not resend into a new process.' : 'Creation may have completed. Check the original attempt before preparing another drawing.'}</p>
      {creation.reconcile
        ? <>
          <Button variant="outline" disabled={pending} onClick={() => editor.refreshList()}>Refresh Saved documents</Button>
          <Button variant="outline" disabled={pending} onClick={() => { setCreation(null); editText('') }}>I checked Saved documents; clear this attempt</Button>
        </>
        : <Button variant="outline" disabled={pending || editor.busy} onClick={() => void create(true)}>Check CDXML creation outcome</Button>}
    </div>}
    {liveSource && liveSource.choices.length === 0 && <p role="status">No page and top-level molecule with IDs can be projected into the editor.</p>}
    {liveSource && liveSource.choices.length > 0 && <>
      <Field>
        <FieldLabel htmlFor="cdxml-choice">Page and molecule</FieldLabel>
        <select id="cdxml-choice" className="w-full min-w-0 max-w-full rounded-md border bg-background p-2 text-sm" value={choice} disabled={pending || locked || editor.busy}
          onChange={(event) => { setChoice(Number(event.currentTarget.value)); setPrepared(null) }}>
          {liveSource.choices.map((item, index) => <option key={`${item.page_id}:${item.fragment_id}`} value={index}>
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
  </section>
}
