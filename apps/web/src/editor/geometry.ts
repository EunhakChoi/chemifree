/**
 * Drawing geometry for the structure canvas. Coordinates are document points (pt) with y
 * growing downward, the same space the API stores. Nothing here perceives chemistry.
 */
import type { AtomView, BondView, DocumentView, Position } from '@/lib/document-contract'

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

/** Atoms take precedence over bonds; the nearest candidate wins. Pass `atoms` to reuse an index. */
export function hitTest(
  document: DocumentView,
  point: Position,
  ignore: ReadonlySet<string> = new Set(),
  atoms: ReadonlyMap<string, AtomView> = atomIndex(document),
): Hit {
  let best: Hit = null
  let bestDistance = ATOM_HIT_RADIUS
  for (const atom of document.atoms) {
    if (ignore.has(atom.id)) continue
    const d = distance(point, atom.position)
    if (d <= bestDistance) {
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

/** Carbon is drawn as a bare vertex unless it is isolated, charged, or isotope-labeled. */
export function showsLabel(atom: AtomView, atomDegree: number) {
  return atom.element !== 'C' || atom.charge !== 0 || atom.isotope !== null || atomDegree === 0
}

export function chargeText(charge: number) {
  if (charge === 0) return ''
  const sign = charge > 0 ? '+' : '−'
  return Math.abs(charge) === 1 ? sign : `${Math.abs(charge)}${sign}`
}

/** Parallel segments for a bond, trimmed where they meet a text label. */
export function bondSegments(bond: BondView, begin: Position, end: Position, trimBegin: boolean, trimEnd: boolean) {
  const length = distance(begin, end)
  if (length === 0) return []
  const ux = (end.x - begin.x) / length
  const uy = (end.y - begin.y) / length
  const trim = 7
  const a = trimBegin ? { x: begin.x + ux * trim, y: begin.y + uy * trim } : begin
  const b = trimEnd ? { x: end.x - ux * trim, y: end.y - uy * trim } : end
  const offsets = bond.order === 'single' ? [0] : bond.order === 'double' ? [-2.2, 2.2] : [-3.4, 0, 3.4]
  return offsets.map((offset) => ({
    x1: a.x - uy * offset,
    y1: a.y + ux * offset,
    x2: b.x - uy * offset,
    y2: b.y + ux * offset,
  }))
}

/** The frame, grown to include every atom with a margin. */
export function viewBox(positions: Position[]) {
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
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY }
}

export function round(value: number) {
  return Math.round(value * 100) / 100
}
