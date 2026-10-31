import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { InterchangeChoice, InterchangeInput, InterchangePreview } from '@/lib/document-contract'

interface Source {
  text: string
  choices: InterchangeChoice[]
}

interface Prepared {
  input: InterchangeInput
  preview: InterchangePreview
  generation: number
}

/** A rich file is projected only after its particular losses have been reviewed. */
export function CdxmlImport({ editor }: { editor: DocumentEditor }) {
  const [source, setSource] = useState<Source | null>(null)
  const [title, setTitle] = useState('Imported drawing')
  const [choice, setChoice] = useState(0)
  const [prepared, setPrepared] = useState<Prepared | null>(null)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const sequence = useRef(0)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false; sequence.current += 1 }
  }, [])

  const current = prepared?.generation === editor.serviceGeneration ? prepared : null

  async function chooseFile(file?: File) {
    const request = ++sequence.current
    setPrepared(null)
    setSource(null)
    setChoice(0)
    setError(null)
    if (!file) return
    if (!file.name.toLowerCase().endsWith('.cdxml')) {
      setError('Choose a .cdxml file.')
      return
    }
    if (file.size > 200 * 1024) {
      setError('This web form accepts files up to 200 KiB. The operation API and CLI have their own limits.')
      return
    }
    setPending(true)
    const generation = editor.currentServiceGeneration()
    try {
      // Preserve a UTF-8 BOM while rejecting invalid input rather than replacing bytes.
      const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(await file.arrayBuffer())
      const choices = await editor.readInterchange(text)
      if (alive.current && sequence.current === request && generation === editor.currentServiceGeneration()) {
        setSource({ text, choices })
        setTitle(file.name.replace(/\.cdxml$/i, '') || 'Imported drawing')
      }
    } catch (cause) {
      if (alive.current && sequence.current === request) setError(cause instanceof Error ? cause.message : 'Could not read this file.')
    } finally {
      if (alive.current && sequence.current === request) setPending(false)
    }
  }

  async function preview() {
    const selected = source?.choices[choice]
    if (!source || !selected || !title.trim()) return
    const input: InterchangeInput = { source: source.text, title: title.trim(), page_id: selected.page_id, fragment_id: selected.fragment_id }
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
      if (alive.current && sequence.current === request) setError(cause instanceof Error ? cause.message : 'Preview failed.')
    } finally {
      if (alive.current && sequence.current === request) setPending(false)
    }
  }

  return <section aria-labelledby="cdxml-import" className="space-y-3 rounded-lg border p-4">
    <h3 id="cdxml-import" className="text-sm font-medium">Create a drawing from CDXML</h3>
    <p className="text-sm text-muted-foreground">Choose a page and molecule. The editor supports one molecule; review the other content that will be left out before creating a new drawing.</p>
    <Field>
      <FieldLabel htmlFor="cdxml-file">CDXML file</FieldLabel>
      <Input id="cdxml-file" type="file" accept=".cdxml,application/xml,text/xml" disabled={pending || editor.busy}
        onChange={(event) => void chooseFile(event.currentTarget.files?.[0])} />
      <FieldDescription>External resources are never fetched.</FieldDescription>
    </Field>
    {source && source.choices.length === 0 && <p role="status">No page and top-level molecule with IDs can be projected into the editor.</p>}
    {source && source.choices.length > 0 && <>
      <Field>
        <FieldLabel htmlFor="cdxml-choice">Page and molecule</FieldLabel>
        <select id="cdxml-choice" className="rounded-md border bg-background p-2 text-sm" value={choice} disabled={pending || editor.busy}
          onChange={(event) => { setChoice(Number(event.currentTarget.value)); setPrepared(null) }}>
          {source.choices.map((item, index) => <option key={`${item.page_id}:${item.fragment_id}`} value={index}>
            Page {item.page_id}, molecule {item.fragment_id} ({item.fragment_path})
          </option>)}
        </select>
      </Field>
      <Field>
        <FieldLabel htmlFor="cdxml-title">New drawing title</FieldLabel>
        <Input id="cdxml-title" value={title} maxLength={200} disabled={pending || editor.busy}
          onChange={(event) => { setTitle(event.currentTarget.value); setPrepared(null) }} />
      </Field>
      <Button variant="outline" disabled={pending || editor.busy || !title.trim()} onClick={() => void preview()}>Preview import</Button>
    </>}
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    {current && <div className="space-y-3 text-sm" data-testid="cdxml-preview">
      <p>Preview: {current.preview.atomLabels.length} atoms ({current.preview.atomLabels.join(', ') || 'none'}), {current.preview.bonds} bonds.</p>
      <p className="font-medium">Source-to-editor IDs</p>
      <ul className="list-disc pl-5">
        {[current.preview.mapping.page, current.preview.mapping.fragment, ...current.preview.mapping.atoms, ...current.preview.mapping.bonds].map((item, index) =>
          <li key={`${item.source_path}:${index}`}>{item.source_path} {item.source_id === null ? '(no ID)' : `#${item.source_id}`} → {item.target_id}</li>)}
      </ul>
      <p className="font-medium">Content changed or left out</p>
      {current.preview.losses.length === 0 ? <p>No loss reported.</p> : <ul className="list-disc space-y-1 pl-5">
        {current.preview.losses.map((loss, index) => <li key={`${loss.source_path}:${index}`}>
          {loss.source_path}: {loss.reason} ({loss.disposition}, {loss.semantic_impact})
        </li>)}
      </ul>}
      <Button disabled={pending || editor.busy} onClick={() => void editor.createInterchange(current.input, current.preview.digest)}>
        Accept losses and create drawing
      </Button>
    </div>}
  </section>
}
