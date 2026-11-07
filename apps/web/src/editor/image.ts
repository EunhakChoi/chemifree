import { graphicPointValid } from '@/editor/graphic-points'
import { embeddedPngMetadata, MAX_DOCUMENT_IMAGE_PIXELS } from '@/lib/png-image'
import type { Command, DocumentView, Position, SceneObject } from '@/lib/document-contract'

export type ImageObject = Extract<SceneObject, { kind: 'image' }>

export interface ImageCreation {
  documentId: string
  revision: number
  serviceGeneration: number
  dataBase64: string
  origin: Position
  sizePt: Position
}

/** Images keep an axis-aligned rectangle; both corners lie inside the shared coordinate boundary. */
export function imageGeometryValid(origin: Position, size: Position): boolean {
  return graphicPointValid(origin) && Number.isFinite(size.x) && Number.isFinite(size.y) && size.x > 0 && size.y > 0 &&
    graphicPointValid({ x: origin.x + size.x, y: origin.y + size.y })
}

export function imageCorners(image: Pick<ImageObject, 'origin' | 'size_pt'>): [Position, Position] {
  return [{ ...image.origin }, { x: image.origin.x + image.size_pt.x, y: image.origin.y + image.size_pt.y }]
}

export function imagePixelTotal(document: DocumentView): number {
  return document.scene?.objects?.reduce((total, object) => total + (object.kind === 'image' ? object.pixel_width * object.pixel_height : 0), 0) ?? 0
}

/** Positive decimal sizes may span the complete coordinate interval, unlike an individual point. */
export function parseImageSize(text: string): number | null {
  const trimmed = text.trim()
  if (!/^[+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(trimmed)) return null
  const value = Number(trimmed)
  return Number.isFinite(value) && value > 0 && value <= 200_000 ? value : null
}

/** Seal one validated asset and explicit physical rectangle to its submit-time document revision. */
export function imageCreationCommands(document: DocumentView, creation: ImageCreation): Command[] | null {
  if (document.id !== creation.documentId || document.revision !== creation.revision || !imageGeometryValid(creation.origin, creation.sizePt)) return null
  const metadata = embeddedPngMetadata(creation.dataBase64)
  if (!metadata || imagePixelTotal(document) + metadata.pixelWidth * metadata.pixelHeight > MAX_DOCUMENT_IMAGE_PIXELS) return null
  return [{ op: 'add_image', mime_type: 'image/png', data_base64: creation.dataBase64, origin: { ...creation.origin }, size_pt: { ...creation.sizePt } }]
}

interface ImageGeometryEdit {
  documentId: string
  revision: number
  image: ImageObject
  origin: Position
  sizePt: Position
}

/** Geometry-only edits retain the embedded bytes, metadata, identity, parent and references. */
export function imageGeometryCommands(document: DocumentView, edit: ImageGeometryEdit): Command[] | null {
  if (document.id !== edit.documentId || document.revision !== edit.revision || !imageGeometryValid(edit.origin, edit.sizePt)) return null
  const current = document.scene?.objects?.find((object) => object.id === edit.image.id)
  if (JSON.stringify(current) !== JSON.stringify(edit.image)) return null
  if (JSON.stringify(edit.origin) === JSON.stringify(edit.image.origin) && JSON.stringify(edit.sizePt) === JSON.stringify(edit.image.size_pt)) return null
  return [{ op: 'update_image_geometry', object: edit.image.id, origin: { ...edit.origin }, size_pt: { ...edit.sizePt } }]
}
