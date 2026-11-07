import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { parseGraphicCoordinate, type SplineGraphicObject } from '@/editor/graphic-points'
import { SPLINE_POINT_NAMES, splinePoints, splinePointsCommands } from '@/editor/spline'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView, Position } from '@/lib/document-contract'

/** Edit the complete ordered cubic definition with the same bounded coordinates as other graphics. */
export function SplinePointsForm({ document, graphic, editor }: { document: DocumentView; graphic: SplineGraphicObject; editor: DocumentEditor }) {
  const id = useId()
  const [values, setValues] = useState(() => graphic.points.map((point) => [String(point.x), String(point.y)]))
  const [composing, setComposing] = useState(false)
  const parsed = values.map(([x, y]) => ({ x: parseGraphicCoordinate(x), y: parseGraphicCoordinate(y) }))
  const points: Position[] = []
  for (const point of parsed) if (point.x !== null && point.y !== null) points.push({ x: point.x, y: point.y })
  const valid = splinePoints(points) !== null
  const unchanged = JSON.stringify(points) === JSON.stringify(graphic.points)
  return <form aria-label={`Edit spline ${graphic.id} points`} className="flex flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (editor.busy || composing || !valid || unchanged) return
    const edit = { documentId: document.id, revision: document.revision, graphic, points }
    void editor.apply((latest) => splinePointsCommands(latest, edit))
  }}>
    <FieldGroup>
      {values.flatMap((coordinates, index) => coordinates.map((value, axis) => {
        const invalid = (axis === 0 ? parsed[index].x : parsed[index].y) === null
        const inputId = `${id}-${index}-${axis}`
        return <Field key={inputId} data-invalid={invalid}>
          <FieldLabel htmlFor={inputId}>{SPLINE_POINT_NAMES[index]} {axis === 0 ? 'x' : 'y'} (pt)</FieldLabel>
          <Input id={inputId} value={value} inputMode="decimal" disabled={editor.busy} aria-invalid={invalid} aria-describedby={`${id}-help`}
            onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
            onChange={(event) => setValues((current) => current.map((entry, at) => at === index ? entry.map((coordinate, i) => i === axis ? event.target.value : coordinate) : entry))} />
        </Field>
      }))}
    </FieldGroup>
    <FieldDescription id={`${id}-help`}>P0 is the start, C1 and C2 are explicit controls, and P3 is the end. Enter finite coordinates from −100000 to 100000 pt. One open cubic stroke with no fill, arrowhead or chemical meaning. Fit, marquee and transform bounds use all four defining points, not curve extrema.</FieldDescription>
    <Button type="submit" size="sm" variant="outline" disabled={editor.busy || composing || !valid || unchanged}>Apply spline points</Button>
  </form>
}
