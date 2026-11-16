// The formats of the web Export card (P48 S5), in the Import card's order with the export-only
// SVG picture last, and the download name of each prepared file. Pure: no React, no app imports.

export type ExportFormat = 'smiles' | 'mol' | 'sdf' | 'cdxml' | 'cdx' | 'svg'

/** The formats written as a file. SMILES is written as text only. */
export type FileExportFormat = Exclude<ExportFormat, 'smiles'>

export const EXPORT_FORMATS: ReadonlyArray<{ id: ExportFormat; label: string }> = [
  { id: 'smiles', label: 'SMILES' },
  { id: 'mol', label: 'MOL' },
  { id: 'sdf', label: 'SDF' },
  { id: 'cdxml', label: 'CDXML' },
  { id: 'cdx', label: 'CDX' },
  { id: 'svg', label: 'SVG picture' },
]

/** The form value as a format, or null for '' and anything unknown (never sent). */
export function parseExportFormat(value: string): ExportFormat | null {
  return EXPORT_FORMATS.find((format) => format.id === value)?.id ?? null
}

/** The download name of a prepared file. SMILES has no file: it is written as text. */
export function exportFileName(format: FileExportFormat): string {
  return `chemifree.${format}`
}
