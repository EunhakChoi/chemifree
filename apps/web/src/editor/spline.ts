import { graphicPointValid, type SplineGraphicObject } from '@/editor/graphic-points'
import type { Command, DocumentView, Position, SceneGraphicShape } from '@/lib/document-contract'

export interface SplineCreation {
  documentId: string
  revision: number
  serviceGeneration: number
  points: Position[]
}

export const SPLINE_POINT_NAMES = ['P0', 'C1', 'C2', 'P3'] as const

/** Exact stored order: start, two explicit controls, end. No controls or closing point are inferred. */
export function splinePoints(points: readonly Position[]): [Position, Position, Position, Position] | null {
  return points.length === 4 && points.every((point) => graphicPointValid(point))
    ? [{ ...points[0] }, { ...points[1] }, { ...points[2] }, { ...points[3] }] : null
}

export function splineDraftCurrent(document: DocumentView, shape: SceneGraphicShape | null, generation: number, draft: SplineCreation): boolean {
  return document.id === draft.documentId && document.revision === draft.revision && generation === draft.serviceGeneration && shape === 'spline'
}

/** Four clicks remain a draft until explicit Finish; a fifth click does not replace any point. */
export function appendSplinePoint(draft: SplineCreation, point: Position): SplineCreation {
  return draft.points.length >= 4 || !graphicPointValid(point) ? draft : { ...draft, points: [...draft.points, { ...point }] }
}

/** One page-root creation, sealed to the first click's document and revision. */
export function splineCreationCommands(document: DocumentView, creation: SplineCreation): Command[] | null {
  const points = splinePoints(creation.points)
  return document.id === creation.documentId && document.revision === creation.revision && points ? [{ op: 'add_spline', points }] : null
}

interface SplinePointsEdit {
  documentId: string
  revision: number
  graphic: SplineGraphicObject
  points: Position[]
}

/** Replace all four points together, preserving target identity and refusing stale or unchanged drafts. */
export function splinePointsCommands(document: DocumentView, edit: SplinePointsEdit): Command[] | null {
  const points = splinePoints(edit.points)
  if (!points || document.id !== edit.documentId || document.revision !== edit.revision) return null
  const current = document.scene?.objects?.find((object) => object.id === edit.graphic.id)
  if (JSON.stringify(current) !== JSON.stringify(edit.graphic) || JSON.stringify(points) === JSON.stringify(edit.graphic.points)) return null
  return [{ op: 'update_spline_points', object: edit.graphic.id, points }]
}
