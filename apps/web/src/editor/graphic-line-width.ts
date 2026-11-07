import { parseGraphicCoordinate, type GraphicObject } from '@/editor/graphic-points'
import { SETTING_RANGES, type Command, type DocumentView } from '@/lib/document-contract'

/** Missing and null overrides inherit the document setting. */
export function graphicLineWidth(graphic: GraphicObject, documentWidth: number): number {
  return graphic.line_width_pt ?? documentWidth
}

/** Blank restores inheritance; invalid decimal input is never sent. */
export function parseGraphicLineWidth(text: string): number | null | undefined {
  if (text.trim() === '') return null
  const value = parseGraphicCoordinate(text)
  const [low, high] = SETTING_RANGES.line_width_pt
  return value !== null && value >= low && value <= high ? value : undefined
}

interface GraphicLineWidthEdit {
  documentId: string
  revision: number
  graphic: GraphicObject
  lineWidth: number | null
}

/** Preserve the press-time target and revision while earlier queued requests settle. */
export function graphicLineWidthCommands(document: DocumentView, edit: GraphicLineWidthEdit): Command[] | null {
  const [low, high] = SETTING_RANGES.line_width_pt
  if (document.id !== edit.documentId || document.revision !== edit.revision ||
      (edit.lineWidth !== null && (!Number.isFinite(edit.lineWidth) || edit.lineWidth < low || edit.lineWidth > high))) return null
  const current = document.scene?.objects?.find((object) => object.id === edit.graphic.id)
  if (JSON.stringify(current) !== JSON.stringify(edit.graphic) || edit.lineWidth === (edit.graphic.line_width_pt ?? null)) return null
  return [{ op: 'set_graphic_line_width', object: edit.graphic.id, line_width_pt: edit.lineWidth }]
}
