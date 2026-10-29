/**
 * The commands a bond-tool gesture makes, built from the document on screen. A bond always
 * begins where the gesture starts and ends where it ends, so a dative bond's donor (its begin)
 * is the atom or point the drag started from.
 */
import { placeNewAtoms, type NewAtom } from '@/editor/placement'
import type { BondOrder, Command, DocumentView, Position } from '@/lib/document-contract'

/** One end of a drawn bond: an existing atom, or a point where a new carbon goes. */
export type BondEnd = { atom: string } | { point: Position }

/** The kinds the bond tool draws: a query bond needs its expression, set in the inspector. */
export type DrawnOrder = Exclude<BondOrder, 'query'>

/** New carbons placed in every placement, each bonded from `bondTo` when it has one. */
function carbons(document: DocumentView, instance: string | null, atoms: NewAtom[], order: DrawnOrder): Command[] {
  const placed = placeNewAtoms(document, instance, atoms)
  return atoms.map(({ bondTo }, index): Command => {
    const add = { op: 'add_atom' as const, element: 'C', ...placed[index] }
    return bondTo === undefined ? add : { ...add, bond_to: { atom: bondTo, order } }
  })
}

/**
 * A bond of `order` drawn in placement `instance` from `from` to `to`. Between two atoms
 * already bonded, the bond gets the new order; a dative bond drawn against an existing bond's
 * direction replaces that bond in the same transaction (one undo step), since a bond's begin
 * cannot be changed in place.
 */
export function drawBond(
  document: DocumentView,
  instance: string | null,
  from: BondEnd,
  to: BondEnd,
  order: DrawnOrder,
): Command[] {
  if ('atom' in from) {
    if ('point' in to) return carbons(document, instance, [{ point: to.point, bondTo: from.atom }], order)
    if (from.atom === to.atom) return []
    const existing = document.bonds.find(
      (bond) => (bond.begin === from.atom && bond.end === to.atom) || (bond.begin === to.atom && bond.end === from.atom),
    )
    if (!existing) return [{ op: 'add_bond', begin: from.atom, end: to.atom, order }]
    if (order === 'dative' && existing.begin !== from.atom) {
      return [
        { op: 'delete_bonds', bonds: [existing.id] },
        { op: 'add_bond', begin: from.atom, end: to.atom, order },
      ]
    }
    return [{ op: 'update_bond', bond: existing.id, order }]
  }
  if ('atom' in to) {
    // Placed relative to the atom it bonds to, but the bond begins at the new atom.
    const [placement] = placeNewAtoms(document, instance, [{ point: from.point, bondTo: to.atom }])
    return [
      { op: 'add_atom', element: 'C', ...placement },
      { op: 'add_bond', begin: '$0', end: to.atom, order },
    ]
  }
  return carbons(document, instance, [{ point: from.point }, { point: to.point, bondTo: '$0' }], order)
}
