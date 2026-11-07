/** Static embedded PNG limits shared with the domain/provider contract. */
export const MAX_PNG_BYTES = 128 * 1024
export const MAX_PNG_EDGE = 2048
export const MAX_PNG_PIXELS = 1_048_576
export const MAX_DOCUMENT_IMAGE_PIXELS = 4_194_304

export interface PngMetadata {
  pixelWidth: number
  pixelHeight: number
  byteLength: number
}

const SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10]

/** Bounded container/header preflight only; the service verifies CRCs and the complete decoded PNG. */
export function pngMetadata(bytes: Uint8Array): PngMetadata | null {
  if (bytes.length > MAX_PNG_BYTES || bytes.length < 45 || SIGNATURE.some((value, index) => bytes[index] !== value)) return null
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (view.getUint32(8) !== 13 || String.fromCharCode(...bytes.subarray(12, 16)) !== 'IHDR') return null
  const pixelWidth = view.getUint32(16)
  const pixelHeight = view.getUint32(20)
  if (!pixelWidth || !pixelHeight || pixelWidth > MAX_PNG_EDGE || pixelHeight > MAX_PNG_EDGE || pixelWidth * pixelHeight > MAX_PNG_PIXELS ||
      bytes[24] !== 8 || (bytes[25] !== 2 && bytes[25] !== 6) || bytes[26] !== 0 || bytes[27] !== 0 || bytes[28] !== 0) return null
  let offset = 8
  let data = false
  while (offset + 12 <= bytes.length) {
    const length = view.getUint32(offset)
    if (length > bytes.length - offset - 12) return null
    const kind = String.fromCharCode(...bytes.subarray(offset + 4, offset + 8))
    if (kind === 'IHDR') { if (offset !== 8 || length !== 13) return null }
    else if (kind === 'IDAT') data = true
    else if (kind !== 'IEND' && !((kind === 'sRGB' && length === 1) || (kind === 'gAMA' && length === 4) || (kind === 'pHYs' && length === 9))) return null
    offset += 12 + length
    if (kind === 'IEND') return length === 0 && data && offset === bytes.length ? { pixelWidth, pixelHeight, byteLength: bytes.length } : null
  }
  return null
}

/** Read canonical padded base64 without accepting a URL or allocating beyond the compressed limit. */
export function embeddedPngMetadata(data: unknown): PngMetadata | null {
  if (typeof data !== 'string' || data.length === 0 || data.length > Math.ceil(MAX_PNG_BYTES / 3) * 4 ||
      data.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(data)) return null
  try {
    const binary = atob(data)
    if (binary.length > MAX_PNG_BYTES || btoa(binary) !== data) return null
    return pngMetadata(Uint8Array.from(binary, (character) => character.charCodeAt(0)))
  } catch {
    return null
  }
}

/** The caller checks File.size before reading, so a huge upload is never materialized or decoded. */
export async function readPngFile(file: File): Promise<{ dataBase64: string; metadata: PngMetadata }> {
  if (file.size > MAX_PNG_BYTES) throw new Error('Choose a PNG of at most 128 KiB compressed.')
  if (file.type && file.type !== 'image/png') throw new Error('Choose a PNG file (image/png).')
  const bytes = new Uint8Array(await file.arrayBuffer())
  const metadata = pngMetadata(bytes)
  if (!metadata) throw new Error('Choose a static, noninterlaced 8-bit RGB/RGBA PNG without embedded ICC/text metadata, up to 2048 pixels per edge and 1048576 pixels in total.')
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return { dataBase64: btoa(binary), metadata }
}
