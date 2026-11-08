import { imageCorners } from '@/editor/image'
import { SceneImage } from '@/editor/scene-image'
import { SplineDraftControls } from '@/components/spline-draft-controls'
import { appendSplinePoint, splineDraftCurrent, splinePoints, SPLINE_POINT_NAMES, type SplineCreation } from '@/editor/spline'
import { PolygonDraftControls } from '@/components/polygon-draft-controls'
import { appendPolygonVertex, polygonDraftCurrent, polygonVerticesValid, type PolygonCreation } from '@/editor/polygon'
import { graphicLineWidth } from '@/editor/graphic-line-width'
import { graphicCreationCurrent, type GraphicCreation } from '@/editor/graphic-creation'
import { translatedGraphicObject, translatedGraphicPoints, graphicPointValid, graphicEndpointCurrent, graphicEndpointPoints, selectedGraphic, type GraphicEndpointGesture, type GraphicPointsEdit } from '@/editor/graphic-points'
import { compactDisplaysIn, abbreviationTargets, compactView, ethylLabel } from '@/editor/abbreviation'
import type { SceneGraphicShape, RingPattern, RingSize } from '@/lib/document-contract'
import { Fragment, memo, useEffect, useEffectEvent, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'

import { SceneCaption } from '@/editor/scene-caption'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Field, FieldLabel } from '@/components/ui/field'
import { marqueeSelection, sceneOnlySelection, selectionGeometry, selectionMove, type SelectionGeometry } from '@/editor/selection-geometry'
import { useCanvasViewport } from '@/hooks/use-canvas-viewport'
import { placements, sameDrawnContent, settingsOf, type AtomView, type Command, type DocumentView, type LabelFont, type Position, type SceneObject } from '@/lib/document-contract'
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
import { atomLabel, atomName, fragmentLabel, labelStyleOf, measureChemicalLabels, placed, type AtomLabel, type Box, type LabelChunk, type LabelMetrics } from '@/editor/label'

export type { DrawnOrder } from '@/editor/bond-gestures'

/** What the bond tool draws: a kind of bond, or a stereo mark on a single bond. */
export type DrawnKind = DrawnOrder | 'wedge' | 'hash' | 'wavy'

export type Tool = 'select' | 'atom' | 'bond' | 'erase' | 'pan' | 'ring' | 'graphic'

/** Below this pointer travel in CSS pixels a press is a click, not a drag. */
const DRAG_THRESHOLD = 3

/**
 * Gestures act in the scene instance (placement) where they start; `instance` is null for a
 * service without scene instances.
 */
type Drag =
  | { kind: 'graphic-create'; start: Position; creation: GraphicCreation }
  | { kind: 'spline-point'; start: Position; creation: SplineCreation }
  | { kind: 'polygon-vertex'; start: Position; creation: PolygonCreation }
  | ({ kind: 'endpoint' } & GraphicEndpointGesture)
  | { kind: 'attach'; start: Position; atom: string; instance: string | null; size: RingSize; pattern: RingPattern }
  | { kind: 'move'; start: Position; selected: Set<string>; instance: string | null; scope: SelectionGeometry }
  | { kind: 'bond'; start: Position; fromAtom: string | null; instance: string | null }
  | { kind: 'marquee'; start: Position; additive: boolean; pressed: Set<string>; instance: string | null }
  | { kind: 'click'; start: Position; hit: Hit; instance: string | null }

type Preview =
  | { kind: 'graphic-create'; creation: GraphicCreation }
  | { kind: 'endpoint'; gesture: GraphicEndpointGesture; points: [Position, Position] }
  | { kind: 'move'; scope: SelectionGeometry; delta: Position }
  | { kind: 'bond' | 'attach'; from: Position; to: Position }
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

/** The SVG measurement of one drawing's captions and labels, with the document it was taken from. */
interface CaptionMeasurement {
  document: DocumentView
  boxes: Map<string, Box>
  labelMetrics: LabelMetrics
  fontsReady: boolean
  viewBox: string | undefined
}

/**
 * The measurement that still describes `document`. Its boxes depend only on what is drawn, so a
 * new object with equal content keeps it; a different drawing drops it, whatever its revision.
 */
export function captionMeasurementFor(measurement: CaptionMeasurement | null, document: DocumentView): CaptionMeasurement | null {
  if (measurement === null) return null
  return sameDrawnContent(measurement.document, document) ? measurement : null
}

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
  /** A captured endpoint edit; the parent checks its revision again in the queue. */
  onGraphicPoints?: (edit: GraphicPointsEdit) => void
  graphicShape?: SceneGraphicShape | null
  serviceGeneration?: number
  onGraphicCreate?: (creation: GraphicCreation) => void
  onSplineCreate?: (creation: SplineCreation) => void
  onPolygonCreate?: (creation: PolygonCreation) => void
  onCommands: (commands: Command[], selectionBefore?: ReadonlySet<string>) => void
  /** A stereo-mark gesture, built from the document on screen; the parent sends it to the latest. */
  onMark: (intent: MarkIntent) => void
  /** An explicit center click; the shared Rust command owns graph and geometry generation. */
  onRing?: (instance: string | null, center: Position) => void
  ringMode?: 'independent' | 'attach'
  ringSize?: RingSize
  ringPattern?: RingPattern
  onAttachRing?: (instance: string | null, atom: string, direction: { dx: number; dy: number }, size: RingSize, pattern: RingPattern) => void
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
  ringMode = 'independent',
  ringSize = 6,
  ringPattern = 'single',
  onAttachRing,
  onCommands,
  onGraphicPoints,
  graphicShape = null,
  serviceGeneration = 0,
  onGraphicCreate,
  onSplineCreate,
  onPolygonCreate,
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
  const graphicDraftContext = JSON.stringify([document.id, document.revision, tool, graphicShape, serviceGeneration])
  const [draftContext, setDraftContext] = useState(graphicDraftContext)
  const [splineDraft, setSplineDraft] = useState<SplineCreation | null>(null)
  const [polygonDraft, setPolygonDraft] = useState<PolygonCreation | null>(null)
  // Reset before rendering a changed editing context so returning to the tool cannot revive a draft.
  if (draftContext !== graphicDraftContext) { setDraftContext(graphicDraftContext); setPolygonDraft(null); setSplineDraft(null) }
  const activeSpline = splineDraft && tool === 'graphic' && splineDraftCurrent(document, graphicShape, serviceGeneration, splineDraft) ? splineDraft : null
  const activeSplinePoints = activeSpline && splinePoints(activeSpline.points)
  const activePolygon = polygonDraft && tool === 'graphic' && polygonDraftCurrent(document, graphicShape, serviceGeneration, polygonDraft) ? polygonDraft : null
  const [hover, setHover] = useState<Located | null>(null)
  const selectedGeometry = selectionGeometry(document, selection, activePlacement)
  const [captionMeasurement, setCaptionMeasurement] = useState<CaptionMeasurement | null>(null)
  // An equal document keeps its measurement: an adopted autosave reply must not unmount caption hit boxes.
  const measured = useMemo(() => captionMeasurementFor(captionMeasurement, document), [captionMeasurement, document])
  const captionBoxes = useMemo<ReadonlyMap<string, Box>>(() => measured?.boxes ?? new Map<string, Box>(), [measured])
  useEffect(() => {
    const svg = svgRef.current
    if (!svg) return
    let active = true
    let frame = 0
    const measure = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        if (!active) return
        const boxes = new Map<string, Box>()
        for (const text of svg.querySelectorAll<SVGTextElement>('[data-caption-text]')) {
          const id = text.getAttribute('data-caption-text')
          if (id === null) continue
          // Each y-positioned script run starts an SVG text chunk. text-anchor would
          // align those chunks separately; shift the complete advance instead.
          // getComputedTextLength includes preserved leading/trailing whitespace.
          const anchor = Number(text.getAttribute('data-caption-anchor-x'))
          const alignment = text.getAttribute('data-caption-alignment')
          const advance = text.getComputedTextLength()
          const offset = alignment === 'right' ? advance : alignment === 'center' ? advance / 2 : 0
          text.setAttribute('x', String(anchor - offset))
          const shadowText = text.parentElement?.querySelector('[data-caption-shadow-text]')
          shadowText?.setAttribute('x', String(anchor - offset))
          const ink = text.getBBox()
          let left = ink.x
          let top = ink.y
          let right = ink.x + ink.width
          let bottom = ink.y + ink.height
          // SVG getBBox excludes strokes. Expand each outlined run independently;
          // filled siblings, empty/whitespace runs and zero-ink formatting characters
          // must not acquire a stroke margin. Zero advance alone does not mean no ink
          // (combining marks); use the browser's shaped run box, including RTL/ligatures.
          for (const span of text.querySelectorAll<SVGTSpanElement>('[data-caption-outline="true"]')) {
            const glyph = span.getBBox()
            if (!span.textContent?.match(/\S/u) || glyph.width <= 0 || glyph.height <= 0) continue
            const radius = Number(span.getAttribute('font-size')) / 32
            left = Math.min(left, glyph.x - radius)
            top = Math.min(top, glyph.y - radius)
            right = Math.max(right, glyph.x + glyph.width + radius)
            bottom = Math.max(bottom, glyph.y + glyph.height + radius)
          }
          // The shadow is a second browser-shaped text tree at a fixed (1,1) pt
          // offset, with no blur. Measure foreground spans so hidden siblings do
          // not enter the box; shadow does not alter advance or anchor alignment.
          for (const span of text.querySelectorAll<SVGTSpanElement>('[data-caption-shadow="true"]')) {
            const glyph = span.getBBox()
            if (!span.textContent?.match(/\S/u) || glyph.width <= 0 || glyph.height <= 0) continue
            const radius = span.getAttribute('data-caption-outline') === 'true' ? Number(span.getAttribute('font-size')) / 32 : 0
            left = Math.min(left, glyph.x + 1 - radius)
            top = Math.min(top, glyph.y + 1 - radius)
            right = Math.max(right, glyph.x + glyph.width + 1 + radius)
            bottom = Math.max(bottom, glyph.y + glyph.height + 1 + radius)
          }
          // SVG text getBBox does not promise decoration ink. Our underline is an
          // explicit rectangle whose geometry also enters fit, selection and hit testing.
          for (const line of text.parentElement?.querySelectorAll<SVGRectElement>('[data-caption-underline]') ?? []) {
            const span = text.children.item(Number(line.getAttribute('data-caption-underline')))
            if (!(span instanceof SVGTSpanElement)) continue
            const characters = span.getNumberOfChars()
            // A fully invisible run may expose no addressable SVG characters. It
            // paints no underline; its empty position must not enlarge content bounds.
            let start = characters === 0 ? span.getBBox().x : Infinity
            let end = characters === 0 ? start : -Infinity
            for (let index = 0; index < characters; index++) {
              const a = span.getStartPositionOfChar(index).x
              const b = span.getEndPositionOfChar(index).x
              start = Math.min(start, a, b)
              end = Math.max(end, a, b)
            }
            if (!Number.isFinite(start) || !Number.isFinite(end)) continue
            const size = Number(span.getAttribute('font-size'))
            const y = Number(span.getAttribute('y')) + size / 12
            const height = size / 16
            line.setAttribute('x', String(start))
            line.setAttribute('y', String(y))
            line.setAttribute('width', String(end - start))
            line.setAttribute('height', String(height))
            const shadow = text.parentElement?.querySelector(`[data-caption-underline-shadow="${line.getAttribute('data-caption-underline')}"]`)
            if (shadow) {
              for (const name of ['x', 'y', 'width', 'height']) shadow.setAttribute(name, line.getAttribute(name) ?? '0')
            }
            if (end > start) {
              left = Math.min(left, start)
              top = Math.min(top, y)
              right = Math.max(right, end + (shadow ? 1 : 0))
              bottom = Math.max(bottom, y + height + (shadow ? 1 : 0))
            }
          }
          boxes.set(id, { x: left - 2, y: top - 2, width: right - left + 4, height: bottom - top + 4 })
        }
        setCaptionMeasurement({ document, boxes, labelMetrics: measureChemicalLabels(svg), fontsReady: globalThis.document.fonts.status === 'loaded', viewBox: svg.getAttribute('viewBox') ?? undefined })
      })
    }
    const observer = new ResizeObserver(measure)
    for (const text of svg.querySelectorAll('[data-caption-text]')) observer.observe(text)
    // Text metrics can change after a fit changes the rendered scale, even with fonts loaded.
    const viewportObserver = new MutationObserver(measure)
    viewportObserver.observe(svg, { attributes: true, attributeFilter: ['viewBox'] })
    globalThis.document.fonts.addEventListener('loadingdone', measure)
    globalThis.document.fonts.addEventListener('loadingerror', measure)
    measure()
    return () => {
      active = false
      cancelAnimationFrame(frame)
      observer.disconnect()
      viewportObserver.disconnect()
      globalThis.document.fonts.removeEventListener('loadingdone', measure)
      globalThis.document.fonts.removeEventListener('loadingerror', measure)
    }
  }, [document])

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
    const style = labelStyleOf(settings, measured?.labelMetrics)
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
      const view = compactView(placedView(document, placement), placement.id)
      const compactAnchors = new Map(compactDisplaysIn(document, placement.id).map((item) => [item.anchor, item]))
      const index = atomIndex(view)
      const left = leaningRight(view)
      const labels = new Map<string, AtomLabel | null>(view.atoms.map((atom) => {
        const compact = compactAnchors.get(atom.id)
        if (!compact) return [atom.id, layoutOf(atom, left.has(atom.id))]
        return [atom.id, compact.runs ? fragmentLabel(compact.runs, style) : ethylLabel(style, compact.text)]
      }))
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
  }, [document, measured])
  // The view box holds every atom, label box, and placement frame (with its ID).
  const box = useMemo(
    () =>
      viewBox(
        [
          ...drawings.flatMap((drawing) => drawing.view.atoms.map((atom) => atom.position)),
          ...(document.scene?.objects?.flatMap((object) => object.kind === 'graphic' ? object.points : object.kind === 'caption' ? [object.anchor] : object.kind === 'image' ? imageCorners(object) : []) ?? []),
        ],
        [...captionBoxes.values(), ...drawings.flatMap((drawing) => [
          ...drawing.view.atoms.flatMap((atom) => labelBox(drawing.labels, atom.id, atom.position) ?? []),
          ...(drawing.frame ? [drawing.frame.box] : []),
          // Group labels are an overlay, but a clipped one is unreadable: they count only while drawn.
          ...groupLabelBoxes(groupLabels, document.bonds, new Map(drawing.view.atoms.map((atom) => [atom.id, atom.position]))),
        ])],
        drawings[0]?.reach ?? 0,
      ),
    [document.scene, document.bonds, drawings, groupLabels, captionBoxes],
  )

  const captionCount = document.scene?.objects?.filter((object) => object.kind === 'caption').length ?? 0
  const styledLabels = settingsOf(document).label_weight === 'bold' || settingsOf(document).label_style === 'italic' || settingsOf(document).label_underline || settingsOf(document).label_outline || settingsOf(document).label_shadow
  const measuredLabels = styledLabels || (document.abbreviations ?? []).some((item) => item.display_label !== 'Et') || (document.fragment_displays?.length ?? 0) > 0
  const chemicalMeasurementComplete = !measuredLabels || drawings.every((drawing) =>
    [...drawing.labels.entries()].every(([id, label]) => label === null ||
      (!styledLabels && !compactDisplaysIn(document, drawing.id).some((item) => item.anchor === id && (!item.ethyl || item.text !== 'Et'))) ||
      label.chunks.every((chunk) => chunk.measured)))
  const captionMeasurementComplete = chemicalMeasurementComplete && ((captionCount === 0 && !measuredLabels) || (measured !== null &&
    measured.fontsReady && measured.boxes.size === captionCount))
  const { containerRef, viewport, viewBox: canvasViewBox, zoom, fit, pan } = useCanvasViewport(document.id, box, {
    // Opening fit needs the first complete measurement, even though fitting will
    // change the viewport and require a second measurement there.
    boundsReady: captionMeasurementComplete,
    measuredViewBox: (captionCount > 0 || measuredLabels) ? measured?.viewBox : undefined,
    contentIdentity: document,
  })
  // Font metrics can change with SVG scale. Completeness permits initial fitting;
  // readiness describes bounds measured in the viewport that is actually painted.
  const captionBoundsReady = captionMeasurementComplete && ((captionCount === 0 && !measuredLabels) || measured?.viewBox === canvasViewBox)
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

  const cancelExpiredGraphic = useEffectEvent(() => {
    const gesture = drag.current
    if (gesture?.kind === 'endpoint' && (tool !== 'select' || !graphicEndpointCurrent(document, selection, gesture))) cancelPointer()
    if (gesture?.kind === 'spline-point' && (tool !== 'graphic' || !splineDraftCurrent(document, graphicShape, serviceGeneration, gesture.creation))) cancelPointer()
    if (gesture?.kind === 'polygon-vertex' && (tool !== 'graphic' || !polygonDraftCurrent(document, graphicShape, serviceGeneration, gesture.creation))) cancelPointer()
    if (gesture?.kind === 'graphic-create' && (tool !== 'graphic' || !graphicCreationCurrent(document, graphicShape, serviceGeneration, gesture.creation))) cancelPointer()
  })
  useEffect(() => { cancelExpiredGraphic() }, [document, selection, tool, graphicShape, serviceGeneration])

  const cancelFromWindow = useEffectEvent(() => { cancelPointer(); setPolygonDraft(null); setSplineDraft(null) })

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
    if (key === 'escape') { cancelPointer(); setPolygonDraft(null); setSplineDraft(null); return }
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
    const covered = kind === 'graphic' ? scope?.objects.has(id)
      : kind === 'atom' ? positions?.has(id)
      : bond !== undefined && positions?.has(bond.begin) && positions.has(bond.end)
    let next = current
    const targets = kind === 'atom' ? abbreviationTargets(document, instance, id) : [id]
    const compact = kind === 'atom' ? compactDisplaysIn(document, instance).find((item) => item.anchor === id) : undefined
    // Contraction preserves member selection; internal bonds may be implicit.
    const targetSelected = compact ? compact.members.every((member) => next.has(member)) : next.has(id)
    const deselected = additive && targetSelected
    if (additive) {
      if (deselected) targets.forEach((target) => { next.delete(target) })
      else targets.forEach((target) => { next.add(target) })
    } else if (!covered) {
      next = new Set(targets)
    } else if (targets.length > 1) {
      // A selected boundary bond covers the anchor geometrically, but not hidden members.
      targets.forEach((target) => { next.add(target) })
    }
    onSelectionChange(next)
    const moving = selectionGeometry(document, next, instance)
    if (!deselected && moving) setDrag({ kind: 'move', start: point, selected: new Set(next), instance, scope: moving })
  }

  function onPointerDown(event: PointerEvent<SVGSVGElement>) {
    if (pointer.current?.id !== event.pointerId || pointer.current.pan) return
    const point = toDocument(event)
    if (!point) return
    if (tool === 'graphic') {
      if (graphicShape === 'spline') {
        if (onSplineCreate && graphicPointValid(point)) setDrag({ kind: 'spline-point', start: point, creation: { documentId: document.id, revision: document.revision, serviceGeneration, points: [] } })
        return
      }
      if (graphicShape === 'polygon') {
        if (onPolygonCreate && graphicPointValid(point)) setDrag({ kind: 'polygon-vertex', start: point, creation: { documentId: document.id, revision: document.revision, serviceGeneration, vertices: [] } })
        return
      }
      if (graphicShape && onGraphicCreate && graphicPointValid(point)) {
        setDrag({ kind: 'graphic-create', start: point, creation: { documentId: document.id, revision: document.revision, serviceGeneration, shape: graphicShape, points: [{ ...point }, { ...point }] } })
      }
      return
    }
    const { hit, instance } = locate(point)
    if (tool === 'ring' && ringMode === 'attach') {
      const atom = hit?.kind === 'atom' ? drawingOf(instance).atoms.get(hit.id) : undefined
      if (atom) setDrag({ kind: 'attach', start: atom.position, atom: atom.id, instance, size: ringSize, pattern: ringPattern })
    } else if (tool === 'select') {
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
    if (!active) return
    if (distance({ x: event.clientX, y: event.clientY }, active.start) < DRAG_THRESHOLD) {
      // A release here makes no edit; discard earlier graphic or movement previews.
      if (gesture.kind === 'graphic-create' || gesture.kind === 'endpoint' || gesture.kind === 'move') setPreview(null)
      return
    }
    if (gesture.kind === 'graphic-create') {
      if (tool !== 'graphic' || !graphicCreationCurrent(document, graphicShape, serviceGeneration, gesture.creation)) { cancelPointer(); return }
      setPreview(graphicPointValid(point) ? { kind: 'graphic-create', creation: { ...gesture.creation, points: [{ ...gesture.start }, { ...point }] } } : null)
    } else if (gesture.kind === 'endpoint') {
      if (tool !== 'select' || !graphicEndpointCurrent(document, selection, gesture)) { cancelPointer(); return }
      const points = graphicEndpointPoints(gesture, point)
      setPreview(points ? { kind: 'endpoint', gesture, points } : null)
    } else if (gesture.kind === 'move') {
      setPreview({
        kind: 'move',
        scope: gesture.scope,
        delta: { x: round(point.x - gesture.start.x), y: round(point.y - gesture.start.y) },
      })
    } else if (gesture.kind === 'bond') {
      const target = bondTarget(gesture, point)
      setPreview(target && { kind: 'bond', from: target.origin, to: target.position })
    } else if (gesture.kind === 'attach') {
      const dx = point.x - gesture.start.x
      const dy = point.y - gesture.start.y
      const length = Math.hypot(dx, dy)
      setPreview(length === 0 ? null : { kind: 'attach', from: gesture.start,
        to: { x: gesture.start.x + bondLength * dx / length, y: gesture.start.y + bondLength * dy / length } })
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
    if (gesture.kind === 'spline-point') {
      if (!dragged && tool === 'graphic' && splineDraftCurrent(document, graphicShape, serviceGeneration, gesture.creation) && graphicPointValid(gesture.start)) {
        setSplineDraft((current) => appendSplinePoint(current && splineDraftCurrent(document, graphicShape, serviceGeneration, current) ? current : gesture.creation, gesture.start))
      }
      return
    }
    if (gesture.kind === 'polygon-vertex') {
      if (!dragged && tool === 'graphic' && polygonDraftCurrent(document, graphicShape, serviceGeneration, gesture.creation) && graphicPointValid(gesture.start)) {
        setPolygonDraft((current) => appendPolygonVertex(current && polygonDraftCurrent(document, graphicShape, serviceGeneration, current) ? current : gesture.creation, gesture.start))
      }
      return
    }
    if (gesture.kind === 'graphic-create') {
      if (dragged && tool === 'graphic' && graphicPointValid(point) && graphicCreationCurrent(document, graphicShape, serviceGeneration, gesture.creation)) {
        onGraphicCreate?.({ ...gesture.creation, points: [{ ...gesture.start }, { ...point }] })
      }
      return
    }
    if (gesture.kind === 'endpoint') {
      if (!dragged || tool !== 'select' || !graphicEndpointCurrent(document, selection, gesture)) return
      const points = graphicEndpointPoints(gesture, point)
      if (points) onGraphicPoints?.({ ...gesture.edit, points })
      return
    }
    if (gesture.kind === 'attach') {
      if (dragged) onAttachRing?.(gesture.instance, gesture.atom, { dx: point.x - gesture.start.x, dy: point.y - gesture.start.y }, gesture.size, gesture.pattern)
      return
    }
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
      case 'spline-point':
      case 'polygon-vertex':
      case 'graphic-create':
      case 'endpoint':
        return []
      case 'move': {
        if (!dragged) return []
        const delta = { x: round(point.x - gesture.start.x), y: round(point.y - gesture.start.y) }
        return selectionMove(document, gesture.selected, gesture.instance, delta) ?? []
      }
      case 'marquee': {
        onSelectionChange(marqueeSelection(document, gesture.instance, gesture.start, point, gesture.pressed, gesture.additive, dragged))
        return []
      }
      case 'attach':
        return []
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
      if (hit?.kind === 'atom') {
        const item = compactDisplaysIn(document, instance).find((item) => item.anchor === hit.id)
        return [{ op: 'delete_atoms', atoms: item ? item.members : [hit.id] }]
      }
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

  const graphicPreview = preview?.kind === 'graphic-create' && tool === 'graphic' && graphicCreationCurrent(document, graphicShape, serviceGeneration, preview.creation) ? preview.creation : null
  const endpointPreview = preview?.kind === 'endpoint' && tool === 'select' && graphicEndpointCurrent(document, selection, preview.gesture) ? preview : null
  const endpointGraphic = tool === 'select' && onGraphicPoints && !disabled ? selectedGraphic(document, selection) : null
  const movedEndpointPoints = endpointGraphic && preview?.kind === 'move' && preview.scope.graphics.has(endpointGraphic.id)
    ? translatedGraphicPoints(endpointGraphic.points, preview.delta) : null

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
  // Derived per document, not per render: pointer moves re-render the canvas.
  const shadowRuns = useMemo(() => shadowRunsFrom(sceneObjects, drawings), [sceneObjects, drawings])
  const drawInstance = (drawing: Drawing) => (
    <g key={drawing.id ?? 'atoms'} data-instance={drawing.id ?? undefined}>
      {drawing.frame && <PlacementFrame id={drawing.id ?? ''} frame={drawing.frame} />}
      {/* Decorative chemical highlights must not become transient pointer targets. */}
      <g className="fill-primary/15 stroke-primary/40" pointerEvents="none">
        {drawing.view.bonds
          .filter((bond) => (drawing.id === activePlacement && selection.has(bond.id)) || (selectedGeometry?.positions.get(drawing.id)?.has(bond.begin) && selectedGeometry.positions.get(drawing.id)?.has(bond.end)) || hovered(drawing, 'bond', bond.id))
          .map((bond) => {
            const a = position(drawing, bond.begin)
            const b = position(drawing, bond.end)
            return <line key={bond.id} data-underlay={bond.id} x1={a.x} y1={a.y} x2={b.x} y2={b.y} strokeWidth={bondUnderlay(bond.order, drawing.lineWidth, drawing.spacing, unitsPerPixel, bond.mark)} strokeLinecap="round" />
          })}
        {drawing.view.atoms
          .filter((atom) => selectedGeometry?.positions.get(drawing.id)?.has(atom.id) || hovered(drawing, 'atom', atom.id))
          .map((atom) => {
            const p = position(drawing, atom.id)
            const label = labelBox(drawing.labels, atom.id, p)
            return label ? (
              <rect key={atom.id} data-atom-underlay={atom.id} {...label} rx={3 * unitsPerPixel} strokeWidth={unitsPerPixel} />
            ) : (
              <circle key={atom.id} data-atom-underlay={atom.id} cx={p.x} cy={p.y} r={8 * unitsPerPixel} strokeWidth={unitsPerPixel} />
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

  /**
   * An instance's foreground, preceded by the chemical shadows of its run when it is the run's
   * first drawing. The run's shadows then sit above whatever an image, graphic or caption painted
   * before the run, and below every foreground of the run.
   */
  const paintInstance = (drawing: Drawing) => {
    const run = shadowRuns.get(drawing.id)
    return (
      <Fragment key={drawing.id ?? 'atoms'}>
        {run && <ChemicalShadowRun run={run} position={position} />}
        {drawInstance(drawing)}
      </Fragment>
    )
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col gap-2" onKeyDownCapture={(event) => { if (event.key === 'Escape' && (activePolygon || activeSpline)) { event.preventDefault(); event.stopPropagation(); cancelPointer(); setPolygonDraft(null); setSplineDraft(null) } }}>
      {tool === 'graphic' && graphicShape === 'spline' && <SplineDraftControls count={activeSpline?.points.length ?? 0} available={Boolean(onSplineCreate)} onFinish={() => {
        if (!activeSpline || !onSplineCreate || !splinePoints(activeSpline.points)) return
        cancelPointer()
        setSplineDraft(null)
        onSplineCreate(activeSpline)
      }} onRemove={() => {
        cancelPointer()
        setSplineDraft((current) => current && splineDraftCurrent(document, graphicShape, serviceGeneration, current) && current.points.length > 1 ? { ...current, points: current.points.slice(0, -1) } : null)
      }} onCancel={() => { cancelPointer(); setSplineDraft(null) }} />}
      {tool === 'graphic' && graphicShape === 'polygon' && <PolygonDraftControls count={activePolygon?.vertices.length ?? 0} available={Boolean(onPolygonCreate)} onFinish={() => {
        if (!activePolygon || !onPolygonCreate || !polygonVerticesValid(activePolygon.vertices)) return
        cancelPointer()
        setPolygonDraft(null)
        onPolygonCreate(activePolygon)
      }} onRemove={() => {
        cancelPointer()
        setPolygonDraft((current) => current && polygonDraftCurrent(document, graphicShape, serviceGeneration, current) && current.vertices.length > 1 ? { ...current, vertices: current.vertices.slice(0, -1) } : null)
      }} onCancel={() => { cancelPointer(); setPolygonDraft(null); setSplineDraft(null) }} />}
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
      data-caption-bounds-ready={captionBoundsReady}
      data-caption-measured-view-box={measured?.viewBox}
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
        if (object.kind === 'image') return (
          <SceneImage key={object.id} object={object} selected={sceneSelected(object)} unitsPerPixel={unitsPerPixel}
            delta={preview?.kind === 'move' && preview.scope.objects.has(object.id) ? preview.delta : undefined}
            onPointerDown={(event) => {
              if (tool !== 'select' || disabled || event.button !== 0 || pointer.current?.id !== event.pointerId) return
              event.stopPropagation()
              const point = toDocument(event)
              if (point) selectTarget(object.id, 'graphic', activePlacement, point, event.shiftKey)
            }} />
        )
        if (object.kind === 'caption') return (
          <SceneCaption key={object.id} object={object} box={captionBoxes.get(object.id)} settings={settingsOf(document)} selected={sceneSelected(object)}
            delta={preview?.kind === 'move' && preview.scope.objects.has(object.id) ? preview.delta : undefined}
            onPointerDown={(event) => {
              if (tool !== 'select' || disabled || event.button !== 0 || pointer.current?.id !== event.pointerId) return
              event.stopPropagation()
              const point = toDocument(event)
              if (point) selectTarget(object.id, 'graphic', activePlacement, point, event.shiftKey)
            }} />
        )
        if (object.kind === 'graphic') return (
          <SceneGraphic key={object.id} object={endpointPreview?.gesture.edit.graphic.id === object.id ? { ...endpointPreview.gesture.edit.graphic, points: endpointPreview.points } : preview?.kind === 'move' && preview.scope.graphics.has(object.id) ? translatedGraphicObject(object, preview.delta) : object} selected={sceneSelected(object)} lineWidth={settingsOf(document).line_width_pt} unitsPerPixel={unitsPerPixel} onPointerDown={(event) => {
            if (tool !== 'select' || disabled || event.button !== 0 || pointer.current?.id !== event.pointerId) return
            event.stopPropagation()
            const point = toDocument(event)
            if (point) selectTarget(object.id, 'graphic', activePlacement, point, event.shiftKey)
          }} />
        )
        const drawing = drawings.find((item) => item.id === object.id)
        return drawing ? paintInstance(drawing) : null
      }) : drawings.map(paintInstance)}
      {activeSpline && <g data-editor-overlay="" data-spline-draft="" pointerEvents="none">
        {activeSplinePoints && <SceneGraphic object={{ kind: 'graphic', id: 'spline-preview', parent: null, shape: 'spline', points: activeSplinePoints }} selected={false} lineWidth={settingsOf(document).line_width_pt} unitsPerPixel={unitsPerPixel} />}
        <polyline points={activeSpline.points.map((point) => `${point.x},${point.y}`).join(' ')} fill="none" className="stroke-muted-foreground" strokeWidth={unitsPerPixel} strokeDasharray={`${3 * unitsPerPixel} ${3 * unitsPerPixel}`} />
        {activeSpline.points.map((point, index) => <circle key={index} data-spline-draft-point={index} cx={point.x} cy={point.y} r={3 * unitsPerPixel} className="fill-primary" role="img" aria-label={`Draft spline ${SPLINE_POINT_NAMES[index]}`} />)}
      </g>}
      {activePolygon && <g data-editor-overlay="" data-polygon-draft="" pointerEvents="none">
        {activePolygon.vertices.length >= 3
          ? <SceneGraphic object={{ kind: 'graphic', id: 'polygon-preview', parent: null, shape: 'polygon', points: activePolygon.vertices }} selected={false} lineWidth={settingsOf(document).line_width_pt} unitsPerPixel={unitsPerPixel} />
          : <polyline points={activePolygon.vertices.map((point) => `${point.x},${point.y}`).join(' ')} fill="none" className="stroke-foreground" strokeWidth={settingsOf(document).line_width_pt} />}
        {activePolygon.vertices.map((point, index) => <circle key={index} data-polygon-draft-vertex={index} cx={point.x} cy={point.y} r={3 * unitsPerPixel} className="fill-primary" role="img" aria-label={`Draft polygon vertex ${index + 1}`} />)}
      </g>}
      {graphicPreview && <g data-editor-overlay="" data-graphic-create-preview={graphicPreview.shape} pointerEvents="none">
        <SceneGraphic object={{ kind: 'graphic', id: 'preview', parent: null, shape: graphicPreview.shape, points: graphicPreview.points }} selected={false} lineWidth={settingsOf(document).line_width_pt} unitsPerPixel={unitsPerPixel} />
      </g>}
      {endpointGraphic && <g data-editor-overlay="" aria-label={`Endpoints of ${endpointGraphic.shape} ${endpointGraphic.id}`}>
        {([0, 1] as const).map((endpoint) => {
          const at = endpointPreview?.gesture.edit.graphic.id === endpointGraphic.id ? endpointPreview.points[endpoint] : (movedEndpointPoints ?? endpointGraphic.points)[endpoint]
          return <circle key={endpoint} data-graphic-endpoint={endpoint} data-endpoint-object={endpointGraphic.id}
            role="img" aria-label={`Endpoint ${endpoint + 1} of ${endpointGraphic.shape} ${endpointGraphic.id}; drag to edit or use the numeric endpoint form`}
            cx={at.x} cy={at.y} r={6 * unitsPerPixel} strokeWidth={1.5 * unitsPerPixel} className="fill-background stroke-primary cursor-crosshair"
            onPointerDown={(event) => {
              const active = pointer.current
              if (disabled || tool !== 'select' || event.button !== 0 || active?.id !== event.pointerId || active.pan) return
              event.stopPropagation()
              const start = toDocument(event)
              if (!start) return
              const graphic = { ...endpointGraphic, points: [{ ...endpointGraphic.points[0] }, { ...endpointGraphic.points[1] }] satisfies [Position, Position] }
              setDrag({ kind: 'endpoint', endpoint, start, edit: { documentId: document.id, revision: document.revision, graphic, points: graphic.points } })
            }} />
        })}
      </g>}
      {preview?.kind === 'bond' && isMarkKind(bondKind) ? (
        <g className="fill-none stroke-primary" strokeWidth={settingsOf(document).line_width_pt} strokeLinejoin="round" data-bond-preview={bondKind}>
          <StrokeShapes strokes={previewStrokes(preview.from, preview.to, bondKind)} bond="preview" fill="fill-primary stroke-none" />
        </g>
      ) : preview?.kind === 'bond' || preview?.kind === 'attach' ? (
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

function SceneGraphic({ object, selected, lineWidth: documentLineWidth, unitsPerPixel, onPointerDown }: {
  object: Extract<SceneObject, { kind: 'graphic' }>
  selected: boolean
  lineWidth: number
  unitsPerPixel: number
  onPointerDown?: (event: PointerEvent<SVGGElement>) => void
}) {
  const lineWidth = graphicLineWidth(object, documentLineWidth)
  if (object.shape === 'spline') {
    const [p0, c1, c2, p3] = object.points
    const d = `M ${p0.x} ${p0.y} C ${c1.x} ${c1.y} ${c2.x} ${c2.y} ${p3.x} ${p3.y}`
    const underlay = graphicUnderlay(lineWidth, unitsPerPixel)
    return <g data-scene-object={object.id} onPointerDown={onPointerDown} strokeLinecap="round" strokeLinejoin="round">
      <path d={d} fill="none" stroke="transparent" strokeWidth={Math.max(10 * unitsPerPixel, lineWidth)} />
      {selected && <path d={d} fill="none" className="stroke-primary/40" strokeWidth={underlay} />}
      <path d={d} fill="none" className="stroke-foreground" strokeWidth={lineWidth} />
    </g>
  }
  if (object.shape === 'polygon') {
    const points = object.points.map((point) => `${point.x},${point.y}`).join(' ')
    const underlay = graphicUnderlay(lineWidth, unitsPerPixel)
    return <g data-scene-object={object.id} onPointerDown={onPointerDown} strokeLinecap="round" strokeLinejoin="round">
      <polygon points={points} fill="none" stroke="transparent" strokeWidth={Math.max(10 * unitsPerPixel, lineWidth)} />
      {selected && <polygon points={points} fill="none" className="stroke-primary/40" strokeWidth={underlay} />}
      <polygon points={points} fill="none" className="stroke-foreground" strokeWidth={lineWidth} />
    </g>
  }
  const [a, b] = object.points
  const length = Math.hypot(b.x - a.x, b.y - a.y)
  const ux = length > 0 ? (b.x - a.x) / length : 0
  const uy = length > 0 ? (b.y - a.y) / length : 0
  const head = `${b.x},${b.y} ${b.x - ux * 6 - uy * 3},${b.y - uy * 6 + ux * 3} ${b.x - ux * 6 + uy * 3},${b.y - uy * 6 - ux * 3}`
  const rect = { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) }
  const BoxTag = object.shape === 'ellipse' ? 'ellipse' : 'rect'
  const box = object.shape === 'ellipse'
    ? { cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2, rx: rect.width / 2, ry: rect.height / 2 } : rect
  const underlay = graphicUnderlay(lineWidth, unitsPerPixel)
  return (
    <g data-scene-object={object.id} onPointerDown={onPointerDown} strokeLinecap="round" strokeLinejoin="round">
      {object.shape === 'rectangle' || object.shape === 'ellipse' ? (
        <>
          <BoxTag {...box} fill="none" stroke="transparent" strokeWidth={Math.max(10 * unitsPerPixel, lineWidth)} />
          {selected && <BoxTag {...box} fill="none" className="stroke-primary/40" strokeWidth={underlay} />}
          <BoxTag {...box} fill="none" className="stroke-foreground" strokeWidth={lineWidth} />
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
          const compact = compactDisplaysIn(drawing.view, drawing.id).find((item) => item.anchor === atom.id)
          const name = compact
            ? `Display label ${compact.text} at ${atom.id}; text does not define chemistry; ${compact.ethyl ? 'underlying ethyl graph' : 'retained fragment graph'}`
            : atomName(atom, placement)
          return label ? (
            <AtomMark key={atom.id} id={atom.id} name={name} label={label} at={position(atom.id)} />
          ) : null
        })}
      </g>
    </>
  )
})

/** `box` as an attribute value, so a test can compare it with what the browser draws. */
const boxAttribute = (box: Box) => `${round(box.x)} ${round(box.y)} ${round(box.width)} ${round(box.height)}`

/** A chunk of label text flowing from its anchor; each run is drawn at its natural width. */
function ChunkText({ chunk, at, shadow = false, ...rest }: { chunk: LabelChunk; at: Position; shadow?: boolean; className?: string } & Record<`data-${string}`, string>) {
  return (
    <>
      <text
        x={round(at.x + chunk.x)}
        y={round(at.y + chunk.y)}
        textAnchor={chunk.anchor}
        xmlSpace="preserve"
        style={{ whiteSpace: 'pre' }}
        dominantBaseline="central"
        aria-hidden="true"
        data-box={shadow ? undefined : boxAttribute(placed(chunk.box, at))}
        data-label-metric-key={shadow ? undefined : chunk.metricKey}
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
              fontStyle={run.style}
              fill={run.outline ? 'none' : undefined}
              stroke={run.outline ? 'currentColor' : undefined}
              strokeWidth={run.outline ? run.size / 16 : undefined}
              strokeLinejoin={run.outline ? 'round' : undefined}
              data-label-outline={String(run.outline)}
              dominantBaseline="central"
              data-part={run.part}
            >
              {run.text}
            </tspan>
          )
        })}
      </text>
      {chunk.underlines.map((line, index) => <rect key={index} data-label-underline={shadow ? undefined : String(index)}
        x={at.x + line.x} y={at.y + line.y} width={line.width} height={line.height}
        aria-hidden="true" pointerEvents="none" />)}
    </>
  )
}

/**
 * The runs of instances that share one chemical shadow group, keyed by the id of each run's first
 * drawing (null without a scene). Instances paint in scene order, each over what precedes it, so an
 * image, graphic or caption ends a run: the instances after it paint above it. A `group` paints
 * nothing and an instance without a drawing paints nothing, so neither ends a run. A run with no
 * shadowed label gets no group.
 */
function shadowRunsFrom(objects: readonly SceneObject[] | undefined, drawings: readonly Drawing[]): ReadonlyMap<string | null, readonly Drawing[]> {
  // Like Array.prototype.find, the first drawing with an id wins.
  const drawingById = new Map<string | null, Drawing>()
  for (const drawing of drawings) {
    if (!drawingById.has(drawing.id)) drawingById.set(drawing.id, drawing)
  }
  const runs: Drawing[][] = []
  if (objects) {
    let current: Drawing[] | null = null
    for (const object of objects) {
      switch (object.kind) {
        case 'group':
          break
        case 'image':
        case 'caption':
        case 'graphic':
          current = null
          break
        case 'instance': {
          const drawing = drawingById.get(object.id)
          if (!drawing) break
          if (current === null) {
            current = []
            runs.push(current)
          }
          current.push(drawing)
          break
        }
        default: {
          const unreachable: never = object
          throw new Error(`Unsupported scene object: ${String(unreachable)}`)
        }
      }
    }
  } else if (drawings.length > 0) {
    runs.push([...drawings])
  }
  const runsByFirst = new Map<string | null, readonly Drawing[]>()
  for (const run of runs) {
    if (run.some((drawing) => drawing.view.atoms.some((atom) => drawing.labels.get(atom.id)?.shadow))) runsByFirst.set(run[0].id, run)
  }
  return runsByFirst
}

/**
 * The chemical shadows of one run of instances, in one group. Group opacity composites overlapping
 * shadows once, and every shadow of the run paints below the run's foregrounds.
 */
function ChemicalShadowRun({ run, position }: { run: readonly Drawing[]; position: (drawing: Drawing, id: string) => Position }) {
  // The instance ids in paint order; a drawing without one (no scene) leaves the attribute empty.
  const ids = run.flatMap((drawing) => (drawing.id === null ? [] : [drawing.id])).join(' ')
  return (
    <g data-chemical-label-shadows={ids} opacity={0.35} transform="translate(1 1)" aria-hidden="true" pointerEvents="none">
      {run.map((drawing) => (
        <g key={drawing.id ?? 'atoms'} className={cn('fill-foreground', drawing.font === 'sans-serif' && 'font-sans')}
          fontFamily={drawing.font === 'sans-serif' ? undefined : drawing.font}>
          {drawing.view.atoms.map((atom) => {
            const label = drawing.labels.get(atom.id)
            return label?.shadow ? label.chunks.map((chunk, index) => (
              <g key={`${atom.id}-${index}`} data-chemical-shadow-atom={atom.id}>
                <ChunkText chunk={chunk} at={position(drawing, atom.id)} shadow />
              </g>
            )) : null
          })}
        </g>
      ))}
    </g>
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
