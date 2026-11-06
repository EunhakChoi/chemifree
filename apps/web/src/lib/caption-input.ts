/** Blank inherits drawing size; decimal/scientific values override it; undefined is invalid. */
export function parseCaptionSize(text: string): number | null | undefined {
  const trimmed = text.trim()
  if (trimmed === '') return null
  // Number also accepts radix-prefixed literals, which are unexpected in a point-size field.
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(trimmed)) return undefined
  const size = Number(trimmed)
  return Number.isFinite(size) && size >= 4 && size <= 72 ? size : undefined
}
