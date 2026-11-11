import type { AtomConfiguration, BondConfiguration, StereoGroupKind, StereoMark } from '@/lib/stereo-contract'
import type {
  BondOrder, BondQuery, CaptionAlignment, CaptionRun, DocumentView, FragmentDisplayRun, GraphicShape, Hydrogens,
  Position, QueryAtom, Radical, ReferenceTarget, SelectionView, SettingsEdit,
} from '@/lib/document-contract/model'
import { isCount, isRecord, onlyKeys } from '@/lib/document-contract/guards'
import { ApiError, type DocumentResponse } from '@/lib/document-contract/responses'

export type RingSize = 5 | 6
export type RingPattern = 'single' | 'kekule'

export type RingPlacement = { kind: 'independent'; center: Position } | { kind: 'attach'; atom: string; direction: { dx: number; dy: number } }

/** An immutable ring mutation whose send method checks the same request each time. */
export interface RingInsertionAttempt {
  send(): Promise<DocumentResponse>
}

/** Native clipboard transport is plain text; its contents are validated again by the server. */
export const MAX_NATIVE_CLIPBOARD_BYTES = 64 * 1024

export interface NativeClipboardCopy {
  snapshot: DocumentResponse
  instance: string
  copied: SelectionView
  boundaryBonds: string[]
  payload: string
}

export interface NativeClipboardPaste {
  snapshot: DocumentResponse
  mapping: { atoms: NativeClipboardIdMapping[]; bonds: NativeClipboardIdMapping[] }
}

export interface NativeSceneClipboardCopy {
  snapshot: DocumentResponse
  copied: SelectionView
  roots: string[]
  payload: string
}

export interface NativeSceneClipboardPaste {
  snapshot: DocumentResponse
  mapping: { objects: NativeClipboardIdMapping[] }
}

/** Retain one scene paste attempt until its original process confirms the outcome. */
export interface NativeSceneClipboardPasteAttempt {
  send(): Promise<NativeSceneClipboardPaste>
}

export interface NativeClipboardIdMapping {
  source: string
  target: string
}

export interface TemplateParameters {
  document_id: string
  expected_revision: number
  payload: string
  instance: string
  source_anchor: string
  atom: string
  direction: { dx: number; dy: number }
  selection_before: SelectionView
}

export type TemplateReplacementParameters = Omit<TemplateParameters, 'direction'>

export interface TemplateReplacementOffer {
  parameters: TemplateReplacementParameters
  source: DocumentResponse
  preview: DocumentResponse
  mapping: NativeClipboardPaste['mapping']
  boundary: string
}

export interface TemplateInspection {
  payload: string
  atoms: Array<{ id: string; element: string; x: number; y: number; supported: boolean }>
}

export interface TemplateAttachmentOffer {
  parameters: TemplateParameters
  source: DocumentResponse
  preview: DocumentResponse
  mapping: NativeClipboardPaste['mapping']
  connection: string
}

/** Retain this exact attempt for Check again; creating another attempt means another paste. */
export interface NativeClipboardPasteAttempt {
  send(): Promise<NativeClipboardPaste>
}

/** Retain this attempt after clipboard write succeeds; Check again must never cut afresh. */
export interface NativeClipboardCutAttempt {
  send(): Promise<DocumentResponse>
}

/** A MOL import already bound to its inspected source, destination and consent digest. */
export interface NativeClipboardMolImportAttempt {
  send(): Promise<DocumentResponse>
}

/** A SMILES import bound to its inspected source, destination and consent decision. */
export interface SmilesClipboardImportAttempt {
  send(): Promise<DocumentResponse>
}

export const MAX_CLIPBOARD_SMILES_BYTES = 16 * 1024

/** Clipboard MOL intake matches the existing 200 KiB web file-import limit. */
export const MAX_CLIPBOARD_MOL_BYTES = 200 * 1024

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

/** Geometry-only arrangements against the selected scene units' bounds. */
export type ArrangementMode =
  | 'align_left' | 'align_horizontal_center' | 'align_right'
  | 'align_top' | 'align_vertical_center' | 'align_bottom'
  | 'distribute_horizontal' | 'distribute_vertical'

/** Page-coordinate transforms; horizontal/vertical name the mirror axis through the pivot. */
export type SelectionTransform =
  | { type: 'rotate'; degrees: number }
  | { type: 'reflect_horizontal' }
  | { type: 'reflect_vertical' }

export type Command =
  | { op: 'insert_ring'; instance: string; center: Position; size?: RingSize; pattern?: RingPattern }
  | { op: 'attach_ring'; instance: string; atom: string; direction: { dx: number; dy: number }; size?: RingSize; pattern?: RingPattern }
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
  | { op: 'collapse_ethyl'; instance: string; anchor: string; terminal: string }
  | { op: 'update_abbreviation_label'; instance: string; anchor: string; text: string | null }
  | { op: 'expand_ethyl'; instance: string; anchor: string }
  | { op: 'contract_fragment'; instance: string; anchor: string; members: string[]; text: string }
  | { op: 'set_fragment_display_text'; instance: string; anchor: string; text: string }
  | { op: 'set_fragment_display_runs'; instance: string; anchor: string; runs: FragmentDisplayRun[] }
  | { op: 'expand_fragment'; instance: string; anchor: string }
  | { op: 'add_image'; mime_type: 'image/png'; data_base64: string; origin: Position; size_pt: Position; parent?: string; index?: number }
  | { op: 'update_image_geometry'; object: string; origin: Position; size_pt: Position }
  | { op: 'add_caption'; anchor: Position; alignment: CaptionAlignment; runs: CaptionRun[]; parent?: string; index?: number }
  | { op: 'update_caption'; object: string; anchor: Position; alignment: CaptionAlignment; runs: CaptionRun[] }
  | { op: 'add_graphic'; shape: GraphicShape; points: [Position, Position]; parent?: string; index?: number }
  | { op: 'add_spline'; points: [Position, Position, Position, Position]; parent?: string; index?: number }
  | { op: 'update_spline_points'; object: string; points: [Position, Position, Position, Position] }
  | { op: 'add_polygon'; vertices: Position[]; parent?: string; index?: number }
  | { op: 'update_polygon_vertices'; object: string; vertices: Position[] }
  | { op: 'update_graphic_points'; object: string; points: [Position, Position] }
  | { op: 'set_graphic_line_width'; object: string; line_width_pt: number | null }
  | { op: 'group'; objects: string[] }
  | { op: 'ungroup'; group: string }
  | { op: 'reorder'; object: string; index: number }
  | { op: 'delete_objects'; objects: string[]; mode: 'subtree' | 'object_only' }
  | { op: 'move_objects'; objects: string[]; delta: Position }
  | { op: 'move_selection'; selection: SelectionView; delta: Position; instance?: string }
  | { op: 'arrange_objects'; objects: string[]; mode: ArrangementMode }
  | { op: 'transform_selection'; selection: SelectionView; pivot: Position; transform: SelectionTransform; instance?: string }
  | { op: 'add_reference' | 'remove_reference'; source: string; target: ReferenceTarget }
  | ({ op: 'update_settings' } & SettingsEdit)
  /** `bond` is an id or `$N`, the bond created by command N (an `add_bond`, or an `add_atom` with `bond_to`). `null` clears. */
  | { op: 'set_bond_mark'; bond: string; mark: StereoMark | null }
  /** Neighbors are atom ids, `$N` for a created atom, `implicit_hydrogen` or `lone_pair`, in any order. `null` clears. */
  | { op: 'set_atom_configuration'; atom: string; configuration: AtomConfiguration | null }
  | { op: 'set_bond_configuration'; bond: string; configuration: BondConfiguration | null }
  /**
   * Stores the configuration the marks and one scene instance's positions show: for the atoms
   * (centers) and bonds (double bonds) listed, or, with neither list, for every one a mark bears
   * on. Ids or `$N`; a list that is present is not empty. `instance` may be left out while the
   * document has one scene instance. The service refuses an item the drawing does not
   * determine with a `stereo_not_derivable_*` reason.
   */
  | { op: 'set_configuration_from_drawing'; atoms?: string[]; bonds?: string[]; instance?: string }
  /**
   * Enhanced-stereo group commands. `group`, `into` and `from` are `eN` or `$N` (the group an earlier
   * command made). Every `number` is an integer in the 32-bit signed range or `null` for none, and is
   * always sent. Atoms are ids or `$N` of stored tetrahedral centers (docs/CONTRACT.md, Stereo groups).
   */
  | { op: 'create_stereo_group'; kind: StereoGroupKind; atoms: string[]; number: number | null }
  | { op: 'delete_stereo_group'; group: string }
  | { op: 'set_stereo_group_kind'; group: string; kind: StereoGroupKind }
  | { op: 'set_stereo_group_number'; group: string; number: number | null }
  | { op: 'add_stereo_group_members' | 'remove_stereo_group_members'; group: string; atoms: string[] }
  | { op: 'merge_stereo_groups'; into: string; from: string }
  | { op: 'split_stereo_group'; group: string; atoms: string[]; number: number | null }

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

export function clipboardIncompatible(): ApiError {
  return new ApiError('incompatible_response', 'The local service returned an incompatible clipboard reply.', 200)
}

export function clipboardId(value: unknown, prefix: 'a' | 'b' | 's'): value is string {
  if (typeof value !== 'string' || !new RegExp(`^${prefix}[1-9][0-9]*$`).test(value)) return false
  return Number(value.slice(1)) <= 0xffff_ffff
}

export function clipboardTextFits(value: string): boolean {
  return new TextEncoder().encode(value).byteLength <= MAX_NATIVE_CLIPBOARD_BYTES
}

export function clipboardSelection(selection: SelectionView): SelectionView {
  return { objects: [...selection.objects], atoms: [...selection.atoms], bonds: [...selection.bonds] }
}

export function sameClipboardIds(first: readonly string[], second: readonly string[]): boolean {
  return first.length === second.length && new Set(first).size === first.length && first.every((id) => second.includes(id))
}

export function clipboardReply(value: unknown, request: string, operation: string): { result: Record<string, unknown>; revision: number; losses: unknown[] } {
  const { result, revision, losses } = operationDataReply(value, request, operation)
  if (!isCount(revision)) throw clipboardIncompatible()
  return { result, revision, losses }
}

/** Common envelope/error checks; each operation still validates its own revision semantics. */
export function operationDataReply(value: unknown, request: string, operation: string): { result: Record<string, unknown>; revision: unknown; losses: unknown[] } {
  if (!isRecord(value) || value.contract !== 1 || value.envelope !== 'operation_result' || value.request_id !== request ||
      value.operation !== operation || value.profile !== 'preserve-semantics' || !Array.isArray(value.diagnostics) || !Array.isArray(value.loss_report)) {
    throw clipboardIncompatible()
  }
  if (value.outcome === 'error' || value.outcome === 'unsupported') {
    const problem: unknown = value.diagnostics[0]
    if (value.result !== null || !isRecord(problem) || typeof problem.code !== 'string' || typeof problem.message !== 'string' ||
        !isRecord(problem.details) || value.loss_report.length !== 0) throw clipboardIncompatible()
    throw new ApiError(problem.code, problem.message, 200, {
      currentRevision: isCount(problem.details.current_revision) ? problem.details.current_revision : undefined,
      stateReason: typeof problem.details.reason === 'string' ? problem.details.reason : undefined,
    })
  }
  if ((value.outcome !== 'ok' && value.outcome !== 'partial') || !isRecord(value.result) ||
      value.diagnostics.length !== 0 || (value.outcome === 'ok') !== (value.loss_report.length === 0)) throw clipboardIncompatible()
  return { result: value.result, revision: value.document_revision, losses: value.loss_report }
}

interface ClipboardPayloadIds {
  atoms: string[]
  bonds: { id: string; begin: string; end: string }[]
}

/** Read only the native structural identifiers needed to verify a service mapping, never commands. */
export function clipboardPayloadIds(text: string): ClipboardPayloadIds {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    throw clipboardIncompatible()
  }
  if (!isRecord(value) || value.format !== 'chemifree.clipboard' || value.version !== 1 ||
      !onlyKeys(value, ['format', 'version', 'settings', 'atoms', 'bonds', 'stereo']) || !isRecord(value.settings) ||
      !Array.isArray(value.atoms) || !Array.isArray(value.bonds) || !Array.isArray(value.stereo)) throw clipboardIncompatible()
  const atoms: string[] = []
  for (const atom of value.atoms) {
    if (!isRecord(atom) || !clipboardId(atom.id, 'a') || atoms.includes(atom.id)) throw clipboardIncompatible()
    atoms.push(atom.id)
  }
  const bonds: ClipboardPayloadIds['bonds'] = []
  for (const bond of value.bonds) {
    if (!isRecord(bond) || !clipboardId(bond.id, 'b') || !clipboardId(bond.begin, 'a') || !clipboardId(bond.end, 'a') ||
        !atoms.includes(bond.begin) || !atoms.includes(bond.end) || bond.begin === bond.end || bonds.some((entry) => entry.id === bond.id)) throw clipboardIncompatible()
    bonds.push({ id: bond.id, begin: bond.begin, end: bond.end })
  }
  if (atoms.length === 0) throw clipboardIncompatible()
  return { atoms, bonds }
}

export function clipboardMappings(value: unknown, sources: readonly string[], prefix: 'a' | 'b', targets: readonly string[]): NativeClipboardIdMapping[] {
  if (!Array.isArray(value) || value.length !== sources.length) throw clipboardIncompatible()
  const mappings: NativeClipboardIdMapping[] = []
  for (const item of value) {
    if (!isRecord(item) || !clipboardId(item.source, prefix) || !clipboardId(item.target, prefix) ||
        !sources.includes(item.source) || mappings.some((prior) => prior.source === item.source || prior.target === item.target)) throw clipboardIncompatible()
    mappings.push({ source: item.source, target: item.target })
  }
  if (!sameClipboardIds(mappings.map((item) => item.target), targets)) throw clipboardIncompatible()
  return mappings
}
