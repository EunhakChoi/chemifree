import type { PointerEvent } from 'react'
import type { ImageObject } from '@/editor/image'
import type { Position } from '@/lib/document-contract'

/** The entire stored rectangle is selectable, including transparent PNG pixels. */
export function SceneImage({ object, selected, unitsPerPixel, delta, onPointerDown }: {
  object: ImageObject
  selected: boolean
  unitsPerPixel: number
  delta?: Position
  onPointerDown: (event: PointerEvent<SVGGElement>) => void
}) {
  const x = object.origin.x + (delta?.x ?? 0)
  const y = object.origin.y + (delta?.y ?? 0)
  return <g data-scene-object={object.id} data-scene-image={object.id} onPointerDown={onPointerDown}>
    <image x={x} y={y} width={object.size_pt.x} height={object.size_pt.y} preserveAspectRatio="none"
      href={`data:image/png;base64,${object.data_base64}`} pointerEvents="none" role="img"
      aria-label={`Embedded PNG image ${object.id}, ${object.pixel_width} by ${object.pixel_height} pixels, ${object.size_pt.x} by ${object.size_pt.y} pt; no chemical interpretation`} />
    <rect data-image-hit={object.id} x={x} y={y} width={object.size_pt.x} height={object.size_pt.y} fill="transparent" pointerEvents="all" aria-hidden="true" />
    {selected && <rect data-image-selection={object.id} x={x} y={y} width={object.size_pt.x} height={object.size_pt.y} fill="none" className="stroke-primary" strokeWidth={2 * unitsPerPixel} pointerEvents="none" aria-hidden="true" />}
  </g>
}
