import type { AgreementRing } from '@/editor/stereo-agreement'
import type { Position } from '@/lib/document-contract'

/** Radius (pt) of a ring: clear of an atom label, inside the canvas margin. */
export const RING_RADIUS = 11

/**
 * Rings around the atoms and double bonds whose stored configuration the drawing of this
 * placement does not agree with. An overlay for the person looking, not part of the drawing:
 * `data-editor-overlay` lets an export leave it out, and it takes no pointer events. A solid
 * destructive ring is a disagreement; a dashed muted ring is a state with nothing to compare (a
 * stored configuration no mark shows, a mark nothing is stored for, a mark that gives none), so
 * the difference is the dash as well as the color, and every ring is a named image.
 */
export function AgreementRings({ rings }: { rings: ReadonlyArray<{ ring: AgreementRing; at: Position }> }) {
  if (rings.length === 0) return null
  return (
    <g className="pointer-events-none fill-none" data-editor-overlay="">
      {rings.map(({ ring, at }) => (
        <circle
          key={`${ring.item.kind}:${ring.item.id}`}
          role="img"
          aria-label={ring.name}
          data-agreement={ring.agreement}
          data-agreement-item={ring.item.id}
          cx={at.x}
          cy={at.y}
          r={RING_RADIUS}
          strokeWidth={1.2}
          strokeDasharray={ring.agreement === 'disagrees' ? undefined : '3 2'}
          className={ring.agreement === 'disagrees' ? 'stroke-destructive' : 'stroke-muted-foreground'}
        />
      ))}
    </g>
  )
}
