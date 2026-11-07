import { useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { parseGraphicCoordinate } from '@/editor/graphic-points'
import { imageGeometryCommands, imageGeometryValid, imagePixelTotal, parseImageSize, type ImageObject } from '@/editor/image'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView, Position } from '@/lib/document-contract'
import { MAX_DOCUMENT_IMAGE_PIXELS, readPngFile, type PngMetadata } from '@/lib/png-image'

interface ImageRectangle { origin: Position; sizePt: Position }

/** File selection does not write or decode an image; Add submits one explicit physical rectangle. */
export function EmbeddedImagePanel({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const chosen = document.scene?.objects?.filter((object) => editor.selection.has(object.id)) ?? []
  const image = chosen.length === 1 && chosen[0].kind === 'image' ? chosen[0] : null
  return <section aria-label="Embedded PNG images" className="flex min-w-0 flex-col gap-3">
    <h4 className="text-sm font-medium">Embedded PNG image</h4>
    <AddImageForm key={`${document.id}:${editor.serviceGeneration}`} document={document} editor={editor} />
    {image && <EditImageForm key={`${document.id}:${image.id}:${image.origin.x}:${image.origin.y}:${image.size_pt.x}:${image.size_pt.y}:${editor.serviceGeneration}`} document={document} image={image} editor={editor} />}
    <p className="text-xs text-muted-foreground">Images have no chemical interpretation. Their complete rectangle, including transparent pixels, can be selected, moved, grouped and aligned. Rotation, reflection and image clipboard copying are not supported.</p>
  </section>
}

function AddImageForm({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const id = useId()
  const [candidate, setCandidate] = useState<{ dataBase64: string; metadata: PngMetadata; name: string } | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const sequence = useRef(0)
  const alive = useRef(true)
  useEffect(() => { alive.current = true; return () => { alive.current = false; sequence.current += 1 } }, [])
  const fits = candidate !== null && imagePixelTotal(document) + candidate.metadata.pixelWidth * candidate.metadata.pixelHeight <= MAX_DOCUMENT_IMAGE_PIXELS
  return <ImageRectangleForm action="Add PNG image" initial={null} busy={editor.busy || loading} enabled={fits} onApply={(rectangle) => {
    if (!candidate || !fits) return
    void editor.createImage({ documentId: document.id, revision: document.revision, serviceGeneration: editor.serviceGeneration, dataBase64: candidate.dataBase64, ...rectangle })
  }}>
    <Field>
      <FieldLabel htmlFor={id}>PNG file</FieldLabel>
      <Input id={id} type="file" accept="image/png" disabled={editor.busy} aria-describedby={`${id}-help`} onChange={(event) => {
        const file = event.target.files?.[0]
        const request = ++sequence.current
        setCandidate(null)
        setError(null)
        setLoading(Boolean(file))
        if (!file) return
        void readPngFile(file).then((value) => {
          if (alive.current && sequence.current === request) setCandidate({ ...value, name: file.name })
        }).catch((reason: unknown) => {
          if (alive.current && sequence.current === request) setError(reason instanceof Error ? reason.message : 'The PNG file could not be read.')
        }).finally(() => { if (alive.current && sequence.current === request) setLoading(false) })
      }} />
      <FieldDescription id={`${id}-help`}>Static 8-bit RGB/RGBA, noninterlaced PNG only; no animation or embedded ICC/text metadata. Limit: 128 KiB compressed, 2048 pixels per edge and 1048576 pixels per image. Enter the size in pt explicitly; pixel density does not choose it. PNG content is checked when added.</FieldDescription>
      {loading && <p role="status" className="text-xs">Reading PNG file…</p>}
      {candidate && <p role="status" className="break-all text-xs">{candidate.name}: image/png · {candidate.metadata.byteLength} compressed bytes (limit 128 KiB) · {candidate.metadata.pixelWidth} × {candidate.metadata.pixelHeight} pixels.</p>}
      {candidate && !fits && <FieldError>This PNG would exceed the drawing limit of 4194304 embedded pixels. Remove an image or choose a smaller PNG.</FieldError>}
      {error && <FieldError role="alert">{error}</FieldError>}
    </Field>
  </ImageRectangleForm>
}

function EditImageForm({ document, image, editor }: { document: DocumentView; image: ImageObject; editor: DocumentEditor }) {
  const padding = image.data_base64.endsWith('==') ? 2 : image.data_base64.endsWith('=') ? 1 : 0
  const bytes = image.data_base64.length / 4 * 3 - padding
  return <ImageRectangleForm action="Apply image geometry" initial={{ origin: image.origin, sizePt: image.size_pt }} busy={editor.busy} enabled onApply={(rectangle) => {
    const edit = { documentId: document.id, revision: document.revision, image, ...rectangle }
    void editor.apply((latest) => imageGeometryCommands(latest, edit))
  }}>
    <p className="text-xs">Selected image {image.id}: image/png · {bytes} compressed bytes · {image.pixel_width} × {image.pixel_height} pixels. Geometry edits preserve these embedded bytes.</p>
  </ImageRectangleForm>
}

/** Shared create/edit rectangle controls retain explicit pt values, IME, invalid and no-op guards. */
function ImageRectangleForm({ action, initial, busy, enabled, onApply, children }: { action: string; initial: ImageRectangle | null; busy: boolean; enabled: boolean; onApply: (rectangle: ImageRectangle) => void; children: ReactNode }) {
  const id = useId()
  const [values, setValues] = useState(() => initial ? [String(initial.origin.x), String(initial.origin.y), String(initial.sizePt.x), String(initial.sizePt.y)] : ['0', '0', '', ''])
  const [composing, setComposing] = useState(false)
  const parsed = values.map((value, index) => index < 2 ? parseGraphicCoordinate(value) : parseImageSize(value))
  const [x, y, width, height] = parsed
  const rectangle = x !== null && y !== null && width !== null && height !== null ? { origin: { x, y }, sizePt: { x: width, y: height } } : null
  const valid = rectangle !== null && imageGeometryValid(rectangle.origin, rectangle.sizePt)
  const unchanged = initial !== null && JSON.stringify(rectangle) === JSON.stringify(initial)
  return <form aria-label={action} className="flex min-w-0 flex-col gap-2" onSubmit={(event) => {
    event.preventDefault()
    if (!busy && enabled && !composing && valid && !unchanged && rectangle) onApply(rectangle)
  }}>
    {children}
    <FieldGroup>
      {values.map((value, index) => <Field key={index} data-invalid={parsed[index] === null && value.trim() !== ''}>
        <FieldLabel htmlFor={`${id}-${index}`}>{['Image origin x (pt)', 'Image origin y (pt)', 'Image width (pt)', 'Image height (pt)'][index]}</FieldLabel>
        <Input id={`${id}-${index}`} value={value} inputMode="decimal" disabled={busy} aria-describedby={`${id}-help`} aria-invalid={parsed[index] === null && value.trim() !== ''}
          onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
          onChange={(event) => setValues((current) => current.map((coordinate, at) => at === index ? event.target.value : coordinate))} />
      </Field>)}
    </FieldGroup>
    <FieldDescription id={`${id}-help`}>Width and height must be positive. The origin and opposite corner must both lie between −100000 and 100000 pt. The PNG fills the rectangle; its aspect ratio is not enforced.</FieldDescription>
    {rectangle && !valid && <FieldError>The image rectangle lies outside the supported coordinate bounds.</FieldError>}
    <Button type="submit" size="sm" variant="outline" disabled={busy || !enabled || composing || !valid || unchanged}>{action}</Button>
  </form>
}
