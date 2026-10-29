/**
 * How recorded atom and bond states are shown and edited. Everything here reads what the
 * document records; nothing computes chemistry. In particular an atom whose hydrogens are
 * `inferred` (to be derived from valence, which nothing derives yet) shows no hydrogen count.
 */
import {
  MAX_ATTACHMENT_POINT,
  MAX_HYDROGEN_COUNT,
  type AtomQuery,
  type AtomView,
  type BondView,
  type Command,
  type DocumentView,
  type Radical,
} from '@/lib/document-contract'

/**
 * The hydrogen text drawn after an atom's label: `H`, `H2`, … for a stated count (`H0` for a
 * stated zero, so it reads apart from an inferred state), `H?` when stated to be unknown, and
 * nothing when inferred or not reported.
 */
export function hydrogenText(atom: AtomView): string {
  const hydrogens = atom.hydrogens
  if (!hydrogens) return ''
  switch (hydrogens.mode) {
    case 'inferred':
      return ''
    case 'unknown':
      return 'H?'
    case 'specified':
      return hydrogens.count === 1 ? 'H' : `H${hydrogens.count}`
    default: {
      const unknown: never = hydrogens
      return unknown
    }
  }
}

/** The dots drawn for a radical: one for a doublet, two for a singlet or triplet, and a dot with a question mark when unknown. */
export function radicalMark(radical: Radical | undefined): { dots: number; uncertain: boolean } | null {
  switch (radical) {
    case undefined:
    case 'none':
      return null
    case 'doublet':
      return { dots: 1, uncertain: false }
    case 'singlet':
    case 'triplet':
      return { dots: 2, uncertain: false }
    case 'unknown':
      return { dots: 1, uncertain: true }
    default: {
      const unknown: never = radical
      return unknown
    }
  }
}

/** The mark drawn for an atom's attachment points, e.g. `*1,3`; empty when it has none. */
export function attachmentText(points: number[] | undefined): string {
  return points?.length ? `*${points.join(',')}` : ''
}

/** A bond's accessible name, e.g. `dative bond b2 from a1 to a3`. */
export function bondName(bond: BondView): string {
  const kind = bond.order === 'query' && bond.query?.kind === 'list' ? `query (${bond.query.orders.join(' or ')})` : bond.order
  return bond.order === 'dative'
    ? `${kind} bond ${bond.id} from ${bond.begin} to ${bond.end}`
    : `${kind} bond ${bond.id}`
}

/** What an atom records beyond its label, e.g. `a2: 2 H stated, doublet radical`; empty when nothing is recorded. */
function atomStates(atom: AtomView): string[] {
  const states: string[] = []
  if (atom.kind === 'query') states.push(`query atom ${atom.element}`)
  if (atom.hydrogens?.mode === 'specified') states.push(`${atom.hydrogens.count} H stated`)
  if (atom.hydrogens?.mode === 'unknown') states.push('hydrogens unknown')
  if (atom.radical && atom.radical !== 'none') states.push(`${atom.radical} radical`)
  if (atom.attachment_points?.length) states.push(`attachment points ${atom.attachment_points.join(', ')}`)
  return states
}

/**
 * The recorded states a drawing shows as marks, in words, for its accessible description:
 * stated or unknown hydrogens, radicals, query atoms, attachment points, and bonds other than
 * single, double, or triple. Inferred hydrogens are not mentioned: no count is known.
 */
export function describeStates(document: DocumentView): string[] {
  const atoms = document.atoms.flatMap((atom) => {
    const states = atomStates(atom)
    return states.length ? [`${atom.id}: ${states.join(', ')}`] : []
  })
  const bonds = document.bonds
    .filter((bond) => bond.order !== 'single' && bond.order !== 'double' && bond.order !== 'triple')
    .map(bondName)
  return [...atoms, ...bonds]
}

const atomIn = (document: DocumentView, id: string) => document.atoms.find((atom) => atom.id === id)

/**
 * Steps a stated hydrogen count, built from the document the step applies to so quick repeated
 * presses all count. Nothing is sent when the atom is gone, its hydrogens are not stated, or the
 * step would leave 0–16.
 */
export function changeHydrogenCount(atom: string, step: number) {
  return (document: DocumentView): Command[] | null => {
    const hydrogens = atomIn(document, atom)?.hydrogens
    if (hydrogens?.mode !== 'specified') return null
    const count = hydrogens.count + step
    if (count < 0 || count > MAX_HYDROGEN_COUNT) return null
    return [{ op: 'update_atom', atom, hydrogens: { mode: 'specified', count } }]
  }
}

/**
 * Adds or removes one attachment point, built from the document it applies to, so two quick
 * presses on different points both take effect.
 */
export function toggleAttachmentPoint(atom: string, point: number) {
  return (document: DocumentView): Command[] | null => {
    const current = atomIn(document, atom)?.attachment_points
    if (!current || point < 1 || point > MAX_ATTACHMENT_POINT) return null
    const next = current.includes(point) ? current.filter((value) => value !== point) : [...current, point].sort((a, b) => a - b)
    return [{ op: 'update_atom', atom, attachment_points: next }]
  }
}

/** What the inspector can make an atom: an element atom, or a query atom of a kind without a list. */
export type AtomKindChoice = 'element' | Exclude<AtomQuery['kind'], 'list'>

/**
 * Makes an atom an element atom (carbon) or a query atom, built from the document it applies to.
 * A query atom has no charge or isotope, so nothing is sent when the atom has either by then
 * (say, a charge step pressed just before), nor when the atom already is what was chosen.
 */
export function changeAtomKind(atom: string, choice: AtomKindChoice) {
  return (document: DocumentView): Command[] | null => {
    const current = atomIn(document, atom)
    if (!current) return null
    if (choice === 'element') {
      return current.kind === 'query' ? [{ op: 'update_atom', atom, element: 'C' }] : null
    }
    if (current.charge !== 0 || current.isotope !== null) return null
    if (current.kind === 'query' && current.query?.kind === choice) return null
    return [{ op: 'update_atom', atom, query: { kind: choice } }]
  }
}

/**
 * Steps an atom's charge, built from the document it applies to, so quick repeated presses all
 * count. Nothing is sent for a query atom (it has no charge), which the atom may have become
 * after the press.
 */
export function changeCharge(atom: string, step: number) {
  return (document: DocumentView): Command[] | null => {
    const current = atomIn(document, atom)
    if (!current || current.kind === 'query') return null
    return [{ op: 'update_atom', atom, charge: current.charge + step }]
  }
}

/** Sets an atom's isotope (`null` for natural abundance) unless it is already that or the atom is a query atom by then. */
export function changeIsotope(atom: string, isotope: number | null) {
  return (document: DocumentView): Command[] | null => {
    const current = atomIn(document, atom)
    if (!current || current.kind === 'query' || current.isotope === isotope) return null
    return [{ op: 'update_atom', atom, isotope }]
  }
}
