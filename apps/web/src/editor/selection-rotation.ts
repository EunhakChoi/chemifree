import { selectionTransform } from '@/editor/selection-geometry'
import type { Command, DocumentView } from '@/lib/document-contract'

/** Parse finite decimal/scientific degrees without a separate magnitude limit. */
export function parseRotationDegrees(text: string): number | null {
  const trimmed = text.trim()
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(trimmed)) return null
  const degrees = Number(trimmed)
  return Number.isFinite(degrees) ? degrees : null
}

interface RotationEdit {
  documentId: string
  selection: ReadonlySet<string>
  instance: string | null
  degrees: number
}

/** Like the fixed rotations, retain pressed targets and rebuild their queued geometry pivot. */
export function selectionRotation(document: DocumentView, edit: RotationEdit): Command[] | null {
  if (document.id !== edit.documentId || !Number.isFinite(edit.degrees)) return null
  const remainder = edit.degrees % 360
  const angle = remainder < 0 ? remainder + 360 : remainder
  // Match the domain's rem_euclid identity, including a tiny negative remainder rounding to 360.
  if (angle === 0 || angle === 360) return null
  return selectionTransform(document, edit.selection, edit.instance, { type: 'rotate', degrees: edit.degrees })
}
