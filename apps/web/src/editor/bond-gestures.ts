/**
 * The commands a bond-tool gesture makes, built from the document on screen. A bond always
 * begins where the gesture starts and ends where it ends, so a dative bond's donor (its begin)
 * is the atom or point the drag started from.
 */
import { markFits } from '@/editor/geometry'
import { placeNewAtoms, type NewAtom } from '@/editor/placement'
import type { BondOrder, BondView, Command, DocumentView, Position } from '@/lib/document-contract'
import { stereoOf } from '@/lib/stereo-contract'

/** One end of a drawn bond: an existing atom, or a point where a new carbon goes. */
export type BondEnd = { atom: string } | { point: Position }

/** The kinds the bond tool draws: a query bond needs its expression, set in the inspector. */
export type DrawnOrder = Exclude<BondOrder, 'query'>

/**
 * What the service would refuse if `bond` became a bond of `order` as it is: a mark the new order
 * cannot carry, and a stored double-bond configuration when it stops being a double bond. The
 * commands that clear them, to send before the `update_bond` in the same transaction.
 */
export function staleStereoClears(document: DocumentView, bond: BondView, order: BondOrder): Command[] {
  const clears: Command[] = []
  if (bond.mark && !markFits(bond.mark.kind, order)) clears.push({ op: 'set_bond_mark', bond: bond.id, mark: null })
  if (order !== 'double' && stereoOf(document).bonds.some((record) => record.bond === bond.id)) {
    clears.push({ op: 'set_bond_configuration', bond: bond.id, configuration: null })
  }
  return clears
}

/** A bond tool click cycles single, double, and triple; any other kind becomes single. */
const nextOrder = (order: BondOrder): DrawnOrder => (order === 'single' ? 'double' : order === 'double' ? 'triple' : 'single')

/**
 * What a bond tool click on `bond` does with an order kind selected: a marked bond loses its
 * mark first (the one direct way to remove it), any other bond takes the next order, after the
 * stereo it could not keep.
 */
export function clickBond(document: DocumentView, bond: BondView): Command[] {
  if (bond.mark) return [{ op: 'set_bond_mark', bond: bond.id, mark: null }]
  const order = nextOrder(bond.order)
  return [...staleStereoClears(document, bond, order), { op: 'update_bond', bond: bond.id, order }]
}

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
 * cannot be changed in place. A mark or stored configuration the new order cannot keep is
 * cleared first in the same transaction.
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
    return [...staleStereoClears(document, existing, order), { op: 'update_bond', bond: existing.id, order }]
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
