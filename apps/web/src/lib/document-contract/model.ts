import type { StereoMark, StereoView } from '@/lib/stereo-contract'

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

/** What a query atom stands for. `chem.substructure` matches it. */
export type QueryAtom =
  | { kind: 'any' | 'any_heavy' | 'heteroatom' | 'halogen' }
  | { kind: 'list'; elements: string[]; negated: boolean }

/** What a query bond stands for. `chem.substructure` matches it. */
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

export type CaptionAlignment = 'left' | 'center' | 'right'

export interface CaptionRun { shadow?: boolean; outline?: boolean; underline?: boolean; color?: string | null; size_pt?: number | null; font?: LabelFont | null; text: string; script: 'baseline' | 'subscript' | 'superscript'; weight: 'normal' | 'bold'; style: 'normal' | 'italic' }

export type GraphicShape = 'line' | 'rectangle' | 'arrow' | 'ellipse'
export type SceneGraphicShape = GraphicShape | 'polygon' | 'spline'

export function isGraphicShape(value: unknown): value is GraphicShape {
  return value === 'line' || value === 'arrow' || value === 'rectangle' || value === 'ellipse'
}

export function isSceneGraphicShape(value: unknown): value is SceneGraphicShape {
  return isGraphicShape(value) || value === 'polygon' || value === 'spline'
}
export const MIN_POLYGON_VERTICES = 3
export const MAX_POLYGON_VERTICES = 64

/** Wire cardinality is shape-specific; multi-point graphics are never truncated into two-point graphics. */
export function graphicPointCountValid(shape: SceneGraphicShape, count: number): boolean {
  return shape === 'polygon' ? Number.isInteger(count) && count >= MIN_POLYGON_VERTICES && count <= MAX_POLYGON_VERTICES : count === (shape === 'spline' ? 4 : 2)
}

export type SceneObject =
  | { kind: 'instance'; id: string; parent: string | null; molecule: string }
  | { kind: 'group'; id: string; parent: string | null; children: string[] }
  | { kind: 'image'; id: string; parent: string | null; mime_type: 'image/png'; data_base64: string; origin: Position; size_pt: Position; pixel_width: number; pixel_height: number }
  | { kind: 'caption'; id: string; parent: string | null; anchor: Position; alignment: CaptionAlignment; runs: CaptionRun[] }
  | { kind: 'graphic'; id: string; parent: string | null; shape: GraphicShape; points: [Position, Position]; line_width_pt?: number | null }
  | { kind: 'graphic'; id: string; parent: string | null; shape: 'spline'; points: [Position, Position, Position, Position]; line_width_pt?: number | null }
  | { kind: 'graphic'; id: string; parent: string | null; shape: 'polygon'; points: Position[]; line_width_pt?: number | null }

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
  /**
   * How the bond is drawn; null for a plain bond, and filled in as null for older replies. A
   * drawing fact only: it states no configuration.
   */
  mark: StereoMark | null
}

/** The generic font family atom labels are drawn in. */
export type LabelFont = 'sans-serif' | 'serif' | 'monospace'
/** Chemical label weight, independent of caption run weights. */
export type LabelWeight = 'normal' | 'bold'
export type LabelStyle = 'normal' | 'italic'
/** `default`: a bonded carbon recording nothing is a bare vertex; `all`: every carbon is labelled. */
export type CarbonLabels = 'default' | 'all'
/** The unit lengths are shown in; lengths are always stored in pt (1 in = 72 pt = 25.4 mm). */
export type DisplayUnit = 'pt' | 'mm' | 'cm' | 'in'

/** A document's style settings, saved and undone with it. Added within v1. */
export interface DocumentSettings {
  bond_length_pt: number
  line_width_pt: number
  label_font: LabelFont
  label_weight: LabelWeight
  label_underline: boolean
  label_outline: boolean
  label_shadow: boolean
  label_style: LabelStyle
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
  label_weight: 'normal',
  label_style: 'normal',
  label_underline: false,
  label_outline: false,
  label_shadow: false,
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

export const LABEL_STYLES: readonly LabelStyle[] = ['normal', 'italic']
export const LABEL_WEIGHTS: readonly LabelWeight[] = ['normal', 'bold']
export const LABEL_FONTS: readonly LabelFont[] = ['sans-serif', 'serif', 'monospace']
export const CARBON_LABELS: readonly CarbonLabels[] = ['default', 'all']
export const DISPLAY_UNITS: readonly DisplayUnit[] = ['pt', 'mm', 'cm', 'in']

export interface FragmentDisplayRun { text: string; script: 'baseline' | 'subscript' | 'superscript' }

export interface FragmentDisplayView { instance: string; anchor: string; members: string[]; text: string; runs?: FragmentDisplayRun[] }

/** Historical snapshots have plain text; new snapshots preserve every run boundary. */
export function fragmentDisplayRuns(item: FragmentDisplayView): FragmentDisplayRun[] {
  return item.runs ?? [{ text: item.text, script: 'baseline' }]
}

export interface AbbreviationView { instance: string; anchor: string; terminal: string; display_label: string }

export interface DocumentView {
  /** User-named fragment presentation; the full chemical graph remains retained. */
  fragment_displays?: FragmentDisplayView[]
  /** Et presentation only; atoms, bonds and coordinates remain complete. */
  abbreviations?: AbbreviationView[]
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
  /**
   * Stated stereo configurations, added within v1: omitted by an older service, and filled in
   * as empty. No record means "unspecified", not "non-stereogenic".
   */
  stereo?: StereoView
}
