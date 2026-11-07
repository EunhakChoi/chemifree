import { graphicPointValid } from '@/editor/graphic-points'
import type { Command, DocumentView, GraphicShape, Position } from '@/lib/document-contract'

export interface GraphicCreation {
  documentId: string
  revision: number
  serviceGeneration: number
  shape: GraphicShape
  points: [Position, Position]
}

/** Press-time context is fixed, including the explicitly chosen shape and service process. */
export function graphicCreationCurrent(document: DocumentView, shape: GraphicShape | null, serviceGeneration: number, creation: GraphicCreation): boolean {
  return document.id === creation.documentId && document.revision === creation.revision &&
    shape === creation.shape && serviceGeneration === creation.serviceGeneration
}

/** Existing add_graphic with no parent creates a page root, retaining the two-point order. */
export function graphicCreationCommands(document: DocumentView, creation: GraphicCreation): Command[] | null {
  if (document.id !== creation.documentId || document.revision !== creation.revision || !creation.points.every((point) => graphicPointValid(point))) return null
  return [{ op: 'add_graphic', shape: creation.shape, points: [{ ...creation.points[0] }, { ...creation.points[1] }] }]
}
