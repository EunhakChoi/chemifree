/**
 * Hand-maintained mirror of the `chem.cip` reply (docs/CONTRACT.md, "CIP labels"): the CIP
 * descriptor each stored configuration has in the current graph, `R` or `S` for a center and
 * `E` or `Z` for a double bond. Representation only. Nothing here computes a label, ranks a
 * ligand or judges a stereocenter: the service does, and this module reads and checks what it
 * answers. A label describes the stored configuration of the graph, never the drawn marks.
 *
 * The validators are strict on purpose and fail closed: a field the reader does not know, a
 * state without the descriptor or reason it needs, a ranking that is not the item's own, a
 * `stored` record the snapshot does not hold, or a warning, loss entry or diagnostic that does
 * not fit the items makes the whole reply unusable ("incompatible"), and the page shows no
 * label for it. Extend a list here in the same change that makes the service send the value.
 */
import {
  ascending,
  serial,
  bondedAtoms,
  IMPLICIT_HYDROGEN,
  LONE_PAIR,
  sameValue,
  type AtomConfiguration,
  type BondConfiguration,
  type GraphBond,
  type StereoView,
} from '@/lib/stereo-contract'

/** The model this reader accepts; a different id is a different contract. */
export const CIP_MODEL = 'chemifree-cip-1'
/** The sequence rules of that model, in order. */
export const CIP_RULES = ['1a', '1b', '2', '3'] as const
export type CipRule = (typeof CIP_RULES)[number]

export type CipState = 'assigned' | 'none' | 'unspecified' | 'unknown' | 'ambiguous' | 'unsupported'
const STATES: readonly unknown[] = ['assigned', 'none', 'unspecified', 'unknown', 'ambiguous', 'unsupported']
/** What a reply says it listed. The web asks for `stored` only. */
export type CipScope = 'stored' | 'stereogenic' | 'named'
const SCOPES: readonly unknown[] = ['stored', 'stereogenic', 'named']

/** The closed list of reasons for `unsupported` and `ambiguous`; a code outside it is refused. */
export const CIP_REASONS = [
  'cip_not_a_candidate',
  'cip_center_structure',
  'cip_double_bond_structure',
  'cip_query_atom',
  'cip_bond_kind',
  'cip_attachment_point',
  'cip_hydrogens_unknown',
  'cip_aromatic_system',
  'cip_mancude_rule_1b',
  'cip_rule_1b_duplicate_distance',
  'cip_mass_number',
  'cip_rule_4_or_5_needed',
  'cip_stereo_cycle',
  'cip_rule3_nested_descriptor',
  'cip_ligand_stereo_unspecified',
  'cip_ranking_inconsistent',
  'cip_digraph_limit',
] as const
export type CipReason = (typeof CIP_REASONS)[number]
const REASONS: readonly unknown[] = CIP_REASONS
/** The one reason an `ambiguous` item carries; an `unsupported` item never does. */
const AMBIGUOUS_REASON: CipReason = 'cip_ligand_stereo_unspecified'

export interface CipPriority {
  /** An atom identifier, `implicit_hydrogen` or `lone_pair`. */
  neighbor: string
  rank: number
}

export interface CipEnd {
  atom: string
  higher: string
  decided_by: CipRule
}

interface CipItem {
  state: CipState
  /** The sentence the service gives for the reason, from the item's loss entry; null without a reason. */
  reasonSentence: string | null
  reason: CipReason | null
  /** True when the service warned that a configuration is stored on a unit that is not stereogenic. */
  storedNotStereogenic: boolean
}

export interface CipAtom extends CipItem {
  atom: string
  descriptor: 'R' | 'S' | null
  stored: AtomConfiguration | null
  priority: CipPriority[] | null
  decided_by: CipRule | null
}

export interface CipBond extends CipItem {
  bond: string
  descriptor: 'E' | 'Z' | null
  stored: BondConfiguration | null
  ends: CipEnd[] | null
}

/** An advisory finding of a `chem.cip` reply, in the order the service sent it. */
export interface CipDiagnostic {
  code: string
  severity: 'info' | 'warning' | 'error'
  message: string
}

/** The items of a successful reply, checked against the snapshot it names. */
export interface CipBody {
  model: typeof CIP_MODEL
  rules: readonly CipRule[]
  scope: CipScope
  atoms: CipAtom[]
  bonds: CipBond[]
  diagnostics: CipDiagnostic[]
}

/** The identifiers a request named (`atoms` and `bonds`); a named item is always listed, whatever the scope. */
export interface CipNamed {
  atoms: readonly string[]
  bonds: readonly string[]
}

/** The snapshot graph a `chem.cip` reply is about. */
export interface CipGraph {
  atoms: ReadonlyArray<{ id: string }>
  bonds: readonly GraphBond[]
  stereo: StereoView
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
const hasOnly = (value: Record<string, unknown>, keys: string[]) => Object.keys(value).every((key) => keys.includes(key))
const hasAll = (value: Record<string, unknown>, keys: string[]) => keys.every((key) => key in value)
const isCount = (value: unknown): value is number => typeof value === 'number' && Number.isInteger(value) && value >= 0
const RULES: readonly unknown[] = CIP_RULES

/** Whether a state carries a ranking: the others carry none (and a descriptor only when assigned). */
const ranked = (state: CipState) => state === 'assigned' || state === 'unknown' || state === 'unspecified'

/** A `stored` field: the snapshot's own record for the item, or null where it holds none. */
function storedFits<T>(value: unknown, record: T | null): value is T | null {
  return value === null ? record === null : record !== null && sameValue(value, record)
}

/** How the state pairs with what is stored: a label needs a stated record, `unknown` an unknown one, `unspecified` none. */
function statePairs(state: CipState, stored: { kind: string } | null): boolean {
  switch (state) {
    case 'assigned':
      return stored !== null && stored.kind !== 'unknown'
    case 'unknown':
      return stored !== null && stored.kind === 'unknown'
    case 'unspecified':
      return stored === null
    case 'none':
    case 'ambiguous':
    case 'unsupported':
      return true
    default: {
      const unhandled: never = state
      return unhandled
    }
  }
}

/** The descriptor and reason fields, which the state decides: a label exactly when assigned, a reason exactly when ambiguous or unsupported. */
function labelFits(entry: Record<string, unknown>, state: CipState, descriptors: readonly string[]): boolean {
  if (state === 'assigned' ? !descriptors.includes(entry.descriptor as string) : entry.descriptor !== null) return false
  if (state === 'ambiguous') return entry.reason === AMBIGUOUS_REASON
  if (state === 'unsupported') return REASONS.includes(entry.reason) && entry.reason !== AMBIGUOUS_REASON
  return entry.reason === null
}

const isNonAtom = (slot: string) => slot === IMPLICIT_HYDROGEN || slot === LONE_PAIR

/** Four positions of a center: its bonded atoms and the non-atom slots that fill it, each once, ranked 1 to 4 in order. */
function priorityFits(value: unknown, bonded: ReadonlySet<string>, stored: AtomConfiguration | null): value is CipPriority[] {
  if (!Array.isArray(value) || value.length !== 4 || bonded.size < 3 || bonded.size > 4) return false
  const list: unknown[] = value
  const entries: CipPriority[] = []
  for (const [index, item] of list.entries()) {
    if (!isRecord(item) || !hasOnly(item, ['neighbor', 'rank']) || !hasAll(item, ['neighbor', 'rank']) ||
        typeof item.neighbor !== 'string' || item.rank !== index + 1) {
      return false
    }
    entries.push({ neighbor: item.neighbor, rank: item.rank })
  }
  const slots = entries.map((entry) => entry.neighbor)
  const atoms = slots.filter((slot) => !isNonAtom(slot))
  const others = slots.filter(isNonAtom)
  if (new Set(slots).size !== 4 || atoms.length !== bonded.size || !atoms.every((atom) => bonded.has(atom)) ||
      others.length !== 4 - bonded.size) {
    return false
  }
  return stored === null || sameValue([...slots].sort(), [...stored.neighbors].sort())
}

/** Where a double bond's end has an alternative to an atom: an end with fewer than two other atoms. */
function endFits(value: unknown, atom: string, partner: string, bonded: ReadonlyMap<string, ReadonlySet<string>>): value is CipEnd {
  if (!isRecord(value) || !hasOnly(value, ['atom', 'higher', 'decided_by']) || !hasAll(value, ['atom', 'higher', 'decided_by']) ||
      value.atom !== atom || typeof value.higher !== 'string' || !RULES.includes(value.decided_by)) {
    return false
  }
  const around = bonded.get(atom) ?? new Set<string>()
  return isNonAtom(value.higher) ? around.size - 1 < 2 : value.higher !== partner && around.has(value.higher)
}

/** The facts the diagnostics and loss entries state about items, indexed by the item they name. */
interface Findings {
  notStereogenic: Set<string>
  slotUnusual: Set<string>
  scope: string
}

const ITEM_KEYS = ['item', 'id']

/**
 * The diagnostics of a successful reply. The codes this reader knows are checked: warnings
 * (`cip_stored_not_stereogenic`, `cip_slot_unusual`) name a listed item and come in the order
 * the service produces them (centers, then bonds, each ascending; one item may have both), and
 * `cip_scope` and `cip_not_checked` each appear once. A stored configuration on a unit found
 * `none` is warned about exactly once. A code this reader does not know is kept when it is a
 * well-formed diagnostic and refused when it is not: docs/CONTRACT.md allows new diagnostic codes
 * within contract 1 ("clients ... treat an unknown diagnostic code by its outcome"), and the
 * outcome of the reply is checked separately.
 */
function readDiagnostics(value: unknown, atoms: readonly CipAtom[], bonds: readonly CipBond[]): { list: CipDiagnostic[]; findings: Findings } | null {
  if (!Array.isArray(value)) return null
  const list: CipDiagnostic[] = []
  const findings: Findings = { notStereogenic: new Set(), slotUnusual: new Set(), scope: '' }
  let scoped = false
  let checked = false
  let last = -1
  for (const item of value as unknown[]) {
    if (!isRecord(item) || !hasOnly(item, ['code', 'severity', 'message', 'path', 'details']) ||
        typeof item.code !== 'string' || typeof item.message !== 'string' ||
        (item.severity !== 'info' && item.severity !== 'warning' && item.severity !== 'error') ||
        !(item.path === undefined || item.path === null || typeof item.path === 'string') ||
        !(item.details === undefined || isRecord(item.details))) {
      return null
    }
    const details = item.details ?? {}
    switch (item.code) {
      case 'cip_stored_not_stereogenic':
      case 'cip_slot_unusual': {
        if (item.severity !== 'warning' || !hasOnly(details, ITEM_KEYS) || !hasAll(details, ITEM_KEYS) || typeof details.id !== 'string') return null
        const listed = details.item === 'atom' ? atoms.some((atom) => atom.atom === details.id) : details.item === 'bond' && bonds.some((bond) => bond.bond === details.id)
        if (!listed) return null
        // The order the service produces: centers by ascending identifier, then bonds.
        const position = (details.item === 'bond' ? 1_000_000_000 : 0) + serial(details.id)
        if (!Number.isFinite(position) || position < last) return null
        last = position
        const key = `${details.item as string}:${details.id}`
        const into = item.code === 'cip_slot_unusual' ? findings.slotUnusual : findings.notStereogenic
        if (into.has(key) || (item.code === 'cip_slot_unusual' && details.item !== 'atom')) return null
        into.add(key)
        break
      }
      case 'cip_scope':
        if (scoped || item.severity !== 'info' || !hasOnly(details, ['scope']) || typeof details.scope !== 'string') return null
        scoped = true
        findings.scope = details.scope
        break
      case 'cip_not_checked':
        if (checked || item.severity !== 'info' || Object.keys(details).length !== 0) return null
        checked = true
        break
      case 'cip_labeler_invalid':
        // Said only of an error reply (outcome `error`, no result), never beside labels.
        return null
      default:
        // An advisory code added within contract 1: shown, never relied on.
        break
    }
    list.push({ code: item.code, severity: item.severity, message: item.message })
  }
  return scoped && checked ? { list, findings } : null
}

/** The sentence after `code: ` of a loss entry, or null when the entry is not that. */
function sentenceOf(reason: unknown, code: string): string | null {
  if (typeof reason !== 'string' || !reason.startsWith(`${code}: `)) return null
  const sentence = reason.slice(code.length + 2)
  return sentence.trim() === '' ? null : sentence
}

/**
 * Whether `reply` (the whole operation reply with its `result`) is exactly what the service
 * sends for `graph`, the snapshot it names, and the scope the caller asked for (`scope` is what
 * the reply must say it listed): the model and rules are the ones this reader knows, atoms and
 * bonds ascend without repeats and name a center or double bond of the graph, each `stored` is
 * the snapshot's own record, the state decides which of descriptor, reason, ranking and ends are
 * set, a label is `R` or `S` on an atom and `E` or `Z` on a bond only, a ranking is the item's
 * own four positions, the outcome is `partial` exactly when an item is ambiguous or unsupported
 * and then each such item has one loss entry, and the diagnostics are the closed list. Any other
 * reply is refused whole.
 */
export function readCipReply(
  reply: Record<string, unknown>,
  graph: CipGraph,
  scope: CipScope,
  named: CipNamed = { atoms: [], bonds: [] },
): CipBody | null {
  const result = reply.result
  if (!isRecord(result) || !hasOnly(result, ['snapshot', 'model', 'rules', 'scope', 'atoms', 'bonds']) ||
      !hasAll(result, ['snapshot', 'model', 'rules', 'scope', 'atoms', 'bonds']) ||
      reply.model_id !== CIP_MODEL || result.model !== CIP_MODEL || !sameValue(result.rules, CIP_RULES) ||
      !SCOPES.includes(result.scope) || result.scope !== scope ||
      !Array.isArray(result.atoms) || !Array.isArray(result.bonds) ||
      (reply.outcome !== 'ok' && reply.outcome !== 'partial') || !Array.isArray(reply.loss_report)) {
    return null
  }
  const bonded = bondedAtoms(graph.bonds)
  const atomIds = new Set(graph.atoms.map((atom) => atom.id))
  const allBonds = new Map(graph.bonds.map((bond) => [bond.id, bond]))

  const atoms: CipAtom[] = []
  for (const entry of result.atoms as unknown[]) {
    const keys = ['atom', 'state', 'descriptor', 'reason', 'stored', 'priority', 'decided_by']
    if (!isRecord(entry) || !hasOnly(entry, keys) || !hasAll(entry, keys) || typeof entry.atom !== 'string' || !atomIds.has(entry.atom) ||
        !STATES.includes(entry.state)) {
      return null
    }
    const state = entry.state as CipState
    const record = graph.stereo.atoms.find((candidate) => candidate.atom === entry.atom)?.configuration ?? null
    if (!storedFits(entry.stored, record) || !statePairs(state, record) || !labelFits(entry, state, ['R', 'S'])) return null
    const around = bonded.get(entry.atom) ?? new Set<string>()
    if (ranked(state)) {
      if (!priorityFits(entry.priority, around, record) || !RULES.includes(entry.decided_by)) return null
    } else if (entry.priority !== null || entry.decided_by !== null) {
      return null
    }
    atoms.push({
      atom: entry.atom,
      state,
      descriptor: entry.descriptor as CipAtom['descriptor'],
      reason: entry.reason as CipReason | null,
      reasonSentence: null,
      storedNotStereogenic: false,
      stored: record,
      priority: entry.priority as CipPriority[] | null,
      decided_by: entry.decided_by as CipRule | null,
    })
  }

  const bonds: CipBond[] = []
  for (const entry of result.bonds as unknown[]) {
    const keys = ['bond', 'state', 'descriptor', 'reason', 'stored', 'ends']
    if (!isRecord(entry) || !hasOnly(entry, keys) || !hasAll(entry, keys) || typeof entry.bond !== 'string' || !STATES.includes(entry.state)) {
      return null
    }
    const bond = allBonds.get(entry.bond)
    if (bond === undefined) return null
    const state = entry.state as CipState
    // A named bond that is not a double bond is listed, refused as a non-candidate and holding nothing.
    if (bond.order !== 'double' && (state !== 'unsupported' || entry.reason !== 'cip_not_a_candidate')) return null
    const record = graph.stereo.bonds.find((candidate) => candidate.bond === entry.bond)?.configuration ?? null
    if (!storedFits(entry.stored, record) || !statePairs(state, record) || !labelFits(entry, state, ['E', 'Z'])) return null
    let ends: CipEnd[] | null = null
    if (ranked(state)) {
      const pair: unknown = entry.ends
      if (!Array.isArray(pair) || pair.length !== 2 || !endFits(pair[0], bond.begin, bond.end, bonded) || !endFits(pair[1], bond.end, bond.begin, bonded)) {
        return null
      }
      ends = pair as CipEnd[]
    } else if (entry.ends !== null) {
      return null
    }
    bonds.push({
      bond: entry.bond,
      state,
      descriptor: entry.descriptor as CipBond['descriptor'],
      reason: entry.reason as CipReason | null,
      reasonSentence: null,
      storedNotStereogenic: false,
      stored: record,
      ends,
    })
  }
  if (!ascending(atoms.map((item) => item.atom)) || !ascending(bonds.map((item) => item.bond))) return null
  // Which items are listed is decided by the scope: `stored` lists every stored record and the named items,
  // `named` exactly the named items, and `stereogenic` those and more (what the scan finds). A dropped item
  // would show "not examined" for a configuration that was asked about, so a shortfall is refused.
  const expectedAtoms = new Set([...(scope === 'named' ? [] : graph.stereo.atoms.map((record) => record.atom)), ...named.atoms])
  const expectedBonds = new Set([...(scope === 'named' ? [] : graph.stereo.bonds.map((record) => record.bond)), ...named.bonds])
  const listedAtoms = new Set(atoms.map((item) => item.atom))
  const listedBonds = new Set(bonds.map((item) => item.bond))
  const covers = (listed: ReadonlySet<string>, expected: ReadonlySet<string>) => [...expected].every((id) => listed.has(id))
  const exact = scope !== 'stereogenic'
  if (!covers(listedAtoms, expectedAtoms) || !covers(listedBonds, expectedBonds) ||
      (exact && (listedAtoms.size !== expectedAtoms.size || listedBonds.size !== expectedBonds.size))) {
    return null
  }

  const found = readDiagnostics(reply.diagnostics, atoms, bonds)
  if (found === null || found.findings.scope !== result.scope) return null
  const { findings } = found
  // A stored configuration on a `none` unit is warned about, and no other unit is; a slot warning needs a stored record.
  for (const atom of atoms) {
    if (findings.notStereogenic.has(`atom:${atom.atom}`) !== (atom.state === 'none' && atom.stored !== null)) return null
    if (findings.slotUnusual.has(`atom:${atom.atom}`) && atom.stored === null) return null
    atom.storedNotStereogenic = findings.notStereogenic.has(`atom:${atom.atom}`)
  }
  for (const bond of bonds) {
    if (findings.notStereogenic.has(`bond:${bond.bond}`) !== (bond.state === 'none' && bond.stored !== null)) return null
    bond.storedNotStereogenic = findings.notStereogenic.has(`bond:${bond.bond}`)
  }

  // The outcome is partial exactly when an item is ambiguous or unsupported, with one loss entry each, atoms first.
  const refused: Array<{ path: string; item: CipAtom | CipBond }> = [
    ...atoms.filter((item) => item.reason !== null).map((item) => ({ path: `atoms[${item.atom}]`, item })),
    ...bonds.filter((item) => item.reason !== null).map((item) => ({ path: `bonds[${item.bond}]`, item })),
  ]
  const losses: unknown[] = reply.loss_report
  if ((reply.outcome === 'partial') !== (refused.length > 0) || losses.length !== refused.length) return null
  for (const [index, { path, item }] of refused.entries()) {
    const loss = losses[index]
    if (!isRecord(loss) || !hasOnly(loss, ['feature', 'source_path', 'disposition', 'reason', 'semantic_impact']) ||
        loss.feature !== 'cip_descriptor' || loss.source_path !== path || loss.disposition !== 'unknown' || loss.semantic_impact !== 'stereo') {
      return null
    }
    const sentence = sentenceOf(loss.reason, item.reason as string)
    if (sentence === null) return null
    item.reasonSentence = sentence
  }
  return { model: CIP_MODEL, rules: CIP_RULES, scope: result.scope as CipScope, atoms, bonds, diagnostics: found.list }
}

/** The `resource_limit` details of an error reply: which part of the request stopped, and the numbers. */
export interface CipResourceLimit {
  stage: 'scan' | 'centers' | 'bonds'
  parameter: 'scope' | 'atoms' | 'bonds'
  limit: number
  request_limit: number
  items: number
  item_limit: number
}

/** What an error reply says: the service's own message, and for a limit what stopped. */
export interface CipFailure {
  code: string
  message: string
  limit: CipResourceLimit | null
}

const STAGE_PARAMETER = { scan: 'scope', centers: 'atoms', bonds: 'bonds' } as const

/**
 * The error reply of `chem.cip`, or null when it is not one the service sends. Every error
 * reply names the labeler's model; none carries a result. A `resource_limit` names the stage
 * and the parameter that stopped, consistently with its path; a `cip_labeler_invalid` names
 * the unit and the problem. Other codes (an unknown document, a malformed request, a refused
 * document, a cancelled run) are read by their message.
 */
export function readCipFailure(reply: Record<string, unknown>): CipFailure | null {
  if (reply.outcome !== 'error' || reply.model_id !== CIP_MODEL || (reply.result !== null && reply.result !== undefined)) return null
  const diagnostics: unknown = reply.diagnostics
  if (!Array.isArray(diagnostics) || diagnostics.length === 0) return null
  const first: unknown = diagnostics[0]
  if (!isRecord(first) || typeof first.code !== 'string' || typeof first.message !== 'string' || first.severity !== 'error') return null
  const base = { code: first.code, message: first.message }
  if (first.code === 'resource_limit') {
    const details = first.details
    const keys = ['stage', 'parameter', 'limit', 'request_limit', 'items', 'item_limit']
    if (!isRecord(details) || !hasOnly(details, keys) || !hasAll(details, keys) ||
        (details.stage !== 'scan' && details.stage !== 'centers' && details.stage !== 'bonds')) {
      return null
    }
    const parameter = STAGE_PARAMETER[details.stage]
    if (details.parameter !== parameter || first.path !== `parameters.${parameter}` ||
        !isCount(details.limit) || !isCount(details.request_limit) || !isCount(details.items) || !isCount(details.item_limit) ||
        details.limit > details.request_limit) {
      return null
    }
    return {
      ...base,
      limit: {
        stage: details.stage, parameter, limit: details.limit,
        request_limit: details.request_limit, items: details.items, item_limit: details.item_limit,
      },
    }
  }
  if (first.code === 'cip_labeler_invalid') {
    const details = first.details
    if (!isRecord(details) || !hasOnly(details, ['unit', 'problem']) || typeof details.unit !== 'string' || typeof details.problem !== 'string') return null
  }
  return { ...base, limit: null }
}
