/**
 * Drawing geometry for the structure canvas. Coordinates are document points (pt) with y
 * growing downward, the same space the API stores. Nothing here perceives chemistry.
 */
import type { AtomView, BondOrder, BondView, DocumentView, Position } from '@/lib/document-contract'
import { placed, type AtomLabel, type Box } from '@/editor/label'

export const BOND_LENGTH = 30
export const ATOM_HIT_RADIUS = 9
export const BOND_HIT_DISTANCE = 5
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
 * Atoms take precedence over bonds; the nearest candidate wins. A labelled atom is hit anywhere
 * in its label box, a bare vertex within [ATOM_HIT_RADIUS]. Pass `atoms` to reuse an index and
 * `labels` (relative to each atom) to hit labels.
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
  for (const atom of document.atoms) {
    if (ignore.has(atom.id)) continue
    const d = distance(point, atom.position)
    const label = labels.get(atom.id)
    const hit = label ? contains(placed(label.box, atom.position), point) : d <= ATOM_HIT_RADIUS
    if (hit && d < bestDistance) {
      best = { kind: 'atom', id: atom.id }
      bestDistance = d
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
  return best
}

/** Fixed-length endpoint whose direction is snapped to 15° steps. */
export function snappedEndpoint(from: Position, toward: Position): Position {
  const raw = Math.atan2(toward.y - from.y, toward.x - from.x)
  const angle = Math.round(raw / ANGLE_STEP) * ANGLE_STEP
  return { x: round(from.x + BOND_LENGTH * Math.cos(angle)), y: round(from.y + BOND_LENGTH * Math.sin(angle)) }
}

/**
 * A free position for a new neighbor: 120° from a single neighbor (zig-zag), otherwise
 * the middle of the widest angular gap. An isolated atom grows up and to the right.
 */
export function freeNeighborPosition(document: DocumentView, atomId: string): Position | null {
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
    x: round(center.position.x + BOND_LENGTH * Math.cos(angle)),
    y: round(center.position.y + BOND_LENGTH * Math.sin(angle)),
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

/** Bonds stop this far outside a label box. */
export const LABEL_GAP = 2
/** A bond squeezed between two labels keeps a stroke this long in the middle of the room left. */
export const SQUEEZED_STROKE = 3
/** A label box keeps this much room inside the view box. */
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
 * The part of the segment from `a` to `b` outside both label boxes, each grown by
 * [LABEL_GAP]: from where it leaves the box at `a` to where it enters the box at `b`. When the
 * boxes leave less than [SQUEEZED_STROKE], a stroke that long stays in the middle of the room
 * there is (of the bond when there is none), so a bond is never drawn reversed or left out.
 */
export function clipped(a: Position, b: Position, aBox: Box | null, bBox: Box | null): [Position, Position] {
  const length = distance(a, b)
  const at = (t: number) => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t })
  const fromA = aBox && inside(a, b, grown(aBox, LABEL_GAP))
  const intoB = bBox && inside(a, b, grown(bBox, LABEL_GAP))
  // Only a box the segment starts (ends) in trims it.
  const start = fromA && fromA[0] <= 0 ? Math.min(1, Math.max(0, fromA[1])) : 0
  const end = intoB && intoB[1] >= 1 ? Math.max(0, Math.min(1, intoB[0])) : 1
  if ((end - start) * length >= SQUEEZED_STROKE) return [at(start), at(end)]
  // In the middle of what room there is, or of the bond when the boxes leave none.
  const middle = end > start ? (start + end) / 2 : 0.5
  const half = Math.min(SQUEEZED_STROKE, length) / 2 / length
  const from = Math.max(0, Math.min(middle - half, 1 - 2 * half))
  return [at(from), at(from + 2 * half)]
}

const OFFSETS: Partial<Record<BondOrder, number[]>> = {
  single: [0],
  double: [-2.2, 2.2],
  triple: [-3.4, 0, 3.4],
}

/**
 * The strokes that draw `bond` from `begin` to `end`, each trimmed where it meets a label box
 * (null for a bare vertex). Every kind looks different: aromatic is a line with a dashed line
 * beside it, dative a line with an arrowhead at its end (the bond points away from its donor),
 * query a dotted line, and unknown a zigzag.
 */
export function bondStrokes(bond: BondView, begin: Position, end: Position, beginBox: Box | null, endBox: Box | null): Stroke[] {
  const length = distance(begin, end)
  if (length === 0) return []
  const ux = (end.x - begin.x) / length
  const uy = (end.y - begin.y) / length
  const shifted = (point: Position, offset: number) => ({ x: point.x - uy * offset, y: point.y + ux * offset })
  const line = (offset: number, pattern: 'solid' | 'dashed' | 'dotted'): Stroke & { shape: 'line' } => {
    const [a, b] = clipped(shifted(begin, offset), shifted(end, offset), beginBox, endBox)
    return { shape: 'line', x1: round(a.x), y1: round(a.y), x2: round(b.x), y2: round(b.y), pattern }
  }
  switch (bond.order) {
    case 'single':
    case 'double':
    case 'triple':
      return (OFFSETS[bond.order] ?? [0]).map((offset) => line(offset, 'solid'))
    case 'aromatic':
      return [line(0, 'solid'), line(3, 'dashed')]
    case 'query':
      return [line(0, 'dotted')]
    case 'dative': {
      const shaft = line(0, 'solid')
      const tip = { x: shaft.x2, y: shaft.y2 }
      const size = Math.min(4, distance({ x: shaft.x1, y: shaft.y1 }, tip))
      const back = { x: tip.x - ux * size, y: tip.y - uy * size }
      const head = [shifted(back, 2.5), tip, shifted(back, -2.5)].map((p) => ({ x: round(p.x), y: round(p.y) }))
      return [shaft, { shape: 'polyline', points: head }]
    }
    case 'unknown': {
      const [a, b] = clipped(begin, end, beginBox, endBox)
      const run = distance(a, b)
      const teeth = Math.max(2, Math.round(run / 3))
      const points = Array.from({ length: teeth + 1 }, (_, index) => {
        const t = index / teeth
        const side = index === 0 || index === teeth ? 0 : index % 2 === 0 ? -2 : 2
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

/** How `bond` is read out: its kind and the atoms it joins. */
export function bondName(bond: BondView) {
  switch (bond.order) {
    case 'dative':
      return `Bond ${bond.id}: dative bond from ${bond.begin} to ${bond.end}`
    case 'query': {
      const what = bond.query?.kind === 'list' ? bond.query.orders.join(' or ') : 'any kind'
      return `Bond ${bond.id}: query bond, ${what}, between ${bond.begin} and ${bond.end}`
    }
    case 'unknown':
      return `Bond ${bond.id}: bond of unknown kind between ${bond.begin} and ${bond.end}`
    case 'single':
    case 'double':
    case 'triple':
    case 'aromatic':
      return `Bond ${bond.id}: ${bond.order} bond between ${bond.begin} and ${bond.end}`
    default: {
      const unhandled: never = bond.order
      return unhandled
    }
  }
}

/** The frame, grown to include every atom with a margin and every label box (already placed). */
export function viewBox(positions: Position[], labels: Box[] = []) {
  let minX = FRAME.x
  let minY = FRAME.y
  let maxX = FRAME.x + FRAME.width
  let maxY = FRAME.y + FRAME.height
  for (const { x, y } of positions) {
    minX = Math.min(minX, x - FRAME_MARGIN)
    minY = Math.min(minY, y - FRAME_MARGIN)
    maxX = Math.max(maxX, x + FRAME_MARGIN)
    maxY = Math.max(maxY, y + FRAME_MARGIN)
  }
  for (const box of labels) {
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
