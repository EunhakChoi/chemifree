import type { PointerEvent } from 'react'

import { captionRunStyle } from '@/editor/caption-style'
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
  const styledRuns = object.runs.map((run) => ({ run, runStyle: captionRunStyle(run, settings) }))
  function renderText(shadow: boolean) {
    return (
      <text aria-hidden={shadow || undefined} pointerEvents={shadow ? 'none' : undefined}
        data-caption-shadow-text={shadow ? object.id : undefined} x={object.anchor.x} y={object.anchor.y} xmlSpace="preserve" style={{ whiteSpace: 'pre', direction: 'ltr', unicodeBidi: 'isolate' }}
        textAnchor="start" data-caption-alignment={object.alignment} data-caption-anchor-x={object.anchor.x}
        className={cn('fill-foreground stroke-foreground', settings.label_font === 'sans-serif' && 'font-sans')}
        fontFamily={settings.label_font === 'sans-serif' ? undefined : settings.label_font}
        fontSize={size} data-caption-text={shadow ? undefined : object.id} role={shadow ? undefined : "img"}
        aria-label={shadow ? undefined : `Caption ${object.id}: ${object.runs.map((run) => run.text).join('')}`}>
        {styledRuns.map(({ run, runStyle }, index) => {
          // A run without its own font inherits the label font from the text element, so it sets no font attribute.
          const ownFont = run.font ?? null
          const outlined = runStyle.outline !== null
          return (
            <tspan visibility={shadow && !run.shadow ? 'hidden' : undefined} data-caption-shadow={shadow ? undefined : run.shadow ? 'true' : 'false'} fill={runStyle.color ?? undefined} data-caption-outline={shadow ? undefined : outlined ? 'true' : 'false'}
              style={{ fill: outlined ? 'none' : runStyle.color ?? undefined, stroke: outlined ? runStyle.color ?? 'inherit' : 'none', strokeWidth: runStyle.outline ?? 0, strokeLinejoin: 'round' }} key={index} data-caption-script={run.script}
              className={ownFont === 'sans-serif' ? 'font-sans' : undefined}
              fontFamily={ownFont === null || ownFont === 'sans-serif' ? undefined : runStyle.family} fontWeight={runStyle.weight} fontStyle={runStyle.style}
              fontSize={runStyle.size}
              y={object.anchor.y + runStyle.shift}>{run.text}</tspan>
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
          {styledRuns.map(({ run, runStyle }, index) => run.shadow && run.underline && <rect key={index} data-caption-underline-shadow={index} fill={runStyle.color ?? undefined} />)}
        </g>
      </g>
      {renderText(false)}
      <g aria-hidden="true" pointerEvents="none" className="fill-foreground" data-caption-underlines="">
        {styledRuns.map(({ run, runStyle }, index) => run.underline && <rect key={index} data-caption-underline={index} fill={runStyle.color ?? undefined} />)}
      </g>
    </g>
  )
}
