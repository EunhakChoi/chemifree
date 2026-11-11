import { MAX_STATED_HYDROGENS, type AtomView, type BondQuery, type BondView, type Hydrogens, type QueryAtom } from '@/lib/document-contract/model'
import { isStereoMark } from '@/lib/stereo-contract'

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export const isCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0
export const isFiniteNumber = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)
const BOND_ORDERS: readonly unknown[] = ['single', 'double', 'triple', 'aromatic', 'dative', 'query', 'unknown']
const QUERY_BOND_ORDERS: readonly unknown[] = ['single', 'double', 'triple', 'aromatic']
const RADICALS: readonly unknown[] = ['none', 'singlet', 'doublet', 'triplet', 'unknown']
const NAMED_QUERIES: readonly unknown[] = ['any', 'any_heavy', 'heteroatom', 'halogen']
/** The recorded-state fields of an atom, which come all together or not at all. */
export const ATOM_STATE_FIELDS = ['kind', 'query', 'hydrogens', 'radical', 'attachment_points'] as const

export const distinct = (items: unknown[]) => new Set(items).size === items.length
/**
 * Closed native clipboard format (CONTRACT.md P33a/P33e): any key outside the list refuses. A
 * listed key may be absent, so a caller that requires it must check its presence.
 */
export function onlyKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  return Object.keys(value).every((key) => keys.includes(key))
}

function isQueryAtom(value: unknown): value is QueryAtom {
  if (!isRecord(value)) return false
  if (NAMED_QUERIES.includes(value.kind)) return true
  return (
    value.kind === 'list' &&
    Array.isArray(value.elements) &&
    value.elements.length >= 1 &&
    value.elements.length <= 10 &&
    value.elements.every((element) => typeof element === 'string') &&
    distinct(value.elements) &&
    typeof value.negated === 'boolean'
  )
}

function isBondQuery(value: unknown): value is BondQuery {
  if (!isRecord(value)) return false
  if (value.kind === 'any') return true
  return (
    value.kind === 'list' &&
    Array.isArray(value.orders) &&
    value.orders.length >= 1 &&
    value.orders.every((order) => QUERY_BOND_ORDERS.includes(order)) &&
    distinct(value.orders)
  )
}

function isHydrogens(value: unknown): value is Hydrogens {
  if (!isRecord(value)) return false
  if (value.mode === 'inferred' || value.mode === 'unknown') return true
  return (
    value.mode === 'specified' &&
    isCount(value.count) &&
    value.count <= MAX_STATED_HYDROGENS
  )
}

const isAttachmentPoints = (value: unknown) =>
  Array.isArray(value) &&
  value.every((point, index) => Number.isInteger(point) && point >= 1 && point <= 4 && (index === 0 || point > value[index - 1]))

/** Which recorded-state fields an atom carries: all, none, or (refused) only some. */
function atomStates(value: object): 'all' | 'none' | 'some' {
  const present = ATOM_STATE_FIELDS.filter((field) => field in value).length
  return present === ATOM_STATE_FIELDS.length ? 'all' : present === 0 ? 'none' : 'some'
}

export function isAtom(value: unknown): value is AtomView {
  if (
    !isRecord(value) ||
    typeof value.id !== 'string' ||
    typeof value.element !== 'string' ||
    !Number.isSafeInteger(value.charge) ||
    !(value.isotope === null || isCount(value.isotope)) ||
    !isRecord(value.position) ||
    !isFiniteNumber(value.position.x) ||
    !isFiniteNumber(value.position.y)
  ) {
    return false
  }
  const states = atomStates(value)
  switch (states) {
    case 'none':
      return true
    case 'some':
      return false
    case 'all':
      return (
        (value.kind === 'element'
          ? value.query === null
          : value.kind === 'query' && isQueryAtom(value.query) && value.charge === 0 && value.isotope === null) &&
        isHydrogens(value.hydrogens) &&
        RADICALS.includes(value.radical) &&
        isAttachmentPoints(value.attachment_points)
      )
    default: {
      const unhandled: never = states
      return unhandled
    }
  }
}

export function isBond(value: unknown): value is BondView {
  if (
    !isRecord(value) ||
    typeof value.id !== 'string' ||
    typeof value.begin !== 'string' ||
    typeof value.end !== 'string' ||
    !BOND_ORDERS.includes(value.order)
  ) {
    return false
  }
  // A reply from before marks has no `mark`; one that has it holds null or a mark the order fits.
  if ('mark' in value && value.mark !== null && !(typeof value.order === 'string' && isStereoMark(value.mark, value.order))) return false
  // A reply from before recorded states has no `query`, and then only the first three orders.
  if (!('query' in value)) return value.order === 'single' || value.order === 'double' || value.order === 'triple'
  return value.order === 'query' ? isBondQuery(value.query) : value.query === null
}

/**
 * Every atom carries the recorded-state fields or none does, and the bonds agree: a reply holds
 * them all (a service with recorded states) or none (one from before them).
 */
export function statesAgree(atoms: readonly object[], bonds: readonly object[]) {
  const shapes = new Set([
    ...atoms.map((atom) => atomStates(atom) === 'all'),
    ...bonds.map((bond) => 'query' in bond),
  ])
  // Marks come all together or not at all, independently of the recorded states.
  const marks = new Set(bonds.map((bond) => 'mark' in bond))
  return shapes.size <= 1 && marks.size <= 1
}

export const isCountOrNull = (value: unknown) => value === null || isCount(value)
