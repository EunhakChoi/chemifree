import { useId, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { parseCaptionSize } from '@/lib/caption-input'
import { isCaptionColor, validCaptionRuns, type CaptionAlignment, type CaptionRun, type DocumentView, type SceneObject } from '@/lib/document-contract'

type Caption = Extract<SceneObject, { kind: 'caption' }>

/** A whole-run editor keeps native text selection and IME behavior inside ordinary inputs. */
export function CaptionPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const [draft, setDraft] = useState<{ original: Caption | null } | null>(null)
  const selected = document.scene?.objects?.filter((object) => editor.selection.has(object.id)) ?? []
  const caption = selected.length === 1 && selected[0].kind === 'caption' ? selected[0] : null
  return (
    <section aria-label="Captions" className="flex flex-col gap-2 rounded-lg border p-3">
      <p className="text-xs text-muted-foreground">Independent text with explicit script, weight and style runs. Captions do not change atoms or chemical formulas. Single line with left, center or right alignment at a fixed anchor; each run follows the drawing size and font or overrides them with a size from 4–72 pt, a generic family, or an opaque #RRGGBB color. Blank color follows the current theme foreground.</p>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={editor.busy} onClick={() => setDraft({ original: null })}>Add caption…</Button>
        <Button variant="outline" size="sm" disabled={editor.busy || caption === null} onClick={() => caption && setDraft({ original: caption })}>Edit selected caption…</Button>
      </div>
      {draft && <CaptionDialog key={`${document.id}:${draft.original?.id ?? 'new'}`} documentId={document.id} original={draft.original} editor={editor} close={() => setDraft(null)} />}
    </section>
  )
}

function CaptionDialog({ documentId, original, editor, close }: {
  documentId: string
  original: Caption | null
  editor: DocumentEditor
  close: () => void
}) {
  const id = useId()
  const [runs, setRuns] = useState<CaptionRun[]>(original?.runs.map((run) => ({ ...run })) ?? [{ text: '', script: 'baseline', weight: 'normal', style: 'normal', font: null, color: null }])
  const [sizes, setSizes] = useState<string[]>(original?.runs.map((run) => run.size_pt == null ? '' : String(run.size_pt)) ?? [''])
  const parsedSizes = sizes.map((size) => parseCaptionSize(size))
  const validSizes = parsedSizes.map((size) => size !== undefined)
  const [alignment, setAlignment] = useState<CaptionAlignment>(original?.alignment ?? 'left')
  const [x, setX] = useState(String(original?.anchor.x ?? 0))
  const [y, setY] = useState(String(original?.anchor.y ?? 0))
  const [composing, setComposing] = useState(false)
  const valid = validCaptionRuns(runs) && validSizes.every((value) => value) && x.trim() !== '' && y.trim() !== '' && Number.isFinite(Number(x)) && Number.isFinite(Number(y))
  function submit() {
    if (!valid || composing || editor.busy) return
    const anchor = { x: Number(x), y: Number(y) }
    const frozen: CaptionRun[] = []
    for (const [index, run] of runs.entries()) {
      const size = parsedSizes[index]
      if (size === undefined) return
      frozen.push({ ...run, size_pt: size })
    }
    void editor.apply((latest) => {
      if (latest.id !== documentId) return null
      if (original) {
        const target = latest.scene?.objects?.find((object) => object.id === original.id)
        // An external edit/move/group while this form is open invalidates the captured target.
        if (JSON.stringify(target) !== JSON.stringify(original)) return null
        return [{ op: 'update_caption', object: original.id, anchor, alignment, runs: frozen }]
      }
      return [{ op: 'add_caption', anchor, alignment, runs: frozen }]
    })
    close()
  }
  return (
    <Dialog open onOpenChange={(open) => { if (!open) close() }}>
      <DialogContent className="max-h-[85dvh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{original ? `Edit caption ${original.id}` : 'Add caption'}</DialogTitle>
          <DialogDescription>Each input stores one complete Unicode run. Text is kept verbatim. Script, weight, style, size, color and font family change appearance only. Alignment includes leading and trailing spaces. Paragraph direction is fixed left-to-right; RTL shaping is browser-native within each run, with runs kept in stored order. Run boundaries may split shaping; avoid splitting combining or emoji sequences.</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field><FieldLabel id={`${id}-alignment`}>Caption alignment</FieldLabel>
            <ToggleGroup variant="outline" size="sm" aria-labelledby={`${id}-alignment`} value={[alignment]} onValueChange={(values) => {
              const value = values[0]
              if (value === 'left' || value === 'center' || value === 'right') setAlignment(value)
            }}>
              <ToggleGroupItem value="left">Left</ToggleGroupItem><ToggleGroupItem value="center">Center</ToggleGroupItem><ToggleGroupItem value="right">Right</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <Field><FieldLabel htmlFor={`${id}-x`}>Caption horizontal position (pt)</FieldLabel><Input id={`${id}-x`} value={x} onChange={(event) => setX(event.target.value)} /></Field>
          <Field><FieldLabel htmlFor={`${id}-y`}>Caption vertical position (pt)</FieldLabel><Input id={`${id}-y`} value={y} onChange={(event) => setY(event.target.value)} /></Field>
          {runs.map((run, index) => (
            <FieldGroup key={index}>
              <Field><FieldLabel htmlFor={`${id}-text-${index}`}>Run {index + 1} text</FieldLabel>
                <Input id={`${id}-text-${index}`} value={run.text}
                  onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
                  onChange={(event) => setRuns((current) => current.map((item, at) => at === index ? { ...item, text: event.target.value } : item))} />
              </Field>
              <Field><FieldLabel id={`${id}-script-${index}`}>Run {index + 1} script</FieldLabel>
                <ToggleGroup variant="outline" size="sm" aria-labelledby={`${id}-script-${index}`} value={[run.script]} onValueChange={(values) => {
                  const value = values[0]
                  if (value === 'baseline' || value === 'subscript' || value === 'superscript') setRuns((current) => current.map((item, at) => at === index ? { ...item, script: value } : item))
                }}>
                  <ToggleGroupItem value="baseline">Baseline</ToggleGroupItem><ToggleGroupItem value="subscript">Subscript</ToggleGroupItem><ToggleGroupItem value="superscript">Superscript</ToggleGroupItem>
                </ToggleGroup>
              </Field>
              <Field data-invalid={!validSizes[index]}><FieldLabel htmlFor={`${id}-size-${index}`}>Run {index + 1} size (pt)</FieldLabel>
                <Input id={`${id}-size-${index}`} inputMode="decimal" placeholder="Drawing size" value={sizes[index]} aria-invalid={!validSizes[index]}
                  onChange={(event) => setSizes((current) => current.map((value, at) => at === index ? event.target.value : value))} />
                <FieldDescription>Blank follows the drawing size; an override must be 4–72 pt. Subscript and superscript render at two thirds of this size.</FieldDescription>
              </Field>
              <Field data-invalid={run.color != null && !isCaptionColor(run.color)}><FieldLabel htmlFor={`${id}-color-${index}`}>Run {index + 1} color</FieldLabel>
                <Input id={`${id}-color-${index}`} placeholder="Theme foreground" value={run.color ?? ''} aria-invalid={run.color != null && !isCaptionColor(run.color)}
                  onChange={(event) => setRuns((current) => current.map((item, at) => at === index ? { ...item, color: event.target.value === '' ? null : event.target.value } : item))} />
                <FieldDescription>Blank follows the theme foreground. Use opaque sRGB #RRGGBB with uppercase hex digits; an override stays fixed when the theme changes.</FieldDescription>
              </Field>
              <Field><FieldLabel id={`${id}-font-${index}`}>Run {index + 1} font family</FieldLabel>
                <ToggleGroup variant="outline" size="sm" aria-labelledby={`${id}-font-${index}`} value={[run.font ?? 'inherit']} onValueChange={(values) => {
                  const value = values[0]
                  if (value === 'inherit' || value === 'sans-serif' || value === 'serif' || value === 'monospace') setRuns((current) => current.map((item, at) => at === index ? { ...item, font: value === 'inherit' ? null : value } : item))
                }}>
                  <ToggleGroupItem value="inherit">Drawing font</ToggleGroupItem><ToggleGroupItem value="sans-serif">Sans-serif</ToggleGroupItem><ToggleGroupItem value="serif">Serif</ToggleGroupItem><ToggleGroupItem value="monospace">Monospace</ToggleGroupItem>
                </ToggleGroup>
              </Field>
              <Field><FieldLabel id={`${id}-weight-${index}`}>Run {index + 1} weight</FieldLabel>
                <ToggleGroup variant="outline" size="sm" aria-labelledby={`${id}-weight-${index}`} value={[run.weight]} onValueChange={(values) => {
                  const value = values[0]
                  if (value === 'normal' || value === 'bold') setRuns((current) => current.map((item, at) => at === index ? { ...item, weight: value } : item))
                }}>
                  <ToggleGroupItem value="normal">Normal</ToggleGroupItem><ToggleGroupItem value="bold">Bold</ToggleGroupItem>
                </ToggleGroup>
              </Field>
              <Field><FieldLabel id={`${id}-style-${index}`}>Run {index + 1} style</FieldLabel>
                <ToggleGroup variant="outline" size="sm" aria-labelledby={`${id}-style-${index}`} value={[run.style]} onValueChange={(values) => {
                  const value = values[0]
                  if (value === 'normal' || value === 'italic') setRuns((current) => current.map((item, at) => at === index ? { ...item, style: value } : item))
                }}>
                  <ToggleGroupItem value="normal">Normal</ToggleGroupItem><ToggleGroupItem value="italic">Italic</ToggleGroupItem>
                </ToggleGroup>
              </Field>
              <Button variant="outline" size="sm" disabled={runs.length === 1 || composing} onClick={() => { setRuns((current) => current.filter((_, at) => at !== index)); setSizes((current) => current.filter((_, at) => at !== index)) }}>Remove run {index + 1}</Button>
            </FieldGroup>
          ))}
        </FieldGroup>
        <Button variant="outline" disabled={runs.length >= 32 || composing} onClick={() => { setRuns((current) => [...current, { text: '', script: 'baseline', weight: 'normal', style: 'normal', font: null, color: null }]); setSizes((current) => [...current, '']) }}>Add run</Button>
        <p className="text-xs text-muted-foreground">1–32 nonempty runs, at most 16384 UTF-8 bytes in total. No line breaks or control characters. Move/group/delete from Scene. Arrangement, rotation, reflection and graphic clipboard do not yet support captions.</p>
        <DialogFooter><Button disabled={!valid || composing || editor.busy} onClick={submit}>{original ? 'Apply caption' : 'Create caption'}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
