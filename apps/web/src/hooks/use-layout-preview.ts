import { useMemo, useRef, useState } from 'react'

import type { LayoutGhost } from '@/editor/layout-ghost'
import { selectionSnapshot } from '@/editor/scene-selection'
import { sameContent, type DocumentEditor } from '@/hooks/use-document-editor'
import type { ComputeStop } from '@/lib/compute-stop'
import {
  ComputeStopped,
  LayoutRefusal,
  sameLayoutRequest,
  type DocumentView,
  type LayoutApplied,
  type LayoutPreview,
  type LayoutRequest,
  type Position,
  type SelectionView,
} from '@/lib/document-contract'

/** What the panel shows. Each kind carries the payload its panel section reads. */
export type LayoutView =
  | { kind: 'idle' }
  | { kind: 'pending' }
  | { kind: 'preview'; preview: LayoutPreview }
  | { kind: 'stale' }
  | { kind: 'stopped'; stop: ComputeStop }
  | { kind: 'refused'; refusal: LayoutRefusal }
  | { kind: 'failed'; message: string }
  | { kind: 'applied'; applied: LayoutApplied }

export interface LayoutController {
  view: LayoutView
  /** Proposed positions to draw over the canvas; non-null only for the `preview` view. */
  ghost: LayoutGhost | null
  pending: boolean
  scopeSelection: boolean
  setScopeSelection: (on: boolean) => void
  /** Why Preview is unavailable, or null when it is available. */
  blocked: string | null
  preview: () => Promise<void>
  apply: () => void
  clear: () => void
}

/**
 * The last press and what came back. Nothing here says whether the outcome still applies:
 * `layoutView` judges that during render, against the drawing and selection on screen.
 */
type LayoutState =
  | { kind: 'idle' }
  | { kind: 'pending'; requested: DocumentView; request: LayoutRequest }
  | { kind: 'preview'; generation: number; requested: DocumentView; preview: LayoutPreview }
  | { kind: 'stopped'; generation: number; requested: DocumentView; request: LayoutRequest; stop: ComputeStop }
  | { kind: 'refused'; generation: number; requested: DocumentView; request: LayoutRequest; refusal: LayoutRefusal }
  | { kind: 'failed'; generation: number; requested: DocumentView; request: LayoutRequest; message: string }
  | { kind: 'applied'; generation: number; documentId: string; revision: number; applied: LayoutApplied }

/** The press a preview was made from. */
interface Press {
  generation: number
  requested: DocumentView
  request: LayoutRequest
}

/** What is on screen now: the drawing, the service process, and the request Preview would send. */
interface Now {
  document: DocumentView
  serviceGeneration: number
  request: LayoutRequest | null
}

/** A result made for `requested` at `generation` and `request` still stands for what is on screen. */
function isCurrent(requested: DocumentView, generation: number, request: LayoutRequest, now: Now): boolean {
  return now.request !== null &&
    requested.id === now.document.id &&
    generation === now.serviceGeneration &&
    (requested === now.document || sameContent(requested, now.document)) &&
    requested.revision === now.document.revision &&
    sameLayoutRequest(request, now.request)
}

/** Why nothing can be laid out, or null. A missing placement is named first. An empty selection never means the whole drawing. */
function layoutBlocked(activePlacement: string | null, selected: SelectionView): string | null {
  if (activePlacement === null) return 'Choose a placement to lay out.'
  if (selected.atoms.length === 0 && selected.bonds.length === 0) return 'Select atoms or bonds to lay out.'
  return null
}

/** What the page shows for `state`. A result for another drawing is not shown; a preview for this drawing that no longer applies is `stale`. */
function layoutView(state: LayoutState, now: Now): LayoutView {
  switch (state.kind) {
    case 'idle':
      return { kind: 'idle' }
    case 'pending':
      return state.requested.id === now.document.id ? { kind: 'pending' } : { kind: 'idle' }
    case 'preview':
      if (state.preview.snapshot.id !== now.document.id) return { kind: 'idle' }
      return isCurrent(state.preview.snapshot, state.generation, state.preview.request, now)
        ? { kind: 'preview', preview: state.preview }
        : { kind: 'stale' }
    case 'stopped':
      return isCurrent(state.requested, state.generation, state.request, now)
        ? { kind: 'stopped', stop: state.stop }
        : { kind: 'idle' }
    case 'refused':
      return isCurrent(state.requested, state.generation, state.request, now)
        ? { kind: 'refused', refusal: state.refusal }
        : { kind: 'idle' }
    case 'failed':
      return isCurrent(state.requested, state.generation, state.request, now)
        ? { kind: 'failed', message: state.message }
        : { kind: 'idle' }
    case 'applied':
      return state.documentId === now.document.id &&
        state.revision === now.document.revision &&
        state.generation === now.serviceGeneration
        ? { kind: 'applied', applied: state.applied }
        : { kind: 'idle' }
    default: {
      const unreachable: never = state
      throw new Error(`Unknown layout state: ${JSON.stringify(unreachable)}`)
    }
  }
}

/** A failed preview: a stop and a refusal keep their own panel sections, anything else is a message. */
function failedPreview(error: unknown, press: Press): LayoutState {
  if (error instanceof ComputeStopped) return { kind: 'stopped', ...press, stop: error.stop }
  if (error instanceof LayoutRefusal) return { kind: 'refused', ...press, refusal: error }
  return { kind: 'failed', ...press, message: error instanceof Error ? error.message : 'Layout failed.' }
}

/**
 * The chain and ring layout of the drawing on screen: the preview, its currency, and the apply.
 * The Editor calls it once, so the canvas ghost and the panel read the same state. Nothing is
 * stored as current; every judgement is made during render.
 */
export function useLayoutPreview({ document, editor, activePlacement }: {
  document: DocumentView
  editor: DocumentEditor
  activePlacement: string | null
}): LayoutController {
  const [state, setState] = useState<LayoutState>({ kind: 'idle' })
  const [scopeSelection, setScopeSelection] = useState(false)
  // Counts presses and clears, so only the reply to the latest press is written.
  const pressCount = useRef(0)

  const selected = selectionSnapshot(editor.selection, document)
  const blocked = layoutBlocked(activePlacement, selected)
  const currentRequest: LayoutRequest | null = activePlacement === null || blocked !== null ? null : {
    documentId: document.id,
    instance: activePlacement,
    scope: scopeSelection ? 'selection' : 'components',
    atoms: selected.atoms,
    bonds: selected.bonds,
  }
  const now: Now = { document, serviceGeneration: editor.serviceGeneration, request: currentRequest }
  const view = layoutView(state, now)

  const shownPreview = view.kind === 'preview' ? view.preview : null
  const ghost = useMemo<LayoutGhost | null>(() => shownPreview === null ? null : {
    instance: shownPreview.request.instance,
    positions: new Map(shownPreview.positions.map((p): [string, Position] => [p.atom, p.after])),
  }, [shownPreview])

  async function preview(): Promise<void> {
    if (currentRequest === null) return
    const press: Press = { generation: editor.serviceGeneration, requested: document, request: currentRequest }
    const pressed = ++pressCount.current
    setState({ kind: 'pending', requested: press.requested, request: press.request })
    let answer: LayoutPreview
    try {
      answer = await editor.previewLayout(press.request)
    } catch (error: unknown) {
      if (pressed === pressCount.current) setState(failedPreview(error, press))
      return
    }
    if (pressed !== pressCount.current) return
    setState({ kind: 'preview', generation: press.generation, requested: press.requested, preview: answer })
  }

  function apply(): void {
    // A page notice shows an apply failure; the preview stays as it was, so its overlay stays too.
    if (view.kind !== 'preview' || state.kind !== 'preview') return
    const { generation, preview: previewed } = state
    void editor.applyLayout(previewed, generation, (applied) => setState({
      kind: 'applied',
      generation,
      documentId: applied.response.document.id,
      revision: applied.response.document.revision,
      applied,
    }))
  }

  function clear(): void {
    pressCount.current += 1
    setState({ kind: 'idle' })
  }

  return {
    view,
    ghost,
    pending: view.kind === 'pending',
    scopeSelection,
    setScopeSelection,
    blocked,
    preview,
    apply,
    clear,
  }
}
