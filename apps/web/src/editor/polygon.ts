import { graphicPointValid, type PolygonGraphicObject } from '@/editor/graphic-points'
import { MAX_POLYGON_VERTICES, MIN_POLYGON_VERTICES, type Command, type DocumentView, type Position, type SceneGraphicShape } from '@/lib/document-contract'

export interface PolygonCreation {
  documentId: string
  revision: number
  serviceGeneration: number
  vertices: Position[]
}

/** Drafts may have fewer than three vertices; only explicit Finish can create a polygon. */
export function polygonDraftCurrent(document: DocumentView, shape: SceneGraphicShape | null, generation: number, draft: PolygonCreation): boolean {
  return document.id === draft.documentId && document.revision === draft.revision && generation === draft.serviceGeneration && shape === 'polygon'
}

/** No minimum edge length, area, orientation or simple-polygon interpretation is invented. */
export function polygonVerticesValid(vertices: readonly Position[]): boolean {
  return vertices.length >= MIN_POLYGON_VERTICES && vertices.length <= MAX_POLYGON_VERTICES && vertices.every((point) => graphicPointValid(point))
}

/** Append one ordered click without mutating the draft or synthesizing a closing vertex. */
export function appendPolygonVertex(draft: PolygonCreation, vertex: Position): PolygonCreation {
  if (draft.vertices.length >= MAX_POLYGON_VERTICES || !graphicPointValid(vertex)) return draft
  return { ...draft, vertices: [...draft.vertices, { ...vertex }] }
}

/** Finish is a single page-root add_polygon bound to the first click's document revision. */
export function polygonCreationCommands(document: DocumentView, creation: PolygonCreation): Command[] | null {
  if (document.id !== creation.documentId || document.revision !== creation.revision || !polygonVerticesValid(creation.vertices)) return null
  return [{ op: 'add_polygon', vertices: creation.vertices.map((vertex) => ({ ...vertex })) }]
}

interface PolygonVerticesEdit {
  documentId: string
  revision: number
  graphic: PolygonGraphicObject
  vertices: Position[]
}

/** Like endpoint edits, vertex-list edits lapse after a document, revision or target change. */
export function polygonVerticesCommands(document: DocumentView, edit: PolygonVerticesEdit): Command[] | null {
  if (document.id !== edit.documentId || document.revision !== edit.revision || !polygonVerticesValid(edit.vertices)) return null
  const current = document.scene?.objects?.find((object) => object.id === edit.graphic.id)
  if (JSON.stringify(current) !== JSON.stringify(edit.graphic) || JSON.stringify(edit.vertices) === JSON.stringify(edit.graphic.points)) return null
  return [{ op: 'update_polygon_vertices', object: edit.graphic.id, vertices: edit.vertices.map((vertex) => ({ ...vertex })) }]
}
