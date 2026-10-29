import { useMemo, useRef, useState, type PointerEvent } from 'react'

import { cn } from '@/lib/utils'
import { placements, type AtomView, type BondOrder, type Command, type DocumentView, type Position } from '@/lib/document-contract'
import { attachmentText, bondName, describeStates, hydrogenText, radicalMark } from '@/editor/atom-states'
import { placeNewAtoms, placedView, placementAt, type NewAtom } from '@/editor/placement'
import {
  atomIndex,
  bondSegments,
  chargeText,
  degrees,
  distance,
  freeNeighborPosition,
  hitTest,
  labelBox,
  round,
  sameHit,
  showsLabel,
  snappedEndpoint,
  viewBox,
  type Hit,
} from '@/editor/geometry'

export type Tool = 'select' | 'atom' | 'bond' | 'erase'

/** Below this pointer travel (pt) a press is a click, not a drag. */
const DRAG_THRESHOLD = 3
/**
 * A click with the bond tool cycles these orders. Other kinds (aromatic, dative, query,
 * unknown) are changed in the inspector, so a stray click never turns one into a plain bond.
 */
const NEXT_ORDER: Partial<Record<BondOrder, BondOrder>> = { single: 'double', double: 'triple', triple: 'single' }

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

/** One placement of the document, drawn with its own atom positions. */
interface Drawing {
  id: string | null
  view: DocumentView
  atoms: ReadonlyMap<string, AtomView>
}

const sameLocated = (a: Located | null, b: Located | null) =>
  a === b || (a !== null && b !== null && a.instance === b.instance && sameHit(a.hit, b.hit))

interface StructureCanvasProps {
  document: DocumentView
  tool: Tool
  element: string
  bondOrder: BondOrder
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
  const drawings = useMemo<Drawing[]>(
    () =>
      placements(document).map((placement) => {
        const view = placedView(document, placement)
        return { id: placement.id, view, atoms: atomIndex(view) }
      }),
    [document],
  )
  const labeled = useMemo(() => {
    const counts = degrees(document)
    return new Map(document.atoms.map((atom) => [atom.id, showsLabel(atom, counts.get(atom.id) ?? 0)]))
  }, [document])
  /** The box each labelled atom's text covers; bonds stop at it. */
  const labelBoxes = useMemo(
    () => new Map(document.atoms.filter((atom) => labeled.get(atom.id)).map((atom) => [atom.id, labelBox(atom)])),
    [document, labeled],
  )
  const box = viewBox(drawings.flatMap((drawing) => drawing.view.atoms.map((atom) => atom.position)))
  const states = useMemo(() => describeStates(document), [document])

  function drawingOf(instance: string | null) {
    return drawings.find((drawing) => drawing.id === instance) ?? drawings[0]
  }

  /** Later placements are drawn on top, so they are hit first. */
  function locate(point: Position, ignore?: ReadonlySet<string>, only?: string | null): Located {
    for (const drawing of [...drawings].reverse()) {
      if (only !== undefined && drawing.id !== only) continue
      const hit = hitTest(drawing.view, point, ignore, drawing.atoms)
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
      const next = bond && NEXT_ORDER[bond.order]
      return bond && next ? [{ op: 'update_bond', bond: bond.id, order: next }] : []
    }
    if (tool === 'atom') {
      if (hit?.kind === 'atom') {
        return atoms.get(hit.id)?.element === element ? [] : [{ op: 'update_atom', atom: hit.id, element }]
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
      role="img"
      aria-label={`${document.title}: ${document.atoms.length} atoms and ${document.bonds.length} bonds${states.length ? `; ${states.join('; ')}` : ''}`}
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
                return <circle key={atom.id} cx={p.x} cy={p.y} r={8} strokeWidth={1} />
              })}
          </g>
          <g className="stroke-foreground" strokeWidth={1.2} strokeLinecap="round">
            {document.bonds.map((bond) => (
              <g key={bond.id} data-bond-kind={bond.order}>
                <title>{bondName(bond)}</title>
                {bondSegments(
                  bond,
                  position(drawing, bond.begin),
                  position(drawing, bond.end),
                  labelBoxes.get(bond.begin) ?? null,
                  labelBoxes.get(bond.end) ?? null,
                ).map(({ dash, ...segment }, index) => (
                  <line key={index} {...segment} strokeDasharray={dash} data-bond={bond.id} />
                ))}
              </g>
            ))}
          </g>
          <g className="fill-foreground font-sans" fontSize={12} fontWeight={500}>
            {document.atoms
              .filter((atom) => labeled.get(atom.id))
              .map((atom) => {
                const p = position(drawing, atom.id)
                const charge = chargeText(atom.charge)
                const hydrogens = hydrogenText(atom)
                // A stated count is a subscript after the H; `H?` stays on the line.
                const count = /^H(\d+)$/.exec(hydrogens)?.[1]
                return (
                  <text key={atom.id} x={p.x} y={p.y} textAnchor="middle" dominantBaseline="central" data-atom={atom.id}>
                    {atom.isotope !== null && (
                      <tspan dy={-5} fontSize={8}>
                        {atom.isotope}
                      </tspan>
                    )}
                    <tspan dy={atom.isotope !== null ? 5 : 0}>{atom.element}</tspan>
                    {hydrogens && (
                      <tspan data-hydrogens={hydrogens}>
                        {count === undefined ? hydrogens : 'H'}
                        {count !== undefined && (
                          <tspan dy={3} fontSize={8}>
                            {count}
                          </tspan>
                        )}
                      </tspan>
                    )}
                    {charge && (
                      <tspan dy={count !== undefined ? -8 : -5} fontSize={8}>
                        {charge}
                      </tspan>
                    )}
                  </text>
                )
              })}
          </g>
          <g className="fill-foreground">
            {document.atoms.map((atom) => {
              const mark = radicalMark(atom.radical)
              if (!mark) return null
              const p = position(drawing, atom.id)
              const xs = mark.dots === 1 ? [p.x] : [p.x - 2.2, p.x + 2.2]
              return (
                <g key={atom.id} data-radical={atom.id}>
                  <title>{`${atom.radical} radical on ${atom.id}`}</title>
                  {xs.map((x) => (
                    <circle key={x} cx={x} cy={p.y - 10} r={1.3} />
                  ))}
                  {mark.uncertain && (
                    <text x={p.x + 3} y={p.y - 10} fontSize={7} dominantBaseline="central" className="font-sans">
                      ?
                    </text>
                  )}
                </g>
              )
            })}
          </g>
          <g className="fill-muted-foreground font-sans" fontSize={8}>
            {document.atoms.map((atom) => {
              const mark = attachmentText(atom.attachment_points)
              if (!mark) return null
              const p = position(drawing, atom.id)
              return (
                <text key={atom.id} x={p.x + 7} y={p.y + 9} dominantBaseline="central" data-attachments={atom.id}>
                  <title>{`attachment points ${atom.attachment_points?.join(', ')} on ${atom.id}`}</title>
                  {mark}
                </text>
              )
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

const FRAME_PADDING = 14

/** A dashed outline and the instance ID around one of several placements, so each reads as its own drawing. */
function PlacementFrame({ drawing }: { drawing: Drawing }) {
  const points = drawing.view.atoms.map((atom) => atom.position)
  if (points.length === 0 || drawing.id === null) return null
  const xs = points.map((point) => point.x)
  const ys = points.map((point) => point.y)
  const left = Math.min(...xs) - FRAME_PADDING
  const top = Math.min(...ys) - FRAME_PADDING
  return (
    <g className="pointer-events-none" aria-hidden="true">
      <rect
        x={left}
        y={top}
        width={Math.max(...xs) - Math.min(...xs) + 2 * FRAME_PADDING}
        height={Math.max(...ys) - Math.min(...ys) + 2 * FRAME_PADDING}
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
