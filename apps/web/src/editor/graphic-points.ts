import { isGraphicShape, type Command, type DocumentView, type Position, type SceneObject, type GraphicShape } from '@/lib/document-contract'

export type GraphicObject = Extract<SceneObject, { kind: 'graphic' }>
export type TwoPointGraphicObject = Extract<GraphicObject, { shape: GraphicShape }>
export type SplineGraphicObject = Extract<GraphicObject, { shape: 'spline' }>
export type PolygonGraphicObject = Extract<GraphicObject, { shape: 'polygon' }>

export function graphicPointValid(point: Position): boolean {
  return Number.isFinite(point.x) && Number.isFinite(point.y) && Math.abs(point.x) <= 100_000 && Math.abs(point.y) <= 100_000
}

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
  graphic: TwoPointGraphicObject
  points: [Position, Position]
}

/** A captured endpoint edit lapses when the target or revision changes in the request queue. */
export function graphicPointsCommands(latest: DocumentView, edit: GraphicPointsEdit): Command[] | null {
  if (!isGraphicShape(edit.graphic.shape) || edit.points.length !== 2 || latest.id !== edit.documentId || latest.revision !== edit.revision) return null
  const current = latest.scene?.objects?.find((object) => object.id === edit.graphic.id)
  if (JSON.stringify(current) !== JSON.stringify(edit.graphic)) return null
  if (!edit.points.every((point) => graphicPointValid(point))) return null
  if (JSON.stringify(edit.points) === JSON.stringify(edit.graphic.points)) return null
  return [{ op: 'update_graphic_points', object: edit.graphic.id, points: [{ ...edit.points[0] }, { ...edit.points[1] }] }]
}

/** Handles require one explicit graphic target; group or mixed selections have no handles. */
export function selectedGraphic(document: DocumentView, selected: ReadonlySet<string>): TwoPointGraphicObject | null {
  if (selected.size !== 1) return null
  const object = document.scene?.objects?.find((item) => selected.has(item.id))
  if (!object || object.kind !== 'graphic' || object.shape === 'polygon' || object.shape === 'spline') return null
  return object.points[0].x === object.points[1].x && object.points[0].y === object.points[1].y ? null : object
}

export interface GraphicEndpointGesture {
  edit: GraphicPointsEdit
  endpoint: 0 | 1
  start: Position
}

/** Whole-object preview geometry shared by the stroke and its endpoint handles. */
export function translatedGraphicPoints(points: readonly [Position, Position, Position, Position], delta: Position): [Position, Position, Position, Position]
export function translatedGraphicPoints(points: readonly [Position, Position], delta: Position): [Position, Position]
export function translatedGraphicPoints(points: readonly Position[], delta: Position): Position[]
export function translatedGraphicPoints(points: readonly Position[], delta: Position): Position[] {
  return points.map((point) => ({ x: point.x + delta.x, y: point.y + delta.y }))
}

/** Whole-object translation retains every defining point and all graphic metadata. */
export function translatedGraphicObject(object: GraphicObject, delta: Position): GraphicObject {
  // Each branch retains the shape's tuple cardinality through the overloads.
  return object.shape === 'spline'
    ? { ...object, points: translatedGraphicPoints(object.points, delta) }
    : object.shape === 'polygon'
    ? { ...object, points: translatedGraphicPoints(object.points, delta) }
    : { ...object, points: translatedGraphicPoints(object.points, delta) }
}

/** Preserve the press offset inside the handle and move only the chosen endpoint. */
export function graphicEndpointPoints(gesture: GraphicEndpointGesture, at: Position): [Position, Position] | null {
  const points: [Position, Position] = [{ ...gesture.edit.graphic.points[0] }, { ...gesture.edit.graphic.points[1] }]
  const original = points[gesture.endpoint]
  points[gesture.endpoint] = { x: original.x + at.x - gesture.start.x, y: original.y + at.y - gesture.start.y }
  return points.every((point) => graphicPointValid(point)) ? points : null
}

/** A gesture remains bound to its press-time document, revision, target and explicit selection. */
export function graphicEndpointCurrent(document: DocumentView, selected: ReadonlySet<string>, gesture: GraphicEndpointGesture): boolean {
  const graphic = selectedGraphic(document, selected)
  return document.id === gesture.edit.documentId && document.revision === gesture.edit.revision &&
    graphic !== null && JSON.stringify(graphic) === JSON.stringify(gesture.edit.graphic)
}
