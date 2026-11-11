import type { CipBody, CipScope } from '@/lib/cip-contract'
import { stereoOf, type CenterInspection, type DoubleInspection, type StereoDiagnostic } from '@/lib/stereo-contract'
import { DEFAULT_SETTINGS, type DocumentSettings, type DocumentView, type SelectionView } from '@/lib/document-contract/model'
import { isRecord } from '@/lib/document-contract/guards'
import type { RecoveryView } from '@/lib/document-contract/responses'

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

/**
 * What `chem.stereo` compared: the stored configurations against what each scene instance's
 * drawing shows, for the exact snapshot named. It judges nothing stereogenic and stores nothing.
 */
export interface StereoInspection {
  model: 'chemifree-stereo-1'
  snapshot: DocumentView
  /** The scene instances inspected, in identifier order. */
  instances: string[]
  atoms: CenterInspection[]
  bonds: DoubleInspection[]
  diagnostics: StereoDiagnostic[]
}

/**
 * What `chem.cip` labeled: the CIP descriptor of each stored configuration in the exact snapshot
 * named. A label describes the stored configuration, not the drawing, and is never stored.
 */
export interface CipInspection extends CipBody {
  snapshot: DocumentView
}

/** The parameters of `chem.cip` the web may send; the editor sends only `scope: 'stored'`. */
export interface CipParameters {
  scope?: CipScope
  atoms?: string[]
  bonds?: string[]
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
  /**
   * Which compared drawings hold stereo marks or stated configurations. This model never reads
   * stereo: `equivalent`, the code, the numbering and the symmetry say nothing about it, so a
   * drawing that holds some makes the whole answer `partial`.
   */
  stereo: { sourcePresent: boolean; otherPresent: boolean | null }
}

/** One pair of a mapping: an item of the query drawing and the item of the target it lies on. */
export interface MatchPair {
  query: string
  target: string
}

export interface SubstructureMapping {
  atoms: MatchPair[]
  bonds: MatchPair[]
}

export interface SubstructureLimits {
  query_atoms: number
  search_steps: number
  scanned_candidates: number
  mappings: number
}

/** Every mapping of the query in the target, bound to the two drawings the service searched. */
export interface SubstructureSearch {
  model: string
  aromaticityModel: string
  query: DocumentView
  target: DocumentView
  limits: SubstructureLimits
  steps: number
  mappings: SubstructureMapping[]
  /** Which drawings hold stereo this search did not read; see `IdentityInspection.stereo`. */
  stereo: { queryPresent: boolean; targetPresent: boolean }
}

/**
 * The service's refusal or limit, with the two drawings it concerns. Its `code` is one of the
 * `chem.substructure` codes in docs/CONTRACT.md; an error of the request itself (an unknown
 * document, say) is an `ApiError` instead.
 */
export class SubstructureRefusal extends Error {
  readonly code: string
  readonly query: DocumentView
  readonly target: DocumentView

  constructor(code: string, message: string, query: DocumentView, target: DocumentView) {
    super(message)
    this.name = 'SubstructureRefusal'
    this.code = code
    this.query = query
    this.target = target
  }
}

/** The outcome each `chem.substructure` code is reported with. */
export const SUBSTRUCTURE_OUTCOMES: ReadonlyMap<string, 'error' | 'unsupported'> = new Map([
  ['empty_query', 'error'],
  ['query_too_large', 'error'],
  ['query_unsatisfiable', 'error'],
  ['match_unsupported', 'unsupported'],
  ['match_limit', 'error'],
])

/** ID and revision alone can be reused across service processes. Compare the analyzed graph
 * as well, so a replacement snapshot cannot inherit an earlier scientific result. */
export function sameInspectionSnapshot(shown: DocumentView, inspected: DocumentView): boolean {
  return shown.id === inspected.id && shown.revision === inspected.revision &&
    JSON.stringify(shown.atoms) === JSON.stringify(inspected.atoms) &&
    JSON.stringify(shown.bonds) === JSON.stringify(inspected.bonds) &&
    JSON.stringify(stereoOf(shown)) === JSON.stringify(stereoOf(inspected))
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
    JSON.stringify(stereoOf(shown)) === JSON.stringify(stereoOf(inspected)) &&
    JSON.stringify(shown.scene) === JSON.stringify(inspected.scene)
}

/** The document's own style, or built-in defaults for an older service. */
export function settingsOf(document: DocumentView): Readonly<DocumentSettings> {
  return document.settings ?? DEFAULT_SETTINGS
}

/** Deepest nesting `sameJson` compares; anything deeper is called different. */
const MAX_COMPARE_DEPTH = 64

/** What `JSON.stringify` writes for a value held in an array: `undefined`, functions and symbols become null. */
function arrayItem(value: unknown): unknown {
  return value === undefined || typeof value === 'function' || typeof value === 'symbol' ? null : value
}

/** A number JSON cannot hold (NaN, infinities) is written as null. */
function jsonNumber(value: unknown): unknown {
  return typeof value === 'number' && !Number.isFinite(value) ? null : value
}

/**
 * Equality of two values as JSON holds them, for values that came out of `JSON.parse` or were
 * built the same way: key order does not matter, array order does, and a key holding
 * `undefined` is absent (JSON drops it), so it equals a missing key. `null` is a value: it
 * differs from an absent key. Two references to one object are equal without being walked;
 * otherwise a value reached deeper than `MAX_COMPARE_DEPTH` is never equal, which is also what
 * ends the comparison of cyclic values.
 */
export function sameJson(a: unknown, b: unknown, depth = 0): boolean {
  if (depth > MAX_COMPARE_DEPTH) return false
  const left = jsonNumber(a)
  const right = jsonNumber(b)
  if (left === right) return true
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length !== right.length) return false
    // A plain loop: `every` skips the holes of a sparse array, which JSON writes as null.
    for (let index = 0; index < left.length; index += 1) {
      if (!sameJson(arrayItem(left[index]), arrayItem(right[index]), depth + 1)) return false
    }
    return true
  }
  if (!isRecord(left) || !isRecord(right)) return false
  for (const key of new Set([...Object.keys(left), ...Object.keys(right)])) {
    if (!sameJson(left[key], right[key], depth + 1)) return false
  }
  return true
}

/** Full drawing comparison for conversion previews, including scene and settings. */
export function sameConversionSnapshot(a: DocumentView, b: DocumentView): boolean {
  return sameJson(a, b)
}

/**
 * Whether two drawings paint and measure alike: the fields the canvas draws from (id, atoms,
 * bonds, scene, settings, abbreviations and fragment displays) are equal under `sameJson`. The
 * revision, title, selection, stereo, molecule and session state are deliberately not compared.
 * Values are walked and never serialized, so a scene holding large embedded images is not copied.
 */
export function sameDrawnContent(a: DocumentView, b: DocumentView): boolean {
  if (a === b) return true
  return (
    sameJson(a.id, b.id) &&
    sameJson(a.atoms, b.atoms) &&
    sameJson(a.bonds, b.bonds) &&
    sameJson(a.scene, b.scene) &&
    sameJson(settingsOf(a), settingsOf(b)) &&
    sameJson(a.abbreviations ?? [], b.abbreviations ?? []) &&
    sameJson(a.fragment_displays ?? [], b.fragment_displays ?? [])
  )
}

/**
 * A comparison against one shown drawing that remembers its answer per snapshot object, so a
 * result and the offers made for it (which hold the same snapshot) cost one comparison between
 * them for as long as the shown drawing is the same object. Make a new one when the drawing
 * changes.
 */
export function conversionSnapshotMatcher(shown: DocumentView): (snapshot: DocumentView) => boolean {
  const answers = new WeakMap<DocumentView, boolean>()
  return (snapshot) => {
    let answer = answers.get(snapshot)
    if (answer === undefined) {
      answer = sameConversionSnapshot(shown, snapshot)
      answers.set(snapshot, answer)
    }
    return answer
  }
}
