/**
 * Scene placements in the editor: the document as one placement draws it, and where a new atom
 * goes in every placement. Where to put an atom in the placements the user did not click is a
 * policy of this editor, not a rule of the document: the service only requires a position for
 * every scene instance.
 */
import { distance, round } from '@/editor/geometry'
import { placements, type DocumentView, type NewAtomPlacement, type Placement, type Position } from '@/lib/document-contract'

/** The document with its atoms at `placement`'s positions, for drawing and hit testing it. */
export function placedView(document: DocumentView, placement: Placement): DocumentView {
  return {
    ...document,
    atoms: document.atoms.map((atom) => ({ ...atom, position: placement.positions.get(atom.id) ?? atom.position })),
  }
}

function nearestAtom(placement: Placement, point: Position) {
  let nearest: { atom: string; distance: number } | null = null
  for (const [atom, position] of placement.positions) {
    const d = distance(point, position)
    if (!nearest || d < nearest.distance) nearest = { atom, distance: d }
  }
  return nearest
}

/**
 * The placement a click on empty canvas at `point` belongs to: the one holding the nearest
 * atom, or the first one when there are no atoms. Null for a service without scene instances.
 */
export function placementAt(document: DocumentView, point: Position): string | null {
  const all = placements(document)
  let best: { id: string | null; distance: number } = { id: all[0].id, distance: Infinity }
  for (const placement of all) {
    const nearest = nearestAtom(placement, point)
    if (nearest && nearest.distance < best.distance) best = { id: placement.id, distance: nearest.distance }
  }
  return best.id
}

/**
 * Where an atom placed at `point` in placement `clicked` goes. With one placement that is the
 * point itself. With several, every other placement gets the point shifted by how far that
 * placement has moved the atom nearest to the point in the clicked placement, so the new atom
 * keeps the same place relative to its neighbors in each drawing. Without atoms every
 * placement gets the point.
 */
export function newAtomPlacement(document: DocumentView, clicked: string | null, point: Position): NewAtomPlacement {
  const all = placements(document)
  const source = all.find((placement) => placement.id === clicked) ?? all[0]
  if (all.length === 1 || source.id === null) return { position: point }
  const anchor = nearestAtom(source, point)?.atom
  const from = anchor === undefined ? undefined : source.positions.get(anchor)
  const positions: Record<string, Position> = {}
  for (const placement of all) {
    if (placement.id === null) continue
    const to = anchor === undefined ? undefined : placement.positions.get(anchor)
    const shift = from && to ? { x: to.x - from.x, y: to.y - from.y } : { x: 0, y: 0 }
    positions[placement.id] =
      placement.id === source.id ? point : { x: round(point.x + shift.x), y: round(point.y + shift.y) }
  }
  return { positions }
}
