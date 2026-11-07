import { graphicPointValid } from '@/editor/graphic-points'
import { isGraphicShape, type Command, type DocumentView, type GraphicShape, type Position, type SceneGraphicShape } from '@/lib/document-contract'

export interface GraphicCreation {
  documentId: string
  revision: number
  serviceGeneration: number
  shape: GraphicShape
  points: [Position, Position]
}

/** Press-time context is fixed, including the explicitly chosen shape and service process. */
export function graphicCreationCurrent(document: DocumentView, shape: SceneGraphicShape | null, serviceGeneration: number, creation: GraphicCreation): boolean {
  return document.id === creation.documentId && document.revision === creation.revision &&
    shape === creation.shape && serviceGeneration === creation.serviceGeneration
}

/** Existing add_graphic with no parent creates a page root, retaining the two-point order. */
export function graphicCreationCommands(document: DocumentView, creation: GraphicCreation): Command[] | null {
  if (!isGraphicShape(creation.shape) || creation.points.length !== 2 || document.id !== creation.documentId || document.revision !== creation.revision || !creation.points.every((point) => graphicPointValid(point))) return null
  return [{ op: 'add_graphic', shape: creation.shape, points: [{ ...creation.points[0] }, { ...creation.points[1] }] }]
}
