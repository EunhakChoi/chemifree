import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { parseGraphicCoordinate, type PolygonGraphicObject } from '@/editor/graphic-points'
import { polygonVerticesCommands } from '@/editor/polygon'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { MAX_POLYGON_VERTICES, MIN_POLYGON_VERTICES, type DocumentView, type Position } from '@/lib/document-contract'

/** Ordered vertex coordinates are applied together; editing the draft never writes. */
export function PolygonVerticesForm({ document, graphic, editor }: { document: DocumentView; graphic: PolygonGraphicObject; editor: DocumentEditor }) {
  const id = useId()
  const [values, setValues] = useState(() => graphic.points.map((point) => [String(point.x), String(point.y)]))
  const [composing, setComposing] = useState(false)
  const parsed = values.map(([x, y]) => ({ x: parseGraphicCoordinate(x), y: parseGraphicCoordinate(y) }))
  const vertices: Position[] = []
  for (const point of parsed) if (point.x !== null && point.y !== null) vertices.push({ x: point.x, y: point.y })
  const valid = vertices.length === values.length && values.length >= MIN_POLYGON_VERTICES && values.length <= MAX_POLYGON_VERTICES
  const unchanged = JSON.stringify(vertices) === JSON.stringify(graphic.points)
  return <form aria-label={`Edit polygon ${graphic.id} vertices`} className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (editor.busy || composing || !valid || unchanged) return
    const edit = { documentId: document.id, revision: document.revision, graphic, vertices }
    void editor.apply((latest) => polygonVerticesCommands(latest, edit))
  }}>
    <p className="text-xs" aria-label="Polygon vertex count">{values.length} vertices</p>
    <FieldGroup>
      {values.flatMap((coordinates, vertex) => coordinates.map((value, axis) => {
        const invalid = (axis === 0 ? parsed[vertex].x : parsed[vertex].y) === null
        const inputId = `${id}-${vertex}-${axis}`
        return <Field key={inputId} data-invalid={invalid}>
          <FieldLabel htmlFor={inputId}>Vertex {vertex + 1} {axis === 0 ? 'x' : 'y'} (pt)</FieldLabel>
          <Input id={inputId} value={value} inputMode="decimal" disabled={editor.busy} aria-invalid={invalid} aria-describedby={`${id}-help`}
            onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
            onChange={(event) => setValues((current) => current.map((entry, index) => index === vertex ? entry.map((coordinate, at) => at === axis ? event.target.value : coordinate) : entry))} />
        </Field>
      }))}
    </FieldGroup>
    <FieldDescription id={`${id}-help`}>Enter 3–64 ordered vertices with finite coordinates from −100000 to 100000 pt. The last edge closes to the first vertex without adding another point. Repeated, collinear and self-intersecting vertices are retained. This is an unfilled drawing outline with no chemical meaning.</FieldDescription>
    <div className="flex flex-wrap gap-2">
      <Button type="button" size="sm" variant="outline" disabled={editor.busy || composing || values.length >= MAX_POLYGON_VERTICES} onClick={() => setValues((current) => [...current, ['', '']])}>Add vertex row</Button>
      <Button type="button" size="sm" variant="outline" disabled={editor.busy || composing || values.length <= MIN_POLYGON_VERTICES} onClick={() => setValues((current) => current.slice(0, -1))}>Remove last vertex row</Button>
      <Button type="submit" size="sm" variant="outline" disabled={editor.busy || composing || !valid || unchanged}>Apply polygon vertices</Button>
    </div>
  </form>
}
