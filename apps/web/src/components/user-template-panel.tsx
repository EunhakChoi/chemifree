import { useEffect, useId, useRef, useState } from 'react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import { sameConversionSnapshot, settingsOf, type DocumentView, type NativeClipboardCopy, type TemplateInspection, type TemplateAttachmentOffer } from '@/lib/document-contract'

interface UserTemplatePanelProps {
  document: DocumentView
  editor: Pick<DocumentEditor, 'busy' | 'serviceGeneration' | 'currentServiceGeneration' | 'copyFragment' | 'pasteFragment' | 'inspectTemplate' | 'previewTemplateAttachment' | 'attachTemplate' | 'clipboardPending' | 'clipboardAction' | 'clipboardNeedsReload' | 'checkClipboardPaste' | 'reconcileClipboardPaste'>
  activePlacement: string | null
}

interface TemplateFile {
  name: string
  payload: string
}

const MAX_TEMPLATE_BYTES = 64 * 1024

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'The template file operation failed.'
}

/** User-authored graph files use the existing native fragment codec and atomic paste. */
export function UserTemplatePanel({ document, editor, activePlacement }: UserTemplatePanelProps) {
  const [mode, setMode] = useState<'save' | 'insert' | 'attach' | null>(null)
  const [offer, setOffer] = useState<NativeClipboardCopy | null>(null)
  const [file, setFile] = useState<TemplateFile | null>(null)
  const [inspection, setInspection] = useState<TemplateInspection | null>(null)
  const [sourceAnchor, setSourceAnchor] = useState('')
  const [targetAtom, setTargetAtom] = useState('')
  const [directionX, setDirectionX] = useState('')
  const [directionY, setDirectionY] = useState('')
  const [attachment, setAttachment] = useState<{ offer: TemplateAttachmentOffer; generation: number } | null>(null)
  const [name, setName] = useState('template')
  const [offsetX, setOffsetX] = useState('')
  const [offsetY, setOffsetY] = useState('')
  const [pending, setPending] = useState<'prepare' | 'read' | 'insert' | 'check' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const alive = useRef(true)
  const sequence = useRef(0)
  const id = useId()

  useEffect(() => {
    alive.current = true
    return () => { alive.current = false; sequence.current += 1 }
  }, [])

  const busy = editor.busy || pending === 'prepare' || pending === 'insert' || pending === 'check'
  const currentOffer = offer && sameConversionSnapshot(document, offer.snapshot.document) ? offer : null
  const x = Number(offsetX)
  const y = Number(offsetY)
  const validX = offsetX.trim() !== '' && Number.isFinite(x)
  const validY = offsetY.trim() !== '' && Number.isFinite(y)
  const singleDestination = document.scene?.instances.length === 1
  const dx = Number(directionX)
  const dy = Number(directionY)
  const validDirection = directionX.trim() !== '' && directionY.trim() !== '' && Number.isFinite(dx) && Number.isFinite(dy) && (dx !== 0 || dy !== 0)
  const currentAttachment = attachment && sameConversionSnapshot(document, attachment.offer.source.document) && attachment.generation === editor.currentServiceGeneration() ? attachment.offer : null
  const unresolved = editor.clipboardPending && editor.clipboardAction === 'template'

  function current(attempt: number, generation: number): boolean {
    return alive.current && sequence.current === attempt && editor.currentServiceGeneration() === generation
  }

  function close() {
    sequence.current += 1
    setMode(null)
    setInspection(null)
    setAttachment(null)
    setOffer(null)
    setFile(null)
    setPending(null)
    setError(null)
  }

  async function prepare() {
    const attempt = ++sequence.current
    const generation = editor.serviceGeneration
    setMode('save')
    setOffer(null)
    setName('template')
    setError(null)
    setPending('prepare')
    try {
      const copied = await editor.copyFragment(document.id, activePlacement)
      if (!current(attempt, generation)) return
      if (copied.boundaryBonds.length > 0) {
        throw new Error('Select complete graph components before saving a template. Crossing bonds are not template attachment instructions.')
      }
      setOffer(copied)
    } catch (cause: unknown) {
      if (current(attempt, generation)) setError(errorMessage(cause))
    } finally {
      if (current(attempt, generation)) setPending(null)
    }
  }

  function download() {
    if (busy || !currentOffer || !name.trim() || editor.currentServiceGeneration() !== editor.serviceGeneration) return
    // Only the filename is named. The graph bytes remain the closed native v1 payload.
    const filename = name.trim().replace(/[^\p{L}\p{N}._-]/gu, '_').replace(/^\.+/, '') || 'template'
    setError(null)
    try {
      const url = URL.createObjectURL(new Blob([currentOffer.payload], { type: 'application/json;charset=utf-8' }))
      try {
        const anchor = globalThis.document.createElement('a')
        anchor.href = url
        anchor.download = `${filename}.chemifree-fragment.json`
        anchor.click()
      } finally {
        // Release on the next task, after the browser has consumed the download URL.
        setTimeout(() => URL.revokeObjectURL(url), 0)
      }
    } catch (cause: unknown) {
      setError(errorMessage(cause))
    }
  }

  function openInsert() {
    sequence.current += 1
    setMode('insert')
    setInspection(null)
    setAttachment(null)
    setSourceAnchor('')
    setTargetAtom('')
    setDirectionX('')
    setDirectionY('')
    setOffer(null)
    setFile(null)
    setOffsetX(String(settingsOf(document).bond_length_pt))
    setOffsetY(String(settingsOf(document).bond_length_pt))
    setError(null)
    setPending(null)
  }

  async function readFile(selected: File | undefined) {
    const attempt = ++sequence.current
    const generation = editor.serviceGeneration
    setFile(null)
    setInspection(null)
    setAttachment(null)
    setSourceAnchor('')
    setError(null)
    setPending(null)
    if (!selected) return
    setPending('read')
    try {
      if (selected.size > MAX_TEMPLATE_BYTES) throw new Error('Template files must be at most 64 KiB.')
      const bytes = await selected.arrayBuffer()
      if (!current(attempt, generation)) return
      // Fatal decoding prevents an invalid UTF-8 byte sequence from becoming replacement text.
      const payload = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes)
      if (!payload.trim()) throw new Error('Choose a non-empty Chemifree graph template file.')
      if (mode === 'attach') {
        const inspected = await editor.inspectTemplate(payload)
        if (!current(attempt, generation)) return
        setInspection(inspected)
      }
      setFile({ name: selected.name, payload })
    } catch (cause: unknown) {
      if (current(attempt, generation)) setError(errorMessage(cause))
    } finally {
      if (current(attempt, generation)) setPending(null)
    }
  }

  async function insert() {
    if (busy || pending === 'read' || !file || !validX || !validY || !singleDestination || editor.clipboardPending) return
    const attempt = ++sequence.current
    const generation = editor.serviceGeneration
    setPending('insert')
    setError(null)
    try {
      await editor.pasteFragment(document.id, file.payload, { x, y }, 'template')
      if (current(attempt, generation)) close()
    } catch (cause: unknown) {
      if (current(attempt, generation)) setError(errorMessage(cause))
    } finally {
      if (current(attempt, generation)) setPending(null)
    }
  }

  async function previewAttachment() {
    if (busy || !file || !inspection || !sourceAnchor || !targetAtom || !activePlacement || !singleDestination || !validDirection) return
    const attempt = ++sequence.current
    const generation = editor.serviceGeneration
    setAttachment(null)
    setError(null)
    setPending('prepare')
    try {
      const offer = await editor.previewTemplateAttachment(document.id, file.payload, activePlacement, sourceAnchor, targetAtom, { dx, dy })
      if (current(attempt, generation)) setAttachment({ offer, generation })
    } catch (cause: unknown) {
      if (current(attempt, generation)) setError(errorMessage(cause))
    } finally {
      if (current(attempt, generation)) setPending(null)
    }
  }

  async function attach() {
    if (busy || !currentAttachment || editor.clipboardPending) return
    const attempt = ++sequence.current
    const generation = editor.serviceGeneration
    setPending('insert')
    setError(null)
    try {
      await editor.attachTemplate(currentAttachment)
      if (current(attempt, generation)) close()
    } catch (cause: unknown) {
      if (current(attempt, generation)) setError(errorMessage(cause))
    } finally {
      if (current(attempt, generation)) setPending(null)
    }
  }

  async function check() {
    const attempt = ++sequence.current
    const generation = editor.serviceGeneration
    setPending('check')
    setError(null)
    try {
      if (editor.clipboardNeedsReload) await editor.reconcileClipboardPaste()
      else await editor.checkClipboardPaste()
      if (current(attempt, generation)) close()
    } catch (cause: unknown) {
      if (current(attempt, generation)) setError(errorMessage(cause))
    } finally {
      if (current(attempt, generation)) setPending(null)
    }
  }

  const recovery = unresolved && <Alert><AlertDescription className="flex flex-col gap-2">
    <p>{editor.clipboardNeedsReload
      ? 'The original template insertion can no longer be confirmed by replay. It may already be present. Reload the document to reconcile its outcome.'
      : 'The template insertion outcome is not confirmed. Check the original attempt before making another change.'}</p>
    <Button size="sm" variant="outline" disabled={pending === 'check'} onClick={() => void check()}>
      {editor.clipboardNeedsReload ? 'Reload document to reconcile' : 'Check template insertion outcome'}
    </Button>
  </AlertDescription></Alert>

  return <section aria-label="User graph templates" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
    <h3 className="text-sm font-medium">User graph templates</h3>
    <p className="text-xs text-muted-foreground">Save complete selected graph components as a reusable file. Insert independently into one placement with matching document style. No automatic attachment, rotation or layout.</p>
    <Dialog open={mode !== null} onOpenChange={(open) => { if (!open) close() }}>
      <div className="flex flex-wrap gap-2">
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy || editor.clipboardPending || !activePlacement} />} onClick={() => void prepare()}>Save template file…</DialogTrigger>
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy || editor.clipboardPending} />} onClick={() => openInsert()}>Insert template file…</DialogTrigger>
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy || editor.clipboardPending} />} onClick={() => { openInsert(); setMode('attach') }}>Attach template file…</DialogTrigger>
      </div>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col overflow-hidden transition-none sm:max-w-lg">
        <DialogHeader className="shrink-0 pr-8"><DialogTitle>{mode === 'save' ? 'Save user graph template' : mode === 'attach' ? 'Attach user graph template' : 'Insert user graph template'}</DialogTitle></DialogHeader>
        <div className="-mx-4 flex min-h-0 flex-col gap-4 overflow-x-hidden overflow-y-auto px-4 pb-4">
          <DialogDescription>{mode === 'save'
            ? 'Only selected atoms, bond endpoints and internal bonds are saved. Scene objects, references and enhanced stereo groups are not template content. The source drawing is unchanged.'
            : mode === 'attach' ? 'Choose one connected native template and an explicit eligible source atom. Choose a destination atom and direction, then preview the translation-only single-bond connection. Preview changes nothing; source-anchor stereo loss is refused.'
            : 'Choose a Chemifree graph template file (up to 64 KiB). Reading does not edit the drawing. The local service validates its graph, style and version only when you press Insert template.'}</DialogDescription>
          {mode !== null && recovery}
          {mode === 'save' ? <>
            {currentOffer && <p role="status">{currentOffer.copied.atoms.length} atoms, {currentOffer.copied.bonds.length} bonds from {currentOffer.instance}, revision {currentOffer.snapshot.document.revision}.</p>}
            {offer && !currentOffer && <p>The drawing changed. Close this dialog and prepare the template again.</p>}
            <FieldGroup key="template-save"><Field>
              <FieldLabel htmlFor={`${id}-name`}>Template filename</FieldLabel>
              <Input id={`${id}-name`} value={name} disabled={busy} onChange={(event) => setName(event.target.value)} />
            </Field></FieldGroup>
            <DialogFooter className="mx-0 mb-0"><Button disabled={busy || !currentOffer || !name.trim()} onClick={() => download()}>Download template</Button></DialogFooter>
          </> : mode === 'attach' ? <>
            {!singleDestination && <p>Attachment requires exactly one destination placement.</p>}
            <FieldGroup key="template-attach">
              <Field><FieldLabel htmlFor={`${id}-attach-file`}>Template file for attachment</FieldLabel>
                <Input id={`${id}-attach-file`} type="file" accept=".json,application/json" disabled={busy || editor.clipboardPending} onChange={(event) => {
                  const selected = event.target.files?.[0]
                  event.target.value = ''
                  void readFile(selected)
                }} />
                {file && <p role="status">Validated {file.name}: {inspection?.atoms.length} local atoms.</p>}
              </Field>
              <Field><FieldLabel htmlFor={`${id}-source`}>Source anchor</FieldLabel>
                <Select value={sourceAnchor || null} disabled={busy || !inspection || editor.clipboardPending} onValueChange={(value) => {
                  setSourceAnchor(typeof value === 'string' ? value : ''); setAttachment(null)
                }}>
                  <SelectTrigger id={`${id}-source`}><SelectValue placeholder="Choose a local atom" /></SelectTrigger>
                  <SelectContent><SelectGroup>{inspection?.atoms.map((atom) => <SelectItem key={atom.id} value={atom.id} disabled={!atom.supported}>
                    {atom.id} · {atom.element} ({atom.x}, {atom.y}){atom.supported ? '' : ' · stored stereo at anchor'}
                  </SelectItem>)}</SelectGroup></SelectContent>
                </Select>
              </Field>
              <Field><FieldLabel htmlFor={`${id}-target`}>Destination atom</FieldLabel>
                <Select value={targetAtom || null} disabled={busy || editor.clipboardPending} onValueChange={(value) => {
                  setTargetAtom(typeof value === 'string' ? value : ''); setAttachment(null)
                }}>
                  <SelectTrigger id={`${id}-target`}><SelectValue placeholder="Choose an existing atom" /></SelectTrigger>
                  <SelectContent><SelectGroup>{document.atoms.map((atom) => <SelectItem key={atom.id} value={atom.id}>{atom.id} · {atom.element}</SelectItem>)}</SelectGroup></SelectContent>
                </Select>
              </Field>
              <Field><FieldLabel htmlFor={`${id}-dx`}>Attachment direction dx</FieldLabel>
                <Input id={`${id}-dx`} value={directionX} inputMode="decimal" disabled={busy || editor.clipboardPending} onChange={(event) => { setDirectionX(event.target.value); setAttachment(null) }} />
              </Field>
              <Field><FieldLabel htmlFor={`${id}-dy`}>Attachment direction dy</FieldLabel>
                <Input id={`${id}-dy`} value={directionY} inputMode="decimal" disabled={busy || editor.clipboardPending} onChange={(event) => { setDirectionY(event.target.value); setAttachment(null) }} />
              </Field>
            </FieldGroup>
            <p>Enter a finite nonzero vector. Magnitude is ignored; the connector uses the current bond length. The saved geometry is translated, without rotation or collision avoidance.</p>
            <Button disabled={busy || pending === 'read' || editor.clipboardPending || !inspection || !sourceAnchor || !targetAtom || !activePlacement || !singleDestination || !validDirection} onClick={() => void previewAttachment()}>Preview template connection</Button>
            {attachment && !currentAttachment && <p>The drawing or service changed. Preview the connection again.</p>}
            {currentAttachment && <div className="flex flex-col gap-2">
              <p role="status">Preview only: {currentAttachment.mapping.atoms.length} fresh atoms, {currentAttachment.mapping.bonds.length} internal bonds and one connector. The destination has not changed.</p>
              {currentAttachment.preview.notes?.map((note, index) => <p key={index}>{note.reason}</p>)}
              <Button disabled={busy || editor.clipboardPending} onClick={() => void attach()}>Attach template</Button>
            </div>}
          </> : mode === 'insert' ? <>
            {!singleDestination && <p>Template insertion requires exactly one destination placement.</p>}
            <p>Offsets in points are added to the saved coordinates. Content may overlap; no viewport positioning or attachment is inferred.</p>
            <FieldGroup key="template-insert">
              <Field>
                <FieldLabel htmlFor={`${id}-file`}>Template file</FieldLabel>
                <Input id={`${id}-file`} type="file" accept=".json,application/json" disabled={busy || editor.clipboardPending} onChange={(event) => {
                  const selected = event.target.files?.[0]
                  event.target.value = ''
                  void readFile(selected)
                }} />
                {file && <p role="status" className="break-all">Selected {file.name}. Ready for validation and insertion.</p>}
              </Field>
              <Field data-invalid={!validX || undefined}>
                <FieldLabel htmlFor={`${id}-x`}>Template offset x (pt)</FieldLabel>
                <Input id={`${id}-x`} inputMode="decimal" value={offsetX} aria-invalid={!validX} disabled={busy || editor.clipboardPending} onChange={(event) => setOffsetX(event.target.value)} />
                {!validX && <FieldError>Enter a finite x offset in points.</FieldError>}
              </Field>
              <Field data-invalid={!validY || undefined}>
                <FieldLabel htmlFor={`${id}-y`}>Template offset y (pt)</FieldLabel>
                <Input id={`${id}-y`} inputMode="decimal" value={offsetY} aria-invalid={!validY} disabled={busy || editor.clipboardPending} onChange={(event) => setOffsetY(event.target.value)} />
                {!validY && <FieldError>Enter a finite y offset in points.</FieldError>}
              </Field>
            </FieldGroup>
            <DialogFooter className="mx-0 mb-0"><Button disabled={busy || pending === 'read' || editor.clipboardPending || !file || !singleDestination || !validX || !validY} onClick={() => void insert()}>Insert template</Button></DialogFooter>
          </> : null}
          {pending && <p role="status">{pending === 'prepare' ? 'Preparing template…' : pending === 'read' ? 'Reading template file…' : 'Working…'}</p>}
          {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
        </div>
      </DialogContent>
    </Dialog>
    {mode === null && recovery}
    {mode === null && error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </section>
}
