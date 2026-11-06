import { useEffect, useId, useRef, useState } from 'react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import type { DocumentEditor, NativeSceneClipboardCutPlan } from '@/hooks/use-document-editor'
import { MAX_NATIVE_CLIPBOARD_BYTES, settingsOf, type DocumentView, type NativeSceneClipboardCopy } from '@/lib/document-contract'

interface SceneClipboardPanelProps {
  document: DocumentView
  editor: Pick<DocumentEditor, 'busy' | 'serviceGeneration' | 'currentServiceGeneration' | 'copySceneGraphics' | 'prepareSceneCut' | 'cutSceneGraphics' | 'pasteSceneGraphics' | 'clipboardAction' | 'clipboardPending' | 'checkClipboardPaste' | 'clipboardNeedsReload' | 'reconcileClipboardPaste'>
}

interface CopyOffer {
  generation: number
  result: NativeSceneClipboardCopy
  cut?: NativeSceneClipboardCutPlan
}

interface PendingAction {
  generation: number
  action: 'copy' | 'cut' | 'read' | 'write' | 'paste' | 'check' | 'reconcile'
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : 'The graphics clipboard request failed.'
}

/** Explicit plain-text transport for the bounded graphic/group forest profile. */
export function SceneClipboardPanel({ document, editor }: SceneClipboardPanelProps) {
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
  const currentOffer = offer?.generation === editor.serviceGeneration ? offer.result : null
  const x = Number(offsetX)
  const y = Number(offsetY)
  const validX = offsetX.trim() !== '' && Number.isFinite(x)
  const validY = offsetY.trim() !== '' && Number.isFinite(y)
  const textFits = new TextEncoder().encode(text).length <= MAX_NATIVE_CLIPBOARD_BYTES

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

  async function prepareCopy(action: 'copy' | 'cut' = 'copy') {
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setMode(action)
    setOffer(null)
    setError(null)
    setStatus(null)
    setPending({ generation, action: 'copy' })
    try {
      // Invoke immediately so the hook captures the complete press-time selection.
      if (action === 'cut') {
        const cut = await editor.prepareSceneCut(document.id)
        if (current(sequence, generation)) setOffer({ generation, result: cut.copy, cut })
      } else {
        const result = await editor.copySceneGraphics(document.id)
        if (current(sequence, generation)) setOffer({ generation, result })
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
        throw new Error('Browser clipboard writing is unavailable. Select and copy the graphics text manually.')
      }
      // The second explicit click retains activation; no backend await precedes writing.
      await navigator.clipboard.writeText(currentOffer.payload)
      if (current(sequence, generation)) setStatus('Native graphics text copied to clipboard.')
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(cause instanceof Error && cause.message.includes('unavailable')
        ? cause.message : 'Clipboard writing was denied or failed. Select and copy the graphics text manually.')
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
        throw new Error('Browser clipboard reading is unavailable. Paste native graphics text manually.')
      }
      const value = await navigator.clipboard.readText()
      if (!current(sequence, generation)) return
      if (new TextEncoder().encode(value).length > MAX_NATIVE_CLIPBOARD_BYTES) {
        setError('Native graphics text exceeds 64 KiB. Clipboard text was not loaded.')
        return
      }
      setText(value)
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(cause instanceof Error && cause.message.includes('unavailable')
        ? cause.message : 'Clipboard reading was denied or failed. Paste native graphics text manually.')
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  async function cutClipboard() {
    if (!currentOffer || !offer?.cut || busy) return
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setPending({ generation, action: 'cut' })
    setError(null)
    setStatus(null)
    let writeFailed = false
    try {
      if (typeof navigator.clipboard?.writeText !== 'function') {
        throw new Error('Browser clipboard writing is unavailable. The graphics were retained; copy the native text manually.')
      }
      // Invoke in the click; the hook owns the write-before-delete queue and frozen scope.
      const write = navigator.clipboard.writeText(currentOffer.payload).catch((cause: unknown) => {
        writeFailed = true
        throw cause
      })
      await editor.cutSceneGraphics(offer.cut, write)
      if (current(sequence, generation)) close()
    } catch (cause: unknown) {
      if (current(sequence, generation)) setError(writeFailed
        ? 'Clipboard writing was denied or failed. The graphics were retained; copy the native text manually.'
        : errorMessage(cause))
    } finally {
      if (current(sequence, generation)) setPending(null)
    }
  }

  async function paste() {
    if (busy || editor.clipboardPending || !text.trim() || !textFits || !validX || !validY) return
    const sequence = ++request.current
    const generation = editor.serviceGeneration
    setPending({ generation, action: 'paste' })
    setError(null)
    try {
      await editor.pasteSceneGraphics(document.id, text, { x, y })
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

  const cutting = editor.clipboardAction === 'scene_cut'
  const unresolved = editor.clipboardPending && (editor.clipboardAction === 'scene_paste' || cutting) && <Alert>
    <AlertDescription className="flex flex-col gap-2">
      <p>{editor.clipboardNeedsReload
        ? `The original graphics ${cutting ? 'Cut' : 'paste'} can no longer be confirmed by replay. ${cutting ? 'The source may already have been removed.' : 'It may already be present.'} Reload the document to reconcile its outcome.`
        : `The graphics ${cutting ? 'Cut' : 'paste'} outcome is not confirmed. Check the original attempt before making another change.`}</p>
      <Button size="sm" variant="outline" disabled={localBusy} onClick={() => void checkPaste(editor.clipboardNeedsReload)}>
        {editor.clipboardNeedsReload ? 'Reload document to reconcile' : cutting ? 'Check graphics Cut outcome' : 'Check graphics paste outcome'}
      </Button>
    </AlertDescription>
  </Alert>

  return <section aria-label="Scene graphics clipboard" className="flex min-w-0 flex-col gap-3 rounded-lg border p-3">
    <h3 className="text-sm font-medium">Scene graphics clipboard</h3>
    <p className="text-xs text-muted-foreground">Copy or cut selected lines, arrows, rectangles and groups with their internal object references. Mixed atom/bond selections, molecule placements, outgoing references outside the forest and groups with fewer than two children are refused. Cut also refuses references from outside the forest. Paste requires matching document style. External graphics formats are not available.</p>
    <Dialog open={mode !== null} onOpenChange={(open) => { if (!open) close() }}>
      <div className="flex flex-wrap gap-2">
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy} />} onClick={() => void prepareCopy()}>Copy graphics…</DialogTrigger>
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy} />} onClick={() => void prepareCopy('cut')}>Cut graphics…</DialogTrigger>
        <DialogTrigger render={<Button size="sm" variant="outline" disabled={busy} />} onClick={() => openPaste()}>Paste graphics…</DialogTrigger>
      </div>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col overflow-hidden transition-none sm:max-w-lg">
        <DialogHeader className="shrink-0 pr-8">
          <DialogTitle>{mode === 'cut' ? 'Cut native graphics' : mode === 'copy' ? 'Copy native graphics' : 'Paste native graphics'}</DialogTitle>
        </DialogHeader>
        <div className="-mx-4 flex min-h-0 flex-col gap-4 overflow-x-hidden overflow-y-auto px-4 pb-4">
          <DialogDescription>{mode !== 'paste'
            ? mode === 'cut' ? 'Cut to clipboard writes this captured forest first, then removes its roots only if the source drawing is unchanged. Clipboard writing and deletion are separate: a refused deletion leaves a copy. Manual copying never authorizes deletion. Undo changes the document only.' : 'This text contains the captured graphics and groups. Select it for manual copying if browser clipboard access is unavailable.'
            : 'Paste Chemifree native graphics text below. Reading the browser clipboard requires your click. Text is sent to the local service only when you press Paste graphics. Graph-fragment and external-format text are not accepted here.'}</DialogDescription>
          {unresolved}
          {mode !== 'paste' ? <>
            {currentOffer && <p role="status">{currentOffer.roots.length} roots, {currentOffer.copied.objects.length} objects, revision {currentOffer.snapshot.document.revision}.</p>}
            {(offer && !currentOffer || pending !== null && pending.generation !== editor.serviceGeneration) && <p>The service changed. Close this dialog and copy the graphics again.</p>}
            <FieldGroup><Field>
              <FieldLabel htmlFor={`${id}-copy`}>Native graphics text</FieldLabel>
              <Textarea ref={copyText} id={`${id}-copy`} className="field-sizing-fixed max-h-48 resize-y" value={currentOffer?.payload ?? ''} readOnly rows={7} spellCheck={false} onFocus={(event) => event.currentTarget.select()} />
            </Field></FieldGroup>
            <DialogFooter className="mx-0 mb-0">
              <Button variant="outline" disabled={busy || !currentOffer} onClick={() => {
                copyText.current?.focus()
                copyText.current?.select()
              }}>Select native graphics text</Button>
              <Button variant="outline" disabled={busy || !currentOffer} onClick={() => void writeClipboard()}>Copy to clipboard</Button>
              {mode === 'cut' && <Button disabled={busy || !currentOffer || !offer?.cut} onClick={() => void cutClipboard()}>Cut to clipboard</Button>}
            </DialogFooter>
          </> : <>
            <p>Offsets in points are added to source coordinates. New roots append to the page, even when a group is selected. Graphics may overlap existing content; viewport position and molecule placement do not change the offset.</p>
            <FieldGroup>
              <Field data-invalid={!textFits || undefined}>
                <FieldLabel htmlFor={`${id}-paste`}>Native graphics text</FieldLabel>
                <Textarea id={`${id}-paste`} className="field-sizing-fixed max-h-48 resize-y" value={text} onChange={(event) => setText(event.target.value)} disabled={busy || editor.clipboardPending} aria-invalid={!textFits} rows={7} spellCheck={false} />
                {!textFits && <FieldError>Native graphics text exceeds 64 KiB.</FieldError>}
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
              <Button disabled={busy || editor.clipboardPending || !text.trim() || !textFits || !validX || !validY} onClick={() => void paste()}>Paste graphics</Button>
            </DialogFooter>
          </>}
          {localBusy && <p role="status">{pending?.action === 'copy' ? 'Preparing graphics…' : 'Working…'}</p>}
          {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
          {status && <p role="status">{status}</p>}
        </div>
      </DialogContent>
    </Dialog>
    {mode === null && unresolved}
    {mode === null && error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
  </section>
}
