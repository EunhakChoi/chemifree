/**
 * Scene placements in the editor: the document as one placement draws it, and where a new atom
 * goes in every placement. Where to put an atom in the placements the user did not click is a
 * policy of this editor, not a rule of the document: the service only requires a position for
 * every scene instance.
 */
import { distance, round } from '@/editor/geometry'
import {
  placements,
  type Command,
  type DocumentView,
  type NewAtomPlacement,
  type Placement,
  type Position,
} from '@/lib/document-contract'

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
 * Where an atom placed at `point` in placement `clicked` goes, when it is bonded to nothing:
 * see `placeNewAtoms`.
 */
export function newAtomPlacement(document: DocumentView, clicked: string | null, point: Position): NewAtomPlacement {
  return placeNewAtoms(document, clicked, [{ point }])[0]
}

/**
 * A new atom of one gesture: where it was put in the clicked placement, and the atom it is
 * bonded to, an existing atom's ID or `$N` for the gesture's Nth new atom (the gesture's
 * commands are its new atoms, in order, so `$N` is also their transaction reference).
 */
export interface NewAtom {
  point: Position
  bondTo?: string
}

/**
 * Where a gesture's new atoms go in every placement, in order. With one placement each goes
 * where it was put. With several, every other placement gets the point shifted so the atom
 * keeps its place relative to an anchor: the atom it is bonded to (an existing atom, or the
 * position already chosen there for an earlier new atom), so new bonds have the same length
 * and direction in every drawing; an atom bonded to nothing follows the atom nearest to it
 * in the clicked placement, and without atoms every placement gets the point.
 */
export function placeNewAtoms(document: DocumentView, clicked: string | null, atoms: NewAtom[]): NewAtomPlacement[] {
  const all = placements(document)
  const source = all.find((placement) => placement.id === clicked) ?? all[0]
  if (all.length === 1 || source.id === null) return atoms.map(({ point }) => ({ position: point }))
  const placed: Record<string, Position>[] = []
  for (const { point, bondTo } of atoms) {
    // Where the anchor is in each placement, by placement ID.
    let anchor: (placement: Placement) => Position | undefined
    const earlier = bondTo?.startsWith('$') ? placed[Number(bondTo.slice(1))] : undefined
    if (earlier) {
      anchor = (placement) => (placement.id === null ? undefined : earlier[placement.id])
    } else {
      const atom = bondTo !== undefined && source.positions.has(bondTo) ? bondTo : nearestAtom(source, point)?.atom
      anchor = (placement) => (atom === undefined ? undefined : placement.positions.get(atom))
    }
    const from = anchor(source)
    const positions: Record<string, Position> = {}
    for (const placement of all) {
      if (placement.id === null) continue
      const to = anchor(placement)
      const shift = from && to ? { x: to.x - from.x, y: to.y - from.y } : { x: 0, y: 0 }
      positions[placement.id] =
        placement.id === source.id ? point : { x: round(point.x + shift.x), y: round(point.y + shift.y) }
    }
    placed.push(positions)
  }
  return placed.map((positions) => ({ positions }))
}

/** Where a duplicated placement goes, relative to the one it copies: below and slightly right. */
export const DUPLICATE_OFFSET: Position = { x: 40, y: 80 }

/**
 * The command that draws the molecule once more: a copy of the last placement, offset from it,
 * so repeated copies step down the page instead of landing on each other. Null for a service
 * without scene instances.
 */
export function duplicatePlacement(document: DocumentView): Command | null {
  const last = document.scene?.instances.at(-1)
  return last ? { op: 'add_instance', source: last.id, offset: DUPLICATE_OFFSET } : null
}

/**
 * The command that moves placement `instance` by `offset`, built from the document it will apply
 * to, so presses queued behind one another each move it once more. Null when the placement is gone.
 */
export function movePlacement(document: DocumentView, instance: string, offset: Position): Command | null {
  return document.scene?.instances.some((item) => item.id === instance)
    ? { op: 'move_instance', instance, offset }
    : null
}

/**
 * The command that deletes placement `instance` from the document it will apply to. Null when the
 * placement is gone or is the last one, which the service would refuse.
 */
export function deletePlacement(document: DocumentView, instance: string): Command | null {
  const instances = document.scene?.instances ?? []
  return instances.length > 1 && instances.some((item) => item.id === instance) ? { op: 'delete_instance', instance } : null
}
