import { memo, useMemo, useRef, useState, type PointerEvent } from 'react'

import { cn } from '@/lib/utils'
import { placements, type AtomView, type BondOrder, type Command, type DocumentView, type Position, type SceneObject } from '@/lib/document-contract'
import { placeNewAtoms, placedView, placementAt } from '@/editor/placement'
import { drawBond, type BondEnd, type DrawnOrder } from '@/editor/bond-gestures'
import {
  atomIndex,
  bondName,
  bondRings,
  bondSides,
  bondStrokes,
  degrees,
  distance,
  drawingStrokes,
  freeNeighborPosition,
  hitTest,
  leaningRight,
  placementFrame,
  round,
  sameHit,
  snappedEndpoint,
  viewBox,
  type Hit,
  type Stroke,
} from '@/editor/geometry'
import { DOT_SIZE, atomLabel, atomName, placed, type AtomLabel, type Box, type LabelChunk } from '@/editor/label'

export type { DrawnOrder } from '@/editor/bond-gestures'

export type Tool = 'select' | 'atom' | 'bond' | 'erase'

/** Below this pointer travel (pt) a press is a click, not a drag. */
const DRAG_THRESHOLD = 3
/** A bond tool click cycles single, double, and triple; any other kind becomes single. */
const nextOrder = (order: BondOrder): DrawnOrder => (order === 'single' ? 'double' : order === 'double' ? 'triple' : 'single')

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
 * One placement of the document, drawn with its own atom positions, and everything derived
 * from it once per document rather than per render: label layouts (relative to each atom;
 * which side stated hydrogens take depends on the placement's bonds), the side of each bond's
 * inner line, every bond's strokes, and its frame when there are several placements.
 */
interface Drawing {
  id: string | null
  view: DocumentView
  atoms: ReadonlyMap<string, AtomView>
  labels: ReadonlyMap<string, AtomLabel | null>
  sides: ReadonlyMap<string, 1 | -1>
  strokes: ReadonlyMap<string, Stroke[]>
  frame: ReturnType<typeof placementFrame> | null
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
    const all = placements(document)
    const order = document.scene?.objects?.filter((object) => object.kind === 'instance').map((object) => object.id)
    if (order) all.sort((a, b) => order.indexOf(a.id ?? '') - order.indexOf(b.id ?? ''))
    // Rings depend on the graph alone: searched once for every placement.
    const rings = bondRings(document)
    // A label depends on the atom and the side its hydrogens take, not on the placement.
    const layouts = new Map<string, AtomLabel | null>()
    const layoutOf = (atom: AtomView, left: boolean) => {
      const key = `${atom.id}:${left}`
      if (!layouts.has(key)) layouts.set(key, atomLabel(atom, counts.get(atom.id) ?? 0, left))
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
            )
          : null
      return { id: placement.id, view, atoms: index, labels, sides, strokes: drawingStrokes(view, labels, index, sides), frame }
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
        ]),
      ),
    [document.scene, drawings],
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

  /** The command adding an unbonded atom of `element` at `point`, placed in every placement. */
  function addAtom(element: string, instance: string | null, point: Position): Command {
    const [placement] = placeNewAtoms(document, instance, [{ point }])
    return { op: 'add_atom', element, ...placement }
  }

  function toDocument(event: PointerEvent<SVGSVGElement>): Position {
    const svg = svgRef.current
    const matrix = svg?.getScreenCTM()
    if (!svg || !matrix) return { x: 0, y: 0 }
    const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse())
    return { x: round(point.x), y: round(point.y) }
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

  /** A bond gesture's commands: the bond begins where the gesture starts (see drawBond). */
  function bondCommands(gesture: Extract<Drag, { kind: 'bond' }>, point: Position, dragged: boolean): Command[] {
    const from: BondEnd = gesture.fromAtom ? { atom: gesture.fromAtom } : { point: gesture.start }
    let to: BondEnd
    if (!dragged) {
      const end = gesture.fromAtom
        ? freeNeighborPosition(drawingOf(gesture.instance).view, gesture.fromAtom)
        : snappedEndpoint(gesture.start, { x: gesture.start.x + 26, y: gesture.start.y - 15 })
      if (!end) return []
      to = { point: end }
    } else {
      const target = bondTarget(gesture, point)
      if (!target) return []
      to = target.atom ? { atom: target.atom } : { point: target.position }
    }
    return drawBond(document, gesture.instance, from, to, bondOrder)
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
      if (!hit) return [addAtom(element, instance, point)]
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
      <DrawingLayer
        drawing={drawing}
        several={drawings.length > 1}
        moved={preview?.kind === 'move' && preview.instance === drawing.id ? preview : null}
      />
    </g>
  )

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
      {sceneObjects ? sceneObjects.map((object) => {
        if (object.kind === 'group') return null
        if (object.kind === 'graphic') return (
          <SceneGraphic key={object.id} object={object} selected={sceneSelected(object)} onPointerDown={(event) => {
            if (tool !== 'select' || disabled) return
            event.stopPropagation()
            const next = event.shiftKey ? new Set(selection) : new Set<string>()
            if (event.shiftKey && next.has(object.id)) next.delete(object.id)
            else next.add(object.id)
            onSelectionChange(next)
          }} />
        )
        const drawing = drawings.find((item) => item.id === object.id)
        return drawing ? drawInstance(drawing) : null
      }) : drawings.map(drawInstance)}
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

function SceneGraphic({ object, selected, onPointerDown }: {
  object: Extract<SceneObject, { kind: 'graphic' }>
  selected: boolean
  onPointerDown: (event: PointerEvent<SVGGElement>) => void
}) {
  const [a, b] = object.points
  const length = Math.hypot(b.x - a.x, b.y - a.y)
  const ux = length > 0 ? (b.x - a.x) / length : 0
  const uy = length > 0 ? (b.y - a.y) / length : 0
  const head = `${b.x},${b.y} ${b.x - ux * 6 - uy * 3},${b.y - uy * 6 + ux * 3} ${b.x - ux * 6 + uy * 3},${b.y - uy * 6 - ux * 3}`
  const rect = { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) }
  return (
    <g data-scene-object={object.id} onPointerDown={onPointerDown}>
      {object.shape === 'rectangle' ? (
        <>
          <rect {...rect} fill="none" stroke="transparent" strokeWidth={10} />
          <rect {...rect} fill="none" className={selected ? 'stroke-primary' : 'stroke-foreground'} strokeWidth={selected ? 2 : 1.2} />
        </>
      ) : (
        <>
          <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="transparent" strokeWidth={10} />
          <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className={selected ? 'stroke-primary' : 'stroke-foreground'} strokeWidth={selected ? 2 : 1.2} />
          {object.shape === 'arrow' && length > 0 && <polygon points={head} className={selected ? 'fill-primary' : 'fill-foreground'} />}
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
    )
  }
  return (
    <>
      <g className="fill-none stroke-foreground" strokeWidth={1.2} strokeLinecap="round" strokeLinejoin="round">
        {drawing.view.bonds.map((bond) => (
          <g key={bond.id} role="img" aria-label={bondName(bond, placement)} data-bond-kind={bond.order}>
            {strokesOf(bond).map((stroke, index) =>
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
        ))}
      </g>
      <g className="fill-foreground font-sans">
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
