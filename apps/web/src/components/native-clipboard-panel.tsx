import { useEffect, useId, useRef, useState } from 'react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import type { DocumentEditor, NativeClipboardCutPlan } from '@/hooks/use-document-editor'
import { settingsOf, type DocumentView, type NativeClipboardCopy } from '@/lib/document-contract'

interface NativeClipboardPanelProps {
  document: DocumentView
  editor: Pick<DocumentEditor, 'busy' | 'serviceGeneration' | 'currentServiceGeneration' | 'copyFragment' | 'prepareCut' | 'cutFragment' | 'clipboardAction' | 'pasteFragment' | 'clipboardPending' | 'checkClipboardPaste' | 'clipboardNeedsReload' | 'reconcileClipboardPaste'>
  activePlacement: string | null
}

interface CopyOffer {
  generation: number
  result: NativeClipboardCopy
  cut?: NativeClipboardCutPlan
}

interface PendingAction {
  generation: number
  action: 'copy' | 'cut' | 'read' | 'write' | 'paste' | 'check' | 'reconcile'
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'The native fragment request failed.'
}

/** Explicit native-text transport. Clipboard permissions always have a manual-text fallback. */
export function NativeClipboardPanel({ document, editor, activePlacement }: NativeClipboardPanelProps) {
  const [mode, setMode] = useState<'copy' | 'cut' | 'paste' | null>(null)
  const [offer, setOffer] = useState<CopyOffer | null>(null)
  const [text, setText] = useState('')
  const [offsetX, setOffsetX] = useState('')
  const [offsetY, setOffsetY] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [status, setStatus] = useState<string | null>(null)
  const [pending, setPending] = useState<PendingAction | null>(null)
  const alive = useRef(true)
  const copyText = useRef<HTMLTextAreaElement>(null)
  const request = useRef(0)
  const id = useId()

  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      request.current += 1
    }
  }, [])

  const localBusy = pending !== null && pending.generation === editor.serviceGeneration
  const busy = editor.busy || localBusy
  const singleDestination = document.scene?.instances.length === 1
  const currentOffer = offer?.generation === editor.serviceGeneration ? offer.result : null
  const x = Number(offsetX)
  const y = Number(offsetY)
  const validX = offsetX.trim() !== '' && Number.isFinite(x)
  const validY = offsetY.trim() !== '' && Number.isFinite(y)

  function current(sequence: number, generation: number): boolean {
    return alive.current && request.current === sequence && editor.currentServiceGeneration() === generation
  }

  function close() {
    request.current += 1
    setMode(null)
    setOffer(null)
    setText('')
    setOffsetX('')
    setOffsetY('')
    setError(null)
    setStatus(null)
    setPending(null)
  }

  async function prepareCopy(cutting = false) {
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    const source = document.id
    const placement = activePlacement
    setMode(cutting ? 'cut' : 'copy')
    setOffer(null)
    setError(null)
    setStatus(null)
    setPending({ generation, action: 'copy' })
    try {
      // Calling now lets the hook capture the selection and this placement at the press.
      if (cutting) {
        const plan = await editor.prepareCut(source, placement)
        if (!current(sequence, generation)) return
        setOffer({ generation, result: plan.copy, cut: plan })
      } else {
        const result = await editor.copyFragment(source, placement)
        if (!current(sequence, generation)) return
        setOffer({ generation, result })
      }
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(errorMessage(cause))
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  function openPaste() {
    request.current += 1
    setMode('paste')
    setOffer(null)
    setText('')
    const offset = String(settingsOf(document).bond_length_pt)
    setOffsetX(offset)
    setOffsetY(offset)
    setError(null)
    setStatus(null)
    setPending(null)
  }

  async function writeClipboard() {
    if (!currentOffer || busy) return
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setPending({ generation, action: 'write' })
    setError(null)
    setStatus(null)
    try {
      if (typeof navigator.clipboard?.writeText !== 'function') {
        throw new Error('Browser clipboard writing is unavailable. Select and copy the native text manually.')
      }
      // No request or await precedes this call: it retains this click's user activation.
      await navigator.clipboard.writeText(currentOffer.payload)
      if (current(sequence, generation)) setStatus('Native fragment text copied to clipboard.')
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(cause instanceof Error && cause.message.includes('unavailable')
        ? cause.message : 'Clipboard writing was denied or failed. Select and copy the native text manually.')
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  async function cutClipboard() {
    if (!currentOffer || !offer?.cut || busy) return
    const plan = offer.cut
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    let writeFailed = false
    setPending({ generation, action: 'cut' })
    setError(null)
    setStatus(null)
    try {
      if (typeof navigator.clipboard?.writeText !== 'function') {
        throw new Error('Browser clipboard writing is unavailable. The source was not cut. Select and copy the native text manually.')
      }
      // Start the browser write directly in this click, then give the promise to the
      // hook immediately. It owns the source checks and may delete only after success.
      const write = navigator.clipboard.writeText(currentOffer.payload).catch((cause: unknown) => {
        writeFailed = true
        throw cause
      })
      await editor.cutFragment(plan, write)
      if (current(sequence, generation)) close()
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(writeFailed
        ? 'Clipboard writing was denied or failed. The source was not cut. Select and copy the native text manually.'
        : errorMessage(cause))
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  async function readClipboard() {
    if (busy || editor.clipboardPending) return
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setPending({ generation, action: 'read' })
    setError(null)
    setStatus(null)
    try {
      if (typeof navigator.clipboard?.readText !== 'function') {
        throw new Error('Browser clipboard reading is unavailable. Paste native fragment text manually.')
      }
      // Reading is opt-in and happens directly in the click, never while opening the dialog.
      const value = await navigator.clipboard.readText()
      if (current(sequence, generation)) setText(value)
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(cause instanceof Error && cause.message.includes('unavailable')
        ? cause.message : 'Clipboard reading was denied or failed. Paste native fragment text manually.')
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  async function paste() {
    if (busy || editor.clipboardPending || !singleDestination || !text.trim() || !validX || !validY) return
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setPending({ generation, action: 'paste' })
    setError(null)
    try {
      await editor.pasteFragment(document.id, text, { x, y })
      if (current(sequence, generation)) close()
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(errorMessage(cause))
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  async function checkPaste(reconcile: boolean) {
    if (localBusy) return
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setPending({ generation, action: reconcile ? 'reconcile' : 'check' })
    setError(null)
    try {
      if (reconcile) await editor.reconcileClipboardPaste()
      else await editor.checkClipboardPaste()
      if (current(sequence, generation)) close()
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(errorMessage(cause))
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  const clipboardAction = editor.clipboardAction === 'mol_import' ? 'MOL import' : editor.clipboardAction ?? 'paste'
  const unresolved = editor.clipboardPending && editor.clipboardAction !== 'mol_import' && editor.clipboardAction !== 'scene_paste' && editor.clipboardAction !== 'scene_cut' && editor.clipboardAction !== 'smiles_import' && editor.clipboardAction !== 'ring' && editor.clipboardAction !== 'template' && <Alert>
    <AlertDescription className="flex flex-col gap-2">
      <p>{editor.clipboardNeedsReload
        ? `The original ${clipboardAction} can no longer be confirmed by replay. ${clipboardAction === 'cut' ? 'The source may already have been removed.' : 'It may already be present.'} Reload the document to reconcile its outcome.`
        : `The ${clipboardAction} outcome is not confirmed. Check the original attempt before making another change.`}</p>
      <Button size="sm" variant="outline" disabled={localBusy} onClick={() => void checkPaste(editor.clipboardNeedsReload)}>
        {editor.clipboardNeedsReload ? 'Reload document to reconcile' : `Check ${clipboardAction} outcome`}
      </Button>
    </AlertDescription>
  </Alert>

  return <section aria-label="Native fragment clipboard" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
    <h3 className="text-sm font-medium">Native fragment clipboard</h3>
    <p className="text-xs text-muted-foreground">Copy selected atoms and bond endpoints from the movement placement. Scene objects and enhanced stereo groups are not supported. Paste requires one placement and matching document style. Cut supports complete graph components in one placement without incoming atom/bond references. External formats are not available.</p>
    <Dialog open={mode !== null} onOpenChange={(open) => { if (!open) close() }}>
      <div className="flex flex-wrap gap-2">
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy || activePlacement === null} />} onClick={() => void prepareCopy()}>Copy fragment…</DialogTrigger>
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy || activePlacement === null} />} onClick={() => void prepareCopy(true)}>Cut fragment…</DialogTrigger>
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy} />} onClick={() => openPaste()}>Paste fragment…</DialogTrigger>
      </div>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col overflow-hidden transition-none sm:max-w-lg">
        <DialogHeader className="shrink-0 pr-8">
          <DialogTitle>{mode === 'copy' ? 'Copy native fragment' : mode === 'cut' ? 'Cut native fragment' : 'Paste native fragment'}</DialogTitle>
        </DialogHeader>
        <div className="-mx-4 flex min-h-0 flex-col gap-4 overflow-x-hidden overflow-y-auto px-4 pb-4">
          <DialogDescription>{mode === 'cut'
            ? 'Review the captured complete graph components. Cut to clipboard removes them only after a successful browser clipboard write and source revalidation. Manually copying this text never removes source content.'
            : mode === 'copy' ? 'This text contains the captured graph fragment. Select it for manual copying if browser clipboard access is unavailable.'
            : 'Paste Chemifree native fragment text below. Reading the browser clipboard is optional and requires your click. Text is sent to the local service only when you press Paste fragment.'}</DialogDescription>
        {mode === 'copy' || mode === 'cut' ? <>
          {mode === 'cut' && unresolved}
          {currentOffer && <p role="status">{currentOffer.copied.atoms.length} atoms, {currentOffer.copied.bonds.length} bonds from {currentOffer.instance}, revision {currentOffer.snapshot.document.revision}.</p>}
          {currentOffer && currentOffer.boundaryBonds.length > 0 && <p>{currentOffer.boundaryBonds.length} crossing bonds omitted; the copied fragment ends at those boundaries.</p>}
          {(offer && !currentOffer || pending !== null && pending.generation !== editor.serviceGeneration) && <p>The service changed. Close this dialog and copy the fragment again.</p>}
          <FieldGroup><Field>
            <FieldLabel htmlFor={`${id}-copy`}>Native fragment text</FieldLabel>
            <Textarea ref={copyText} id={`${id}-copy`} className="field-sizing-fixed max-h-48 resize-y" value={currentOffer?.payload ?? ''} readOnly rows={7} spellCheck={false} onFocus={(event) => event.currentTarget.select()} />
          </Field></FieldGroup>
          <DialogFooter className="mx-0 mb-0">
            <Button variant="outline" disabled={busy || !currentOffer} onClick={() => {
              copyText.current?.focus()
              copyText.current?.select()
            }}>Select native text</Button>
            {mode === 'cut'
              ? <Button disabled={busy || !currentOffer || !offer?.cut} onClick={() => void cutClipboard()}>Cut to clipboard</Button>
              : <Button variant="outline" disabled={busy || !currentOffer} onClick={() => void writeClipboard()}>Copy to clipboard</Button>}
          </DialogFooter>
        </> : <>
          {mode === 'paste' && unresolved}
          {!singleDestination && <p>Paste requires a document with exactly one placement.</p>}
          <p>Offsets in points are added to the source coordinates. Pasted content may overlap existing content; it is not positioned from the viewport.</p>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor={`${id}-paste`}>Native fragment text</FieldLabel>
              <Textarea id={`${id}-paste`} className="field-sizing-fixed max-h-48 resize-y" value={text} onChange={(event) => setText(event.target.value)} disabled={busy || editor.clipboardPending} rows={7} spellCheck={false} />
            </Field>
            <Field data-invalid={!validX || undefined}>
              <FieldLabel htmlFor={`${id}-x`}>Offset x (pt)</FieldLabel>
              <Input id={`${id}-x`} inputMode="decimal" value={offsetX} onChange={(event) => setOffsetX(event.target.value)} aria-invalid={!validX} disabled={busy || editor.clipboardPending} />
              {!validX && <FieldError>Enter a finite x offset in points.</FieldError>}
            </Field>
            <Field data-invalid={!validY || undefined}>
              <FieldLabel htmlFor={`${id}-y`}>Offset y (pt)</FieldLabel>
              <Input id={`${id}-y`} inputMode="decimal" value={offsetY} onChange={(event) => setOffsetY(event.target.value)} aria-invalid={!validY} disabled={busy || editor.clipboardPending} />
              {!validY && <FieldError>Enter a finite y offset in points.</FieldError>}
            </Field>
          </FieldGroup>
          <DialogFooter className="mx-0 mb-0">
            <Button variant="outline" disabled={busy || editor.clipboardPending} onClick={() => void readClipboard()}>Read clipboard</Button>
            <Button disabled={busy || editor.clipboardPending || !singleDestination || !text.trim() || !validX || !validY} onClick={() => void paste()}>Paste fragment</Button>
          </DialogFooter>
        </>}
        {localBusy && <p role="status">{pending?.action === 'copy' ? 'Preparing fragment…' : 'Working…'}</p>}
        {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
        {status && <p role="status">{status}</p>}
        </div>
      </DialogContent>
    </Dialog>
    {(mode === null || mode === 'copy') && unresolved}
    {mode === null && error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </section>
}
