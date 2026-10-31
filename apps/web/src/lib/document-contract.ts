/** Hand-maintained mirror of the v1 document contract in crates/application/src/contract.rs. */
import { API_VERSION } from '@/lib/contract'

/**
 * A bond's kind. `aromatic`, `dative` (from `begin`, the donor), `query`, and `unknown` were
 * added within v1. Recording is separate from bounded aromaticity inspection; no query matches.
 */
export type BondOrder = 'single' | 'double' | 'triple' | 'aromatic' | 'dative' | 'query' | 'unknown'
/** The kinds a query bond may list. */
export type QueryBondOrder = 'single' | 'double' | 'triple' | 'aromatic'

export interface Position {
  x: number
  y: number
}

/** What a query atom stands for. Nothing matches it. */
export type QueryAtom =
  | { kind: 'any' | 'any_heavy' | 'heteroatom' | 'halogen' }
  | { kind: 'list'; elements: string[]; negated: boolean }

/** What a query bond stands for. Nothing matches it. */
export type BondQuery = { kind: 'any' } | { kind: 'list'; orders: QueryBondOrder[] }

/**
 * An atom's recorded hydrogen state. `inferred` stores no count; the separate valence
 * inspector can derive a count for supported states without changing this record.
 */
export type Hydrogens = { mode: 'inferred' } | { mode: 'unknown' } | { mode: 'specified'; count: number }

export type Radical = 'none' | 'singlet' | 'doublet' | 'triplet' | 'unknown'

/** The most hydrogens an atom can be stated to carry, and the attachment points it may have. */
export const MAX_STATED_HYDROGENS = 16
export const ATTACHMENT_POINTS = [1, 2, 3, 4] as const

export interface AtomView {
  id: string
  /**
   * The element symbol, or for a query atom its label (`*`, `A`, `Q`, `X`, `[N,O]`, `![N,O]`),
   * which is not a symbol: `kind` says which.
   */
  element: string
  charge: number
  isotope: number | null
  /** The atom's position in the first scene instance. */
  position: Position
  /**
   * Recorded states, added within v1. A reply from before them leaves them out; the client
   * fills in the defaults (an element, inferred hydrogens, no radical, no attachment points).
   */
  kind: 'element' | 'query'
  query: QueryAtom | null
  hydrogens: Hydrogens
  radical: Radical
  /** Ascending, 1–4. */
  attachment_points: number[]
}

/** One atom's position in a scene instance. */
export interface InstancePosition {
  atom: string
  x: number
  y: number
}

/** A molecule scene instance: the molecule drawn on a page, one position per atom. */
export interface InstanceView {
  id: string
  page: string
  molecule: string
  positions: InstancePosition[]
}

export type GraphicShape = 'line' | 'rectangle' | 'arrow'

export type SceneObject =
  | { kind: 'instance'; id: string; parent: string | null; molecule: string }
  | { kind: 'group'; id: string; parent: string | null; children: string[] }
  | { kind: 'graphic'; id: string; parent: string | null; shape: GraphicShape; points: [Position, Position] }

export interface ReferenceTarget {
  kind: 'object' | 'atom' | 'bond'
  id: string
}

export interface SceneReference {
  source: string
  target: ReferenceTarget
}

export interface SelectionView {
  objects: string[]
  atoms: string[]
  bonds: string[]
}

export interface SelectionSnapshots {
  before?: SelectionView
  after?: SelectionView
}

export interface SceneView {
  pages: { id: string }[]
  /** In order; the first one's positions are `atoms[].position`. */
  instances: InstanceView[]
  /** Both fields are absent when the service predates scene objects. */
  roots?: string[]
  objects?: SceneObject[]
  references?: SceneReference[]
}

export interface BondView {
  id: string
  begin: string
  end: string
  order: BondOrder
  /** With order `query`, what it stands for; null otherwise. Filled in as null for older replies. */
  query: BondQuery | null
}

/** The generic font family atom labels are drawn in. */
export type LabelFont = 'sans-serif' | 'serif' | 'monospace'
/** `default`: a bonded carbon recording nothing is a bare vertex; `all`: every carbon is labelled. */
export type CarbonLabels = 'default' | 'all'
/** The unit lengths are shown in; lengths are always stored in pt (1 in = 72 pt = 25.4 mm). */
export type DisplayUnit = 'pt' | 'mm' | 'cm' | 'in'

/** A document's style settings, saved and undone with it. Added within v1. */
export interface DocumentSettings {
  bond_length_pt: number
  line_width_pt: number
  label_font: LabelFont
  label_size_pt: number
  carbon_labels: CarbonLabels
  display_unit: DisplayUnit
}

/** Settings fields to change; the rest keep their values. */
export type SettingsEdit = Partial<DocumentSettings>

/** The built-in defaults: what a document starts from unless preferences name other values. */
export const DEFAULT_SETTINGS: Readonly<DocumentSettings> = Object.freeze({
  bond_length_pt: 30,
  line_width_pt: 1.2,
  label_font: 'sans-serif',
  label_size_pt: 12,
  carbon_labels: 'default',
  display_unit: 'pt',
})

/** The settings stored as a number, each with the range (in pt) the service accepts. */
export const SETTING_RANGES = Object.freeze({
  bond_length_pt: [5, 200],
  line_width_pt: [0.1, 10],
  label_size_pt: [4, 72],
} as const)
export type NumericSetting = keyof typeof SETTING_RANGES

export const LABEL_FONTS: readonly LabelFont[] = ['sans-serif', 'serif', 'monospace']
export const CARBON_LABELS: readonly CarbonLabels[] = ['default', 'all']
export const DISPLAY_UNITS: readonly DisplayUnit[] = ['pt', 'mm', 'cm', 'in']

export interface DocumentView {
  id: string
  title: string
  revision: number
  /** The chemical graph `molecule` holds: atoms and bonds. */
  atoms: AtomView[]
  bonds: BondView[]
  /**
   * The graph and its scene, added within v1. A service from before them leaves both out; its
   * document is one drawing of the atoms at their positions.
   */
  molecule?: { id: string }
  scene?: SceneView
  selection?: SelectionView
  /** Omitted by services predating document style settings. */
  settings?: DocumentSettings
}

export interface ValenceAtom {
  atom_id: string
  bond_order_sum: number | null
  hydrogens: number | null
  status: 'valid' | 'invalid' | 'unsupported'
  reason: string | null
}

export interface ValenceInspection {
  model: string
  snapshot: DocumentView
  atoms: ValenceAtom[]
}

export interface AromaticitySystem {
  atoms: string[]
  bonds: string[]
  cycle_rank: number
  classification: 'aromatic' | 'non_aromatic' | 'unsupported'
  electrons: number | null
  reason: string | null
}

export interface AromaticityInspection {
  model: string
  snapshot: DocumentView
  systems: AromaticitySystem[]
}

export type IdentityMode = 'recorded' | 'chemical'

export interface CanonicalIdentity {
  code: string
  numbering: string[]
  symmetry: string[][]
  folded_hydrogens: { hydrogen: string; attached_to: string }[]
}

export interface IdentityInspection {
  model: string
  snapshot: DocumentView
  identity: CanonicalIdentity
  comparison: { snapshot: DocumentView; recovery: RecoveryView | null; identity: CanonicalIdentity; equivalent: boolean } | null
}

/** ID and revision alone can be reused across service processes. Compare the analyzed graph
 * as well, so a replacement snapshot cannot inherit an earlier scientific result. */
export function sameInspectionSnapshot(shown: DocumentView, inspected: DocumentView): boolean {
  return shown.id === inspected.id && shown.revision === inspected.revision &&
    JSON.stringify(shown.atoms) === JSON.stringify(inspected.atoms) &&
    JSON.stringify(shown.bonds) === JSON.stringify(inspected.bonds)
}

export type PropertiesScope = 'molecule' | 'selection' | 'components'
export type PropertyKind = 'formula' | 'masses' | 'distribution'

export interface OmittedIsotopeAbundance { element: string; isotope: number }
export interface PropertyMass {
  average_u: number
  exact_u: number
  average_display: string
  exact_display: string
  exact_natural_weight_fallbacks: string[]
  average_bases: { element: string; basis: string }[]
  exact_selections: { element: string; isotope: number; basis: string }[]
  average_omitted_abundances: OmittedIsotopeAbundance[]
  exact_omitted_abundances: OmittedIsotopeAbundance[]
}

export interface PropertyComponent {
  atoms: string[]
  formula: string
  formal_charge: number
  mass: PropertyMass | null
  isotope_distribution: null | { mass_u: number; mass_display: string; probability: number }[]
  omitted_abundances: OmittedIsotopeAbundance[]
  abundance_sums: { element: string; input_sum: number }[]
}

export interface PropertiesInspection {
  model: string
  snapshot: DocumentView
  scope: PropertiesScope
  property: PropertyKind
  selection: SelectionView | null
  display_decimals: number
  valence_model: string
  arithmetic: string
  data: null | { registration_id: string; generation: number; dataset_id: string; dataset_version: string; profile_id: string; manifest_sha256: string; source_schema_id?: string | null; calculation_profile: string; charge_correction_u_per_e: number; charge_convention: string; abundance_policy: string; uncertainty: string }
  components: PropertyComponent[]
  warnings: string[]
}

/** Graph content is checked as well as revision, which can be reused after a restart. */
export function samePropertySnapshot(shown: DocumentView, inspected: DocumentView): boolean {
  return shown.id === inspected.id && shown.revision === inspected.revision &&
    JSON.stringify(shown.atoms) === JSON.stringify(inspected.atoms) &&
    JSON.stringify(shown.bonds) === JSON.stringify(inspected.bonds) &&
    JSON.stringify(shown.scene) === JSON.stringify(inspected.scene)
}

/** The document's own style, or built-in defaults for an older service. */
export function settingsOf(document: DocumentView): Readonly<DocumentSettings> {
  return document.settings ?? DEFAULT_SETTINGS
}

export interface SessionView {
  saved_revision: number
  unsaved_changes: boolean
  undo_depth: number
  redo_depth: number
  /** The revision this session's recovery draft holds, or null when it has none. */
  draft_revision: number | null
  /**
   * The service's latest failed attempt to draft work the draft does not hold, while it
   * retries. Added within v1, so a service from before it leaves it out.
   */
  draft_failure?: DraftFailureView | null
}

export interface DraftFailureView {
  /** Failed attempts in a row. */
  attempts: number
  retry_in_ms: number
  /**
   * The latest attempt's error code and, for `storage_failed`, its reason, as in the v1 error
   * envelope. Added within v1 after the record itself, so an earlier service leaves them out.
   */
  code?: string
  reason?: StorageReason
  message: string
}

export type RecoveryState = 'recoverable' | 'outdated' | 'unreadable' | 'in_use'

/** An abandoned recovery draft: unsaved work of a session whose process ended. */
export interface RecoveryView {
  state: RecoveryState
  revision: number | null
  base_revision: number | null
  saved_at_ms: number | null
  message: string | null
}

export interface DocumentResponse {
  api_version: typeof API_VERSION
  document: DocumentView
  session: SessionView
  recovery: RecoveryView | null
}

/** A document saved as a new one; the copy is open in its own session. */
export interface CopyResponse extends DocumentResponse {
  source: { id: string; closed: boolean; discarded_changes: boolean }
}

export interface DocumentSummary {
  id: string
  title: string
  revision: number
  atom_count: number
  bond_count: number
  open: boolean
  unsaved_changes: boolean
  recovery: RecoveryView | null
  /** When storage last committed it, in milliseconds since the Unix epoch. */
  saved_at_ms: number | null
}

/** Title order, or most recently saved first. */
export type ListSort = 'title' | 'recent'

export interface ListQuery {
  /** Text the title must contain, ignoring case. */
  query: string
  sort: ListSort
}

export interface TrashedSummary {
  id: string
  title: string
  revision: number
  atom_count: number
  bond_count: number
  deleted_at_ms: number | null
}

export interface TrashListResponse {
  api_version: typeof API_VERSION
  documents: TrashedSummary[]
  unreadable: { id: string; message: string }[]
}

export interface DocumentListResponse {
  api_version: typeof API_VERSION
  documents: DocumentSummary[]
  unreadable: { id: string; message: string }[]
}

/** `"$N"` names the atom added by command N earlier in the same transaction. */
export type AtomRef = string

/**
 * Where a new atom goes: one `position` while the document has one scene instance, otherwise
 * `positions` with a point for every instance.
 */
export type NewAtomPlacement = { position: Position } | { positions: Record<string, Position> }

/** The recorded states a command may set; omitted ones keep their default or current value. */
export interface AtomStates {
  hydrogens?: Hydrogens
  radical?: Radical
  attachment_points?: number[]
}

/** A bond's kind as a command sends it: `query` comes with order `query` and only with it. */
export interface BondKind {
  order: BondOrder
  query?: BondQuery
}

export type Command =
  /** Named by exactly one of `element` and `query`; a query atom carries no charge or isotope. */
  | ({ op: 'add_atom'; bond_to?: { atom: AtomRef } & BondKind } & ({ element: string } | { query: QueryAtom }) &
      NewAtomPlacement &
      AtomStates)
  | ({ op: 'add_bond'; begin: AtomRef; end: AtomRef } & BondKind)
  /** At most one of `element` and `query`; a query atom carries no charge or isotope. */
  | ({
      op: 'update_atom'
      atom: string
      element?: string
      query?: QueryAtom
      charge?: number
      isotope?: number | null
    } & AtomStates)
  /** `instance` may be left out while the document has one scene instance. */
  | { op: 'move_atoms'; atoms: string[]; delta: Position; instance?: string }
  | ({ op: 'update_bond'; bond: string } & BondKind)
  | { op: 'delete_atoms'; atoms: string[] }
  | { op: 'delete_bonds'; bonds: string[] }
  | { op: 'rename'; title: string }
  /** Draws the molecule again: a scene instance copying `source` (default: the first) moved by `offset`. */
  | { op: 'add_instance'; source?: string; offset: Position }
  | { op: 'move_instance'; instance: string; offset: Position }
  | { op: 'delete_instance'; instance: string }
  | { op: 'add_graphic'; shape: GraphicShape; points: [Position, Position]; parent?: string; index?: number }
  | { op: 'group'; objects: string[] }
  | { op: 'ungroup'; group: string }
  | { op: 'reorder'; object: string; index: number }
  | { op: 'delete_objects'; objects: string[]; mode: 'subtree' | 'object_only' }
  | { op: 'move_objects'; objects: string[]; delta: Position }
  | { op: 'add_reference' | 'remove_reference'; source: string; target: ReferenceTarget }
  | ({ op: 'update_settings' } & SettingsEdit)

/**
 * One drawing of the document's atoms: a scene instance, or, for a service from before scene
 * instances (`id` null), the atoms at their own positions.
 */
export interface Placement {
  id: string | null
  positions: ReadonlyMap<string, Position>
}

/** The document's drawings in order. There is always at least one. */
export function placements(document: DocumentView): Placement[] {
  if (!document.scene) {
    return [{ id: null, positions: new Map(document.atoms.map((atom) => [atom.id, atom.position])) }]
  }
  return document.scene.instances.map((instance) => ({
    id: instance.id,
    positions: new Map(instance.positions.map(({ atom, x, y }) => [atom, { x, y }])),
  }))
}

/** Why storage failed (`storage_failed`), so the page can say whether retrying can help. */
export type StorageReason = 'read_only' | 'busy' | 'io'

const isStorageReason = (value: unknown): value is StorageReason =>
  value === 'read_only' || value === 'busy' || value === 'io'

/** Optional fields of a v1 error envelope. */
export interface ErrorDetails {
  currentRevision?: number
  commandIndex?: number
  reason?: StorageReason
  /** The code of a refused state, scene rule, or document/new-document setting. */
  stateReason?: string
}

/**
 * A v1 error envelope, or a failure found by the client. Status 0 means no reply arrived
 * (code `unreachable`), so the request may or may not have taken effect.
 */
export class ApiError extends Error {
  readonly code: string
  readonly status: number
  readonly currentRevision?: number
  readonly commandIndex?: number
  readonly reason?: StorageReason
  readonly stateReason?: string

  constructor(code: string, message: string, status: number, details: ErrorDetails = {}) {
    super(message)
    this.name = 'ApiError'
    this.code = code
    this.status = status
    this.currentRevision = details.currentRevision
    this.commandIndex = details.commandIndex
    this.reason = details.reason
    this.stateReason = details.stateReason
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

const isCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0
const isFiniteNumber = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)
const BOND_ORDERS: readonly unknown[] = ['single', 'double', 'triple', 'aromatic', 'dative', 'query', 'unknown']
const QUERY_BOND_ORDERS: readonly unknown[] = ['single', 'double', 'triple', 'aromatic']
const RADICALS: readonly unknown[] = ['none', 'singlet', 'doublet', 'triplet', 'unknown']
const NAMED_QUERIES: readonly unknown[] = ['any', 'any_heavy', 'heteroatom', 'halogen']
/** The recorded-state fields of an atom, which come all together or not at all. */
const ATOM_STATE_FIELDS = ['kind', 'query', 'hydrogens', 'radical', 'attachment_points'] as const

const distinct = (items: unknown[]) => new Set(items).size === items.length
const hasOnly = (value: Record<string, unknown>, keys: string[]) => Object.keys(value).every((key) => keys.includes(key))

function isQueryAtom(value: unknown): value is QueryAtom {
  if (!isRecord(value)) return false
  if (NAMED_QUERIES.includes(value.kind)) return hasOnly(value, ['kind'])
  return (
    value.kind === 'list' &&
    hasOnly(value, ['kind', 'elements', 'negated']) &&
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
  if (value.kind === 'any') return hasOnly(value, ['kind'])
  return (
    value.kind === 'list' &&
    hasOnly(value, ['kind', 'orders']) &&
    Array.isArray(value.orders) &&
    value.orders.length >= 1 &&
    value.orders.every((order) => QUERY_BOND_ORDERS.includes(order)) &&
    distinct(value.orders)
  )
}

function isHydrogens(value: unknown): value is Hydrogens {
  if (!isRecord(value)) return false
  if (value.mode === 'inferred' || value.mode === 'unknown') return hasOnly(value, ['mode'])
  return (
    value.mode === 'specified' &&
    hasOnly(value, ['mode', 'count']) &&
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

function isAtom(value: unknown): value is AtomView {
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

function isBond(value: unknown): value is BondView {
  if (
    !isRecord(value) ||
    typeof value.id !== 'string' ||
    typeof value.begin !== 'string' ||
    typeof value.end !== 'string' ||
    !BOND_ORDERS.includes(value.order)
  ) {
    return false
  }
  // A reply from before recorded states has no `query`, and then only the first three orders.
  if (!('query' in value)) return value.order === 'single' || value.order === 'double' || value.order === 'triple'
  return value.order === 'query' ? isBondQuery(value.query) : value.query === null
}

/**
 * Every atom carries the recorded-state fields or none does, and the bonds agree: a reply holds
 * them all (a service with recorded states) or none (one from before them).
 */
function statesAgree(atoms: readonly object[], bonds: readonly object[]) {
  const shapes = new Set([
    ...atoms.map((atom) => atomStates(atom) === 'all'),
    ...bonds.map((bond) => 'query' in bond),
  ])
  return shapes.size <= 1
}

/** A document whose atoms and bonds carry every recorded-state field, defaults filled in. */
function withStates(response: DocumentResponse): DocumentResponse {
  const atoms = response.document.atoms as Array<AtomView | Omit<AtomView, (typeof ATOM_STATE_FIELDS)[number]>>
  const bonds = response.document.bonds as Array<BondView | Omit<BondView, 'query'>>
  return {
    ...response,
    document: {
      ...response.document,
      atoms: atoms.map((atom) =>
        'kind' in atom
          ? atom
          : { ...atom, kind: 'element', query: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [] },
      ),
      bonds: bonds.map((bond) => ('query' in bond ? bond : { ...bond, query: null })),
    },
  }
}

const isCountOrNull = (value: unknown) => value === null || isCount(value)

const SETTING_FIELDS = ['bond_length_pt', 'line_width_pt', 'label_font', 'label_size_pt', 'carbon_labels', 'display_unit']

/** Whether `value` is a valid value of the setting `field`. */
function isSettingValue(field: string, value: unknown): boolean {
  const range = Object.entries(SETTING_RANGES).find(([name]) => name === field)?.[1]
  if (range) return isFiniteNumber(value) && value >= range[0] && value <= range[1]
  switch (field) {
    case 'label_font':
      return (LABEL_FONTS as readonly unknown[]).includes(value)
    case 'carbon_labels':
      return (CARBON_LABELS as readonly unknown[]).includes(value)
    case 'display_unit':
      return (DISPLAY_UNITS as readonly unknown[]).includes(value)
    default:
      return false
  }
}

/** Settings fields, each valid; `complete` requires every field. */
function isSettings(value: unknown, complete: true): value is DocumentSettings
function isSettings(value: unknown, complete: false): value is SettingsEdit
function isSettings(value: unknown, complete: boolean) {
  return (
    isRecord(value) &&
    Object.entries(value).every(([field, item]) => isSettingValue(field, item)) &&
    (!complete || SETTING_FIELDS.every((field) => field in value))
  )
}

function isRecovery(value: unknown): value is RecoveryView | null {
  return (
    value === null ||
    (isRecord(value) &&
      (value.state === 'recoverable' || value.state === 'outdated' || value.state === 'unreadable' || value.state === 'in_use') &&
      isCountOrNull(value.revision) &&
      isCountOrNull(value.base_revision) &&
      isCountOrNull(value.saved_at_ms) &&
      (value.message === null || typeof value.message === 'string'))
  )
}

/**
 * The graph and scene fields, which come together or not at all: one page, at least one
 * instance with unique IDs on that page drawing `molecule`, each with exactly one position per
 * atom, and the first instance's positions repeated as `atoms[].position`.
 */
function isScene(document: Record<string, unknown>, atoms: AtomView[]): boolean {
  const { molecule, scene } = document
  if (molecule === undefined && scene === undefined) return true
  if (!isRecord(molecule) || typeof molecule.id !== 'string' || !isRecord(scene)) return false
  const { pages, instances } = scene
  if (
    !Array.isArray(pages) ||
    pages.length !== 1 ||
    !isRecord(pages[0]) ||
    typeof pages[0].id !== 'string' ||
    !Array.isArray(instances) ||
    instances.length === 0
  ) {
    return false
  }
  const page = pages[0].id
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const instanceIds = new Set<string>()
  for (const instance of instances) {
    if (
      !isRecord(instance) ||
      typeof instance.id !== 'string' ||
      instanceIds.has(instance.id) ||
      instance.page !== page ||
      instance.molecule !== molecule.id ||
      !Array.isArray(instance.positions)
    ) {
      return false
    }
    instanceIds.add(instance.id)
    const placed = new Set<string>()
    for (const position of instance.positions) {
      if (
        !isRecord(position) ||
        typeof position.atom !== 'string' ||
        !atomIds.has(position.atom) ||
        placed.has(position.atom) ||
        !isFiniteNumber(position.x) ||
        !isFiniteNumber(position.y)
      ) {
        return false
      }
      placed.add(position.atom)
    }
    if (placed.size !== atomIds.size) return false
  }
  // Every instance, the first included, was checked above.
  const first = new Map((instances[0] as InstanceView).positions.map((position) => [position.atom, position]))
  return atoms.every((atom) => {
    const position = first.get(atom.id)
    return position !== undefined && position.x === atom.position.x && position.y === atom.position.y
  })
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string') && distinct(value)
}

function isSelection(value: unknown): value is SelectionView {
  return isRecord(value) && isStringArray(value.objects) && isStringArray(value.atoms) && isStringArray(value.bonds)
}

function isSceneObject(value: unknown): value is SceneObject {
  if (!isRecord(value) || typeof value.id !== 'string' || (value.parent !== null && typeof value.parent !== 'string')) return false
  switch (value.kind) {
    case 'instance':
      return typeof value.molecule === 'string'
    case 'group':
      return isStringArray(value.children)
    case 'graphic':
      return (
        (value.shape === 'line' || value.shape === 'rectangle' || value.shape === 'arrow') &&
        Array.isArray(value.points) && value.points.length === 2 &&
        value.points.every((point) => isRecord(point) && isFiniteNumber(point.x) && isFiniteNumber(point.y))
      )
    default:
      return false
  }
}

function isSceneReference(value: unknown): value is SceneReference {
  return isRecord(value) && typeof value.source === 'string' && isRecord(value.target) &&
    (value.target.kind === 'object' || value.target.kind === 'atom' || value.target.kind === 'bond') &&
    typeof value.target.id === 'string'
}

/** The optional scene tree and selection must arrive together and describe one ordered tree. */
function isSceneTree(document: Record<string, unknown>, atoms: AtomView[], bonds: BondView[]): boolean {
  const { scene, selection } = document
  if (scene === undefined) return selection === undefined
  if (!isRecord(scene)) return false
  if (scene.roots === undefined && scene.objects === undefined && scene.references === undefined && selection === undefined) return true
  if (!isStringArray(scene.roots) || !Array.isArray(scene.objects) || !scene.objects.every(isSceneObject) ||
      !Array.isArray(scene.references) || !scene.references.every(isSceneReference) ||
      !Array.isArray(scene.instances) || !isSelection(selection) || !isRecord(document.molecule)) return false
  const sceneObjects = scene.objects
  const objects = new Map(sceneObjects.map((object) => [object.id, object]))
  if (objects.size !== sceneObjects.length) return false
  const visited = new Set<string>()
  const order: string[] = []
  const stack = scene.roots.slice().reverse().map((id) => ({ id, parent: null as string | null, depth: 1 }))
  while (stack.length) {
    const current = stack.pop()
    if (!current) return false
    const object = objects.get(current.id)
    if (!object || visited.has(current.id) || object.parent !== current.parent || current.depth > 16) return false
    visited.add(current.id)
    order.push(current.id)
    if (object.kind === 'instance' && object.molecule !== document.molecule.id) return false
    if (object.kind === 'group') {
      for (const child of object.children.slice().reverse()) stack.push({ id: child, parent: object.id, depth: current.depth + 1 })
    }
  }
  if (visited.size !== objects.size || order.some((id, index) => sceneObjects[index].id !== id)) return false
  const instanceIds = new Set(scene.instances.map((instance) => isRecord(instance) ? instance.id : undefined))
  const objectInstances = sceneObjects.filter((object) => object.kind === 'instance')
  if (objectInstances.length !== instanceIds.size || objectInstances.some((object) => !instanceIds.has(object.id))) return false
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const bondIds = new Set(bonds.map((bond) => bond.id))
  const links = new Set<string>()
  for (const reference of scene.references) {
    const key = `${reference.source}:${reference.target.kind}:${reference.target.id}`
    const targets = reference.target.kind === 'object' ? objects : reference.target.kind === 'atom' ? atomIds : bondIds
    if (!objects.has(reference.source) || !targets.has(reference.target.id) || links.has(key)) return false
    links.add(key)
  }
  return selection.objects.every((id) => objects.has(id)) &&
    selection.atoms.every((id) => atomIds.has(id)) && selection.bonds.every((id) => bondIds.has(id))
}

/** Rejects malformed snapshots, including bonds that name atoms the snapshot lacks. */
export function isDocumentResponse(value: unknown): value is DocumentResponse {
  if (!isRecord(value) || value.api_version !== API_VERSION) return false
  const { document, session } = value
  if (
    !isRecord(document) ||
    typeof document.id !== 'string' ||
    typeof document.title !== 'string' ||
    !isCount(document.revision) ||
    !Array.isArray(document.atoms) ||
    !Array.isArray(document.bonds) ||
    !document.atoms.every(isAtom) ||
    !document.bonds.every(isBond)
  ) {
    return false
  }
  const atomIds = new Set(document.atoms.map((atom) => atom.id))
  const bondIds = new Set(document.bonds.map((bond) => bond.id))
  return (
    atomIds.size === document.atoms.length &&
    bondIds.size === document.bonds.length &&
    document.bonds.every((bond) => atomIds.has(bond.begin) && atomIds.has(bond.end)) &&
    statesAgree(document.atoms, document.bonds) &&
    isScene(document, document.atoms) &&
    isSceneTree(document, document.atoms, document.bonds) &&
    (document.settings === undefined || isSettings(document.settings, true)) &&
    isRecord(session) &&
    isCount(session.saved_revision) &&
    typeof session.unsaved_changes === 'boolean' &&
    isCount(session.undo_depth) &&
    isCount(session.redo_depth) &&
    isCountOrNull(session.draft_revision) &&
    isDraftFailure(session.draft_failure) &&
    isRecovery(value.recovery)
  )
}

function isDraftFailure(value: unknown): value is DraftFailureView | null | undefined {
  return (
    value == null ||
    (isRecord(value) &&
      isCount(value.attempts) &&
      isCount(value.retry_in_ms) &&
      (value.code === undefined || typeof value.code === 'string') &&
      (value.reason === undefined || isStorageReason(value.reason)) &&
      typeof value.message === 'string')
  )
}

function isCopy(value: unknown): value is CopyResponse {
  if (!isDocumentResponse(value)) return false
  const { source } = value as { source?: unknown }
  return (
    isRecord(source) &&
    typeof source.id === 'string' &&
    typeof source.closed === 'boolean' &&
    typeof source.discarded_changes === 'boolean'
  )
}

function isSummary(value: unknown): value is DocumentSummary {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    isCount(value.revision) &&
    isCount(value.atom_count) &&
    isCount(value.bond_count) &&
    typeof value.open === 'boolean' &&
    typeof value.unsaved_changes === 'boolean' &&
    isRecovery(value.recovery) &&
    isCountOrNull(value.saved_at_ms)
  )
}

function isTrashed(value: unknown): value is TrashedSummary {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.title === 'string' &&
    isCount(value.revision) &&
    isCount(value.atom_count) &&
    isCount(value.bond_count) &&
    isCountOrNull(value.deleted_at_ms)
  )
}

function isTrashList(value: unknown): value is TrashListResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    Array.isArray(value.documents) &&
    value.documents.every(isTrashed) &&
    Array.isArray(value.unreadable) &&
    value.unreadable.every((item) => isRecord(item) && typeof item.id === 'string' && typeof item.message === 'string')
  )
}

/** A delete or restore reply: which document moved, and its title. */
function isMoved(value: unknown): value is { id: string; title: string } {
  return isRecord(value) && value.api_version === API_VERSION && typeof value.id === 'string' && typeof value.title === 'string'
}

function isList(value: unknown): value is DocumentListResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    Array.isArray(value.documents) &&
    value.documents.every(isSummary) &&
    Array.isArray(value.unreadable) &&
    value.unreadable.every((item) => isRecord(item) && typeof item.id === 'string' && typeof item.message === 'string')
  )
}

function isClose(value: unknown): value is { discarded_changes: boolean } {
  return isRecord(value) && value.api_version === API_VERSION && typeof value.discarded_changes === 'boolean'
}

const TIMEOUT_MS = 10_000
/**
 * While the service reports that a request whose reply was lost is still running, its
 * outcome is asked for this often, at most `OUTCOME_CHECKS` times. The service answers every
 * such request within its own time budget (about 8 s plus the write itself).
 */
const OUTCOME_CHECK_INTERVAL_MS = 1000
const OUTCOME_CHECKS = 12
const GATEWAY_STATUSES = new Set([502, 503, 504])

/** The colour theme of the user interface. */
export type Theme = 'system' | 'light' | 'dark'
export const THEMES: readonly Theme[] = ['system', 'light', 'dark']

/** A computation preference: where it will belong, and that nothing stores or applies it yet. */
export interface ComputationSetting {
  status: 'not_implemented'
  owner: string
  followup: string
  note: string
}

/**
 * The preferences of the service's data directory. They choose the settings a new document
 * starts from (copied once, when it is created) and the theme; they never change an existing
 * document. Added within v1.
 */
export interface PreferencesResponse {
  revision: number
  ui: { theme: Theme }
  /** The fields new documents start from, as stored; the rest are the built-in defaults. */
  new_documents: SettingsEdit
  /** The settings a document created now starts from. */
  new_document_settings: DocumentSettings
  computation: {
    nmr: ComputationSetting
    numeric_precision: ComputationSetting
    /** Added within v1; absent from older services. */
    mass_precision?: { status: 'available'; owner: 'preferences'; decimals: number }
  }
}

/** The preferences a write sets: all of them, replacing what is stored. */
export interface PreferencesChange {
  ui: { theme: Theme }
  new_documents: SettingsEdit
  computation?: { mass_precision: { decimals: number } }
}

/** Where a reset takes a document's settings from. */
export type SettingsSource = 'defaults' | 'preferences'

function isComputationSetting(value: unknown): value is ComputationSetting {
  return (
    isRecord(value) &&
    value.status === 'not_implemented' &&
    typeof value.owner === 'string' &&
    typeof value.followup === 'string' &&
    typeof value.note === 'string'
  )
}

export function isPreferencesResponse(value: unknown): value is PreferencesResponse {
  return (
    isRecord(value) &&
    value.api_version === API_VERSION &&
    isCount(value.revision) &&
    isRecord(value.ui) &&
    (THEMES as readonly unknown[]).includes(value.ui.theme) &&
    isSettings(value.new_documents, false) &&
    isSettings(value.new_document_settings, true) &&
    isRecord(value.computation) &&
    isComputationSetting(value.computation.nmr) &&
    isComputationSetting(value.computation.numeric_precision) &&
    (value.computation.mass_precision === undefined || (
      isRecord(value.computation.mass_precision) &&
      value.computation.mass_precision.status === 'available' &&
      value.computation.mass_precision.owner === 'preferences' &&
      isCount(value.computation.mass_precision.decimals) &&
      value.computation.mass_precision.decimals <= 8
    ))
  )
}

function pause(ms: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, ms))
}

/** No reply arrived, or the service still runs the request: either way it may take effect. */
function outcomeOpen(error: ApiError) {
  return error.status === 0 || error.code === 'request_in_progress'
}

/**
 * A write whose outcome is unknown. `sentTo` is the session token of the service that may
 * have run it: each service process has its own token, and a different process has no record
 * of the request, so asking it again with the same ID would run the request a second time.
 */
interface Unconfirmed {
  requestId: string
  sentTo?: string
}

function restarted(status: number) {
  return new ApiError(
    'service_restarted',
    'The local service restarted, so unsaved edits it held are gone. Reload to continue from the last save.',
    status,
  )
}

export type CtFormat = 'mol' | 'sdf'

export interface CtLoss {
  feature: string
  source_path: string
  disposition: string
  reason: string
  semantic_impact: string
}

export interface CtPreview {
  digest: string
  snapshot: DocumentView
  records: Array<{ title: string; atoms: number; bonds: number; fields: Array<{ name: string; value: string }> }>
  losses: CtLoss[]
}

export interface CtImport {
  documentId: string
  revision: number
  format: CtFormat
  source: string
  record: number
  digest: string
}

/** Full drawing comparison for conversion previews, including scene and settings. */
export function sameConversionSnapshot(a: DocumentView, b: DocumentView): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

interface CtSourceRecord {
  title: string
  atoms: unknown[]
  bonds: unknown[]
  fields: Array<{ name: string; value: string }>
}

function isCtSourceRecord(item: unknown): item is CtSourceRecord {
  return isRecord(item) && typeof item.title === 'string' && Array.isArray(item.atoms) &&
    Array.isArray(item.bonds) && Array.isArray(item.fields) && item.fields.every((field: unknown) =>
      isRecord(field) && typeof field.name === 'string' && typeof field.value === 'string')
}

function ctReply(value: unknown): { result: Record<string, unknown>; losses: CtLoss[] } {
  if (!isRecord(value) || !Array.isArray(value.diagnostics)) {
    throw new ApiError('incompatible_response', 'The service returned an incompatible conversion response.', 200)
  }
  if (value.outcome === 'error' || value.outcome === 'unsupported') {
    const diagnostic: unknown = value.diagnostics[0]
    if (isRecord(diagnostic) && typeof diagnostic.code === 'string' && typeof diagnostic.message === 'string') {
      const details = isRecord(diagnostic.details) ? diagnostic.details : {}
      const record = isCount(details.record) ? ` Record ${details.record}.` : ''
      const line = isCount(details.line) ? ` Line ${details.line}.` : ''
      throw new ApiError(diagnostic.code, `${diagnostic.message}${record}${line}`, 200, {
        currentRevision: isCount(details.current_revision) ? details.current_revision : undefined,
      })
    }
    throw new ApiError('conversion_failed', 'The conversion failed.', 200)
  }
  if ((value.outcome !== 'ok' && value.outcome !== 'partial') || !isRecord(value.result) ||
      !Array.isArray(value.loss_report) || !value.loss_report.every((loss): loss is CtLoss =>
        isRecord(loss) && typeof loss.feature === 'string' && typeof loss.source_path === 'string' &&
        typeof loss.disposition === 'string' && typeof loss.reason === 'string' && typeof loss.semantic_impact === 'string')) {
    throw new ApiError('incompatible_response', 'The service returned an incompatible conversion response.', 200)
  }
  return { result: value.result, losses: value.loss_report }
}

/** Client for the local document API. Writes carry the per-process session token. */
export class DocumentClient {
  private token: Promise<{ header: string; token: string }> | null = null
  /**
   * `reread` found that the process holding `token` was replaced. Writes keep presenting the
   * token so they meet the restart; the next `get` learns the new process's token.
   */
  private replaced = false
  /**
   * Writes that create a document and whose outcome is unknown (no response arrived), by what
   * they asked for. They are not guarded by a revision, so a retry reuses the request ID and
   * the API replays the document it already committed instead of creating a second one.
   */
  private unconfirmed = new Map<string, Unconfirmed>()
  /** Saves whose outcome is unknown, by document and revision. */
  private unconfirmedSaves = new Map<string, Unconfirmed>()
  private unconfirmedImports = new Map<string, Unconfirmed>()
  private readonly restartListeners = new Set<() => void>()

  /**
   * Calls `listener` each time a write finds that the service restarted (its session token was
   * refused). Returns a function that removes the listener.
   */
  onRestart(listener: () => void): () => void {
    this.restartListeners.add(listener)
    return () => {
      this.restartListeners.delete(listener)
    }
  }

  private static sessionOf(value: unknown) {
    if (!isRecord(value) || typeof value.header !== 'string' || typeof value.token !== 'string') {
      throw new ApiError('incompatible_response', 'The local service returned an invalid session.', 200)
    }
    return { header: value.header, token: value.token }
  }

  private notifyRestart() {
    for (const listener of this.restartListeners) listener()
  }

  private async session() {
    this.token ??= this.request('GET', '/api/v1/session').then((value) => DocumentClient.sessionOf(value))
    try {
      return await this.token
    } catch (error) {
      this.token = null
      throw error
    }
  }

  private async request(method: 'GET' | 'POST' | 'PUT', path: string, body?: unknown, headers: Record<string, string> = {}) {
    let response: Response
    try {
      response = await fetch(path, {
        method,
        headers: {
          Accept: 'application/json',
          ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
          ...headers,
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(TIMEOUT_MS),
      })
    } catch (error) {
      const timeout = error instanceof Error && error.name === 'TimeoutError'
      throw new ApiError(
        'unreachable',
        timeout ? 'The local service did not respond in time.' : 'The local service could not be reached.',
        0,
      )
    }
    const value: unknown = await response.json().catch(() => null)
    if (!response.ok) {
      if (isRecord(value) && typeof value.code === 'string' && typeof value.message === 'string') {
        throw new ApiError(value.code, value.message, response.status, {
          currentRevision: isCount(value.current_revision) ? value.current_revision : undefined,
          commandIndex: isCount(value.command_index) ? value.command_index : undefined,
          reason: isStorageReason(value.reason) ? value.reason : undefined,
          stateReason:
            (value.code === 'invalid_command' || value.code === 'invalid_request') && typeof value.reason === 'string'
              ? value.reason
              : undefined,
        })
      }
      // A gateway status without a v1 body comes from the proxy in front of the service (Vite's
      // answers 502 when the connection drops): the service's own reply never arrived.
      if (GATEWAY_STATUSES.has(response.status)) {
        throw new ApiError('unreachable', `The local service did not answer (HTTP ${response.status} from the proxy).`, 0)
      }
      throw new ApiError('http_error', `The local service returned HTTP ${response.status}.`, response.status)
    }
    return value
  }

  /**
   * A restarted API issues a new token and has lost its working copies, so revisions this
   * page has seen may since name different states. A first create attempt is retried with the
   * new token; an edit to an open document is not, and asks for a reload instead.
   * `sending` is called with the token each attempt is about to be sent with, and may throw
   * to stop it.
   */
  private async write(
    path: string,
    body: Record<string, unknown>,
    onRestart: 'retry' | 'reload',
    sending?: (token: string) => void,
    method: 'POST' | 'PUT' = 'POST',
  ) {
    for (let attempt = 0; ; attempt += 1) {
      const { header, token } = await this.session()
      sending?.(token)
      try {
        return await this.request(method, path, body, { [header]: token })
      } catch (error) {
        if (!(error instanceof ApiError && error.code === 'unauthorized')) throw error
        this.token = null
        this.replaced = false
        this.notifyRestart()
        if (onRestart === 'retry' && attempt === 0) continue
        if (onRestart === 'reload') throw restarted(error.status)
        throw error
      }
    }
  }

  private static document(value: unknown): DocumentResponse {
    if (!isDocumentResponse(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible document.', 200)
    }
    return withStates(value)
  }

  /** The server returns the actual inspected snapshot, not just its potentially reused revision. */
  async inspectValence(id: string): Promise<ValenceInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics',
      operation: 'chem.validate', parameters: { document_id: id },
    }, 'reload')
    if (!isRecord(value) || value.outcome !== 'ok' || typeof value.model_id !== 'string' ||
        !isRecord(value.result) || !isRecord(value.result.snapshot) || !Array.isArray(value.result.atoms)) {
      throw new ApiError('analysis_unavailable', 'The local service could not inspect the drawing.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const atoms = value.result.atoms
    if (snapshot.id !== id || snapshot.revision !== value.document_revision ||
        !atoms.every((a): a is ValenceAtom => isRecord(a) && typeof a.atom_id === 'string' &&
          (a.bond_order_sum === null || isCount(a.bond_order_sum)) && (a.hydrogens === null || isCount(a.hydrogens)) &&
          (a.status === 'valid' || a.status === 'invalid' || a.status === 'unsupported') &&
          (a.reason === null || typeof a.reason === 'string')) ||
        atoms.length !== snapshot.atoms.length || new Set(atoms.map((a) => a.atom_id)).size !== atoms.length ||
        atoms.some((a) => !snapshot.atoms.some((atom) => atom.id === a.atom_id))) {
      throw new ApiError('analysis_unavailable', 'The local service returned an incompatible inspection.', 200)
    }
    return { model: value.model_id, snapshot, atoms }
  }

  async previewCt(documentId: string, revision: number, format: CtFormat, source: string, record: number): Promise<CtPreview> {
    const reply = ctReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.import',
      parameters: { document_id: documentId, expected_revision: revision, format, source, record, preview: true },
    }, 'reload'))
    const { result } = reply
    if (typeof result.preview_digest !== 'string' || !/^[a-f0-9]{64}$/.test(result.preview_digest) ||
        result.selected_record !== record || !Array.isArray(result.records) ||
        !result.records.every(isCtSourceRecord) || !isRecord(result.snapshot) ||
        !isRecord(result.document) || result.document.id !== documentId || !Array.isArray(result.document.atoms) ||
        !Array.isArray(result.document.bonds)) {
      throw new ApiError('incompatible_response', 'The service returned an incompatible import preview.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    if (snapshot.id !== documentId || snapshot.revision !== revision) {
      throw new ApiError('incompatible_response', 'The import preview used a different drawing or revision.', 200)
    }
    const records = result.records.map((item) => ({
      title: item.title, atoms: item.atoms.length, bonds: item.bonds.length, fields: item.fields,
    }))
    return { digest: result.preview_digest, snapshot, records, losses: reply.losses }
  }

  async importCt(input: CtImport): Promise<DocumentResponse> {
    // An unanswered request might have committed. Reuse its ID only against the same service.
    const key = JSON.stringify(input)
    const pending = this.unconfirmedImports.get(key) ?? { requestId: requestId() }
    this.unconfirmedImports.set(key, pending)
    let sentTo: string | undefined
    try {
      const reply = ctReply(await this.write('/api/v1/operations', {
        contract: 1, request_id: pending.requestId, profile: 'preserve-semantics', operation: 'document.import',
        parameters: { document_id: input.documentId, expected_revision: input.revision, format: input.format,
          source: input.source, record: input.record, preview_digest: input.digest, accept_loss: true },
      }, 'reload', (token) => {
        if (pending.sentTo !== undefined && pending.sentTo !== token) throw restarted(401)
        sentTo = token
      }))
      const response = DocumentClient.document({ api_version: API_VERSION, ...reply.result })
      if (response.document.id !== input.documentId) {
        throw new ApiError('incompatible_response', 'The import returned a different drawing.', 200)
      }
      this.unconfirmedImports.delete(key)
      return response
    } catch (error) {
      if (error instanceof ApiError && outcomeOpen(error) && sentTo !== undefined) {
        pending.sentTo = sentTo
        throw new ApiError(error.code, `${error.message} The import may have completed; try again to check its result.`, error.status)
      }
      this.unconfirmedImports.delete(key)
      throw error
    }
  }

  async exportCt(documentId: string, format: CtFormat): Promise<{ content: string; losses: CtLoss[]; snapshot: DocumentView }> {
    const reply = ctReply(await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'document.export',
      parameters: { format, records: [{ document_id: documentId }] },
    }, 'reload'))
    if (typeof reply.result.content !== 'string' || !Array.isArray(reply.result.records) ||
        reply.result.records.length !== 1 || !isRecord(reply.result.records[0]) || !isRecord(reply.result.records[0].snapshot)) {
      throw new ApiError('incompatible_response', 'The service returned invalid export content.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...reply.result.records[0].snapshot }).document
    if (snapshot.id !== documentId) throw new ApiError('incompatible_response', 'The export used a different drawing.', 200)
    return { content: reply.result.content, losses: reply.losses, snapshot }
  }

  /** Read-only properties are tied to the returned document snapshot and source profile. */
  async inspectProperties(
    id: string,
    scope: PropertiesScope,
    property: PropertyKind,
    selection: SelectionView | null,
    dataRegistrationId: string | null,
  ): Promise<PropertiesInspection> {
    const parameters = {
      document_id: id, scope, property,
      ...(selection === null ? {} : { selection }),
      ...(dataRegistrationId === null ? {} : { data_registration_id: dataRegistrationId }),
    }
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.properties', parameters,
    }, 'reload')
    if (!isRecord(value) || (value.outcome !== 'ok' && value.outcome !== 'partial')) {
      const first = isRecord(value) && Array.isArray(value.diagnostics) ? value.diagnostics[0] : null
      const message = isRecord(first) && typeof first.message === 'string' ? first.message : 'The local service could not calculate these properties.'
      throw new ApiError('properties_unavailable', message, 200)
    }
    if (value.model_id !== 'chemifree-properties-1' || !isRecord(value.result) || !Array.isArray(value.loss_report)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible properties.', 200)
    }
    const result = value.result
    if (!isRecord(result.snapshot) || !Array.isArray(result.components) || !Array.isArray(result.valence_warnings)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible properties.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...result.snapshot }).document
    const validOmitted = (rows: unknown) => Array.isArray(rows) && rows.every((row) => isRecord(row) &&
      typeof row.element === 'string' && isCount(row.isotope) && row.isotope > 0)
    const validMass = (mass: unknown) => isRecord(mass) && isFiniteNumber(mass.average_u) &&
      isFiniteNumber(mass.exact_u) && typeof mass.average_display === 'string' && typeof mass.exact_display === 'string' &&
      Array.isArray(mass.exact_natural_weight_fallbacks) && mass.exact_natural_weight_fallbacks.every((item) => typeof item === 'string') &&
      Array.isArray(mass.average_bases) && mass.average_bases.every((row) => isRecord(row) && typeof row.element === 'string' && typeof row.basis === 'string') &&
      Array.isArray(mass.exact_selections) && mass.exact_selections.every((row) => isRecord(row) && typeof row.element === 'string' && isCount(row.isotope) && row.isotope > 0 && typeof row.basis === 'string') &&
      validOmitted(mass.average_omitted_abundances) && validOmitted(mass.exact_omitted_abundances)
    const validPeaks = (peaks: unknown) => Array.isArray(peaks) && peaks.every((peak) => isRecord(peak) &&
      isFiniteNumber(peak.mass_u) && typeof peak.mass_display === 'string' && isFiniteNumber(peak.probability) &&
      peak.probability >= 0 && peak.probability <= 1)
    const components = result.components
    const componentValid = (component: unknown): component is PropertyComponent => (
      isRecord(component) && typeof component.formula === 'string' &&
      Number.isSafeInteger(component.formal_charge) && Array.isArray(component.atoms) &&
      component.atoms.length > 0 && component.atoms.every((atom: unknown) => typeof atom === 'string' && snapshot.atoms.some((shown) => shown.id === atom)) &&
      validOmitted(component.omitted_abundances) && Array.isArray(component.abundance_sums) &&
      component.abundance_sums.every((row) => isRecord(row) && typeof row.element === 'string' && isFiniteNumber(row.input_sum)) &&
      (property === 'formula' ? component.mass === null && component.isotope_distribution === null :
        validMass(component.mass) && (property === 'distribution' ? validPeaks(component.isotope_distribution) : component.isotope_distribution === null))
    )
    const data = result.data
    const dataValid = property === 'formula' ? data === null : isRecord(data) &&
      typeof data.registration_id === 'string' && isCount(data.generation) && typeof data.profile_id === 'string' &&
      typeof data.manifest_sha256 === 'string' &&
      (data.source_schema_id === undefined || data.source_schema_id === null || typeof data.source_schema_id === 'string') &&
      isFiniteNumber(data.charge_correction_u_per_e) &&
      typeof data.dataset_id === 'string' && typeof data.dataset_version === 'string' &&
      typeof data.calculation_profile === 'string' && typeof data.charge_convention === 'string' &&
      typeof data.abundance_policy === 'string' && typeof data.uncertainty === 'string' && result.mass_unit === 'u'
    const losses = value.loss_report
    const valence = result.valence_warnings
    const atomIds: unknown[] = components.flatMap((component) => isRecord(component) && Array.isArray(component.atoms) ? component.atoms : [])
    if (snapshot.id !== id || snapshot.revision !== value.document_revision || result.scope !== scope || result.property !== property ||
        !(result.selection === null || isSelection(result.selection)) || JSON.stringify(result.selection) !== JSON.stringify(selection) ||
        !isCount(result.display_decimals) || result.display_decimals > 8 || typeof result.valence_model !== 'string' || typeof result.arithmetic !== 'string' ||
        components.length === 0 || !components.every(componentValid) || new Set(atomIds).size !== atomIds.length || !dataValid ||
        !losses.every((loss) => isRecord(loss) && typeof loss.reason === 'string') ||
        (value.outcome === 'partial' && losses.length === 0) ||
        !valence.every((warning) => isRecord(warning) && typeof warning.atom === 'string' &&
          (warning.status === 'invalid' || warning.status === 'unsupported') && (warning.reason === null || typeof warning.reason === 'string'))) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible properties.', 200)
    }
    return {
      model: value.model_id, snapshot, scope, property, selection, display_decimals: result.display_decimals,
      valence_model: result.valence_model, arithmetic: result.arithmetic, data: data as PropertiesInspection['data'], components: components as PropertyComponent[],
      warnings: [
        ...losses.map((loss) => (loss as { reason: string }).reason),
        ...valence.map((warning) => `${(warning as { atom: string }).atom}: valence ${(warning as { status: string }).status}`),
      ],
    }
  }

  async inspectAromaticity(id: string): Promise<AromaticityInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics',
      operation: 'chem.aromaticity', parameters: { document_id: id },
    }, 'reload')
    if (!isRecord(value) || value.outcome !== 'ok' || value.model_id !== 'chemifree-aromaticity-1' ||
        !isRecord(value.result) || !isRecord(value.result.snapshot) || !Array.isArray(value.result.systems)) {
      throw new ApiError('analysis_unavailable', 'The local service could not inspect aromaticity.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const systems = value.result.systems
    if (snapshot.id !== id || snapshot.revision !== value.document_revision ||
        !systems.every((item): item is AromaticitySystem => isRecord(item) &&
          Array.isArray(item.atoms) && item.atoms.length >= 3 && new Set(item.atoms).size === item.atoms.length &&
          item.atoms.every((atom: unknown) => typeof atom === 'string' && snapshot.atoms.some((shown) => shown.id === atom)) &&
          Array.isArray(item.bonds) && item.bonds.length >= 3 && new Set(item.bonds).size === item.bonds.length &&
          item.bonds.every((bond: unknown) => typeof bond === 'string' && snapshot.bonds.some((shown) => shown.id === bond)) &&
          isCount(item.cycle_rank) && item.cycle_rank > 0 &&
          (item.classification === 'aromatic' || item.classification === 'non_aromatic' || item.classification === 'unsupported') &&
          (item.electrons === null || isCount(item.electrons)) && (item.reason === null || typeof item.reason === 'string'))) {
      throw new ApiError('analysis_unavailable', 'The local service returned an incompatible aromaticity inspection.', 200)
    }
    return { model: value.model_id, snapshot, systems }
  }

  /** Canonical identity and optional comparison are bound to the returned graph snapshots. */
  async inspectIdentity(id: string, mode: IdentityMode, otherId: string | null): Promise<IdentityInspection> {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.identity',
      parameters: { document_id: id, mode, ...(otherId === null ? {} : { other_document_id: otherId }) },
    }, 'reload')
    const expectedModel = mode === 'recorded' ? 'chemifree-recorded-graph-1' : 'chemifree-chemical-graph-1'
    if (!isRecord(value) || value.outcome !== 'ok') {
      const first = isRecord(value) && Array.isArray(value.diagnostics) ? value.diagnostics[0] : null
      const message = isRecord(first) && typeof first.message === 'string' ? first.message : 'The local service could not compare these graphs.'
      const details = isRecord(first) && isRecord(first.details) ? first.details : null
      const reason = details && typeof details.reason === 'string' ? details.reason.replaceAll('_', ' ') : null
      if (reason && (details?.side === 'source' || details?.side === 'target')) {
        if (!isRecord(value) || value.model_id !== expectedModel || value.result !== null || !isRecord(details.snapshot)) {
          throw new ApiError('incompatible_response', 'The local service returned an incompatible identity failure.', 200)
        }
        const snapshot = DocumentClient.document({ api_version: API_VERSION, ...details.snapshot }).document
        const target = details.side === 'target'
        if (snapshot.id !== (target ? otherId : id) || (!target && snapshot.revision !== value.document_revision)) {
          throw new ApiError('incompatible_response', 'The local service returned an incompatible identity failure.', 200)
        }
        const side = target ? 'Comparison' : 'Current drawing'
        throw new ApiError('identity_unavailable', `${message} ${side} snapshot ${snapshot.id}, revision ${snapshot.revision}: ${reason}. Inspect again to include later edits.`, 200)
      }
      throw new ApiError('identity_unavailable', message, 200)
    }
    if (value.model_id !== expectedModel || !isRecord(value.result) || !isRecord(value.result.snapshot)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible graph identity.', 200)
    }
    const snapshot = DocumentClient.document({ api_version: API_VERSION, ...value.result.snapshot }).document
    const validIdentity = (item: unknown, document: DocumentView): item is CanonicalIdentity => {
      if (!isRecord(item) || typeof item.code !== 'string' || !Array.isArray(item.numbering) ||
          !Array.isArray(item.symmetry) || !Array.isArray(item.folded_hydrogens) ||
          !item.numbering.every((atom: unknown) => typeof atom === 'string')) return false
      const numbered = new Set<string>(item.numbering as string[])
      const atoms = new Map(document.atoms.map((atom) => [atom.id, atom]))
      if (numbered.size !== item.numbering.length || [...numbered].some((id) => !atoms.has(id))) return false
      const folded = new Set<string>()
      for (const row of item.folded_hydrogens) {
        if (!isRecord(row) || typeof row.hydrogen !== 'string' || typeof row.attached_to !== 'string' ||
            folded.has(row.hydrogen) || numbered.has(row.hydrogen) || !numbered.has(row.attached_to) ||
            atoms.get(row.hydrogen)?.element !== 'H') return false
        folded.add(row.hydrogen)
      }
      if (numbered.size + folded.size !== atoms.size || (mode === 'recorded' && folded.size !== 0)) return false
      const members: string[] = []
      for (const orbit of item.symmetry) {
        if (!Array.isArray(orbit) || orbit.length === 0 ||
            !orbit.every((atom: unknown) => typeof atom === 'string' && atoms.has(atom))) return false
        members.push(...orbit)
      }
      return members.length === atoms.size && new Set(members).size === atoms.size
    }
    const sourceIdentity = value.result.identity
    if (snapshot.id !== id || snapshot.revision !== value.document_revision || !validIdentity(sourceIdentity, snapshot)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible graph identity.', 200)
    }
    const other = value.result.comparison
    let comparison: IdentityInspection['comparison'] = null
    if (otherId === null) {
      if (other !== null) throw new ApiError('incompatible_response', 'The local service returned an unexpected comparison.', 200)
    } else {
      if (!isRecord(other) || !isRecord(other.snapshot) || typeof other.equivalent !== 'boolean') {
        throw new ApiError('incompatible_response', 'The local service returned an incompatible comparison.', 200)
      }
      const otherResponse = DocumentClient.document({ api_version: API_VERSION, ...other.snapshot })
      const otherSnapshot = otherResponse.document
      if (otherSnapshot.id !== otherId || !validIdentity(other.identity, otherSnapshot) ||
          other.equivalent !== (sourceIdentity.code === other.identity.code) ||
          (otherId === id && !sameInspectionSnapshot(snapshot, otherSnapshot))) {
        throw new ApiError('incompatible_response', 'The local service returned an incompatible comparison.', 200)
      }
      comparison = { snapshot: otherSnapshot, recovery: otherResponse.recovery, identity: other.identity, equivalent: other.equivalent }
    }
    return { model: expectedModel, snapshot, identity: sourceIdentity, comparison }
  }

  /** Apply one model conversion under the revision the user inspected, with history selection. */
  async convertAromaticity(id: string, expectedRevision: number, to: 'aromatic' | 'kekule', selection: SelectionView) {
    const value = await this.write('/api/v1/operations', {
      contract: 1, request_id: requestId(), profile: 'preserve-semantics', operation: 'chem.aromaticity_convert',
      parameters: { document_id: id, expected_revision: expectedRevision, to, selection_before: selection, selection_after: selection },
    }, 'reload')
    if (!isRecord(value) || value.outcome !== 'ok' || !isRecord(value.result)) {
      const first = isRecord(value) && Array.isArray(value.diagnostics) ? value.diagnostics[0] : null
      const code = isRecord(first) && typeof first.code === 'string' ? first.code : 'analysis_unavailable'
      const message = isRecord(first) && typeof first.message === 'string' ? first.message : 'The local service could not convert aromaticity.'
      const details = isRecord(first) && isRecord(first.details) ? first.details : null
      throw new ApiError(code, message, 200, {
        currentRevision: details && isCount(details.current_revision) ? details.current_revision : undefined,
        stateReason: details && typeof details.reason === 'string' ? details.reason : undefined,
      })
    }
    if (value.model_id !== 'chemifree-aromaticity-1') {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible conversion.', 200)
    }
    const response = DocumentClient.document({ api_version: API_VERSION, ...value.result })
    if (response.document.id !== id || response.document.revision !== value.document_revision) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible conversion.', 200)
    }
    return response
  }

  async list(query?: ListQuery): Promise<DocumentListResponse> {
    const params = new URLSearchParams()
    if (query?.query.trim()) params.set('query', query.query.trim())
    if (query && query.sort !== 'title') params.set('sort', query.sort)
    const search = params.size ? `?${params}` : ''
    const value = await this.request('GET', `/api/v1/documents${search}`)
    if (!isList(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible list.', 200)
    }
    return value
  }

  async trash(): Promise<TrashListResponse> {
    const value = await this.request('GET', '/api/v1/trash')
    if (!isTrashList(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible trash list.', 200)
    }
    return value
  }

  /** Commits a new title for a document without unsaved changes. */
  async rename(id: string, expectedRevision: number, title: string) {
    return DocumentClient.document(
      await this.guarded(`${documentPath(id)}/rename`, { expected_revision: expectedRevision, title }, 'reload'),
    )
  }

  /** Moves a document without unsaved changes to the trash; `expectedRevision` is the one the user confirmed. */
  async delete(id: string, expectedRevision: number) {
    return DocumentClient.moved(
      await this.guarded(`${documentPath(id)}/delete`, { expected_revision: expectedRevision }, 'reload'),
    )
  }

  /** Restoring again after a restart is safe: a restored document is simply no longer in the trash. */
  async restore(id: string) {
    return DocumentClient.moved(await this.guarded(`/api/v1/trash/${encodeURIComponent(id)}/restore`, {}, 'retry'))
  }

  /**
   * A write that its own precondition (a revision, or the document being in the trash) keeps
   * from taking effect twice. A lost reply is asked about once more with the same request ID,
   * and while the service reports the request still running it is asked again, so the page
   * learns what happened instead of meeting its own first attempt as a conflict. Any later
   * repeat needs no record: the precondition refuses it. As for saves, an attempt that may
   * have run is asked about only in the service process it was sent to.
   */
  private async guarded(
    path: string,
    body: Record<string, unknown>,
    onRestart: 'retry' | 'reload',
    method: 'POST' | 'PUT' = 'POST',
  ) {
    const request = { ...body, request_id: requestId() }
    let sentTo: string | undefined
    let token: string | undefined
    const sending = (current: string) => {
      if (sentTo !== undefined && sentTo !== current) throw restarted(401)
      token = current
    }
    let askedAgain = false
    for (let checks = 0; ; ) {
      try {
        return await this.write(path, request, onRestart, sending, method)
      } catch (error) {
        if (!(error instanceof ApiError)) throw error
        if (outcomeOpen(error) && token !== undefined) sentTo ??= token
        if (error.status === 0 && !askedAgain) {
          askedAgain = true
          continue
        }
        if (error.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
          checks += 1
          await pause(OUTCOME_CHECK_INTERVAL_MS)
          continue
        }
        throw error
      }
    }
  }

  private static moved(value: unknown) {
    if (!isMoved(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible reply.', 200)
    }
    return value
  }

  /**
   * Reads a document. The service process's session token is learned first, so the view is
   * tied to that process and a later `reread` can tell that it restarted since, even when the
   * page has not written anything yet.
   */
  async get(id: string) {
    if (this.replaced) {
      this.token = null
      this.replaced = false
    }
    await this.session()
    return DocumentClient.document(await this.request('GET', documentPath(id)))
  }

  /**
   * Reads a document again to see what the service holds now, from the service process this
   * client writes to. A restarted process has lost the working copy the page shows and would
   * answer with the stored document only, so a restart is reported instead of that reply
   * (`service_restarted`, also to the `onRestart` listeners). Reads carry no token, so the
   * process is compared by its session token after the read, which catches a restart at any
   * point before the reply. Until the page reads the document again with `get`, the old token
   * is kept, so a write is refused and meets the restart as writes do, rather than reaching
   * the new process as if nothing happened.
   */
  async reread(id: string) {
    const { token } = await this.session()
    const value = await this.request('GET', documentPath(id))
    if (DocumentClient.sessionOf(await this.request('GET', '/api/v1/session')).token !== token) {
      this.replaced = true
      this.notifyRestart()
      throw restarted(200)
    }
    return DocumentClient.document(value)
  }

  /**
   * Sends a write that creates a document at most once. A restarted service has forgotten the
   * request_id of an earlier unanswered attempt, so that attempt is not resent: if it was
   * committed, a resend would create a second document. `settle` turns the reply into the
   * result; `replayed` says the reply may be the service's record of an earlier attempt,
   * which describes the document as it was then. The request ID is kept until `settle` is
   * done, so a failure there leaves the request to be asked about again.
   */
  private async createOnce<T>(
    key: string,
    path: string,
    body: Record<string, unknown>,
    unknown: ApiError,
    settle: (value: unknown, replayed: boolean) => Promise<T>,
  ): Promise<T> {
    const pending = this.unconfirmed.get(key) ?? { requestId: requestId() }
    this.unconfirmed.set(key, pending)
    let token: string | undefined
    const sending = (current: string) => {
      // Another request found that the service restarted since the unknown attempt.
      if (pending.sentTo !== undefined && pending.sentTo !== current) throw restarted(401)
      token = current
    }
    for (let checks = 0; ; checks += 1) {
      let value: unknown
      try {
        value = await this.write(
          path,
          { request_id: pending.requestId, ...body },
          pending.sentTo === undefined ? 'retry' : 'reload',
          sending,
        )
      } catch (error) {
        if (!(error instanceof ApiError)) throw error
        if (error.code === 'service_restarted') {
          // The restarted service has forgotten the ID, so a later attempt is a new request.
          this.unconfirmed.delete(key)
          throw unknown
        }
        // Only an attempt that was sent can have taken effect.
        if (outcomeOpen(error) && token !== undefined) pending.sentTo = token
        if (error.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
          await pause(OUTCOME_CHECK_INTERVAL_MS)
          continue
        }
        if (!outcomeOpen(error)) {
          this.unconfirmed.delete(key)
          throw error
        }
        // The ID is kept, so asking again finds out without creating twice.
        throw new ApiError(
          error.code,
          `${error.message} It may have been carried out; trying again finds out without repeating it.`,
          error.status,
        )
      }
      const replayed = pending.sentTo !== undefined
      // The service answered, so the document exists. Until it is shown, the ID is kept, and
      // trying again asks about it instead of creating another.
      pending.sentTo = token
      const result = await settle(value, replayed)
      this.unconfirmed.delete(key)
      return result
    }
  }

  /**
   * Saves the working copy at `expectedRevision`. When no reply arrives the save may have
   * committed, so the client asks again with the same request_id, at once and then while the
   * service reports it in progress; the service returns the committed reply rather than
   * saving twice. If it stays unanswered, the save is reported as `save_unconfirmed`, and
   * saving the same revision again asks about the same request; saving a later revision is a
   * new save, which also commits the earlier changes.
   */
  async save(id: string, expectedRevision: number) {
    const key = `${id}:${expectedRevision}`
    const pending = this.unconfirmedSaves.get(key) ?? { requestId: requestId() }
    this.unconfirmedSaves.set(key, pending)
    const request = { request_id: pending.requestId, expected_revision: expectedRevision }
    let token: string | undefined
    const sending = (current: string) => {
      // Another request found that the service restarted since the unknown attempt.
      if (pending.sentTo !== undefined && pending.sentTo !== current) throw restarted(401)
      token = current
    }
    let askedAgain = false
    for (let checks = 0; ; ) {
      try {
        const value = DocumentClient.document(
          await this.write(`${documentPath(id)}/save`, request, 'reload', sending),
        )
        // This commit covers every earlier revision of the document, so their checks are moot.
        for (const pending of this.unconfirmedSaves.keys()) {
          if (pending.startsWith(`${id}:`)) this.unconfirmedSaves.delete(pending)
        }
        return value
      } catch (error) {
        if (!(error instanceof ApiError)) throw error
        // Only an attempt that was sent can have taken effect.
        if (outcomeOpen(error) && token !== undefined) pending.sentTo = token
        if (error.status === 0 && !askedAgain) {
          askedAgain = true
          continue
        }
        if (error.code === 'request_in_progress' && checks < OUTCOME_CHECKS) {
          checks += 1
          await pause(OUTCOME_CHECK_INTERVAL_MS)
          continue
        }
        if (!outcomeOpen(error)) {
          this.unconfirmedSaves.delete(key)
          throw error
        }
        throw new ApiError(
          'save_unconfirmed',
          'The local service has not confirmed this save, so it may or may not be saved. Check again to ask.',
          error.status,
        )
      }
    }
  }

  async create(title?: string) {
    return this.createOnce(
      `create:${JSON.stringify(title ?? null)}`,
      '/api/v1/documents',
      { title },
      new ApiError(
        'create_unconfirmed',
        'The local service restarted before confirming the earlier new document, so it may already exist. Check Saved documents before creating another.',
        401,
      ),
      async (value, replayed) => {
        const created = DocumentClient.document(value)
        return replayed ? this.get(created.document.id) : created
      },
    )
  }

  private static copy(value: unknown): CopyResponse {
    if (!isCopy(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible copy.', 200)
    }
    return { ...withStates(value), source: value.source }
  }

  private static copyUnconfirmed() {
    return new ApiError(
      'copy_unconfirmed',
      'The local service restarted before confirming the copy, so it may already exist. Check Saved documents before copying again.',
      401,
    )
  }

  /** The copy as it is now, with what became of its source when it was made. */
  private async settleCopy(value: unknown, replayed: boolean): Promise<CopyResponse> {
    const copied = DocumentClient.copy(value)
    return replayed ? { ...(await this.get(copied.document.id)), source: copied.source } : copied
  }

  /** Saves the working copy as a new document; the source returns to its last save. */
  async saveAs(id: string, expectedRevision: number, title?: string) {
    return this.createOnce(
      `save-as:${id}:${expectedRevision}:${JSON.stringify(title ?? null)}`,
      `${documentPath(id)}/save-as`,
      { expected_revision: expectedRevision, title },
      DocumentClient.copyUnconfirmed(),
      (value, replayed) => this.settleCopy(value, replayed),
    )
  }

  async recoverAsCopy(id: string, title?: string) {
    return this.createOnce(
      `recover-as-copy:${id}:${JSON.stringify(title ?? null)}`,
      `${documentPath(id)}/recover-as-copy`,
      { title },
      DocumentClient.copyUnconfirmed(),
      (value, replayed) => this.settleCopy(value, replayed),
    )
  }

  /**
   * Writes the recovery draft. After a restart the new service holds none of this page's
   * unsaved work, so the page must reload (where the old process's draft is offered).
   */
  async autosave(id: string) {
    return DocumentClient.document(await this.write(`${documentPath(id)}/autosave`, { request_id: requestId() }, 'reload'))
  }

  /** Recovering again after a restart is safe: the draft is abandoned again either way. */
  async recover(id: string) {
    return DocumentClient.document(await this.write(`${documentPath(id)}/recover`, { request_id: requestId() }, 'retry'))
  }

  async discardDraft(id: string) {
    const value = await this.write(`${documentPath(id)}/discard-draft`, { request_id: requestId() }, 'retry')
    if (!isRecord(value) || value.api_version !== API_VERSION || typeof value.id !== 'string') {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible reply.', 200)
    }
  }

  async transact(id: string, expectedRevision: number, commands: Command[], selection?: SelectionSnapshots) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/transactions`,
        {
          request_id: requestId(),
          expected_revision: expectedRevision,
          commands,
          selection_before: selection?.before,
          selection_after: selection?.after,
        },
        'reload',
      ),
    )
  }

  async revisionAction(id: string, action: 'undo' | 'redo', expectedRevision: number) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/${action}`,
        { request_id: requestId(), expected_revision: expectedRevision },
        'reload',
      ),
    )
  }

  /**
   * Replaces the document's settings with the built-in defaults or the preferred new-document
   * settings, as one undoable edit.
   */
  async resetSettings(id: string, expectedRevision: number, to: SettingsSource, selection?: SelectionSnapshots) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/settings/reset`,
        { request_id: requestId(), expected_revision: expectedRevision, to, selection_before: selection?.before, selection_after: selection?.after },
        'reload',
      ),
    )
  }

  /** Applies the settings of document `source` to document `id`, as one undoable edit. */
  async importSettings(id: string, expectedRevision: number, source: string, selection?: SelectionSnapshots) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/settings/import`,
        { request_id: requestId(), expected_revision: expectedRevision, source, selection_before: selection?.before, selection_after: selection?.after },
        'reload',
      ),
    )
  }

  private static preferencesOf(value: unknown): PreferencesResponse {
    if (!isPreferencesResponse(value)) {
      throw new ApiError('incompatible_response', 'The local service returned incompatible preferences.', 200)
    }
    return value
  }

  async preferences() {
    return DocumentClient.preferencesOf(await this.request('GET', PREFERENCES_PATH))
  }

  /**
   * Preference writes are stored at once and guarded by their revision, so a lost reply is
   * asked about and a restart retried, as for a rename.
   */
  async setPreferences(expectedRevision: number, change: PreferencesChange) {
    return DocumentClient.preferencesOf(
      await this.guarded(PREFERENCES_PATH, { expected_revision: expectedRevision, ...change }, 'retry', 'PUT'),
    )
  }

  async resetPreferences(expectedRevision: number) {
    return DocumentClient.preferencesOf(
      await this.guarded(`${PREFERENCES_PATH}/reset`, { expected_revision: expectedRevision }, 'retry'),
    )
  }

  /** Makes new documents start from every setting of `document` (as the service holds it now). */
  async preferencesFromDocument(expectedRevision: number, document: string) {
    return DocumentClient.preferencesOf(
      await this.guarded(`${PREFERENCES_PATH}/import`, { expected_revision: expectedRevision, document }, 'retry'),
    )
  }

  async close(id: string, discardUnsaved: boolean, expectedRevision?: number) {
    // A refused token is retried: the revision check still guards what a close may discard.
    const value = await this.write(
      `${documentPath(id)}/close`,
      { request_id: requestId(), discard_unsaved: discardUnsaved, expected_revision: expectedRevision },
      'retry',
    )
    if (!isClose(value)) {
      throw new ApiError('incompatible_response', 'The local service returned an incompatible reply.', 200)
    }
    return value
  }
}

const PREFERENCES_PATH = '/api/v1/preferences'

function documentPath(id: string) {
  return `/api/v1/documents/${encodeURIComponent(id)}`
}

function requestId() {
  return crypto.randomUUID()
}
