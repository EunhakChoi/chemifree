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
  function renderText(shadow: boolean) {
    return (
      <text aria-hidden={shadow || undefined} pointerEvents={shadow ? 'none' : undefined}
        data-caption-shadow-text={shadow ? object.id : undefined} x={object.anchor.x} y={object.anchor.y} xmlSpace="preserve" style={{ whiteSpace: 'pre', direction: 'ltr', unicodeBidi: 'isolate' }}
        textAnchor="start" data-caption-alignment={object.alignment} data-caption-anchor-x={object.anchor.x}
        className={cn('fill-foreground stroke-foreground', settings.label_font === 'sans-serif' && 'font-sans')}
        fontFamily={settings.label_font === 'sans-serif' ? undefined : settings.label_font}
        fontSize={size} data-caption-text={shadow ? undefined : object.id} role={shadow ? undefined : "img"}
        aria-label={shadow ? undefined : `Caption ${object.id}: ${object.runs.map((run) => run.text).join('')}`}>
        {object.runs.map((run, index) => {
          const runSize = run.size_pt ?? size
          const glyphSize = run.script === 'baseline' ? runSize : runSize * 2 / 3
          return (
            <tspan visibility={shadow && !run.shadow ? 'hidden' : undefined} data-caption-shadow={shadow ? undefined : run.shadow ? 'true' : 'false'} fill={run.color ?? undefined} data-caption-outline={shadow ? undefined : run.outline ? 'true' : 'false'}
              style={{ fill: run.outline ? 'none' : run.color ?? undefined, stroke: run.outline ? run.color ?? 'inherit' : 'none', strokeWidth: run.outline ? glyphSize / 16 : 0, strokeLinejoin: 'round' }} key={index} data-caption-script={run.script}
              className={run.font === 'sans-serif' ? 'font-sans' : undefined}
              fontFamily={run.font === 'sans-serif' ? undefined : run.font ?? undefined} fontWeight={run.weight} fontStyle={run.style}
              fontSize={glyphSize}
              y={object.anchor.y + (run.script === 'subscript' ? runSize / 3 : run.script === 'superscript' ? -runSize * 5 / 12 : 0)}>{run.text}</tspan>
          )
        })}
      </text>
    )
  }
  return (
    <g data-scene-object={object.id} data-caption="" onPointerDown={onPointerDown}
      transform={delta ? `translate(${delta.x} ${delta.y})` : undefined}>
      {box && <rect {...box} className={selected ? 'fill-primary/10 stroke-primary' : 'fill-transparent'} strokeWidth={selected ? 0.8 : 0} />}
      <g data-caption-shadow-layer="" aria-hidden="true" pointerEvents="none" opacity={0.35} transform="translate(1 1)">
        {renderText(true)}
        <g className="fill-foreground">
          {object.runs.map((run, index) => run.shadow && run.underline && <rect key={index} data-caption-underline-shadow={index} fill={run.color ?? undefined} />)}
        </g>
      </g>
      {renderText(false)}
      <g aria-hidden="true" pointerEvents="none" className="fill-foreground" data-caption-underlines="">
        {object.runs.map((run, index) => run.underline && <rect key={index} data-caption-underline={index} fill={run.color ?? undefined} />)}
      </g>
    </g>
  )
}
