/**
 * Hand-maintained mirror of the stereo parts of the v1 document contract in
 * crates/application/src/contract.rs: bond marks and stated configurations. Representation
 * only. Nothing here derives a configuration from a mark, computes a CIP label, or compares
 * two drawings; the editor draws marks (editor/geometry.ts) and sends marks and the
 * configuration the service derives from them (editor/mark-gestures.ts). (The service derives
 * a configuration from the marks and one instance's positions only when it is sent
 * `set_configuration_from_drawing`; this module reads and checks what the service answers, it
 * takes no part in the derivation.)
 *
 * The validators are strict on purpose: they accept exactly the values the service sends
 * today. A later mark kind (unknown, hollow, bold) or slot kind makes them refuse the whole
 * reply as incompatible, which is the safe failure: drawing such a bond as a plain line would
 * hide it. Extend the validator in the same change that makes the service send the new value.
 */

/** Which end of a bond a mark's narrow side is at. The bond's own `begin` and `end` never swap. */
export type BondEnd = 'begin' | 'end'

/**
 * How a bond is drawn. A drawing fact only: it states no configuration. `null` on a bond means
 * plain, not "non-stereogenic". Wavy and crossed have no narrow end.
 */
export type StereoMark = { kind: 'wedge' | 'hash'; narrow: BondEnd } | { kind: 'wavy' | 'crossed' }

/** Looking from the first neighbor toward the center: `anticlockwise` is OpenSMILES `@`. */
export type Turn = 'anticlockwise' | 'clockwise'
/** `opposite_sides` is the public model's `opposite_side`. */
export type Relation = 'same_side' | 'opposite_sides'

/** A neighbor position that is not an atom: a hydrogen that is not an atom of the drawing, or a lone pair. */
export const IMPLICIT_HYDROGEN = 'implicit_hydrogen'
export const LONE_PAIR = 'lone_pair'

/** Four distinct neighbors in canonical order: atom ids ascending, then the hydrogen, then the lone pair. */
export type CenterNeighbors = [string, string, string, string]

/** What is stated about a tetrahedral stereocenter. `unknown` names the neighbor set it is about. */
export type AtomConfiguration =
  | { kind: 'tetrahedral'; neighbors: CenterNeighbors; turn: Turn }
  | { kind: 'unknown'; neighbors: CenterNeighbors }

/**
 * What is stated about a double bond. The neighbors are the lowest-id other neighbor of each
 * end, or `implicit_hydrogen` where the end has no other atom.
 */
export type BondConfiguration =
  | { kind: 'relative'; begin_neighbor: string; end_neighbor: string; relation: Relation }
  | { kind: 'unknown' }

/**
 * The stated configurations of a document. No record means "unspecified"; it says nothing about
 * whether an atom or bond is stereogenic.
 */
export interface StereoView {
  atoms: Array<{ atom: string; configuration: AtomConfiguration }>
  bonds: Array<{ bond: string; configuration: BondConfiguration }>
}

/**
 * What a transaction cleared of stereo because it changed what the record was about: one entry
 * per record, shaped like the contract's loss entries. Describes that request only.
 */
export interface StereoNote {
  feature: 'stereo_configuration'
  /** `atoms[a1]` for a center, `bonds[b5]` for a double bond. */
  source_path: string
  disposition: 'dropped'
  reason: string
  semantic_impact: 'stereo'
}

/** A document with no stated configuration, and what an older service's reply is read as. */
export const NO_STEREO: Readonly<StereoView> = Object.freeze({ atoms: [], bonds: [] })

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
const hasOnly = (value: Record<string, unknown>, keys: string[]) => Object.keys(value).every((key) => keys.includes(key))

const BOND_ENDS: readonly unknown[] = ['begin', 'end']
const TURNS: readonly unknown[] = ['anticlockwise', 'clockwise']
const RELATIONS: readonly unknown[] = ['same_side', 'opposite_sides']

/** Whether `value` is a mark that a bond of `order` can carry: wedge, hash, wavy on single; crossed on double. */
export function isStereoMark(value: unknown, order: string): value is StereoMark {
  if (!isRecord(value)) return false
  switch (value.kind) {
    case 'wedge':
    case 'hash':
      return hasOnly(value, ['kind', 'narrow']) && BOND_ENDS.includes(value.narrow) && order === 'single'
    case 'wavy':
      return hasOnly(value, ['kind']) && order === 'single'
    case 'crossed':
      return hasOnly(value, ['kind']) && order === 'double'
    default:
      return false
  }
}

interface GraphBond {
  id: string
  begin: string
  end: string
  order: string
}

/**
 * Whether `value` is a stereo table that fits the graph it arrived with: one record per center
 * or double bond, each naming atoms and bonds the graph has, a center's neighbors exactly its
 * bonded atoms (plus the hydrogen or lone pair that fills a third-bond center), and a double
 * bond's references atoms bonded to the right end.
 */
export function isStereoView(value: unknown, atoms: ReadonlyArray<{ id: string }>, bonds: readonly GraphBond[]): value is StereoView {
  if (!isRecord(value) || !hasOnly(value, ['atoms', 'bonds']) || !Array.isArray(value.atoms) || !Array.isArray(value.bonds)) {
    return false
  }
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const neighbors = new Map<string, Set<string>>()
  for (const bond of bonds) {
    for (const [from, to] of [[bond.begin, bond.end], [bond.end, bond.begin]] as const) {
      const bonded = neighbors.get(from) ?? new Set<string>()
      bonded.add(to)
      neighbors.set(from, bonded)
    }
  }
  const centers = new Set<string>()
  for (const record of value.atoms as unknown[]) {
    if (!isRecord(record) || !hasOnly(record, ['atom', 'configuration']) || typeof record.atom !== 'string' ||
        !atomIds.has(record.atom) || centers.has(record.atom) || !isCenterConfiguration(record.configuration, neighbors.get(record.atom) ?? new Set())) {
      return false
    }
    centers.add(record.atom)
  }
  const doubles = new Map(bonds.filter((bond) => bond.order === 'double').map((bond) => [bond.id, bond]))
  const configured = new Set<string>()
  for (const record of value.bonds as unknown[]) {
    if (!isRecord(record) || !hasOnly(record, ['bond', 'configuration']) || typeof record.bond !== 'string' || configured.has(record.bond)) {
      return false
    }
    const bond = doubles.get(record.bond)
    if (bond === undefined || !isBondConfiguration(record.configuration, bond, neighbors)) return false
    configured.add(record.bond)
  }
  return true
}

function isCenterConfiguration(value: unknown, bonded: ReadonlySet<string>): value is AtomConfiguration {
  if (!isRecord(value) || !Array.isArray(value.neighbors) || value.neighbors.length !== 4) return false
  const slots: unknown[] = value.neighbors
  if (!slots.every((slot) => typeof slot === 'string') || new Set(slots).size !== 4) return false
  const named = slots as string[]
  const explicit = named.filter((slot) => slot !== IMPLICIT_HYDROGEN && slot !== LONE_PAIR)
  const exact = bonded.size >= 3 && bonded.size <= 4 && explicit.length === bonded.size && explicit.every((slot) => bonded.has(slot))
  if (!exact) return false
  if (value.kind === 'tetrahedral') return hasOnly(value, ['kind', 'neighbors', 'turn']) && TURNS.includes(value.turn)
  return value.kind === 'unknown' && hasOnly(value, ['kind', 'neighbors'])
}

function isBondConfiguration(value: unknown, bond: GraphBond, neighbors: ReadonlyMap<string, ReadonlySet<string>>): value is BondConfiguration {
  if (!isRecord(value)) return false
  if (value.kind === 'unknown') return hasOnly(value, ['kind'])
  // A reference is a bonded atom of its own end other than the double bond's partner, or the hydrogen.
  const reference = (slot: unknown, own: string, partner: string) =>
    slot === IMPLICIT_HYDROGEN || (typeof slot === 'string' && slot !== partner && (neighbors.get(own)?.has(slot) ?? false))
  return (
    value.kind === 'relative' &&
    hasOnly(value, ['kind', 'begin_neighbor', 'end_neighbor', 'relation']) &&
    reference(value.begin_neighbor, bond.begin, bond.end) &&
    reference(value.end_neighbor, bond.end, bond.begin) &&
    RELATIONS.includes(value.relation)
  )
}

/** A document's stereo table, empty for an older service that has none. */
export function stereoOf(document: { stereo?: StereoView }): Readonly<StereoView> {
  return document.stereo ?? NO_STEREO
}

/** The marks on a document's bonds and the configurations it states; nothing is derived. */
export function stereoCounts(document: { bonds: ReadonlyArray<{ mark?: StereoMark | null }>; stereo?: StereoView }) {
  const stereo = stereoOf(document)
  return {
    marks: document.bonds.filter((bond) => bond.mark != null).length,
    configurations: stereo.atoms.length + stereo.bonds.length,
  }
}

/** Whether a drawing holds any mark or stated configuration, of any kind. */
export function holdsStereo(document: { bonds: ReadonlyArray<{ mark?: StereoMark | null }>; stereo?: StereoView }): boolean {
  const { marks, configurations } = stereoCounts(document)
  return marks + configurations > 0
}

/**
 * The words for a mark kind. The public specification's plain hash bond is untapered; the mark
 * here, tapered with a narrow end, is its hashed wedge, so that is the name.
 */
export const MARK_NAMES: Record<StereoMark['kind'], string> = {
  wedge: 'wedge',
  hash: 'hashed wedge',
  wavy: 'wavy',
  crossed: 'crossed',
}

/** How a mark reads, naming the atom at the narrow end: "wedge, narrow end at a1". */
export function markName(mark: StereoMark, bond: { begin: string; end: string }): string {
  switch (mark.kind) {
    case 'wedge':
    case 'hash':
      return `${MARK_NAMES[mark.kind]}, narrow end at ${mark.narrow === 'begin' ? bond.begin : bond.end}`
    case 'wavy':
    case 'crossed':
      return MARK_NAMES[mark.kind]
    default: {
      const unhandled: never = mark
      return unhandled
    }
  }
}

/** Whether `value` is the `notes` list of a document reply, which holds only stereo notes. */
export function isStereoNotes(value: unknown): value is StereoNote[] {
  return (
    Array.isArray(value) &&
    value.every(
      (note) =>
        isRecord(note) &&
        hasOnly(note, ['feature', 'source_path', 'disposition', 'reason', 'semantic_impact']) &&
        note.feature === 'stereo_configuration' &&
        typeof note.source_path === 'string' &&
        note.disposition === 'dropped' &&
        typeof note.reason === 'string' &&
        note.semantic_impact === 'stereo',
    )
  )
}
