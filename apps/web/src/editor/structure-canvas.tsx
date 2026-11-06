import { memo, useEffect, useEffectEvent, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'

import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Field, FieldLabel } from '@/components/ui/field'
import { marqueeSelection, sceneOnlySelection, selectionGeometry, selectionMove, type SelectionGeometry } from '@/editor/selection-geometry'
import { useCanvasViewport } from '@/hooks/use-canvas-viewport'
import { placements, settingsOf, type AtomView, type Command, type DocumentView, type LabelFont, type Position, type SceneObject } from '@/lib/document-contract'
import { placeNewAtoms, placedView, placementAt } from '@/editor/placement'
import { clickBond, drawBond, type BondEnd, type DrawnOrder } from '@/editor/bond-gestures'
import { AgreementRings } from '@/editor/agreement-rings'
import { isMarkKind, type MarkIntent } from '@/editor/mark-gestures'
import { CipLabelLayer } from '@/editor/cip-label-layer'
import { GroupLabelLayer } from '@/editor/group-label-layer'
import { groupLabelAt, groupLabelBoxes, type GroupMark } from '@/editor/stereo-groups'
import { labelAt, type CipMark } from '@/editor/cip-labels'
import type { AgreementRing, ItemRef } from '@/editor/stereo-agreement'
import { StrokeShapes } from '@/editor/stroke-shapes'
import {
  atomIndex,
  bondName,
  bondRings,
  bondSides,
  bondSpacing,
  bondStrokes,
  bondUnderlay,
  degrees,
  distance,
  drawingStrokes,
  freeNeighborPosition,
  graphicUnderlay,
  hitTest,
  leaningRight,
  placementFrame,
  round,
  sameHit,
  snappedEndpoint,
  strokeReach,
  viewBox,
  type Hit,
  type Stroke,
} from '@/editor/geometry'
import { atomLabel, atomName, labelStyleOf, placed, type AtomLabel, type Box, type LabelChunk } from '@/editor/label'

export type { DrawnOrder } from '@/editor/bond-gestures'

/** What the bond tool draws: a kind of bond, or a stereo mark on a single bond. */
export type DrawnKind = DrawnOrder | 'wedge' | 'hash' | 'wavy'

export type Tool = 'select' | 'atom' | 'bond' | 'erase' | 'pan' | 'ring'

/** Below this pointer travel in CSS pixels a press is a click, not a drag. */
const DRAG_THRESHOLD = 3

/**
 * Gestures act in the scene instance (placement) where they start; `instance` is null for a
 * service without scene instances.
 */
type Drag =
  | { kind: 'move'; start: Position; selected: Set<string>; instance: string | null; scope: SelectionGeometry }
  | { kind: 'bond'; start: Position; fromAtom: string | null; instance: string | null }
  | { kind: 'marquee'; start: Position; additive: boolean; pressed: Set<string>; instance: string | null }
  | { kind: 'click'; start: Position; hit: Hit; instance: string | null }

type Preview =
  | { kind: 'move'; scope: SelectionGeometry; delta: Position }
  | { kind: 'bond'; from: Position; to: Position }
  | { kind: 'marquee'; from: Position; to: Position }
  | null

/** What is under the pointer, and the placement it belongs to. */
interface Located {
  hit: Hit
  instance: string | null
}

/**
 * One placement of the document, drawn with its own atom positions, and everything derived
 * from it once per document rather than per render: label layouts (relative to each atom;
 * which side stated hydrogens take depends on the placement's bonds), the side of each bond's
 * inner line, every bond's strokes, and its frame when there are several placements. The
 * document's settings give the label style, the stroke width, and the spacing of bond lines.
 */
interface Drawing {
  id: string | null
  view: DocumentView
  atoms: ReadonlyMap<string, AtomView>
  labels: ReadonlyMap<string, AtomLabel | null>
  sides: ReadonlyMap<string, 1 | -1>
  strokes: ReadonlyMap<string, Stroke[]>
  frame: ReturnType<typeof placementFrame> | null
  lineWidth: number
  spacing: number
  reach: number
  font: LabelFont
}

const sameLocated = (a: Located | null, b: Located | null) =>
  a === b || (a !== null && b !== null && a.instance === b.instance && sameHit(a.hit, b.hit))

/** A missing, non-finite or singular screen transform cannot locate an edit. */
function pointerPosition(svg: SVGSVGElement | null, event: { clientX: number; clientY: number }): Position | null {
  const matrix = svg?.getScreenCTM()
  if (!matrix || ![matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f].every(Number.isFinite) || matrix.a * matrix.d - matrix.b * matrix.c === 0) return null
  try {
    const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse())
    return Number.isFinite(point.x) && Number.isFinite(point.y) ? { x: round(point.x), y: round(point.y) } : null
  } catch {
    // A detached or singular SVG has no document-space pointer location.
    return null
  }
}

interface StructureCanvasProps {
  document: DocumentView
  tool: Tool
  element: string
  bondKind: DrawnKind
  /** Selected scene objects, atoms and bonds; graph movement uses the active placement. */
  selection: ReadonlySet<string>
  onSelectionChange: (selection: Set<string>) => void
  activePlacement: string | null
  onActivePlacementChange: (instance: string | null) => void
  onCommands: (commands: Command[], selectionBefore?: ReadonlySet<string>) => void
  /** A stereo-mark gesture, built from the document on screen; the parent sends it to the latest. */
  onMark: (intent: MarkIntent) => void
  /** An explicit center click; the shared Rust command owns graph and geometry generation. */
  onRing?: (instance: string | null, center: Position) => void
  /** Rings for the items whose stored configuration the drawing does not agree with; none while it is unknown. */
  rings?: readonly AgreementRing[]
  /** CIP labels to draw over the atoms and double bonds they are about; an overlay, never part of the document. */
  cipLabels?: readonly CipMark[]
  /** Group labels to draw beside the members of enhanced stereo groups; an overlay, never part of the document. */
  groupLabels?: readonly GroupMark[]
  /** Called when a pointer gesture starts (true) and ends (false). */
  onDragging?: (dragging: boolean) => void
  disabled: boolean
}

/** A stable empty list, so a canvas without group labels does not recompute its view box on every render. */
const NO_GROUP_LABELS: readonly GroupMark[] = []

/**
 * Renders a document as SVG, every scene instance at its own positions, and turns pointer
 * gestures into document commands. It holds only transient gesture state; the document itself
 * always comes from the API.
 */
export function StructureCanvas({
  document,
  tool,
  element,
  bondKind,
  selection,
  onSelectionChange,
  activePlacement,
  onActivePlacementChange,
  onRing,
  onCommands,
  onMark,
  rings = [],
  cipLabels = [],
  groupLabels = NO_GROUP_LABELS,
  onDragging,
  disabled,
}: StructureCanvasProps) {
  const svgRef = useRef<SVGSVGElement>(null)
  const drag = useRef<Drag | null>(null)
  const pointer = useRef<{ id: number; start: Position; last: Position; pan: boolean } | null>(null)
  const space = useRef(false)
  const setDrag = (next: Drag | null) => { drag.current = next }
  const [interacting, setInteracting] = useState(false)
  const [preview, setPreview] = useState<Preview>(null)
  const [hover, setHover] = useState<Located | null>(null)
  const selectedGeometry = selectionGeometry(document, selection, activePlacement)

  function switchPlacement(instance: string) {
    cancelPointer()
    if (instance !== activePlacement) onSelectionChange(sceneOnlySelection(document, selection))
    onActivePlacementChange(instance)
  }


  // Derived per document, not per render: pointer moves re-render the canvas.
  const atoms = useMemo(() => atomIndex(document), [document])
  const bondLength = settingsOf(document).bond_length_pt
  // One label layout per atom and placement: what is drawn, and the box that bond trimming,
  // the view box, and hit testing all use.
  const drawings = useMemo<Drawing[]>(() => {
    const settings = settingsOf(document)
    const style = labelStyleOf(settings)
    const lineWidth = settings.line_width_pt
    const spacing = bondSpacing(lineWidth)
    const reach = strokeReach(lineWidth, spacing) + 3
    const counts = degrees(document)
    const all = placements(document)
    const order = document.scene?.objects?.filter((object) => object.kind === 'instance').map((object) => object.id)
    if (order) all.sort((a, b) => order.indexOf(a.id ?? '') - order.indexOf(b.id ?? ''))
    // Rings depend on the graph alone: searched once for every placement.
    const rings = bondRings(document)
    // A label depends on the atom and the side its hydrogens take, not on the placement.
    const layouts = new Map<string, AtomLabel | null>()
    const layoutOf = (atom: AtomView, left: boolean) => {
      const key = `${atom.id}:${left}`
      if (!layouts.has(key)) layouts.set(key, atomLabel(atom, counts.get(atom.id) ?? 0, left, style))
      return layouts.get(key) ?? null
    }
    return all.map((placement) => {
      const view = placedView(document, placement)
      const index = atomIndex(view)
      const left = leaningRight(view)
      const labels = new Map<string, AtomLabel | null>(view.atoms.map((atom) => [atom.id, layoutOf(atom, left.has(atom.id))]))
      const sides = bondSides(view, index, rings)
      const frame =
        all.length > 1 && placement.id !== null && view.atoms.length > 0
          ? placementFrame(
              view.atoms.map((atom) => atom.position),
              view.atoms.flatMap((atom) => labelBox(labels, atom.id, atom.position) ?? []),
              placement.id,
              reach,
            )
          : null
      return { id: placement.id, view, atoms: index, labels, sides, strokes: drawingStrokes(view, labels, index, sides, spacing, lineWidth), frame, lineWidth, spacing, reach, font: style.font }
    })
  }, [document])
  // The view box holds every atom, label box, and placement frame (with its ID).
  const box = useMemo(
    () =>
      viewBox(
        [
          ...drawings.flatMap((drawing) => drawing.view.atoms.map((atom) => atom.position)),
          ...(document.scene?.objects?.flatMap((object) => object.kind === 'graphic' ? object.points : []) ?? []),
        ],
        drawings.flatMap((drawing) => [
          ...drawing.view.atoms.flatMap((atom) => labelBox(drawing.labels, atom.id, atom.position) ?? []),
          ...(drawing.frame ? [drawing.frame.box] : []),
          // Group labels are an overlay, but a clipped one is unreadable: they count only while drawn.
          ...groupLabelBoxes(groupLabels, document.bonds, new Map(drawing.view.atoms.map((atom) => [atom.id, atom.position]))),
        ]),
        drawings[0]?.reach ?? 0,
      ),
    [document.scene, document.bonds, drawings, groupLabels],
  )

  const { containerRef, viewport, viewBox: canvasViewBox, zoom, fit, pan } = useCanvasViewport(document.id, box)
  const unitsPerPixel = 1 / viewport.scale

  function drawingOf(instance: string | null) {
    return drawings.find((drawing) => drawing.id === instance) ?? drawings[0]
  }

  /** Later placements are drawn on top, so they are hit first. */
  function locate(point: Position, ignore?: ReadonlySet<string>, only?: string | null): Located {
    for (const drawing of [...drawings].reverse()) {
      if (only !== undefined && drawing.id !== only) continue
      const hit = hitTest(drawing.view, point, ignore, drawing.atoms, drawing.labels, unitsPerPixel, drawing.strokes)
      if (hit) return { hit, instance: drawing.id }
    }
    return { hit: null, instance: only === undefined ? placementAt(document, point) : only }
  }

  /** The command adding an unbonded atom of `element` at `point`, placed in every placement. */
  function addAtom(element: string, instance: string | null, point: Position): Command {
    const [placement] = placeNewAtoms(document, instance, [{ point }])
    return { op: 'add_atom', element, ...placement }
  }

  function toDocument(event: { clientX: number; clientY: number }): Position | null {
    return pointerPosition(svgRef.current, event)
  }

  function cancelPointer() {
    const active = pointer.current
    pointer.current = null
    setInteracting(false)
    setDrag(null)
    setPreview(null)
    if (active) {
      onDragging?.(false)
      const svg = svgRef.current
      if (svg?.hasPointerCapture(active.id)) svg.releasePointerCapture(active.id)
    }
  }

  const cancelFromWindow = useEffectEvent(() => cancelPointer())

  useEffect(() => {
    function onBlur() { space.current = false; cancelFromWindow() }
    function keyDown(event: globalThis.KeyboardEvent) {
      if (event.key !== ' ' || event.ctrlKey || event.metaKey || event.altKey) return
      const target = event.target
      if (target instanceof Element && target.closest('input, textarea, select, button, [role="combobox"], [role="listbox"], [role="option"], [contenteditable="true"], [role="dialog"], [role="alertdialog"]')) return
      if (window.document.querySelector('[role="dialog"], [role="alertdialog"]')) return
      event.preventDefault()
      space.current = true
    }
    function keyUp(event: globalThis.KeyboardEvent) { if (event.key === ' ') space.current = false }
    window.addEventListener('blur', onBlur)
    window.addEventListener('keydown', keyDown)
    window.addEventListener('keyup', keyUp)
    return () => {
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('keydown', keyDown)
      window.removeEventListener('keyup', keyUp)
      space.current = false
      if (pointer.current) onDragging?.(false)
      pointer.current = null
      drag.current = null
    }
  }, [onDragging])

  useEffect(() => {
    const svg = svgRef.current
    if (!svg) return
    const canvas = svg
    function wheel(event: WheelEvent) {
      if (!event.ctrlKey && !event.metaKey) return
      event.preventDefault()
      if (pointer.current || !pointerPosition(canvas, event)) return
      const rect = canvas.getBoundingClientRect()
      const pixels = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? rect.height : 1)
      zoom(Math.exp(-Math.max(-500, Math.min(500, pixels)) * 0.002), { x: event.clientX - rect.left, y: event.clientY - rect.top })
    }
    // React's wheel listener is passive; browser zoom must be prevented for this gesture.
    svg.addEventListener('wheel', wheel, { passive: false })
    return () => svg.removeEventListener('wheel', wheel)
  }, [zoom])

  function onKeyDown(event: KeyboardEvent<SVGSVGElement>) {
    if (event.ctrlKey || event.metaKey || event.altKey) return
    const key = event.key.toLowerCase()
    if (key === ' ') {
      event.preventDefault()
      space.current = true
      return
    }
    if (key === 'escape') { cancelPointer(); return }
    if (pointer.current) return
    if (key === '+' || key === '=') zoom(1.25)
    else if (key === '-') zoom(0.8)
    else if (key === 'f') fit()
    else if (key === '0') zoom(1 / viewport.scale)
    else if (key === 'arrowleft') pan({ x: 40, y: 0 })
    else if (key === 'arrowright') pan({ x: -40, y: 0 })
    else if (key === 'arrowup') pan({ x: 0, y: 40 })
    else if (key === 'arrowdown') pan({ x: 0, y: -40 })
    else return
    event.preventDefault()
    event.stopPropagation()
  }

  /** Capture navigation before a graphic's own selection handler can see the press. */
  function onPointerDownCapture(event: PointerEvent<SVGSVGElement>) {
    if (pointer.current || disabled || (event.button !== 0 && event.button !== 1)) {
      event.stopPropagation()
      return
    }
    if (!toDocument(event)) { event.stopPropagation(); return }
    const pan = event.button === 1 || space.current || tool === 'pan'
    const start = { x: event.clientX, y: event.clientY }
    pointer.current = { id: event.pointerId, start, last: start, pan }
    try {
      event.currentTarget.setPointerCapture(event.pointerId)
    } catch {
      pointer.current = null
      event.stopPropagation()
      return
    }
    event.currentTarget.focus({ preventScroll: true })
    setInteracting(true)
    onDragging?.(true)
    if (pan) {
      event.preventDefault()
      event.stopPropagation()
      setHover(null)
    }
  }

  function selectTarget(id: string, kind: 'atom' | 'bond' | 'graphic', instance: string | null, point: Position, additive: boolean) {
    const changingPlacement = kind !== 'graphic' && instance !== activePlacement
    const current = changingPlacement ? sceneOnlySelection(document, selection) : new Set(selection)
    if (kind !== 'graphic') onActivePlacementChange(instance)
    const scope = selectionGeometry(document, current, instance)
    const positions = scope?.positions.get(instance)
    const bond = kind === 'bond' ? document.bonds.find((bond) => bond.id === id) : undefined
    const covered = kind === 'graphic' ? scope?.graphics.has(id)
      : kind === 'atom' ? positions?.has(id)
      : bond !== undefined && positions?.has(bond.begin) && positions.has(bond.end)
    let next = current
    const deselected = additive && next.has(id)
    if (additive) {
      if (deselected) next.delete(id)
      else next.add(id)
    } else if (!covered) {
      next = new Set([id])
    }
    onSelectionChange(next)
    const moving = selectionGeometry(document, next, instance)
    if (!deselected && moving) setDrag({ kind: 'move', start: point, selected: new Set(next), instance, scope: moving })
  }

  function onPointerDown(event: PointerEvent<SVGSVGElement>) {
    if (pointer.current?.id !== event.pointerId || pointer.current.pan) return
    const point = toDocument(event)
    if (!point) return
    const { hit, instance } = locate(point)
    if (tool === 'select') {
      if (!hit) {
        setDrag({ kind: 'marquee', start: point, additive: event.shiftKey, pressed: new Set(selection), instance: activePlacement })
        return
      }
      selectTarget(hit.id, hit.kind, instance, point, event.shiftKey)
    } else if (tool === 'bond' && hit?.kind !== 'bond') {
      setDrag({ kind: 'bond', start: point, fromAtom: hit?.kind === 'atom' ? hit.id : null, instance })
    } else {
      setDrag({ kind: 'click', start: point, hit, instance })
    }
  }

  /**
   * The bond a drag makes, within the placement it started in. Null when the gesture's start
   * atom no longer exists (e.g. undone mid-drag).
   */
  function bondTarget(gesture: Extract<Drag, { kind: 'bond' }>, point: Position) {
    const drawing = drawingOf(gesture.instance)
    const from = gesture.fromAtom ? drawing.atoms.get(gesture.fromAtom) : undefined
    if (gesture.fromAtom && !from) return null
    const origin = from?.position ?? gesture.start
    const { hit: target } = locate(point, new Set(gesture.fromAtom ? [gesture.fromAtom] : []), gesture.instance)
    const targetAtom = target?.kind === 'atom' ? drawing.atoms.get(target.id) : undefined
    if (targetAtom) return { origin, atom: targetAtom.id, position: targetAtom.position }
    return { origin, atom: null, position: snappedEndpoint(origin, point, bondLength) }
  }

  function onPointerMove(event: PointerEvent<SVGSVGElement>) {
    const active = pointer.current
    if (active && active.id !== event.pointerId) return
    if (active?.pan) {
      pan({ x: event.clientX - active.last.x, y: event.clientY - active.last.y })
      active.last = { x: event.clientX, y: event.clientY }
      return
    }
    const point = toDocument(event)
    if (!point) { if (active) cancelPointer(); return }
    const gesture = drag.current
    if (!gesture) {
      const located = disabled ? null : locate(point)
      const next = located?.hit ? located : null
      setHover((current) => (sameLocated(current, next) ? current : next))
      return
    }
    if (!active || distance({ x: event.clientX, y: event.clientY }, active.start) < DRAG_THRESHOLD) return
    if (gesture.kind === 'move') {
      setPreview({
        kind: 'move',
        scope: gesture.scope,
        delta: { x: round(point.x - gesture.start.x), y: round(point.y - gesture.start.y) },
      })
    } else if (gesture.kind === 'bond') {
      const target = bondTarget(gesture, point)
      setPreview(target && { kind: 'bond', from: target.origin, to: target.position })
    } else if (gesture.kind === 'marquee') {
      setPreview({ kind: 'marquee', from: gesture.start, to: point })
    }
  }

  function finish(event: PointerEvent<SVGSVGElement>) {
    const active = pointer.current
    if (!active || active.id !== event.pointerId) return
    const gesture = drag.current
    const point = toDocument(event)
    cancelPointer()
    // Busy is checked when a gesture starts, not here: a request queued meanwhile (e.g. a
    // field's blur) must not swallow it. The parent applies the commands to the latest
    // document and drops them if what they name is gone.
    if (active.pan || !gesture || !point) return
    const dragged = distance({ x: event.clientX, y: event.clientY }, active.start) >= DRAG_THRESHOLD
    if (tool === 'ring') {
      if (!dragged) onRing?.(gesture.instance, gesture.start)
      return
    }
    const intent = markIntent(gesture, point, dragged)
    if (intent) {
      onMark(intent)
      return
    }
    const commands = gestureCommands(gesture, point, dragged)
    if (commands.length > 0) onCommands(commands, gesture.kind === 'move' ? gesture.selected : undefined)
  }

  /**
   * What a bond tool gesture means when a mark is selected: null for every other gesture. The
   * commands are built from the latest document when the request runs (see `buildMark`).
   */
  function markIntent(gesture: Drag, point: Position, dragged: boolean): MarkIntent | null {
    if (tool !== 'bond' || !isMarkKind(bondKind)) return null
    if (gesture.kind === 'bond') {
      const ends = bondEnds(gesture, point, dragged)
      return ends && { kind: 'draw', instance: gesture.instance, from: ends.from, to: ends.to, mark: bondKind }
    }
    if (gesture.kind === 'click' && !dragged && gesture.hit?.kind === 'bond') {
      return { kind: 'cycle', instance: gesture.instance, bond: gesture.hit.id, mark: bondKind }
    }
    return null
  }

  function gestureCommands(gesture: Drag, point: Position, dragged: boolean): Command[] {
    switch (gesture.kind) {
      case 'move': {
        if (!dragged) return []
        const delta = { x: round(point.x - gesture.start.x), y: round(point.y - gesture.start.y) }
        return selectionMove(document, gesture.selected, gesture.instance, delta) ?? []
      }
      case 'marquee': {
        onSelectionChange(marqueeSelection(document, gesture.instance, gesture.start, point, gesture.pressed, gesture.additive, dragged))
        return []
      }
      case 'bond':
        return bondCommands(gesture, point, dragged)
      case 'click':
        if (dragged) return []
        return clickCommands(gesture.hit, gesture.instance, gesture.start)
    }
  }

  /** Where a bond gesture's bond begins (where the gesture starts) and ends; null if it has no end. */
  function bondEnds(gesture: Extract<Drag, { kind: 'bond' }>, point: Position, dragged: boolean): { from: BondEnd; to: BondEnd } | null {
    const from: BondEnd = gesture.fromAtom ? { atom: gesture.fromAtom } : { point: gesture.start }
    if (!dragged) {
      const end = gesture.fromAtom
        ? freeNeighborPosition(drawingOf(gesture.instance).view, gesture.fromAtom, bondLength)
        : snappedEndpoint(gesture.start, { x: gesture.start.x + 26, y: gesture.start.y - 15 }, bondLength)
      return end && { from, to: { point: end } }
    }
    const target = bondTarget(gesture, point)
    return target && { from, to: target.atom ? { atom: target.atom } : { point: target.position } }
  }

  /** A bond gesture's commands: the bond begins where the gesture starts (see drawBond). */
  function bondCommands(gesture: Extract<Drag, { kind: 'bond' }>, point: Position, dragged: boolean): Command[] {
    const ends = bondEnds(gesture, point, dragged)
    return ends && !isMarkKind(bondKind) ? drawBond(document, gesture.instance, ends.from, ends.to, bondKind) : []
  }

  function clickCommands(hit: Hit, instance: string | null, point: Position): Command[] {
    if (tool === 'erase') {
      if (hit?.kind === 'atom') return [{ op: 'delete_atoms', atoms: [hit.id] }]
      if (hit?.kind === 'bond') return [{ op: 'delete_bonds', bonds: [hit.id] }]
      return []
    }
    if (tool === 'bond' && hit?.kind === 'bond') {
      const bond = document.bonds.find((candidate) => candidate.id === hit.id)
      return bond ? clickBond(document, bond) : []
    }
    if (tool === 'atom') {
      if (hit?.kind === 'atom') {
        const current = atoms.get(hit.id)
        return current?.kind === 'element' && current.element === element ? [] : [{ op: 'update_atom', atom: hit.id, element }]
      }
      if (!hit) return [addAtom(element, instance, point)]
    }
    return []
  }

  /** What a mark drag would draw: the mark on a single bond that begins where the drag started. */
  function previewStrokes(from: Position, to: Position, kind: Extract<DrawnKind, 'wedge' | 'hash' | 'wavy'>): Stroke[] {
    const { lineWidth, spacing } = drawingOf(null)
    const mark = kind === 'wavy' ? { kind } : { kind, narrow: 'begin' as const }
    return bondStrokes({ id: 'preview', begin: 'a', end: 'b', order: 'single', query: null, mark }, from, to, null, null, 1, spacing, lineWidth)
  }

  /** An atom's position in a drawing, moved by the drag preview in the drag's placement. */
  const position = (drawing: Drawing, id: string) => {
    const atom = drawing.atoms.get(id)!
    if (preview?.kind === 'move' && preview.scope.positions.get(drawing.id)?.has(id)) {
      return { x: atom.position.x + preview.delta.x, y: atom.position.y + preview.delta.y }
    }
    return atom.position
  }
  const hovered = (drawing: Drawing, kind: 'atom' | 'bond', id: string) =>
    hover?.instance === drawing.id && hover.hit?.kind === kind && hover.hit.id === id

  /** Where an atom or the middle of a double bond is in one placement, as moved by a drag in progress; null where the placement lacks it. */
  const itemAt = (drawing: Drawing, item: ItemRef): Position | null => {
    if (item.kind === 'atom') return drawing.atoms.has(item.id) ? position(drawing, item.id) : null
    const bond = document.bonds.find((candidate) => candidate.id === item.id)
    if (!bond || !drawing.atoms.has(bond.begin) || !drawing.atoms.has(bond.end)) return null
    const [a, b] = [position(drawing, bond.begin), position(drawing, bond.end)]
    return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
  }
  /** The rings of one placement, each at its atom or the middle of its double bond. */
  const ringsAt = (drawing: Drawing) =>
    rings.flatMap((ring) => {
      if (ring.instance !== drawing.id) return []
      const at = itemAt(drawing, ring.item)
      return at ? [{ ring, at }] : []
    })
  /**
   * The CIP labels of one placement: a label belongs to the atom or bond, so every placement that
   * draws it shows it, beside the item and clear of its bonds (see `labelAt`).
   */
  const cipAt = (drawing: Drawing) =>
    cipLabels.flatMap((label) => {
      const anchor = itemAt(drawing, label.item)
      if (!anchor) return []
      const bonded = document.bonds.flatMap((bond) => {
        if (label.item.kind === 'bond') return bond.id === label.item.id ? [bond.begin, bond.end] : []
        if (bond.begin === label.item.id) return [bond.end]
        return bond.end === label.item.id ? [bond.begin] : []
      })
      const around = bonded.filter((id) => drawing.atoms.has(id)).map((id) => position(drawing, id))
      return [{ label, at: labelAt(label.item.kind, anchor, around) }]
    })

  /** The group labels of one placement: beside each member, stacked with its CIP label (see `groupLabelAt`). */
  const groupAt = (drawing: Drawing) =>
    groupLabels.flatMap((label) => {
      if (!drawing.atoms.has(label.atom)) return []
      const around = document.bonds
        .flatMap((bond) => (bond.begin === label.atom ? [bond.end] : bond.end === label.atom ? [bond.begin] : []))
        .filter((id) => drawing.atoms.has(id))
        .map((id) => position(drawing, id))
      return [{ label, at: groupLabelAt(position(drawing, label.atom), around) }]
    })

  const sceneObjects = document.scene?.objects
  const byId = new Map(sceneObjects?.map((object) => [object.id, object]) ?? [])
  const sceneSelected = (object: SceneObject) => {
    let current: SceneObject | undefined = object
    while (current) {
      if (selection.has(current.id)) return true
      current = current.parent === null ? undefined : byId.get(current.parent)
    }
    return false
  }
  const drawInstance = (drawing: Drawing) => (
    <g key={drawing.id ?? 'atoms'} data-instance={drawing.id ?? undefined}>
      {drawing.frame && <PlacementFrame id={drawing.id ?? ''} frame={drawing.frame} />}
      <g className="fill-primary/15 stroke-primary/40">
        {document.bonds
          .filter((bond) => (drawing.id === activePlacement && selection.has(bond.id)) || (selectedGeometry?.positions.get(drawing.id)?.has(bond.begin) && selectedGeometry.positions.get(drawing.id)?.has(bond.end)) || hovered(drawing, 'bond', bond.id))
          .map((bond) => {
            const a = position(drawing, bond.begin)
            const b = position(drawing, bond.end)
            return <line key={bond.id} data-underlay={bond.id} x1={a.x} y1={a.y} x2={b.x} y2={b.y} strokeWidth={bondUnderlay(bond.order, drawing.lineWidth, drawing.spacing, unitsPerPixel, bond.mark)} strokeLinecap="round" />
          })}
        {document.atoms
          .filter((atom) => selectedGeometry?.positions.get(drawing.id)?.has(atom.id) || hovered(drawing, 'atom', atom.id))
          .map((atom) => {
            const p = position(drawing, atom.id)
            const label = labelBox(drawing.labels, atom.id, p)
            return label ? (
              <rect key={atom.id} {...label} rx={3 * unitsPerPixel} strokeWidth={unitsPerPixel} />
            ) : (
              <circle key={atom.id} cx={p.x} cy={p.y} r={8 * unitsPerPixel} strokeWidth={unitsPerPixel} />
            )
          })}
      </g>
      <DrawingLayer
        drawing={drawing}
        several={drawings.length > 1}
        moved={preview?.kind === 'move' ? { atoms: preview.scope.positions.get(drawing.id) ?? new Set<string>(), delta: preview.delta } : null}
      />
      <AgreementRings rings={ringsAt(drawing)} />
      <CipLabelLayer labels={cipAt(drawing)} />
      <GroupLabelLayer labels={groupAt(drawing)} />
    </g>
  )

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col gap-2">
      <div role="toolbar" aria-label="Canvas view" className="flex flex-wrap items-center gap-1">
        <Button size="sm" variant="outline" disabled={interacting} onClick={() => zoom(1.25)}>Zoom in</Button>
        <Button size="sm" variant="outline" disabled={interacting} onClick={() => zoom(0.8)}>Zoom out</Button>
        <Button size="sm" variant="outline" onClick={() => { cancelPointer(); fit() }}>Fit drawing</Button>
        <Button size="sm" variant="outline" onClick={() => { cancelPointer(); zoom(1 / viewport.scale) }}>Reset zoom (100%)</Button>
        <output aria-label="Zoom level" data-testid="zoom-level" className="px-1 text-xs tabular-nums">{viewport.scale < 0.01 ? Number((viewport.scale * 100).toFixed(3)) : Math.round(viewport.scale * 100)}%</output>
      </div>
      {document.scene && (document.scene.instances.length > 1 || activePlacement === null) && (
        <Field className="min-w-0">
          <FieldLabel htmlFor="canvas-movement-placement">Atom and bond movement placement</FieldLabel>
          <Select value={activePlacement} items={document.scene?.instances.map((instance) => ({ value: instance.id, label: instance.id }))}
            onValueChange={(value) => { if (typeof value === 'string') switchPlacement(value) }}>
            <SelectTrigger id="canvas-movement-placement" aria-label="Atom and bond movement placement" size="sm" className="w-full min-w-0"><SelectValue placeholder="Choose placement" /></SelectTrigger>
            <SelectContent><SelectGroup>
              {document.scene?.instances.map((instance) => <SelectItem key={instance.id} value={instance.id}>{instance.id}</SelectItem>)}
            </SelectGroup></SelectContent>
          </Select>
        </Field>
      )}
      <p className="text-xs text-muted-foreground" role="status">Atom and bond movement: {activePlacement ?? (document.scene ? 'choose a placement' : 'drawing')}. Graph edits affect every placement. Switching placement clears atom and bond selection.</p>
      <div ref={containerRef} className="min-h-0 flex-1 overflow-hidden rounded-lg bg-card ring-1 ring-border">
    <svg
      ref={svgRef}
      role="group"
      data-structure-canvas=""
      aria-label={`${document.title}: ${document.atoms.length} atoms and ${document.bonds.length} bonds`}
      viewBox={canvasViewBox}
      data-viewport-scale={viewport.scale}
      tabIndex={0}
      aria-keyshortcuts="+ - 0 F ArrowLeft ArrowRight ArrowUp ArrowDown Space"
      aria-describedby="canvas-view-help"
      onKeyDown={onKeyDown}
      onKeyUp={(event) => { if (event.key === ' ') { space.current = false; event.preventDefault() } }}
      onBlur={() => { space.current = false; cancelPointer() }}
      className={cn(
        'block h-full w-full touch-none select-none outline-none focus-visible:ring-2 focus-visible:ring-ring',
        tool === 'pan' ? 'cursor-grab' : tool === 'select' ? 'cursor-default' : 'cursor-crosshair',
        disabled && 'cursor-progress',
      )}
      onPointerDownCapture={onPointerDownCapture}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={(event) => { if (pointer.current?.id === event.pointerId) cancelPointer() }}
      onLostPointerCapture={(event) => { if (pointer.current?.id === event.pointerId) cancelPointer() }}
      onPointerLeave={() => setHover(null)}
    >
      {sceneObjects ? sceneObjects.map((object) => {
        if (object.kind === 'group') return null
        if (object.kind === 'graphic') return (
          <SceneGraphic key={object.id} object={preview?.kind === 'move' && preview.scope.graphics.has(object.id) ? {
            ...object, points: [
              { x: object.points[0].x + preview.delta.x, y: object.points[0].y + preview.delta.y },
              { x: object.points[1].x + preview.delta.x, y: object.points[1].y + preview.delta.y },
            ],
          } : object} selected={sceneSelected(object)} lineWidth={settingsOf(document).line_width_pt} unitsPerPixel={unitsPerPixel} onPointerDown={(event) => {
            if (tool !== 'select' || disabled || event.button !== 0 || pointer.current?.id !== event.pointerId) return
            event.stopPropagation()
            const point = toDocument(event)
            if (point) selectTarget(object.id, 'graphic', activePlacement, point, event.shiftKey)
          }} />
        )
        const drawing = drawings.find((item) => item.id === object.id)
        return drawing ? drawInstance(drawing) : null
      }) : drawings.map(drawInstance)}
      {preview?.kind === 'bond' && isMarkKind(bondKind) ? (
        <g className="fill-none stroke-primary" strokeWidth={settingsOf(document).line_width_pt} strokeLinejoin="round" data-bond-preview={bondKind}>
          <StrokeShapes strokes={previewStrokes(preview.from, preview.to, bondKind)} bond="preview" fill="fill-primary stroke-none" />
        </g>
      ) : preview?.kind === 'bond' ? (
        <line
          x1={preview.from.x}
          y1={preview.from.y}
          x2={preview.to.x}
          y2={preview.to.y}
          className="stroke-primary"
          strokeWidth={settingsOf(document).line_width_pt}
          strokeDasharray="3 2"
        />
      ) : null}
      {preview?.kind === 'marquee' && (
        <rect
          x={Math.min(preview.from.x, preview.to.x)}
          y={Math.min(preview.from.y, preview.to.y)}
          width={Math.abs(preview.to.x - preview.from.x)}
          height={Math.abs(preview.to.y - preview.from.y)}
          className="fill-primary/10 stroke-primary/60"
          strokeWidth={0.8}
          strokeDasharray="3 2"
        />
      )}
    </svg>
      </div>
      <p id="canvas-view-help" className="text-xs text-muted-foreground">Ctrl/⌘ + wheel: zoom at pointer. Focus canvas: +/− zoom, 0 reset, F fit, arrows pan. Hold Space or middle-drag to pan; Pan view (H) also works with touch. View changes are not saved. Select: Shift-click toggles; drag empty canvas to select atom centers in the active placement and fully contained graphics. Shift-drag adds to the selection. Select whole placements and groups in Scene.</p>
    </div>
  )
}

function SceneGraphic({ object, selected, lineWidth, unitsPerPixel, onPointerDown }: {
  object: Extract<SceneObject, { kind: 'graphic' }>
  selected: boolean
  lineWidth: number
  unitsPerPixel: number
  onPointerDown: (event: PointerEvent<SVGGElement>) => void
}) {
  const [a, b] = object.points
  const length = Math.hypot(b.x - a.x, b.y - a.y)
  const ux = length > 0 ? (b.x - a.x) / length : 0
  const uy = length > 0 ? (b.y - a.y) / length : 0
  const head = `${b.x},${b.y} ${b.x - ux * 6 - uy * 3},${b.y - uy * 6 + ux * 3} ${b.x - ux * 6 + uy * 3},${b.y - uy * 6 - ux * 3}`
  const rect = { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) }
  const underlay = graphicUnderlay(lineWidth, unitsPerPixel)
  return (
    <g data-scene-object={object.id} onPointerDown={onPointerDown} strokeLinecap="round" strokeLinejoin="round">
      {object.shape === 'rectangle' ? (
        <>
          <rect {...rect} fill="none" stroke="transparent" strokeWidth={Math.max(10 * unitsPerPixel, lineWidth)} />
          {selected && <rect {...rect} fill="none" className="stroke-primary/40" strokeWidth={underlay} />}
          <rect {...rect} fill="none" className="stroke-foreground" strokeWidth={lineWidth} />
        </>
      ) : (
        <>
          <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="transparent" strokeWidth={Math.max(10 * unitsPerPixel, lineWidth)} />
          {selected && <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="stroke-primary/40" strokeWidth={underlay} />}
          <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="stroke-foreground" strokeWidth={lineWidth} />
          {object.shape === 'arrow' && length > 0 && selected && <polygon points={head} className="fill-primary/40 stroke-primary/40" strokeWidth={underlay} />}
          {object.shape === 'arrow' && length > 0 && <polygon points={head} className="fill-foreground stroke-foreground" strokeWidth={lineWidth} />}
        </>
      )}
    </g>
  )
}

/** An atom's label box placed at `at`, or null for a bare vertex. */
function labelBox(labels: ReadonlyMap<string, AtomLabel | null>, id: string, at: Position): Box | null {
  const label = labels.get(id)
  return label ? placed(label.box, at) : null
}

/** A drag preview moving some atoms of one placement by `delta`. */
interface Moved {
  atoms: ReadonlySet<string>
  delta: Position
}

/**
 * One placement's bonds and atom labels. It re-renders only when its drawing changes or atoms
 * of this placement are being dragged, not on hover or selection, which large documents would
 * otherwise pay for on every pointer move.
 */
const DrawingLayer = memo(function DrawingLayer({
  drawing,
  several,
  moved,
}: {
  drawing: Drawing
  several: boolean
  moved: Moved | null
}) {
  const position = (id: string) => {
    const at = drawing.atoms.get(id)!.position
    return moved?.atoms.has(id) ? { x: at.x + moved.delta.x, y: at.y + moved.delta.y } : at
  }
  const placement = several ? drawing.id : null
  const strokesOf = (bond: (typeof drawing.view.bonds)[number]) => {
    if (!moved || !(moved.atoms.has(bond.begin) || moved.atoms.has(bond.end))) return drawing.strokes.get(bond.id) ?? []
    const begin = position(bond.begin)
    const end = position(bond.end)
    return bondStrokes(
      bond,
      begin,
      end,
      labelBox(drawing.labels, bond.begin, begin),
      labelBox(drawing.labels, bond.end, end),
      drawing.sides.get(bond.id),
      drawing.spacing,
      drawing.lineWidth,
    )
  }
  const { font } = drawing
  return (
    <>
      <g
        className="fill-none stroke-foreground"
        strokeWidth={drawing.lineWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        data-bonds=""
      >
        {drawing.view.bonds.map((bond) => (
          <g key={bond.id} role="img" aria-label={bondName(bond, placement)} data-bond-kind={bond.order} data-bond-mark={bond.mark?.kind}>
            <StrokeShapes strokes={strokesOf(bond)} bond={bond.id} />
          </g>
        ))}
      </g>
      {/* The bundled font for sans-serif; the generic family the label boxes were measured in otherwise. */}
      <g
        className={cn('fill-foreground', font === 'sans-serif' && 'font-sans')}
        fontFamily={font === 'sans-serif' ? undefined : font}
        data-labels={font}
      >
        {drawing.view.atoms.map((atom) => {
          const label = drawing.labels.get(atom.id)
          return label ? (
            <AtomMark key={atom.id} id={atom.id} name={atomName(atom, placement)} label={label} at={position(atom.id)} />
          ) : null
        })}
      </g>
    </>
  )
})

/** `box` as an attribute value, so a test can compare it with what the browser draws. */
const boxAttribute = (box: Box) => `${round(box.x)} ${round(box.y)} ${round(box.width)} ${round(box.height)}`

/** A chunk of label text flowing from its anchor; each run is drawn at its natural width. */
function ChunkText({ chunk, at, ...rest }: { chunk: LabelChunk; at: Position; className?: string } & Record<`data-${string}`, string>) {
  return (
    <text
      x={round(at.x + chunk.x)}
      y={round(at.y + chunk.y)}
      textAnchor={chunk.anchor}
      dominantBaseline="central"
      aria-hidden="true"
      data-box={boxAttribute(placed(chunk.box, at))}
      {...rest}
    >
      {chunk.runs.map((run, index) => {
        const shift = run.shift - (index === 0 ? 0 : chunk.runs[index - 1].shift)
        return (
          <tspan
            key={index}
            dy={shift === 0 ? undefined : shift}
            fontSize={run.size}
            fontWeight={run.weight}
            dominantBaseline="central"
            data-part={run.part}
          >
            {run.text}
          </tspan>
        )
      })}
    </text>
  )
}

/**
 * An atom's label as laid out: its text chunks, its marks, and its radical dots. The whole
 * group is one image named in full; `data-box` is the label box the canvas reserves for it.
 */
function AtomMark({ id, name, label, at }: { id: string; name: string; label: AtomLabel; at: Position }) {
  return (
    <g role="img" aria-label={name} data-atom-label={id} data-box={boxAttribute(placed(label.box, at))}>
      {label.chunks.map((chunk) => (
        <ChunkText key={chunk.role} chunk={chunk} at={at} data-atom={id} data-chunk={chunk.role} />
      ))}
      {label.marks.map((mark) => (
        <ChunkText key={mark.role} chunk={mark} at={at} className="fill-muted-foreground" data-mark={mark.role} />
      ))}
      {label.dots.map((dot, index) => (
        <circle
          key={index}
          cx={round(at.x + dot.dx)}
          cy={round(at.y + dot.dy)}
          r={round(dot.radius)}
          className={dot.hollow ? 'fill-none stroke-foreground' : undefined}
          // A ring 0.6 wide at the built-in radius of 1.4, scaled with the label.
          strokeWidth={dot.hollow ? round((dot.radius * 3) / 7) : undefined}
          data-mark="radical"
          aria-hidden="true"
        />
      ))}
    </g>
  )
}

/**
 * A dashed outline and the instance ID around one of several placements, so each reads as its
 * own drawing. It encloses every atom and label box; the view box holds all of it.
 */
function PlacementFrame({ id, frame }: { id: string; frame: ReturnType<typeof placementFrame> }) {
  return (
    <g className="pointer-events-none" aria-hidden="true">
      <rect
        x={frame.rect.x}
        y={frame.rect.y}
        width={frame.rect.width}
        height={frame.rect.height}
        rx={4}
        className="fill-none stroke-muted-foreground/40"
        strokeWidth={0.8}
        strokeDasharray="4 3"
        data-frame={id}
      />
      <text
        x={frame.idAt.x}
        y={frame.idAt.y}
        fontSize={frame.idSize}
        dominantBaseline="central"
        className="fill-muted-foreground font-sans"
        data-frame-id={id}
      >
        {id}
      </text>
    </g>
  )
}
