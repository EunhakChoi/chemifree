/**
 * Drawing geometry for the structure canvas. Coordinates are document points (pt) with y
 * growing downward, the same space the API stores. Nothing here perceives chemistry.
 */
import type { AtomView, BondOrder, BondView, DocumentView, Position } from '@/lib/document-contract'
import { placed, textWidth, union, type AtomLabel, type Box } from '@/editor/label'

export const BOND_LENGTH = 30
export const ATOM_HIT_RADIUS = 9
const ATOM_HIT_SHARE = 0.3
export const BOND_HIT_DISTANCE = 5
export const SELECTION_STROKE = 8
/** The canvas always shows at least this frame so drawing does not rescale constantly. */
export const FRAME = { x: 0, y: 0, width: 400, height: 240 }
const FRAME_MARGIN = 30
const ANGLE_STEP = Math.PI / 12

export type Hit = { kind: 'atom'; id: string } | { kind: 'bond'; id: string } | null

export function distance(a: Position, b: Position) {
  return Math.hypot(a.x - b.x, a.y - b.y)
}

function distanceToSegment(p: Position, a: Position, b: Position) {
  const dx = b.x - a.x
  const dy = b.y - a.y
  const lengthSquared = dx * dx + dy * dy
  const t = lengthSquared === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / lengthSquared))
  return distance(p, { x: a.x + t * dx, y: a.y + t * dy })
}

export function atomIndex(document: DocumentView) {
  return new Map(document.atoms.map((atom) => [atom.id, atom]))
}

/**
 * Vertices take precedence. For a short bond, its central strip stays clickable even when
 * atom label boxes overlap it; elsewhere labels take precedence over bonds. Pass `atoms` to
 * reuse an index and `labels` (relative to each atom) to hit labels.
 */
export function hitTest(
  document: DocumentView,
  point: Position,
  ignore: ReadonlySet<string> = new Set(),
  atoms: ReadonlyMap<string, AtomView> = atomIndex(document),
  labels: ReadonlyMap<string, AtomLabel | null> = new Map(),
): Hit {
  let best: Hit = null
  let bestDistance = Infinity
  let labelHit: Hit = null
  let labelDistance = Infinity
  const radius = new Map<string, number>()
  for (const bond of document.bonds) {
    const begin = atoms.get(bond.begin)
    const end = atoms.get(bond.end)
    if (!begin || !end) continue
    const share = ATOM_HIT_SHARE * distance(begin.position, end.position)
    for (const id of [bond.begin, bond.end]) radius.set(id, Math.min(radius.get(id) ?? ATOM_HIT_RADIUS, share))
  }
  for (const atom of document.atoms) {
    if (ignore.has(atom.id)) continue
    const d = distance(point, atom.position)
    const label = labels.get(atom.id)
    if (d <= (radius.get(atom.id) ?? ATOM_HIT_RADIUS) && d < bestDistance) {
      best = { kind: 'atom', id: atom.id }
      bestDistance = d
    }
    if (label && contains(placed(label.box, atom.position), point) && d < labelDistance) {
      labelHit = { kind: 'atom', id: atom.id }
      labelDistance = d
    }
  }
  if (best) return best
  bestDistance = BOND_HIT_DISTANCE
  for (const bond of document.bonds) {
    const begin = atoms.get(bond.begin)
    const end = atoms.get(bond.end)
    if (!begin || !end) continue
    const d = distanceToSegment(point, begin.position, end.position)
    if (d <= bestDistance) {
      best = { kind: 'bond', id: bond.id }
      bestDistance = d
    }
  }
  if (best?.kind === 'bond' && labelHit) {
    const bond = document.bonds.find((candidate) => candidate.id === best?.id)
    const begin = bond && atoms.get(bond.begin)?.position
    const end = bond && atoms.get(bond.end)?.position
    if (begin && end) {
      const length = distance(begin, end)
      const along = length === 0 ? 0 : ((point.x - begin.x) * (end.x - begin.x) + (point.y - begin.y) * (end.y - begin.y)) / (length * length)
      if (length < 2 * ATOM_HIT_RADIUS && along >= ATOM_HIT_SHARE && along <= 1 - ATOM_HIT_SHARE && bestDistance <= length * 0.2) {
        return best
      }
    }
  }
  return labelHit ?? best
}

/** The endpoint `length` away from `from`, its direction snapped to 15° steps. */
export function snappedEndpoint(from: Position, toward: Position, length = BOND_LENGTH): Position {
  const raw = Math.atan2(toward.y - from.y, toward.x - from.x)
  const angle = Math.round(raw / ANGLE_STEP) * ANGLE_STEP
  return { x: round(from.x + length * Math.cos(angle)), y: round(from.y + length * Math.sin(angle)) }
}

/**
 * A free position `length` away for a new neighbor: 120° from a single neighbor (zig-zag),
 * otherwise the middle of the widest angular gap. An isolated atom grows up and to the right.
 */
export function freeNeighborPosition(document: DocumentView, atomId: string, length = BOND_LENGTH): Position | null {
  const atoms = atomIndex(document)
  const center = atoms.get(atomId)
  if (!center) return null
  const angles = neighbors(document, atomId)
    .map((id) => atoms.get(id))
    .filter((atom): atom is AtomView => atom !== undefined)
    .map((atom) => Math.atan2(atom.position.y - center.position.y, atom.position.x - center.position.x))
    .sort((a, b) => a - b)
  let angle: number
  if (angles.length === 0) {
    angle = -Math.PI / 6
  } else if (angles.length === 1) {
    const [only] = angles
    const options = [only + (2 * Math.PI) / 3, only - (2 * Math.PI) / 3]
    // Prefer the option that keeps chains horizontal-ish, then the upper one.
    angle = options.sort((a, b) => Math.abs(Math.sin(a)) - Math.abs(Math.sin(b)) || Math.sin(a) - Math.sin(b))[0]
  } else {
    let widest = 0
    angle = 0
    angles.forEach((current, index) => {
      const next = index + 1 < angles.length ? angles[index + 1] : angles[0] + 2 * Math.PI
      if (next - current > widest) {
        widest = next - current
        angle = current + widest / 2
      }
    })
  }
  return {
    x: round(center.position.x + length * Math.cos(angle)),
    y: round(center.position.y + length * Math.sin(angle)),
  }
}

export function neighbors(document: DocumentView, atomId: string) {
  return document.bonds.flatMap((bond) =>
    bond.begin === atomId ? [bond.end] : bond.end === atomId ? [bond.begin] : [],
  )
}

/** Bond count per atom in one pass over the bonds. */
export function degrees(document: DocumentView) {
  const counts = new Map<string, number>()
  for (const { begin, end } of document.bonds) {
    counts.set(begin, (counts.get(begin) ?? 0) + 1)
    counts.set(end, (counts.get(end) ?? 0) + 1)
  }
  return counts
}

export function sameHit(a: Hit, b: Hit) {
  return a === b || (a !== null && b !== null && a.kind === b.kind && a.id === b.id)
}

/**
 * The atoms whose bonds lean right in `view`, so that their stated hydrogens go on the left
 * ("H3C–") instead of over a bond.
 */
export function leaningRight(view: DocumentView): Set<string> {
  const atoms = atomIndex(view)
  const lean = new Map<string, number>()
  for (const bond of view.bonds) {
    const begin = atoms.get(bond.begin)
    const end = atoms.get(bond.end)
    if (!begin || !end) continue
    const length = distance(begin.position, end.position)
    if (length === 0) continue
    const dx = (end.position.x - begin.position.x) / length
    lean.set(bond.begin, (lean.get(bond.begin) ?? 0) + dx)
    lean.set(bond.end, (lean.get(bond.end) ?? 0) - dx)
  }
  // A small threshold keeps a vertical bond from tipping the side by rounding.
  return new Set([...lean].filter(([, sum]) => sum > 0.01).map(([id]) => id))
}

/**
 * The ink of a bond stops this far outside a label box: a stroke ends this much plus half its
 * width (its round cap) away, 2 pt at the built-in line width.
 */
export const LABEL_GAP = 1.4
/** A bond squeezed between two labels keeps a stroke this long in the middle of the room left. */
/** A label box or placement frame keeps this much room inside the view box. */
const LABEL_MARGIN = 4

/** One stroke of a drawn bond: a straight line, or a polyline for an arrowhead or zigzag. */
export type Stroke =
  | { shape: 'line'; x1: number; y1: number; x2: number; y2: number; pattern: 'solid' | 'dashed' | 'dotted' }
  | { shape: 'polyline'; points: Position[] }

function grown(box: Box, by: number): Box {
  return { x: box.x - by, y: box.y - by, width: box.width + 2 * by, height: box.height + 2 * by }
}

export function contains(box: Box, point: Position) {
  return point.x >= box.x && point.x <= box.x + box.width && point.y >= box.y && point.y <= box.y + box.height
}

/**
 * Where the line through `a` and `b` (`a` at 0, `b` at 1) runs inside `box`: its entry and exit
 * parameters, or null when it misses the box.
 */
function inside(a: Position, b: Position, box: Box): [number, number] | null {
  const dx = b.x - a.x
  const dy = b.y - a.y
  let enter = -Infinity
  let leave = Infinity
  const edges: Array<[number, number]> = [
    [-dx, a.x - box.x],
    [dx, box.x + box.width - a.x],
    [-dy, a.y - box.y],
    [dy, box.y + box.height - a.y],
  ]
  for (const [direction, room] of edges) {
    if (direction === 0) {
      if (room < 0) return null
      continue
    }
    const t = room / direction
    if (direction < 0) enter = Math.max(enter, t)
    else leave = Math.min(leave, t)
  }
  return enter <= leave ? [enter, leave] : null
}

/**
 * The part of the segment from `a` to `b` outside both label boxes, each grown by `gap`
 * ([LABEL_GAP] and half the built-in stroke width by default): from where it leaves the box at
 * `a` to where it enters the box at `b`. If the boxes leave no room, there is no visible
 * stroke; hit testing still uses the full bond. A narrow gap produces a shorter stroke.
 */
export function clipped(
  a: Position,
  b: Position,
  aBox: Box | null,
  bBox: Box | null,
  gap = LABEL_GAP + LINE_WIDTH / 2,
): [Position, Position] | null {
  const length = distance(a, b)
  const at = (t: number) => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t })
  const fromA = aBox && inside(a, b, grown(aBox, gap))
  const intoB = bBox && inside(a, b, grown(bBox, gap))
  // Only a box the segment starts (ends) in trims it.
  const start = fromA && fromA[0] <= 0 ? Math.min(1, Math.max(0, fromA[1])) : 0
  const end = intoB && intoB[1] >= 1 ? Math.max(0, Math.min(1, intoB[0])) : 1
  return end > start && length > 0 ? [at(start), at(end)] : null
}

/** How far an aromatic bond's dashed line runs beside its line. */
const INNER_OFFSET = 3
/** How much shorter the dashed line is at a bare vertex, so inner lines meeting there do not cross. */
const INNER_INSET = 4

/** The largest ring (in atoms) whose side an aromatic bond's inner line follows. */
const RING_LIMIT = 8

/**
 * For each bond in a ring of at most [RING_LIMIT] atoms, the atoms of the smallest such ring
 * through it (one of them when rings of that size are fused there). It depends only on the
 * graph, so every placement of a document shares one result.
 */
export function bondRings(view: Pick<DocumentView, 'bonds'>): Map<string, string[]> {
  const around = new Map<string, { atom: string; bond: string }[]>()
  const add = (from: string, atom: string, bond: string) => {
    const steps = around.get(from)
    if (steps) steps.push({ atom, bond })
    else around.set(from, [{ atom, bond }])
  }
  for (const { id, begin, end } of view.bonds) {
    add(begin, end, id)
    add(end, begin, id)
  }
  const rings = new Map<string, string[]>()
  for (const bond of view.bonds) {
    // Breadth first from begin to end without this bond, at most RING_LIMIT - 1 steps.
    const previous = new Map<string, string | null>([[bond.begin, null]])
    let frontier = [bond.begin]
    for (let steps = 1; steps < RING_LIMIT && frontier.length > 0 && !previous.has(bond.end); steps += 1) {
      const next: string[] = []
      for (const atom of frontier) {
        for (const step of around.get(atom) ?? []) {
          if (step.bond === bond.id || previous.has(step.atom)) continue
          previous.set(step.atom, atom)
          next.push(step.atom)
        }
      }
      frontier = next
    }
    if (!previous.has(bond.end)) continue
    const ring: string[] = []
    for (let atom: string | null | undefined = bond.end; atom; atom = previous.get(atom)) ring.push(atom)
    rings.set(bond.id, ring)
  }
  return rings
}

/**
 * Which side of each bond (+1: left of begin → end, in document coordinates; -1: right) its
 * inner line goes. A bond in a ring (see bondRings; pass the document's, found once) takes the
 * side of that ring's centre, so the line is inside the ring however its bonds were drawn and
 * whatever is bonded to it. Any other bond takes the side where more of the atoms bonded to
 * either end lie, and +1 without such neighbours.
 */
export function bondSides(
  view: DocumentView,
  atoms: ReadonlyMap<string, AtomView> = atomIndex(view),
  rings: ReadonlyMap<string, readonly string[]> = bondRings(view),
) {
  const around = new Map<string, string[]>()
  for (const [from, to] of view.bonds.flatMap(({ begin, end }) => [
    [begin, end],
    [end, begin],
  ])) {
    const neighbours = around.get(from)
    if (neighbours) neighbours.push(to)
    else around.set(from, [to])
  }
  const sides = new Map<string, 1 | -1>()
  for (const bond of view.bonds) {
    const begin = atoms.get(bond.begin)?.position
    const end = atoms.get(bond.end)?.position
    if (!begin || !end) continue
    const length = distance(begin, end)
    if (length === 0) continue
    const normal = { x: -(end.y - begin.y) / length, y: (end.x - begin.x) / length }
    const toward = (point: Position, from: Position) => (point.x - from.x) * normal.x + (point.y - from.y) * normal.y
    const ring = rings.get(bond.id)?.flatMap((id) => atoms.get(id)?.position ?? [])
    if (ring && ring.length > 0) {
      const centre = {
        x: ring.reduce((sum, point) => sum + point.x, 0) / ring.length,
        y: ring.reduce((sum, point) => sum + point.y, 0) / ring.length,
      }
      const lean = toward(centre, begin)
      if (lean !== 0) {
        sides.set(bond.id, lean < 0 ? -1 : 1)
        continue
      }
    }
    let lean = 0
    for (const [from, other, at] of [
      [bond.begin, bond.end, begin],
      [bond.end, bond.begin, end],
    ] as const) {
      for (const id of around.get(from) ?? []) {
        const neighbour = atoms.get(id)?.position
        if (id === other || !neighbour) continue
        lean += Math.sign(toward(neighbour, at))
      }
    }
    sides.set(bond.id, lean < 0 ? -1 : 1)
  }
  return sides
}

const OFFSETS: Partial<Record<BondOrder, number[]>> = {
  single: [0],
  double: [-2.2, 2.2],
  triple: [-3.4, 0, 3.4],
}

/** The built-in stroke width of bonds and graphics; a document's own is in its settings. */
export const LINE_WIDTH = 1.2

/** A triple bond's outer lines, the farthest any bond puts a line from its axis. */
const WIDEST_OFFSET = 3.4
/** How far a dative bond's arrowhead and an unknown bond's zigzag reach beside the axis. */
const ARROWHEAD_HALF_WIDTH = 2.5
const ZIGZAG_HALF_WIDTH = 2

/**
 * How much farther apart than built in the lines of a double, triple, or aromatic bond are
 * drawn with strokes `lineWidth` wide. Thinner strokes keep the built-in spacing.
 */
export function bondSpacing(lineWidth: number) {
  return Math.max(1, lineWidth / LINE_WIDTH)
}

/** How far the ink of a bond of `order` reaches from its axis, its round caps included. */
export function bondReach(order: BondOrder, lineWidth: number, spacing: number) {
  const beside: Record<BondOrder, number> = {
    single: 0,
    query: 0,
    double: (OFFSETS.double?.[1] ?? 0) * spacing,
    triple: WIDEST_OFFSET * spacing,
    aromatic: INNER_OFFSET * spacing,
    dative: ARROWHEAD_HALF_WIDTH,
    unknown: ZIGZAG_HALF_WIDTH,
  }
  return beside[order] + lineWidth / 2
}

/**
 * How far the ink of any bond reaches from its axis, and so beyond its atoms: placement frames,
 * what a placement draws, and the view box keep at least this much room around atoms.
 */
export function strokeReach(lineWidth: number, spacing: number) {
  return Math.max(WIDEST_OFFSET * spacing, ARROWHEAD_HALF_WIDTH, ZIGZAG_HALF_WIDTH) + lineWidth / 2
}

/** The width of the stroke underlaying a selected or hovered bond: it shows beyond its outermost line. */
export function bondUnderlay(order: BondOrder, lineWidth: number, spacing: number) {
  return Math.max(SELECTION_STROKE, 2 * bondReach(order, lineWidth, spacing) + 6)
}

/** The width of the stroke underlaying a selected graphic drawn `lineWidth` wide. */
export function graphicUnderlay(lineWidth: number) {
  return Math.max(SELECTION_STROKE, lineWidth + 6)
}

/**
 * The strokes that draw `bond` from `begin` to `end`, each trimmed where it meets a label box
 * (null for a bare vertex) so that strokes `lineWidth` wide keep [LABEL_GAP] from it. Every
 * kind looks different: aromatic is a line with a dashed line beside it on `side` (see
 * bondSides), shortened at bare vertices; dative a line with an arrowhead at its end (the bond
 * points away from its donor); query a dotted line; and unknown a zigzag. `spacing` (see
 * bondSpacing) multiplies how far apart parallel lines are.
 */
export function bondStrokes(
  bond: BondView,
  begin: Position,
  end: Position,
  beginBox: Box | null,
  endBox: Box | null,
  side: 1 | -1 = 1,
  spacing = 1,
  lineWidth = LINE_WIDTH,
): Stroke[] {
  const length = distance(begin, end)
  if (length === 0) return []
  const gap = LABEL_GAP + lineWidth / 2
  const clip = (a: Position, b: Position) => clipped(a, b, beginBox, endBox, gap)
  const ux = (end.x - begin.x) / length
  const uy = (end.y - begin.y) / length
  const shifted = (point: Position, offset: number) => ({ x: point.x - uy * offset, y: point.y + ux * offset })
  const line = (offset: number, pattern: 'solid' | 'dashed' | 'dotted', inset = 0): (Stroke & { shape: 'line' }) | null => {
    const pulled = Math.min(inset, length / 4)
    const from = beginBox ? begin : { x: begin.x + ux * pulled, y: begin.y + uy * pulled }
    const to = endBox ? end : { x: end.x - ux * pulled, y: end.y - uy * pulled }
    const segment = clip(shifted(from, offset), shifted(to, offset))
    if (!segment) return null
    const [a, b] = segment
    return { shape: 'line', x1: round(a.x), y1: round(a.y), x2: round(b.x), y2: round(b.y), pattern }
  }
  switch (bond.order) {
    case 'single':
    case 'double':
    case 'triple':
      return (OFFSETS[bond.order] ?? [0]).map((offset) => line(offset * spacing, 'solid')).filter((stroke) => stroke !== null)
    case 'aromatic':
      return [line(0, 'solid'), line(side * INNER_OFFSET * spacing, 'dashed', INNER_INSET)].filter((stroke) => stroke !== null)
    case 'query':
      return [line(0, 'dotted')].filter((stroke) => stroke !== null)
    case 'dative': {
      const shaft = line(0, 'solid')
      if (!shaft) return []
      const tip = { x: shaft.x2, y: shaft.y2 }
      const size = Math.min(4, distance({ x: shaft.x1, y: shaft.y1 }, tip))
      const back = { x: tip.x - ux * size, y: tip.y - uy * size }
      const head = [shifted(back, ARROWHEAD_HALF_WIDTH), tip, shifted(back, -ARROWHEAD_HALF_WIDTH)].map((p) => ({ x: round(p.x), y: round(p.y) }))
      return [shaft, { shape: 'polyline', points: head }]
    }
    case 'unknown': {
      const segment = clip(begin, end)
      if (!segment) return []
      const [a, b] = segment
      const run = distance(a, b)
      const teeth = Math.max(2, Math.round(run / 3))
      const points = Array.from({ length: teeth + 1 }, (_, index) => {
        const t = index / teeth
        const side = index === 0 || index === teeth ? 0 : index % 2 === 0 ? -ZIGZAG_HALF_WIDTH : ZIGZAG_HALF_WIDTH
        return shifted({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, side)
      }).map((p) => ({ x: round(p.x), y: round(p.y) }))
      return [{ shape: 'polyline', points }]
    }
    default: {
      const unhandled: never = bond.order
      return unhandled
    }
  }
}

/**
 * Strokes of every bond of a drawing, by bond ID: label boxes come from `labels` (relative to
 * each atom), inner-line sides from bondSides, the spacing of parallel lines from `spacing`,
 * strokes are `lineWidth` wide.
 */
export function drawingStrokes(
  view: DocumentView,
  labels: ReadonlyMap<string, AtomLabel | null>,
  atoms: ReadonlyMap<string, AtomView> = atomIndex(view),
  sides: ReadonlyMap<string, 1 | -1> = bondSides(view, atoms),
  spacing = 1,
  lineWidth = LINE_WIDTH,
) {
  const strokes = new Map<string, Stroke[]>()
  for (const bond of view.bonds) {
    const begin = atoms.get(bond.begin)
    const end = atoms.get(bond.end)
    if (!begin || !end) continue
    const box = (atom: AtomView) => {
      const label = labels.get(atom.id)
      return label ? placed(label.box, atom.position) : null
    }
    strokes.set(
      bond.id,
      bondStrokes(bond, begin.position, end.position, box(begin), box(end), sides.get(bond.id), spacing, lineWidth),
    )
  }
  return strokes
}

/** How `bond` is read out: its kind and the atoms it joins, and its placement when given. */
export function bondName(bond: BondView, placement: string | null = null) {
  const id = placement === null ? bond.id : `${bond.id} in ${placement}`
  switch (bond.order) {
    case 'dative':
      return `Bond ${id}: dative bond from ${bond.begin} to ${bond.end}`
    case 'query': {
      const what = bond.query?.kind === 'list' ? bond.query.orders.join(' or ') : 'any kind'
      return `Bond ${id}: query bond, ${what}, between ${bond.begin} and ${bond.end}`
    }
    case 'unknown':
      return `Bond ${id}: bond of unknown kind between ${bond.begin} and ${bond.end}`
    case 'single':
    case 'double':
    case 'triple':
    case 'aromatic':
      return `Bond ${id}: ${bond.order} bond between ${bond.begin} and ${bond.end}`
    default: {
      const unhandled: never = bond.order
      return unhandled
    }
  }
}

/** Room left around a placement's labels inside its frame. */
export const FRAME_PADDING = 14
const FRAME_ID_SIZE = 8

/**
 * The dashed frame drawn around one of several placements: `rect` keeps FRAME_PADDING around
 * its atoms and their label boxes (already placed), more when bonds reach farther than that
 * from their atoms (`reach`, see strokeReach), the instance ID is drawn above it from `idAt`
 * (start of the text, its middle), and `box` holds both, stroke included.
 */
export function placementFrame(positions: Position[], labels: Box[], id: string, reach = 0) {
  const extent = union([...positions.map(({ x, y }) => ({ x, y, width: 0, height: 0 })), ...labels])
  const rect = grown(extent, Math.max(FRAME_PADDING, reach + LABEL_MARGIN))
  const idAt = { x: rect.x + 2, y: rect.y - 6 }
  const idBox = {
    x: idAt.x - 0.04 * FRAME_ID_SIZE,
    y: idAt.y - 0.7 * FRAME_ID_SIZE,
    width: textWidth(id, FRAME_ID_SIZE) + 0.08 * FRAME_ID_SIZE,
    height: 1.4 * FRAME_ID_SIZE,
  }
  const stroke = { x: rect.x - 0.5, y: rect.y - 0.5, width: rect.width + 1, height: rect.height + 1 }
  return { rect, idAt, idSize: FRAME_ID_SIZE, box: union([stroke, idBox]) }
}

/**
 * The frame, grown to include every atom with a margin and every box (already placed) drawn
 * around them: label boxes and placement frames.
 */
export function viewBox(positions: Position[], boxes: Box[] = [], reach = 0) {
  let minX = FRAME.x
  let minY = FRAME.y
  let maxX = FRAME.x + FRAME.width
  let maxY = FRAME.y + FRAME.height
  const margin = Math.max(FRAME_MARGIN, reach + LABEL_MARGIN)
  for (const { x, y } of positions) {
    minX = Math.min(minX, x - margin)
    minY = Math.min(minY, y - margin)
    maxX = Math.max(maxX, x + margin)
    maxY = Math.max(maxY, y + margin)
  }
  for (const box of boxes) {
    minX = Math.min(minX, box.x - LABEL_MARGIN)
    minY = Math.min(minY, box.y - LABEL_MARGIN)
    maxX = Math.max(maxX, box.x + box.width + LABEL_MARGIN)
    maxY = Math.max(maxY, box.y + box.height + LABEL_MARGIN)
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY }
}

export function round(value: number) {
  return Math.round(value * 100) / 100
}
