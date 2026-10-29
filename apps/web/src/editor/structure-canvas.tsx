import { useMemo, useRef, useState, type PointerEvent } from 'react'

import { cn } from '@/lib/utils'
import { placements, type AtomView, type BondOrder, type Command, type DocumentView, type Position } from '@/lib/document-contract'
import { placeNewAtoms, placedView, placementAt, type NewAtom } from '@/editor/placement'
import {
  atomIndex,
  bondName,
  bondStrokes,
  degrees,
  distance,
  freeNeighborPosition,
  hitTest,
  leaningRight,
  round,
  sameHit,
  snappedEndpoint,
  viewBox,
  type Hit,
  type Stroke,
} from '@/editor/geometry'
import { DOT_SIZE, atomLabel, placed, union, type AtomLabel, type Box } from '@/editor/label'

export type Tool = 'select' | 'atom' | 'bond' | 'erase'

/** Below this pointer travel (pt) a press is a click, not a drag. */
const DRAG_THRESHOLD = 3
/** A bond tool click cycles single, double, and triple; any other kind becomes single. */
const nextOrder = (order: BondOrder): DrawnOrder => (order === 'single' ? 'double' : order === 'double' ? 'triple' : 'single')

/** The kinds the bond tool draws: a query bond needs its expression, set in the inspector. */
export type DrawnOrder = Exclude<BondOrder, 'query'>

const DASHES: Record<Extract<Stroke, { shape: 'line' }>['pattern'], string | undefined> = {
  solid: undefined,
  dashed: '2.5 2',
  dotted: '0.1 2.2',
}

/**
 * Gestures act in the scene instance (placement) where they start; `instance` is null for a
 * service without scene instances.
 */
type Drag =
  | { kind: 'move'; start: Position; atoms: string[]; instance: string | null }
  | { kind: 'bond'; start: Position; fromAtom: string | null; instance: string | null }
  | { kind: 'marquee'; start: Position; additive: boolean }
  | { kind: 'click'; start: Position; hit: Hit; instance: string | null }

type Preview =
  | { kind: 'move'; atoms: ReadonlySet<string>; delta: Position; instance: string | null }
  | { kind: 'bond'; from: Position; to: Position }
  | { kind: 'marquee'; from: Position; to: Position }
  | null

/** What is under the pointer, and the placement it belongs to. */
interface Located {
  hit: Hit
  instance: string | null
}

/**
 * One placement of the document, drawn with its own atom positions. Its label layouts are
 * relative to each atom; which side stated hydrogens take depends on the placement's bonds.
 */
interface Drawing {
  id: string | null
  view: DocumentView
  atoms: ReadonlyMap<string, AtomView>
  labels: ReadonlyMap<string, AtomLabel | null>
}

const sameLocated = (a: Located | null, b: Located | null) =>
  a === b || (a !== null && b !== null && a.instance === b.instance && sameHit(a.hit, b.hit))

interface StructureCanvasProps {
  document: DocumentView
  tool: Tool
  element: string
  bondOrder: DrawnOrder
  /** Selected atom (`a…`) and bond (`b…`) identifiers. */
  selection: ReadonlySet<string>
  onSelectionChange: (selection: Set<string>) => void
  onCommands: (commands: Command[]) => void
  disabled: boolean
}

/**
 * Renders a document as SVG, every scene instance at its own positions, and turns pointer
 * gestures into document commands. It holds only transient gesture state; the document itself
 * always comes from the API.
 */
export function StructureCanvas({
  document,
  tool,
  element,
  bondOrder,
  selection,
  onSelectionChange,
  onCommands,
  disabled,
}: StructureCanvasProps) {
  const svgRef = useRef<SVGSVGElement>(null)
  const drag = useRef<Drag | null>(null)
  const [preview, setPreview] = useState<Preview>(null)
  const [hover, setHover] = useState<Located | null>(null)

  // Derived per document, not per render: pointer moves re-render the canvas.
  const atoms = useMemo(() => atomIndex(document), [document])
  // One label layout per atom and placement: what is drawn, and the box that bond trimming,
  // the view box, and hit testing all use.
  const drawings = useMemo<Drawing[]>(() => {
    const counts = degrees(document)
    return placements(document).map((placement) => {
      const view = placedView(document, placement)
      const left = leaningRight(view)
      const labels = new Map<string, AtomLabel | null>(
        view.atoms.map((atom) => [atom.id, atomLabel(atom, counts.get(atom.id) ?? 0, left.has(atom.id))]),
      )
      return { id: placement.id, view, atoms: atomIndex(view), labels }
    })
  }, [document])
  const box = useMemo(
    () =>
      viewBox(
        drawings.flatMap((drawing) => drawing.view.atoms.map((atom) => atom.position)),
        drawings.flatMap((drawing) =>
          drawing.view.atoms.flatMap((atom) => labelBox(drawing.labels, atom.id, atom.position) ?? []),
        ),
      ),
    [drawings],
  )

  function drawingOf(instance: string | null) {
    return drawings.find((drawing) => drawing.id === instance) ?? drawings[0]
  }

  /** Later placements are drawn on top, so they are hit first. */
  function locate(point: Position, ignore?: ReadonlySet<string>, only?: string | null): Located {
    for (const drawing of [...drawings].reverse()) {
      if (only !== undefined && drawing.id !== only) continue
      const hit = hitTest(drawing.view, point, ignore, drawing.atoms, drawing.labels)
      if (hit) return { hit, instance: drawing.id }
    }
    return { hit: null, instance: only === undefined ? placementAt(document, point) : only }
  }

  /**
   * Commands adding a gesture's new atoms, in order, each placed in every placement and bonded
   * to `bondTo` (an existing atom or `$N`, the gesture's Nth new atom) when it has one.
   */
  function addAtoms(element: string, instance: string | null, atoms: NewAtom[]): Command[] {
    const placed = placeNewAtoms(document, instance, atoms)
    return atoms.map(({ bondTo }, index): Command => {
      const add = { op: 'add_atom' as const, element, ...placed[index] }
      return bondTo === undefined ? add : { ...add, bond_to: { atom: bondTo, order: bondOrder } }
    })
  }

  function toDocument(event: PointerEvent<SVGSVGElement>): Position {
    const svg = svgRef.current
    const matrix = svg?.getScreenCTM()
    if (!svg || !matrix) return { x: 0, y: 0 }
    const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse())
    return { x: round(point.x), y: round(point.y) }
  }

  function bondBetween(a: string, b: string) {
    return document.bonds.find((bond) => (bond.begin === a && bond.end === b) || (bond.begin === b && bond.end === a))
  }

  function movableAtoms(ids: ReadonlySet<string>) {
    const moving = new Set<string>()
    for (const id of ids) {
      if (atoms.has(id)) moving.add(id)
      const bond = document.bonds.find((candidate) => candidate.id === id)
      if (bond) {
        moving.add(bond.begin)
        moving.add(bond.end)
      }
    }
    return [...moving]
  }

  function onPointerDown(event: PointerEvent<SVGSVGElement>) {
    if (disabled || event.button !== 0) return
    event.currentTarget.setPointerCapture(event.pointerId)
    const point = toDocument(event)
    const { hit, instance } = locate(point)
    if (tool === 'select') {
      if (!hit) {
        drag.current = { kind: 'marquee', start: point, additive: event.shiftKey }
        return
      }
      let next = new Set(selection)
      if (event.shiftKey) {
        if (next.has(hit.id)) next.delete(hit.id)
        else next.add(hit.id)
      } else if (!next.has(hit.id)) {
        next = new Set([hit.id])
      }
      onSelectionChange(next)
      drag.current = { kind: 'move', start: point, atoms: next.has(hit.id) ? movableAtoms(next) : [], instance }
    } else if (tool === 'bond' && hit?.kind !== 'bond') {
      drag.current = { kind: 'bond', start: point, fromAtom: hit?.kind === 'atom' ? hit.id : null, instance }
    } else {
      drag.current = { kind: 'click', start: point, hit, instance }
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
    return { origin, atom: null, position: snappedEndpoint(origin, point) }
  }

  function onPointerMove(event: PointerEvent<SVGSVGElement>) {
    const point = toDocument(event)
    const gesture = drag.current
    if (!gesture) {
      const located = disabled ? null : locate(point)
      const next = located?.hit ? located : null
      setHover((current) => (sameLocated(current, next) ? current : next))
      return
    }
    if (distance(point, gesture.start) < DRAG_THRESHOLD) return
    if (gesture.kind === 'move' && gesture.atoms.length > 0) {
      setPreview({
        kind: 'move',
        atoms: new Set(gesture.atoms),
        delta: { x: round(point.x - gesture.start.x), y: round(point.y - gesture.start.y) },
        instance: gesture.instance,
      })
    } else if (gesture.kind === 'bond') {
      const target = bondTarget(gesture, point)
      setPreview(target && { kind: 'bond', from: target.origin, to: target.position })
    } else if (gesture.kind === 'marquee') {
      setPreview({ kind: 'marquee', from: gesture.start, to: point })
    }
  }

  function finish(event: PointerEvent<SVGSVGElement>) {
    const gesture = drag.current
    drag.current = null
    setPreview(null)
    // Busy is checked when a gesture starts, not here: a request queued meanwhile (e.g. a
    // field's blur) must not swallow it. The parent applies the commands to the latest
    // document and drops them if what they name is gone.
    if (!gesture) return
    const point = toDocument(event)
    const dragged = distance(point, gesture.start) >= DRAG_THRESHOLD
    const commands = gestureCommands(gesture, point, dragged)
    if (commands.length > 0) onCommands(commands)
  }

  function gestureCommands(gesture: Drag, point: Position, dragged: boolean): Command[] {
    switch (gesture.kind) {
      case 'move': {
        if (!dragged || gesture.atoms.length === 0) return []
        const delta = { x: round(point.x - gesture.start.x), y: round(point.y - gesture.start.y) }
        const move: Command =
          gesture.instance === null
            ? { op: 'move_atoms', atoms: gesture.atoms, delta }
            : { op: 'move_atoms', atoms: gesture.atoms, delta, instance: gesture.instance }
        return [move]
      }
      case 'marquee': {
        const left = Math.min(gesture.start.x, point.x)
        const right = Math.max(gesture.start.x, point.x)
        const top = Math.min(gesture.start.y, point.y)
        const bottom = Math.max(gesture.start.y, point.y)
        const inside = drawings.flatMap((drawing) =>
          drawing.view.atoms
            .filter(({ position: { x, y } }) => dragged && x >= left && x <= right && y >= top && y <= bottom)
            .map((atom) => atom.id),
        )
        onSelectionChange(new Set([...(gesture.additive ? selection : []), ...inside]))
        return []
      }
      case 'bond':
        return bondCommands(gesture, point, dragged)
      case 'click':
        if (dragged) return []
        return clickCommands(gesture.hit, gesture.instance, gesture.start)
    }
  }

  function bondCommands(gesture: Extract<Drag, { kind: 'bond' }>, point: Position, dragged: boolean): Command[] {
    const carbons = (atoms: NewAtom[]) => addAtoms('C', gesture.instance, atoms)
    if (!dragged) {
      if (gesture.fromAtom) {
        const position = freeNeighborPosition(drawingOf(gesture.instance).view, gesture.fromAtom)
        return position ? carbons([{ point: position, bondTo: gesture.fromAtom }]) : []
      }
      const end = snappedEndpoint(gesture.start, { x: gesture.start.x + 26, y: gesture.start.y - 15 })
      return carbons([{ point: gesture.start }, { point: end, bondTo: '$0' }])
    }
    const target = bondTarget(gesture, point)
    if (!target) return []
    if (gesture.fromAtom && target.atom) {
      const existing = bondBetween(gesture.fromAtom, target.atom)
      return existing
        ? [{ op: 'update_bond', bond: existing.id, order: bondOrder }]
        : [{ op: 'add_bond', begin: gesture.fromAtom, end: target.atom, order: bondOrder }]
    }
    if (gesture.fromAtom) {
      return carbons([{ point: target.position, bondTo: gesture.fromAtom }])
    }
    if (target.atom) {
      return carbons([{ point: gesture.start, bondTo: target.atom }])
    }
    return carbons([{ point: gesture.start }, { point: target.position, bondTo: '$0' }])
  }

  function clickCommands(hit: Hit, instance: string | null, point: Position): Command[] {
    if (tool === 'erase') {
      if (hit?.kind === 'atom') return [{ op: 'delete_atoms', atoms: [hit.id] }]
      if (hit?.kind === 'bond') return [{ op: 'delete_bonds', bonds: [hit.id] }]
      return []
    }
    if (tool === 'bond' && hit?.kind === 'bond') {
      const bond = document.bonds.find((candidate) => candidate.id === hit.id)
      return bond ? [{ op: 'update_bond', bond: bond.id, order: nextOrder(bond.order) }] : []
    }
    if (tool === 'atom') {
      if (hit?.kind === 'atom') {
        const current = atoms.get(hit.id)
        return current?.kind === 'element' && current.element === element ? [] : [{ op: 'update_atom', atom: hit.id, element }]
      }
      if (!hit) return addAtoms(element, instance, [{ point }])
    }
    return []
  }

  /** An atom's position in a drawing, moved by the drag preview in the drag's placement. */
  const position = (drawing: Drawing, id: string) => {
    const atom = drawing.atoms.get(id)!
    if (preview?.kind === 'move' && preview.instance === drawing.id && preview.atoms.has(id)) {
      return { x: atom.position.x + preview.delta.x, y: atom.position.y + preview.delta.y }
    }
    return atom.position
  }
  const hovered = (drawing: Drawing, kind: 'atom' | 'bond', id: string) =>
    hover?.instance === drawing.id && hover.hit?.kind === kind && hover.hit.id === id

  return (
    <svg
      ref={svgRef}
      role="group"
      data-structure-canvas=""
      aria-label={`${document.title}: ${document.atoms.length} atoms and ${document.bonds.length} bonds`}
      viewBox={`${box.x} ${box.y} ${box.width} ${box.height}`}
      className={cn(
        'block h-full w-full touch-none select-none rounded-lg border bg-card',
        tool === 'select' ? 'cursor-default' : 'cursor-crosshair',
        disabled && 'cursor-progress',
      )}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={() => {
        drag.current = null
        setPreview(null)
      }}
      onPointerLeave={() => setHover(null)}
    >
      {drawings.map((drawing) => (
        <g key={drawing.id ?? 'atoms'} data-instance={drawing.id ?? undefined}>
          {drawings.length > 1 && <PlacementFrame drawing={drawing} />}
          <g className="fill-primary/15 stroke-primary/40">
            {document.bonds
              .filter((bond) => selection.has(bond.id) || hovered(drawing, 'bond', bond.id))
              .map((bond) => {
                const a = position(drawing, bond.begin)
                const b = position(drawing, bond.end)
                return <line key={bond.id} x1={a.x} y1={a.y} x2={b.x} y2={b.y} strokeWidth={8} strokeLinecap="round" />
              })}
            {document.atoms
              .filter((atom) => selection.has(atom.id) || hovered(drawing, 'atom', atom.id))
              .map((atom) => {
                const p = position(drawing, atom.id)
                const label = labelBox(drawing.labels, atom.id, p)
                return label ? (
                  <rect key={atom.id} {...label} rx={3} strokeWidth={1} />
                ) : (
                  <circle key={atom.id} cx={p.x} cy={p.y} r={8} strokeWidth={1} />
                )
              })}
          </g>
          <g className="fill-none stroke-foreground" strokeWidth={1.2} strokeLinecap="round" strokeLinejoin="round">
            {document.bonds.map((bond) => {
              const begin = position(drawing, bond.begin)
              const end = position(drawing, bond.end)
              const strokes = bondStrokes(
                bond,
                begin,
                end,
                labelBox(drawing.labels, bond.begin, begin),
                labelBox(drawing.labels, bond.end, end),
              )
              return (
                <g key={bond.id} role="img" aria-label={bondName(bond)} data-bond-kind={bond.order}>
                  {strokes.map((stroke, index) =>
                    stroke.shape === 'line' ? (
                      <line
                        key={index}
                        x1={stroke.x1}
                        y1={stroke.y1}
                        x2={stroke.x2}
                        y2={stroke.y2}
                        strokeDasharray={DASHES[stroke.pattern]}
                        data-bond={bond.id}
                        data-pattern={stroke.pattern}
                      />
                    ) : (
                      <polyline key={index} points={stroke.points.map((p) => `${p.x},${p.y}`).join(' ')} data-bond={bond.id} />
                    ),
                  )}
                </g>
              )
            })}
          </g>
          <g className="fill-foreground font-sans">
            {document.atoms.map((atom) => {
              const label = drawing.labels.get(atom.id)
              return label ? <AtomMark key={atom.id} id={atom.id} label={label} at={position(drawing, atom.id)} /> : null
            })}
          </g>
        </g>
      ))}
      {preview?.kind === 'bond' && (
        <line
          x1={preview.from.x}
          y1={preview.from.y}
          x2={preview.to.x}
          y2={preview.to.y}
          className="stroke-primary"
          strokeWidth={1.2}
          strokeDasharray="3 2"
        />
      )}
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
  )
}

/** An atom's label box placed at `at`, or null for a bare vertex. */
function labelBox(labels: ReadonlyMap<string, AtomLabel | null>, id: string, at: Position): Box | null {
  const label = labels.get(id)
  return label ? placed(label.box, at) : null
}

/**
 * An atom's label as laid out: its text runs, each drawn at the computed width, then its
 * marks. The whole group is one image whose name gives the full description.
 */
function AtomMark({ id, label, at }: { id: string; label: AtomLabel; at: Position }) {
  return (
    <g role="img" aria-label={label.name} data-atom-label={id}>
      <text data-atom={id} aria-hidden="true">
        {label.texts.map((run) => (
          <tspan
            key={run.part}
            x={round(at.x + run.dx)}
            y={round(at.y + run.dy)}
            fontSize={run.size}
            fontWeight={run.part === 'symbol' || run.part === 'hydrogens' ? 500 : 400}
            dominantBaseline="central"
            textLength={round(run.width)}
            lengthAdjust="spacingAndGlyphs"
            data-part={run.part}
          >
            {run.text}
          </tspan>
        ))}
      </text>
      {label.marks.map((mark) => (
        <text
          key={mark.mark}
          x={round(at.x + mark.dx)}
          y={round(at.y + mark.dy)}
          fontSize={mark.size}
          dominantBaseline="central"
          textLength={round(mark.width)}
          lengthAdjust="spacingAndGlyphs"
          className="fill-muted-foreground"
          data-mark={mark.mark}
          aria-hidden="true"
        >
          {mark.text}
        </text>
      ))}
      {label.dots.map((dot, index) => (
        <circle
          key={index}
          cx={round(at.x + dot.dx)}
          cy={round(at.y + dot.dy)}
          r={DOT_SIZE}
          className={dot.hollow ? 'fill-none stroke-foreground' : undefined}
          strokeWidth={dot.hollow ? 0.6 : undefined}
          data-mark="radical"
          aria-hidden="true"
        />
      ))}
    </g>
  )
}

const FRAME_PADDING = 14

/**
 * A dashed outline and the instance ID around one of several placements, so each reads as its
 * own drawing. It encloses every atom and every label box.
 */
function PlacementFrame({ drawing }: { drawing: Drawing }) {
  if (drawing.view.atoms.length === 0 || drawing.id === null) return null
  const extent = union(
    drawing.view.atoms.map(
      (atom) =>
        labelBox(drawing.labels, atom.id, atom.position) ?? { x: atom.position.x, y: atom.position.y, width: 0, height: 0 },
    ),
  )
  const left = extent.x - FRAME_PADDING
  const top = extent.y - FRAME_PADDING
  return (
    <g className="pointer-events-none" aria-hidden="true">
      <rect
        x={left}
        y={top}
        width={extent.width + 2 * FRAME_PADDING}
        height={extent.height + 2 * FRAME_PADDING}
        rx={4}
        className="fill-none stroke-muted-foreground/40"
        strokeWidth={0.8}
        strokeDasharray="4 3"
      />
      <text x={left + 2} y={top - 3} fontSize={8} className="fill-muted-foreground font-sans">
        {drawing.id}
      </text>
    </g>
  )
}
