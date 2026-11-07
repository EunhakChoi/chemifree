import type { Command, DocumentView, Position, SceneObject } from '@/lib/document-contract'

export type GraphicObject = Extract<SceneObject, { kind: 'graphic' }>

/** Decimal point coordinates match the domain's finite ±100000 point boundary. */
export function parseGraphicCoordinate(text: string): number | null {
  const trimmed = text.trim()
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(trimmed)) return null
  const value = Number(trimmed)
  return Number.isFinite(value) && Math.abs(value) <= 100_000 ? value : null
}

export interface GraphicPointsEdit {
  documentId: string
  revision: number
  graphic: GraphicObject
  points: [Position, Position]
}

/** A captured endpoint edit lapses when the target or revision changes in the request queue. */
export function graphicPointsCommands(latest: DocumentView, edit: GraphicPointsEdit): Command[] | null {
  if (latest.id !== edit.documentId || latest.revision !== edit.revision) return null
  const current = latest.scene?.objects?.find((object) => object.id === edit.graphic.id)
  if (JSON.stringify(current) !== JSON.stringify(edit.graphic)) return null
  if (edit.points.some((point) => !Number.isFinite(point.x) || !Number.isFinite(point.y) || Math.abs(point.x) > 100_000 || Math.abs(point.y) > 100_000)) return null
  if (JSON.stringify(edit.points) === JSON.stringify(edit.graphic.points)) return null
  return [{ op: 'update_graphic_points', object: edit.graphic.id, points: [{ ...edit.points[0] }, { ...edit.points[1] }] }]
}
