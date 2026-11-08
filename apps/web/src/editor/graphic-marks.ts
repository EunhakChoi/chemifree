import type { GraphicObject } from '@/editor/graphic-points'
import { graphicLineWidth } from '@/editor/graphic-line-width'
import type { Position } from '@/lib/document-contract'

/** One piece of ink of a scene graphic, in pt, as plain numbers; the renderer formats them into attributes. */
export type GraphicMark =
  | { shape: 'line'; x1: number; y1: number; x2: number; y2: number }
  | { shape: 'head'; points: [Position, Position, Position] }
  | { shape: 'rect'; x: number; y: number; width: number; height: number }
  | { shape: 'ellipse'; cx: number; cy: number; rx: number; ry: number }
  | { shape: 'polygon'; points: Position[] }
  | { shape: 'cubic'; points: [Position, Position, Position, Position] }

/**
 * The effective stroke width and the ink of a scene graphic, computed from its stored points alone.
 * An arrow yields its shaft first and its head second. The head is 6 pt long and 3 pt half-wide,
 * not scaled by the line width, and is emitted only when the shaft has length. Hit strokes and
 * selection underlays are editor overlays and are not part of the ink.
 */
export function graphicMarks(object: GraphicObject, documentLineWidth: number): { width: number; marks: GraphicMark[] } {
  const lineWidth = graphicLineWidth(object, documentLineWidth)
  if (object.shape === 'spline') {
    const [p0, c1, c2, p3] = object.points
    return { width: lineWidth, marks: [{ shape: 'cubic', points: [copyPoint(p0), copyPoint(c1), copyPoint(c2), copyPoint(p3)] }] }
  }
  if (object.shape === 'polygon') {
    return { width: lineWidth, marks: [{ shape: 'polygon', points: object.points.map((point) => copyPoint(point)) }] }
  }
  const [a, b] = object.points
  if (object.shape === 'rectangle') {
    return {
      width: lineWidth,
      marks: [{ shape: 'rect', x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) }],
    }
  }
  if (object.shape === 'ellipse') {
    return {
      width: lineWidth,
      marks: [{ shape: 'ellipse', cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2, rx: Math.abs(a.x - b.x) / 2, ry: Math.abs(a.y - b.y) / 2 }],
    }
  }
  const marks: GraphicMark[] = [{ shape: 'line', x1: a.x, y1: a.y, x2: b.x, y2: b.y }]
  const length = Math.hypot(b.x - a.x, b.y - a.y)
  if (object.shape === 'arrow' && length > 0) {
    const ux = (b.x - a.x) / length
    const uy = (b.y - a.y) / length
    marks.push({
      shape: 'head',
      points: [
        { x: b.x, y: b.y },
        { x: b.x - ux * 6 - uy * 3, y: b.y - uy * 6 + ux * 3 },
        { x: b.x - ux * 6 + uy * 3, y: b.y - uy * 6 - ux * 3 },
      ],
    })
  }
  return { width: lineWidth, marks }
}

/** Copies a stored point so that marks never alias the document's objects. */
function copyPoint(point: Position): Position {
  return { x: point.x, y: point.y }
}
