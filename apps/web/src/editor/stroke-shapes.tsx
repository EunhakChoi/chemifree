import { wavePath, bondStrokes, type Stroke } from '@/editor/geometry'
import type { StereoMark } from '@/lib/stereo-contract'

const DASHES: Record<Extract<Stroke, { shape: 'line' }>['pattern'], string | undefined> = {
  solid: undefined,
  dashed: '2.5 2',
  dotted: '0.1 2.2',
}

/**
 * The elements of a bond's strokes, to sit in a group that sets the stroke color and width and
 * has no fill. A wedge is filled with `fill`'s classes and has no outline of its own; the
 * stripes of a hashed wedge have square ends.
 */
export function StrokeShapes({ strokes, bond, fill = 'fill-foreground stroke-none' }: { strokes: readonly Stroke[]; bond: string; fill?: string }) {
  return strokes.map((stroke, index) => {
    switch (stroke.shape) {
      case 'line':
        return (
          <line
            key={index}
            x1={stroke.x1}
            y1={stroke.y1}
            x2={stroke.x2}
            y2={stroke.y2}
            strokeDasharray={DASHES[stroke.pattern]}
            data-bond={bond}
            data-pattern={stroke.pattern}
          />
        )
      case 'polyline':
        return <polyline key={index} points={stroke.points.map((p) => `${p.x},${p.y}`).join(' ')} data-bond={bond} />
      case 'polygon':
        return <polygon key={index} points={stroke.points.map((p) => `${p.x},${p.y}`).join(' ')} className={fill} data-bond={bond} data-stroke="wedge" />
      case 'stripes':
        return (
          <path
            key={index}
            d={stroke.segments.map(([a, b]) => `M ${a.x} ${a.y} L ${b.x} ${b.y}`).join(' ')}
            strokeLinecap="butt"
            data-bond={bond}
            data-stroke="hash"
            data-stripes={stroke.segments.length}
          />
        )
      case 'wave':
        return <path key={index} d={wavePath(stroke)} data-bond={bond} data-stroke="wave" />
      default: {
        const unhandled: never = stroke
        return unhandled
      }
    }
  })
}

/** A 16 by 10 picture of a mark for a tool button: the same strokes the canvas draws, narrow end at the left. */
export function MarkGlyph({ mark }: { mark: StereoMark }) {
  const bond = { id: 'glyph', begin: 'a', end: 'b', order: 'single' as const, query: null, mark }
  const strokes = bondStrokes(bond, { x: 1, y: 5 }, { x: 15, y: 5 }, null, null, 1, 1, 1)
  return (
    <svg width="16" height="10" viewBox="0 0 16 10" aria-hidden="true" className="fill-none stroke-current" strokeWidth={1}>
      <StrokeShapes strokes={strokes} bond="glyph" fill="fill-current stroke-none" />
    </svg>
  )
}
