/** Hand-maintained mirror of the v1 document contract in crates/application/src/contract.rs. */
import { API_VERSION } from '@/lib/contract'

/**
 * A bond's kind. `aromatic`, `dative` (from `begin`, the donor), `query`, and `unknown` were
 * added within v1. They are recorded only: aromaticity is not perceived and no query matches.
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
 * An atom's hydrogen state. `inferred` means "to be derived from valence"; nothing derives it,
 * so it has no count and nothing is shown for it.
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

export interface SceneView {
  pages: { id: string }[]
  /**
   * In identifier (creation) order, wherever each is contained; the first one's positions are
   * `atoms[].position`.
   */
  instances: InstanceView[]
  /**
   * Every scene object depth first from the page's roots, children in order. Each molecule
   * object is the instance of the same ID. Added within v1: a service from before groups and
   * graphics leaves it out (see `sceneObjects`).
   */
  objects?: SceneObjectView[]
}

/** What a graphic draws between its two points. */
export type Shape = 'line' | 'rectangle' | 'arrow'

/** A scene object and where it is contained: `parent` is the page's ID or a group's. */
export type SceneObjectView =
  | { kind: 'molecule'; id: string; parent: string; molecule: string }
  | { kind: 'group'; id: string; parent: string; children: string[] }
  | { kind: 'graphic'; id: string; parent: string; shape: Shape; points: [Position, Position] }

/** Selected scene objects, atoms, and bonds. */
export interface SelectionView {
  objects: string[]
  atoms: string[]
  bonds: string[]
}

export interface BondView {
  id: string
  begin: string
  end: string
  order: BondOrder
  /** With order `query`, what it stands for; null otherwise. Filled in as null for older replies. */
  query: BondQuery | null
}

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
  /**
   * What the latest edit, undo, or redo left selected. Added within v1 together with
   * `scene.objects`; a service from before them leaves both out and takes no selection.
   */
  selection?: SelectionView
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
  /** A line, rectangle, or arrow, at the end of `parent` (a group) or of the page's roots. */
  | { op: 'add_graphic'; shape: Shape; points: [Position, Position]; parent?: string }
  /** Objects sharing a parent become a new group where the first of them was. */
  | { op: 'group'; objects: string[] }
  | { op: 'ungroup'; group: string }
  /** `mode` is required: everything each object contains too, or the object alone. */
  | { op: 'delete_objects'; objects: string[]; mode: DeleteMode }
  /** Every instance and graphic in the union of the objects' subtrees moves once. */
  | { op: 'move_objects'; objects: string[]; offset: Position }
  | { op: 'reorder'; object: string; index: number }

/** What `delete_objects` removes besides the objects it names. */
export type DeleteMode = 'subtree' | 'object_only'

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
  /**
   * With `invalid_command`, the refusal's stable code: a combination of recorded states, a
   * scene command (`object_unknown`, `delete_mode_missing`, …), or the selection.
   */
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

const SHAPES: readonly unknown[] = ['line', 'rectangle', 'arrow']

const isPoint = (value: unknown): value is Position => isRecord(value) && isFiniteNumber(value.x) && isFiniteNumber(value.y)

/**
 * `scene.objects` and `selection`, which come together or not at all: every object once, of a
 * known kind, contained by the page or by a group that lists it (so each has one parent and
 * every one is reached from the page, which rules out cycles), every molecule object an
 * instance and every instance a molecule object, and a selection naming only what the
 * document holds.
 */
function isTreeAndSelection(document: Record<string, unknown>, atoms: AtomView[], bonds: BondView[]): boolean {
  const scene = isRecord(document.scene) ? document.scene : undefined
  const objects = scene?.objects
  const { selection } = document
  if (objects === undefined && selection === undefined) return true
  if (!scene || !Array.isArray(objects) || !isRecord(selection)) return false
  const page = (scene.pages as { id: string }[])[0].id
  const instances = new Set((scene.instances as InstanceView[]).map((instance) => instance.id))
  const byId = new Map<string, SceneObjectView>()
  for (const object of objects) {
    if (!isRecord(object) || typeof object.id !== 'string' || typeof object.parent !== 'string' || byId.has(object.id)) return false
    const shaped =
      (object.kind === 'molecule' && object.molecule === (document.molecule as { id: string }).id && instances.has(object.id)) ||
      (object.kind === 'group' && Array.isArray(object.children) && object.children.every((child) => typeof child === 'string')) ||
      (object.kind === 'graphic' &&
        SHAPES.includes(object.shape) &&
        Array.isArray(object.points) &&
        object.points.length === 2 &&
        object.points.every(isPoint))
    if (!shaped) return false
    byId.set(object.id, object as unknown as SceneObjectView)
  }
  if ([...byId.values()].filter((object) => object.kind === 'molecule').length !== instances.size) return false
  // The objects are listed depth first: each root object of the page (in the order listed)
  // followed by its subtree, a group's children in its order. Every child names its group as
  // parent and is met once, so the walk lists every object exactly where the reply does.
  const walked: string[] = []
  const reached = new Set<string>()
  const stack = [...byId.values()]
    .filter((object) => object.parent === page)
    .map((object) => object.id)
    .reverse()
  while (stack.length > 0) {
    const id = stack.pop() as string
    if (reached.has(id)) return false
    reached.add(id)
    walked.push(id)
    const object = byId.get(id)
    if (object?.kind !== 'group') continue
    for (const child of object.children) {
      if (byId.get(child)?.parent !== id) return false
    }
    stack.push(...[...object.children].reverse())
  }
  if (walked.length !== objects.length || walked.some((id, index) => (objects[index] as { id: string }).id !== id)) return false
  const names = (key: string, known: (id: string) => boolean) => {
    const list = selection[key]
    return Array.isArray(list) && list.every((id) => typeof id === 'string' && known(id))
  }
  const atomIds = new Set(atoms.map((atom) => atom.id))
  const bondIds = new Set(bonds.map((bond) => bond.id))
  return (
    names('objects', (id) => byId.has(id)) &&
    names('atoms', (id) => atomIds.has(id)) &&
    names('bonds', (id) => bondIds.has(id))
  )
}

/**
 * The page's objects depth first. A service from before groups and graphics reports only its
 * instances, all on the page; one from before scene instances reports none.
 */
export function sceneObjects(document: DocumentView): SceneObjectView[] {
  const scene = document.scene
  if (!scene) return []
  return (
    scene.objects ??
    scene.instances.map((instance) => ({ kind: 'molecule', id: instance.id, parent: instance.page, molecule: instance.molecule }))
  )
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
    isTreeAndSelection(document, document.atoms, document.bonds) &&
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

  private async request(method: 'GET' | 'POST', path: string, body?: unknown, headers: Record<string, string> = {}) {
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
          stateReason: value.code === 'invalid_command' && typeof value.reason === 'string' ? value.reason : undefined,
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
  ) {
    for (let attempt = 0; ; attempt += 1) {
      const { header, token } = await this.session()
      sending?.(token)
      try {
        return await this.request('POST', path, body, { [header]: token })
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
  private async guarded(path: string, body: Record<string, unknown>, onRestart: 'retry' | 'reload') {
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
        return await this.write(path, request, onRestart, sending)
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

  /**
   * Applies `commands`. With `selections`, for a service that records selections in history,
   * sends the selection before the edit (undo restores it) and after it (redo restores it); the
   * one after may name an object the edit creates as `$N`, the index of the command creating it.
   * Both are left out for a service that takes none.
   */
  async transact(
    id: string,
    expectedRevision: number,
    commands: Command[],
    selections?: { before: SelectionView; after: SelectionView },
  ) {
    return DocumentClient.document(
      await this.write(
        `${documentPath(id)}/transactions`,
        {
          request_id: requestId(),
          expected_revision: expectedRevision,
          commands,
          ...(selections ? { selection: selections.after, selection_before: selections.before } : {}),
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

function documentPath(id: string) {
  return `/api/v1/documents/${encodeURIComponent(id)}`
}

function requestId() {
  return crypto.randomUUID()
}
