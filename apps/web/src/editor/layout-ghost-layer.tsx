import type { GhostSegment } from '@/editor/layout-ghost'
import type { Position } from '@/lib/document-contract'

/**
 * Proposed layout positions drawn over a placement: a dot for each layout atom (moved or not) and one dashed line
 * for each bond. An overlay for the person looking, not part of the drawing: `data-editor-overlay`
 * lets an export leave it out, and it takes no pointer events, as AgreementRings does. The dash
 * and the primary color set it apart from the drawing, which has neither.
 */
export function LayoutGhostLayer({
  atoms,
  segments,
  lineWidth,
  unitsPerPixel,
}: {
  atoms: ReadonlyArray<{ atom: string; at: Position }>
  segments: readonly GhostSegment[]
  lineWidth: number
  unitsPerPixel: number
}) {
  if (atoms.length === 0 && segments.length === 0) return null
  return (
    <g
      className="pointer-events-none"
      data-editor-overlay=""
      data-layout-ghost=""
      role="img"
      aria-label="Proposed layout positions"
    >
      {segments.map((s) => (
        <line
          key={s.bond}
          data-layout-ghost-bond={s.bond}
          x1={s.from.x}
          y1={s.from.y}
          x2={s.to.x}
          y2={s.to.y}
          className="stroke-primary"
          strokeWidth={lineWidth}
          strokeDasharray={`${4 * unitsPerPixel} ${3 * unitsPerPixel}`}
          strokeLinecap="round"
        />
      ))}
      {atoms.map((a) => (
        <circle
          key={a.atom}
          data-layout-ghost-atom={a.atom}
          cx={a.at.x}
          cy={a.at.y}
          r={3 * unitsPerPixel}
          className="fill-primary"
        />
      ))}
    </g>
  )
}
