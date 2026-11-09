/**
 * Hand-maintained mirror of the stereo parts of the v1 document contract in
 * crates/application/src/contract.rs and of the `chem.stereo` reply: bond marks, stated
 * configurations, and the service's comparison of the two. Representation only. Nothing here
 * derives a configuration from a mark, computes a CIP label (that is `chem.cip`, read by
 * lib/cip-contract.ts), or compares two drawings: the
 * service does (it derives from the marks and one instance's positions, for
 * `set_configuration_from_drawing` and for `chem.stereo`), and this module reads and checks what
 * it answers. The editor draws marks (editor/geometry.ts), sends marks and the configuration
 * the service derives from them (editor/mark-gestures.ts), and shows the comparison
 * (editor/stereo-agreement.ts).
 *
 * The validators fail closed on what they know: an unknown mark, configuration, slot or group
 * kind, a wrong type or value, a missing field, or a record that does not fit the graph refuses
 * the whole reply. A field the reader does not know is ignored at every level (docs/CONTRACT.md,
 * "Version transitions"), even one that belongs to another variant (the editor reads a mark's `narrow` end only for a wedge or a hashed wedge). The exception is a stored record, which `sameValue` compares with the snapshot's own record key for key. An unknown bond style is
 * refused rather than drawn as a plain line, which would hide it. Extend the validator when the
 * service sends a new kind.
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
  /**
   * Enhanced-stereo groups in identifier order, added within v1. Omitted by an older service, and
   * read as no groups. A group states which stored centers belong together; it is no ratio.
   */
  groups?: StereoGroup[]
}

/** What an enhanced-stereo group states. `absolute` has no number. */
export type StereoGroupKind = 'absolute' | 'and' | 'or'

/**
 * One enhanced-stereo group as a snapshot shows it. `stored_number` is the number as stated
 * (`null` when none; 0 and negatives are kept), `number` the interpreted one (`null` for
 * `absolute`, otherwise the stored number or 0) and `label` the display text the service built
 * from them (`abs`, `and2`, `or0`). `members` are atom ids, ascending, each a stored tetrahedral
 * center of the same snapshot.
 */
export interface StereoGroup {
  id: string
  kind: StereoGroupKind
  stored_number: number | null
  number: number | null
  label: string
  members: string[]
}

/** The note features the service sends; the one source of the type and of the validator. */
export const NOTE_FEATURES = ['stereo_configuration', 'stereo_group_member', 'stereo_group', 'stereo_group_relation'] as const

/**
 * What a transaction cleared of stereo because it changed what the record was about: one entry
 * per record, shaped like the contract's loss entries. Describes that request only.
 */
export interface StereoNote {
  feature: StereoNoteFeature
  /** `atoms[a1]` for a center or a group member, `bonds[b5]` for a double bond, `groups[e1]` for a group. */
  source_path: string
  disposition: 'dropped' | 'normalized'
  reason: string
  semantic_impact: 'stereo'
}

/**
 * `stereo_configuration`: a stated configuration was cleared. The group features report a
 * change of what a group states: `stereo_group_member` (a member left), `stereo_group` (a group
 * disappeared) and `stereo_group_relation` (which centers belong together changed).
 */
export type StereoNoteFeature = (typeof NOTE_FEATURES)[number]

/** A document with no stated configuration, and what an older service's reply is read as. */
export const NO_STEREO: Readonly<StereoView> = Object.freeze({ atoms: [], bonds: [], groups: [] })

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const BOND_ENDS: readonly unknown[] = ['begin', 'end']
const TURNS: readonly unknown[] = ['anticlockwise', 'clockwise']
const RELATIONS: readonly unknown[] = ['same_side', 'opposite_sides']

/** Whether `value` is a mark that a bond of `order` can carry: wedge, hash, wavy on single; crossed on double. */
export function isStereoMark(value: unknown, order: string): value is StereoMark {
  if (!isRecord(value)) return false
  switch (value.kind) {
    case 'wedge':
    case 'hash':
      return BOND_ENDS.includes(value.narrow) && order === 'single'
    case 'wavy':
      return order === 'single'
    case 'crossed':
      return order === 'double'
    default:
      return false
  }
}

export interface GraphBond {
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
  if (!isRecord(value) || !Array.isArray(value.atoms) || !Array.isArray(value.bonds)) {
    return false
  }
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const neighbors = bondedAtoms(bonds)
  const centers = new Set<string>()
  for (const record of value.atoms as unknown[]) {
    if (!isRecord(record) || typeof record.atom !== 'string' ||
        !atomIds.has(record.atom) || centers.has(record.atom) || !isCenterConfiguration(record.configuration, neighbors.get(record.atom) ?? new Set())) {
      return false
    }
    centers.add(record.atom)
  }
  const doubles = new Map(bonds.filter((bond) => bond.order === 'double').map((bond) => [bond.id, bond]))
  const configured = new Set<string>()
  for (const record of value.bonds as unknown[]) {
    if (!isRecord(record) || typeof record.bond !== 'string' || configured.has(record.bond)) {
      return false
    }
    const bond = doubles.get(record.bond)
    if (bond === undefined || !isBondConfiguration(record.configuration, bond, neighbors)) return false
    configured.add(record.bond)
  }
  return value.groups === undefined || isStereoGroups(value.groups, value.atoms as StereoView['atoms'])
}

const GROUP_KINDS: readonly unknown[] = ['absolute', 'and', 'or']
const INT32_MIN = -(2 ** 31)
const INT32_MAX = 2 ** 31 - 1
const isInt32 = (value: unknown): value is number => Number.isInteger(value) && (value as number) >= INT32_MIN && (value as number) <= INT32_MAX

/**
 * Whether `value` is the group list of a snapshot whose stated centers are `records`: ids
 * ascending, a known kind, a stored number that is `null` or a 32-bit integer (none on
 * `absolute`), the interpreted number and label exactly what the service derives from kind and
 * stored number, members ascending, each a stored tetrahedral center in no other group, at most
 * one absolute group, and one number for each `and` and `or` group. It checks no chemistry.
 */
function isStereoGroups(value: unknown, records: StereoView['atoms']): value is StereoGroup[] {
  if (!Array.isArray(value)) return false
  const tetrahedral = new Set(records.filter((record) => record.configuration.kind === 'tetrahedral').map((record) => record.atom))
  const grouped = new Set<string>()
  const numbered = new Set<string>()
  let absolute = false
  let previous = 0
  for (const group of value as unknown[]) {
    if (!isRecord(group) ||
        typeof group.id !== 'string' || !/^e[1-9]\d*$/.test(group.id) || !GROUP_KINDS.includes(group.kind) ||
        !(group.stored_number === null || isInt32(group.stored_number)) || !Array.isArray(group.members) ||
        group.members.length === 0 || !ascending(group.members as string[])) {
      return false
    }
    if (serial(group.id) <= previous) return false
    previous = serial(group.id)
    const interpreted = group.kind === 'absolute' ? null : (group.stored_number ?? 0)
    if (group.number !== interpreted || group.label !== (interpreted === null ? 'abs' : `${group.kind as string}${interpreted}`)) return false
    if (group.kind === 'absolute') {
      if (group.stored_number !== null || absolute) return false
      absolute = true
    } else {
      const key = `${group.kind as string}:${interpreted}`
      if (numbered.has(key)) return false
      numbered.add(key)
    }
    for (const member of group.members as unknown[]) {
      if (typeof member !== 'string' || !tetrahedral.has(member) || grouped.has(member)) return false
      grouped.add(member)
    }
  }
  return true
}

/** The atoms each atom is bonded to. */
export function bondedAtoms(bonds: readonly GraphBond[]): Map<string, Set<string>> {
  const neighbors = new Map<string, Set<string>>()
  for (const bond of bonds) {
    for (const [from, to] of [[bond.begin, bond.end], [bond.end, bond.begin]] as const) {
      const bonded = neighbors.get(from) ?? new Set<string>()
      bonded.add(to)
      neighbors.set(from, bonded)
    }
  }
  return neighbors
}

function isCenterConfiguration(value: unknown, bonded: ReadonlySet<string>): value is AtomConfiguration {
  if (!isRecord(value) || !Array.isArray(value.neighbors) || value.neighbors.length !== 4) return false
  const slots: unknown[] = value.neighbors
  if (!slots.every((slot) => typeof slot === 'string') || new Set(slots).size !== 4) return false
  const named = slots as string[]
  const explicit = named.filter((slot) => slot !== IMPLICIT_HYDROGEN && slot !== LONE_PAIR)
  const exact = bonded.size >= 3 && bonded.size <= 4 && explicit.length === bonded.size && explicit.every((slot) => bonded.has(slot))
  if (!exact) return false
  if (value.kind === 'tetrahedral') return TURNS.includes(value.turn)
  return value.kind === 'unknown'
}

function isBondConfiguration(value: unknown, bond: GraphBond, neighbors: ReadonlyMap<string, ReadonlySet<string>>): value is BondConfiguration {
  if (!isRecord(value)) return false
  if (value.kind === 'unknown') return true
  // A reference is a bonded atom of its own end other than the double bond's partner, or the hydrogen.
  const reference = (slot: unknown, own: string, partner: string) =>
    slot === IMPLICIT_HYDROGEN || (typeof slot === 'string' && slot !== partner && (neighbors.get(own)?.has(slot) ?? false))
  return (
    value.kind === 'relative' &&
    reference(value.begin_neighbor, bond.begin, bond.end) &&
    reference(value.end_neighbor, bond.end, bond.begin) &&
    RELATIONS.includes(value.relation)
  )
}

/**
 * A document's stereo table, empty for an older service that has none. An omitted group list
 * reads as no groups (`groups` is additive within contract 1), so a snapshot from an older
 * service and one from a current service with `groups: []` are the same table.
 */
export function stereoOf(document: { stereo?: StereoView }): Readonly<StereoView> {
  const { stereo } = document
  if (stereo === undefined) return NO_STEREO
  return stereo.groups === undefined ? { ...stereo, groups: [] } : stereo
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

/**
 * What each note feature may carry, as the service emits it: the dispositions and the shape of
 * `source_path`. A feature this build does not know is refused, as every other unknown value.
 */
const NOTE_SHAPES: Record<StereoNoteFeature, { dispositions: readonly unknown[]; path: RegExp }> = {
  stereo_configuration: { dispositions: ['dropped'], path: /^(atoms\[a[1-9]\d*\]|bonds\[b[1-9]\d*\])$/ },
  stereo_group_member: { dispositions: ['dropped'], path: /^atoms\[a[1-9]\d*\]$/ },
  stereo_group: { dispositions: ['dropped'], path: /^groups\[e[1-9]\d*\]$/ },
  stereo_group_relation: { dispositions: ['dropped', 'normalized'], path: /^groups\[e[1-9]\d*\]$/ },
}

const isNoteFeature = (value: unknown): value is StereoNoteFeature => NOTE_FEATURES.some((feature) => feature === value)

/** Whether `value` is the `notes` list of a document reply, which holds only stereo notes. */
export function isStereoNotes(value: unknown): value is StereoNote[] {
  return (
    Array.isArray(value) &&
    value.every(
      (note) =>
        isRecord(note) &&
        isNoteFeature(note.feature) &&
        typeof note.source_path === 'string' &&
        NOTE_SHAPES[note.feature].path.test(note.source_path) &&
        NOTE_SHAPES[note.feature].dispositions.includes(note.disposition) &&
        typeof note.reason === 'string' &&
        note.semantic_impact === 'stereo',
    )
  )
}

/**
 * How one stored item and the drawing of one scene instance relate, as `chem.stereo` decides it
 * (docs/CONTRACT.md, Stereo inspection). Whether anything is a stereocenter is not part of it.
 */
export type Agreement = 'agrees' | 'disagrees' | 'only_stored' | 'only_drawn' | 'neither'

const AGREEMENTS: readonly unknown[] = ['agrees', 'disagrees', 'only_stored', 'only_drawn', 'neither']

/**
 * Why a drawing gives no configuration for an item: the six `stereo_not_derivable_*` reasons, and
 * the structural `stereo_*` codes a drawing can show (the model cannot hold a state there).
 * `stereo_mark_bond_order` and `stereo_record_invalid` never describe a drawing, so they are not
 * accepted.
 */
export const DRAWN_REASONS = [
  'stereo_not_derivable_no_marks',
  'stereo_not_derivable_conflicting_marks',
  'stereo_not_derivable_degenerate',
  'stereo_not_derivable_coincident_atoms',
  'stereo_not_derivable_fourth_position',
  'stereo_not_derivable_no_reference',
  'stereo_center_unsupported',
  'stereo_center_neighbors',
  'stereo_double_bond_required',
  'stereo_reference_invalid',
  'stereo_geometry_unsupported',
] as const
export type DrawnReason = (typeof DRAWN_REASONS)[number]
const REASONS: readonly unknown[] = DRAWN_REASONS

/** What one scene instance's drawing shows for a center. `configuration` is null exactly when `reason` is set. */
export interface DrawnCenter {
  instance: string
  configuration: AtomConfiguration | null
  reason: DrawnReason | null
  agreement: Agreement
  /** Only the label of the non-atom position (hydrogen or lone pair) differs; `agrees` ignores it. */
  label_differs: boolean
}

/** What one scene instance's drawing shows for a double bond. */
export interface DrawnDouble {
  instance: string
  configuration: BondConfiguration | null
  reason: DrawnReason | null
  agreement: Agreement
}

export interface CenterInspection {
  atom: string
  stored: AtomConfiguration | null
  /** One entry per inspected instance, in `instances` order. */
  drawn: DrawnCenter[]
}

export interface DoubleInspection {
  bond: string
  stored: BondConfiguration | null
  drawn: DrawnDouble[]
}

/** The items of a `chem.stereo` reply, checked against the snapshot it names. */
export interface StereoInspectionBody {
  instances: string[]
  atoms: CenterInspection[]
  bonds: DoubleInspection[]
}

/** An advisory finding. The code is any string: a new one is shown as the service words it. */
export interface StereoDiagnostic {
  code: string
  severity: 'info' | 'warning'
  message: string
}

/** The snapshot graph a `chem.stereo` reply is about. */
export interface InspectedGraph {
  atoms: ReadonlyArray<{ id: string }>
  bonds: readonly GraphBond[]
  stereo: StereoView
  /** The scene instances of the snapshot; the reply must inspect exactly these. */
  instances: readonly string[]
}

/**
 * Same value whatever the order of an object's keys; an absent value equals nothing. It is an
 * identity check (the `stored` record must equal the snapshot's own record key for key), not a
 * shape check, so it is deliberately not relaxed.
 */
export function sameValue(a: unknown, b: unknown): boolean {
  const canonical = (value: unknown) =>
    JSON.stringify(value, (_key, item: unknown) =>
      isRecord(item) ? Object.fromEntries(Object.entries(item).sort(([x], [y]) => (x < y ? -1 : 1))) : item)
  return canonical(a) === canonical(b)
}

/** Identifiers are a letter prefix and a serial; the service orders them by serial. */
export function serial(id: string): number {
  const digits = /^[a-z]+(\d+)$/.exec(id)?.[1]
  return digits === undefined ? Number.NaN : Number(digits)
}

export const ascending = (ids: readonly string[]) => ids.every((id, i) => Number.isFinite(serial(id)) && (i === 0 || serial(ids[i - 1]) < serial(id)))

function judge<T>(stored: T | null, drawn: T | null, agree: (a: T, b: T) => boolean): Agreement {
  if (stored !== null) return drawn === null ? 'only_stored' : agree(stored, drawn) ? 'agrees' : 'disagrees'
  return drawn === null ? 'neither' : 'only_drawn'
}

/** The neighbor positions of a center, a hydrogen and a lone pair being the same position. */
const positions = (center: AtomConfiguration) => center.neighbors.map((slot) => (slot === LONE_PAIR ? IMPLICIT_HYDROGEN : slot))

/** The service's rule: same kind, same neighbor positions, and for a tetrahedral center the same turn. */
function centersAgree(a: AtomConfiguration, b: AtomConfiguration): boolean {
  if (a.kind !== b.kind || !sameValue(positions(a), positions(b))) return false
  return a.kind !== 'tetrahedral' || (b.kind === 'tetrahedral' && a.turn === b.turn)
}

const nonAtomSlot = (center: AtomConfiguration) => center.neighbors.find((slot) => slot === IMPLICIT_HYDROGEN || slot === LONE_PAIR)

function labelDiffers(a: AtomConfiguration, b: AtomConfiguration): boolean {
  const [x, y] = [nonAtomSlot(a), nonAtomSlot(b)]
  return x !== undefined && y !== undefined && x !== y
}

/** The part of a drawn entry both items share, or null when it is not one the service can send. */
function readDrawn<T>(
  value: unknown,
  instance: string,
  stored: T | null,
  isConfiguration: (candidate: unknown) => candidate is T,
  agree: (a: T, b: T) => boolean,
): { instance: string; configuration: T | null; reason: DrawnReason | null; agreement: Agreement } | null {
  if (!isRecord(value) || value.instance !== instance) return null
  const { configuration, reason, agreement } = value
  if (configuration !== null && !isConfiguration(configuration)) return null
  if (reason !== null && !REASONS.includes(reason)) return null
  if ((configuration === null) === (reason === null)) return null
  if (!AGREEMENTS.includes(agreement) || agreement !== judge(stored, configuration, agree)) return null
  return { instance, configuration, reason: reason as DrawnReason | null, agreement: agreement as Agreement }
}

/**
 * Whether the items of a `chem.stereo` reply (`result` minus its snapshot) are what the service
 * sends, apart from fields this reader does not know, and about `graph`: the instances are the
 * snapshot's, atoms and bonds ascend without repeats, each `stored` is the snapshot's own record
 * (and every record of the snapshot is an item), each instance has one entry in order, a drawn
 * configuration fits the graph, a configuration is null exactly when a reason is given, and the
 * agreement and the label flag are the ones the stored and drawn states decide. Any other reply
 * is refused whole.
 */
export function readStereoInspection(result: Record<string, unknown>, graph: InspectedGraph): StereoInspectionBody | null {
  if (typeof result.valence_model !== 'string' || result.stereogenicity_assessed !== false ||
      !Array.isArray(result.instances) || !Array.isArray(result.atoms) || !Array.isArray(result.bonds)) {
    return null
  }
  const expected = [...graph.instances].sort((a, b) => serial(a) - serial(b))
  const instances: unknown[] = result.instances
  if (instances.length !== expected.length || !instances.every((id, i) => id === expected[i])) return null
  const bonded = bondedAtoms(graph.bonds)
  const atomIds = new Set(graph.atoms.map((atom) => atom.id))
  const doubles = new Map(graph.bonds.filter((bond) => bond.order === 'double').map((bond) => [bond.id, bond]))

  const atoms: CenterInspection[] = []
  for (const entry of result.atoms as unknown[]) {
    if (!isRecord(entry) || typeof entry.atom !== 'string' || !atomIds.has(entry.atom) ||
        !Array.isArray(entry.drawn) || entry.drawn.length !== expected.length) {
      return null
    }
    const around = bonded.get(entry.atom) ?? new Set<string>()
    const record = graph.stereo.atoms.find((candidate) => candidate.atom === entry.atom)?.configuration ?? null
    if (!sameValue(entry.stored, record)) return null
    const isCenter = (candidate: unknown): candidate is AtomConfiguration => isCenterConfiguration(candidate, around)
    const drawn: DrawnCenter[] = []
    for (const [i, item] of (entry.drawn as unknown[]).entries()) {
      const read = readDrawn(item, expected[i], record, isCenter, centersAgree)
      const differs = record !== null && read?.configuration != null && labelDiffers(record, read.configuration)
      if (!read || !isRecord(item) || item.label_differs !== differs) return null
      drawn.push({ ...read, label_differs: differs })
    }
    atoms.push({ atom: entry.atom, stored: record, drawn })
  }

  const bonds: DoubleInspection[] = []
  for (const entry of result.bonds as unknown[]) {
    if (!isRecord(entry) || typeof entry.bond !== 'string' ||
        !Array.isArray(entry.drawn) || entry.drawn.length !== expected.length) {
      return null
    }
    const bond = doubles.get(entry.bond)
    if (bond === undefined) return null
    const record = graph.stereo.bonds.find((candidate) => candidate.bond === entry.bond)?.configuration ?? null
    if (!sameValue(entry.stored, record)) return null
    const isDouble = (candidate: unknown): candidate is BondConfiguration => isBondConfiguration(candidate, bond, bonded)
    const drawn: DrawnDouble[] = []
    for (const [i, item] of (entry.drawn as unknown[]).entries()) {
      const read = readDrawn(item, expected[i], record, isDouble, sameValue)
      if (!read) return null
      drawn.push(read)
    }
    bonds.push({ bond: entry.bond, stored: record, drawn })
  }

  if (!ascending(atoms.map((item) => item.atom)) || !ascending(bonds.map((item) => item.bond))) return null
  const listedAtoms = new Set(atoms.map((item) => item.atom))
  const listedBonds = new Set(bonds.map((item) => item.bond))
  if (!graph.stereo.atoms.every((record) => listedAtoms.has(record.atom)) || !graph.stereo.bonds.every((record) => listedBonds.has(record.bond))) return null
  return { instances: expected, atoms, bonds }
}

/** The advisory findings of a `chem.stereo` reply, or null when they are not a list of findings. */
export function readStereoDiagnostics(value: unknown): StereoDiagnostic[] | null {
  if (!Array.isArray(value)) return null
  const found: StereoDiagnostic[] = []
  for (const item of value as unknown[]) {
    if (!isRecord(item) || typeof item.code !== 'string' || typeof item.message !== 'string' ||
        (item.severity !== 'info' && item.severity !== 'warning')) {
      return null
    }
    found.push({ code: item.code, severity: item.severity, message: item.message })
  }
  return found
}
