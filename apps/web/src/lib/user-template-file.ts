// Pure checks for user graph template files: the read limit and strict decoding of a selected
// file, the download filename, and the refusal to save a selection that crosses bonds. The
// component keeps the request ordering and service-generation guards.

/** Largest template file the panel reads, in bytes. */
export const MAX_TEMPLATE_BYTES = 64 * 1024

/** Throws when a selected file is larger than the read limit. Runs before the bytes are read. */
export function checkTemplateFileSize(size: number): void {
  if (size > MAX_TEMPLATE_BYTES) throw new Error('Template files must be at most 64 KiB.')
}

/**
 * Decodes template bytes as UTF-8. Fatal decoding prevents an invalid byte sequence from becoming
 * replacement text; a leading byte order mark is kept for the service to refuse.
 */
export function decodeTemplateFile(bytes: ArrayBuffer | Uint8Array): string {
  const payload = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes)
  if (!payload.trim()) throw new Error('Choose a non-empty Chemifree graph template file.')
  return payload
}

/** Download filename for a template name. Only the filename is named; the payload is unchanged. */
export function templateDownloadName(name: string): string {
  const stem = name.trim().replace(/[^\p{L}\p{N}._-]/gu, '_').replace(/^\.+/, '') || 'template'
  return `${stem}.chemifree-fragment.json`
}

/** Why a copied selection cannot be saved as a template, or null when it can. */
export function templateSaveRefusal(boundaryBondCount: number): string | null {
  return boundaryBondCount > 0
    ? 'Select complete graph components before saving a template. Crossing bonds are not template attachment instructions.'
    : null
}
