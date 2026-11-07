import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { parseRotationDegrees, selectionRotation } from '@/editor/selection-rotation'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView } from '@/lib/document-contract'

/** Arbitrary angles use the same captured selection and queued pivot as the fixed rotations. */
export function SelectionRotationForm({ document, editor, activePlacement }: { document: DocumentView; editor: DocumentEditor; activePlacement: string | null }) {
  const id = useId()
  const [text, setText] = useState('')
  const [composing, setComposing] = useState(false)
  const degrees = parseRotationDegrees(text)
  const ready = degrees !== null && selectionRotation(document, { documentId: document.id, selection: editor.selection, instance: activePlacement, degrees }) !== null
  const invalid = text.trim() !== '' && degrees === null
  return <form aria-label="Rotate selected geometry" className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (composing || !ready || degrees === null) return
    const pressed = editor.selectionAtPress()
    const edit = { documentId: document.id, selection: pressed, instance: activePlacement, degrees }
    void editor.apply((latest) => selectionRotation(latest, edit), { kind: 'gesture', before: pressed })
  }}>
    <FieldGroup><Field data-invalid={invalid}>
      <FieldLabel htmlFor={id}>Rotation angle (degrees)</FieldLabel>
      <Input id={id} value={text} inputMode="decimal" placeholder="e.g. 30" aria-invalid={invalid} aria-describedby={`${id}-help`}
        onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)} onChange={(event) => setText(event.target.value)} />
      <FieldDescription id={`${id}-help`}>Use a finite angle. Positive rotates clockwise, negative counterclockwise, around the selected geometry center. Full turns make no change. Captions are unsupported; rectangles require exact quarter turns.</FieldDescription>
    </Field></FieldGroup>
    <Button type="submit" variant="outline" size="sm" disabled={composing || !ready}>Rotate</Button>
  </form>
}
