import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { graphicPointsCommands, parseGraphicCoordinate, type GraphicObject } from '@/editor/graphic-points'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView, Position } from '@/lib/document-contract'

/** Edit the stored geometry of one selected graphic through the shared transaction API. */
export function GraphicPointsPanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const chosen = document.scene?.objects?.filter((object) => editor.selection.has(object.id)) ?? []
  const graphic = chosen.length === 1 && chosen[0].kind === 'graphic' ? chosen[0] : null
  return <section aria-label="Graphic endpoints" className="flex flex-col gap-2">
    {graphic
      ? <GraphicPointsForm key={`${document.id}:${JSON.stringify(graphic)}`} document={document} graphic={graphic} editor={editor} />
      : <p className="text-xs text-muted-foreground">Select one line, arrow or rectangle to edit its two stored endpoints.</p>}
  </section>
}

function GraphicPointsForm({ document, graphic, editor }: { document: DocumentView; graphic: GraphicObject; editor: DocumentEditor }) {
  const id = useId()
  const [values, setValues] = useState(() => graphic.points.flatMap((point) => [String(point.x), String(point.y)]))
  const [composing, setComposing] = useState(false)
  const coordinates = values.map((value) => parseGraphicCoordinate(value))
  const [x1, y1, x2, y2] = coordinates
  const valid = coordinates.every((value) => value !== null)
  const unchanged = x1 === graphic.points[0].x && y1 === graphic.points[0].y && x2 === graphic.points[1].x && y2 === graphic.points[1].y
  return <form aria-label={`Edit ${graphic.shape} ${graphic.id} endpoints`} className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (editor.busy || composing || !valid || unchanged || x1 === null || y1 === null || x2 === null || y2 === null) return
    const points: [Position, Position] = [{ x: x1, y: y1 }, { x: x2, y: y2 }]
    const edit = { documentId: document.id, revision: document.revision, graphic, points }
    void editor.apply((latest) => graphicPointsCommands(latest, edit))
  }}>
    <FieldGroup>
      {values.map((value, index) => {
        const label = `Endpoint ${index < 2 ? 1 : 2} ${index % 2 === 0 ? 'x' : 'y'} (pt)`
        const invalid = coordinates[index] === null
        return <Field key={index} data-invalid={invalid}>
          <FieldLabel htmlFor={`${id}-${index}`}>{label}</FieldLabel>
          <Input id={`${id}-${index}`} value={value} inputMode="decimal" disabled={editor.busy} aria-invalid={invalid} aria-describedby={`${id}-help`}
            onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
            onChange={(event) => setValues((current) => current.map((entry, at) => at === index ? event.target.value : entry))} />
        </Field>
      })}
    </FieldGroup>
    <FieldDescription id={`${id}-help`}>Edit the two points of {graphic.id} ({graphic.shape}) in document coordinates. Use finite decimal values from −100000 to 100000 pt. Reversed or coincident points are allowed. Group membership and references are retained. Drag distinct endpoints on the canvas, or use this form for coincident endpoints.</FieldDescription>
    <Button type="submit" variant="outline" size="sm" disabled={editor.busy || composing || !valid || unchanged}>Apply graphic endpoints</Button>
  </form>
}
