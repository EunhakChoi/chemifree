import type { PointerEvent } from 'react'

import type { Box } from '@/editor/label'
import type { DocumentSettings, Position, SceneObject } from '@/lib/document-contract'
import { cn } from '@/lib/utils'

/** Independent annotation. The enclosing canvas measures the actual SVG ink after fonts load. */
export function SceneCaption({ object, box, settings, selected, delta, onPointerDown }: {
  object: Extract<SceneObject, { kind: 'caption' }>
  box?: Box
  settings: DocumentSettings
  selected: boolean
  delta?: Position
  onPointerDown: (event: PointerEvent<SVGGElement>) => void
}) {
  const size = settings.label_size_pt
  return (
    <g data-scene-object={object.id} data-caption="" onPointerDown={onPointerDown}
      transform={delta ? `translate(${delta.x} ${delta.y})` : undefined}>
      {box && <rect {...box} className={selected ? 'fill-primary/10 stroke-primary' : 'fill-transparent'} strokeWidth={selected ? 0.8 : 0} />}
      <text x={object.anchor.x} y={object.anchor.y} xmlSpace="preserve" style={{ whiteSpace: 'pre' }}
        className={cn('fill-foreground', settings.label_font === 'sans-serif' && 'font-sans')}
        fontFamily={settings.label_font === 'sans-serif' ? undefined : settings.label_font}
        fontSize={size} data-caption-text={object.id} role="img"
        aria-label={`Caption ${object.id}: ${object.runs.map((run) => run.text).join('')}`}>
        {object.runs.map((run, index) => (
          <tspan key={index} data-caption-script={run.script}
            fontSize={run.script === 'baseline' ? size : size * 2 / 3}
            y={object.anchor.y + (run.script === 'subscript' ? size / 3 : run.script === 'superscript' ? -size * 5 / 12 : 0)}>{run.text}</tspan>
        ))}
      </text>
    </g>
  )
}
