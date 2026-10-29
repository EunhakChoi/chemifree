/**
 * Inspector edits, each built from the latest document when its request runs, after every
 * request queued before it. A builder returns null when its change no longer applies (the atom
 * or bond is gone, the value is already set, or a limit is reached), so quick presses never
 * send a combination the service refuses: setting a query clears the charge and isotope it
 * cannot carry, and a charge or isotope is never sent for a query atom.
 */
import {
  ATTACHMENT_POINTS,
  MAX_STATED_HYDROGENS,
  type AtomView,
  type BondKind,
  type BondView,
  type Command,
  type DocumentView,
  type Hydrogens,
  type QueryAtom,
  type QueryBondOrder,
  type Radical,
} from '@/lib/document-contract'

export type Edit = (latest: DocumentView) => Command[] | null

export const MIN_CHARGE = -8
export const MAX_CHARGE = 8
/** The kinds a query bond may list, in the order they are sent. */
export const QUERY_BOND_ORDERS: QueryBondOrder[] = ['single', 'double', 'triple', 'aromatic']

const atomIn = (document: DocumentView, id: string): AtomView | undefined => document.atoms.find((atom) => atom.id === id)
const bondIn = (document: DocumentView, id: string): BondView | undefined => document.bonds.find((bond) => bond.id === id)
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

/** Makes the atom an element atom of `symbol`, from an element or a query. */
export function setElement(atom: string, symbol: string): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || (current.kind === 'element' && current.element === symbol)) return null
    return [{ op: 'update_atom', atom, element: symbol }]
  }
}

/** Makes the atom a query atom, clearing the charge and isotope a query atom does not carry. */
export function setQuery(atom: string, query: QueryAtom): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || (current.kind === 'query' && same(current.query, query))) return null
    return [
      {
        op: 'update_atom',
        atom,
        query,
        ...(current.charge === 0 ? {} : { charge: 0 }),
        ...(current.isotope === null ? {} : { isotope: null }),
      },
    ]
  }
}

/** Steps an element atom's charge within −8..8. A query atom has none. */
export function stepCharge(atom: string, step: number): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || current.kind === 'query') return null
    const charge = current.charge + step
    if (charge < MIN_CHARGE || charge > MAX_CHARGE) return null
    return [{ op: 'update_atom', atom, charge }]
  }
}

/** Sets an element atom's isotope (null for natural abundance). A query atom has none. */
export function setIsotope(atom: string, isotope: number | null): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || current.kind === 'query' || current.isotope === isotope) return null
    return [{ op: 'update_atom', atom, isotope }]
  }
}

/** Switches how hydrogens are recorded; a newly stated count starts at one. */
export function setHydrogenMode(atom: string, mode: Hydrogens['mode']): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || current.hydrogens.mode === mode) return null
    const hydrogens: Hydrogens = mode === 'specified' ? { mode, count: 1 } : { mode }
    return [{ op: 'update_atom', atom, hydrogens }]
  }
}

/** Steps a stated hydrogen count within 0..16; nothing to step unless a count is stated. */
export function stepHydrogens(atom: string, step: number): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (current?.hydrogens.mode !== 'specified') return null
    const count = current.hydrogens.count + step
    if (count < 0 || count > MAX_STATED_HYDROGENS) return null
    return [{ op: 'update_atom', atom, hydrogens: { mode: 'specified', count } }]
  }
}

export function setRadical(atom: string, radical: Radical): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || current.radical === radical) return null
    return [{ op: 'update_atom', atom, radical }]
  }
}

/** Adds or removes one attachment point, keeping the list ascending. */
export function toggleAttachmentPoint(atom: string, point: number): Edit {
  return (latest) => {
    const current = atomIn(latest, atom)
    if (!current || !ATTACHMENT_POINTS.some((value) => value === point)) return null
    const points = current.attachment_points.includes(point)
      ? current.attachment_points.filter((value) => value !== point)
      : [...current.attachment_points, point].sort((a, b) => a - b)
    return [{ op: 'update_atom', atom, attachment_points: points }]
  }
}

/** Sets a bond's kind; `query` comes with order `query` and only with it. */
export function setBondKind(bond: string, kind: BondKind): Edit {
  return (latest) => {
    const current = bondIn(latest, bond)
    if (!current || (current.order === kind.order && same(current.query, kind.query ?? null))) return null
    return [{ op: 'update_bond', bond, ...kind }]
  }
}

/**
 * Adds or removes one kind a query bond allows. A bond that is not yet a query becomes one
 * allowing just that kind; removing the last kind leaves a query for any kind.
 */
export function toggleQueryBondOrder(bond: string, order: QueryBondOrder): Edit {
  return (latest) => {
    const current = bondIn(latest, bond)
    if (!current) return null
    const listed = current.query?.kind === 'list' ? current.query.orders : []
    const orders = listed.includes(order)
      ? listed.filter((value) => value !== order)
      : QUERY_BOND_ORDERS.filter((value) => value === order || listed.includes(value))
    return setBondKind(bond, { order: 'query', query: orders.length > 0 ? { kind: 'list', orders } : { kind: 'any' } })(latest)
  }
}

/**
 * The elements of a typed query list ("N, O, S"), or null unless it names 1 to 10 distinct
 * symbols. Whether each is an element is the service's to say.
 */
export function parseQueryList(text: string): string[] | null {
  const symbols = text
    .split(/[\s,]+/)
    .map((symbol) => symbol.trim())
    .filter((symbol) => symbol !== '')
  const wellFormed = symbols.every((symbol) => /^[A-Z][a-z]{0,2}$/.test(symbol))
  const distinct = new Set(symbols).size === symbols.length
  return wellFormed && distinct && symbols.length >= 1 && symbols.length <= 10 ? symbols : null
}
