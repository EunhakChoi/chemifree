import { API_VERSION } from '@/lib/contract'
import type { DocumentView, Position } from '@/lib/document-contract/model'
import { distinct, isCount, isFiniteNumber, isRecord } from '@/lib/document-contract/guards'
import type { DocumentResponse } from '@/lib/document-contract/responses'
import { documentResponse } from '@/lib/document-contract/transport'

/** The model of `layout.structure` (CONTRACT.md "Chain and simple ring layout"). */
export const LAYOUT_MODEL = 'chemifree-chain-layout-2'

export type LayoutScope = 'components' | 'selection'

/** What a layout press asks for. The arrays are copies, taken at press time. */
export interface LayoutRequest {
  documentId: string
  instance: string
  scope: LayoutScope
  atoms: string[]
  bonds: string[]
}

export interface LayoutOverlap {
  pairs: [string, string][]
  count: number
}

/** A checked preview: the proposal for `request`, bound to the snapshot it was computed on. */
export interface LayoutPreview {
  model: string
  snapshot: DocumentView
  request: LayoutRequest
  bondLengthPt: number
  atoms: string[]
  bondCount: number
  componentCount: number
  ringCount: number
  positions: { atom: string; before: Position; after: Position }[]
  overlap: LayoutOverlap | null
  noBonds: boolean
  notImplemented: string
}

export interface LayoutApplied {
  response: DocumentResponse
  unchanged: boolean
  overlap: LayoutOverlap | null
}

/** A `layout_unsupported` refusal of a preview. `atoms` and `limit` are what the service named, if anything. */
export class LayoutRefusal extends Error {
  readonly reason: string
  readonly atoms: string[]
  readonly limit: number | null

  constructor(reason: string, message: string, details: Record<string, unknown>) {
    super(message)
    this.name = 'LayoutRefusal'
    this.reason = reason
    this.atoms = isStrings(details.atoms) ? [...details.atoms] : []
    this.limit = isCount(details.limit) ? details.limit : null
  }
}

const MOVE_TOLERANCE_PT = 1e-9
const LAYOUT_CODES: readonly string[] = [
  'layout_overlap', 'layout_no_bonds', 'layout_preview_only', 'layout_unchanged', 'layout_not_implemented',
]

interface LayoutDiagnostics {
  overlap: LayoutOverlap | null
  noBonds: boolean
  previewOnly: boolean
  unchanged: boolean
  notImplemented: string | null
}

function isStrings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
}

export function sameLayoutRequest(a: LayoutRequest, b: LayoutRequest): boolean {
  return a.documentId === b.documentId && a.instance === b.instance && a.scope === b.scope &&
    sameStrings(a.atoms, b.atoms) && sameStrings(a.bonds, b.bonds)
}

function sameStrings(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((item, index) => item === b[index])
}

/** The number of positions that move, by the tolerance of the apply (CONTRACT.md "Zero difference"). */
export function layoutMovedCount(preview: LayoutPreview): number {
  return preview.positions.filter(({ before, after }) =>
    Math.abs(after.x - before.x) > MOVE_TOLERANCE_PT || Math.abs(after.y - before.y) > MOVE_TOLERANCE_PT,
  ).length
}

/**
 * Reads a preview reply for `request`. Returns null when the reply is not exactly the proposal
 * the service sends for that request and snapshot. The snapshot must be a well-formed document,
 * otherwise `documentResponse` throws.
 */
export function readLayoutPreview(value: unknown, request: LayoutRequest): LayoutPreview | null {
  if (!isRecord(value) || value.outcome !== 'ok' || value.model_id !== LAYOUT_MODEL ||
      (value.loss_report !== undefined && !(Array.isArray(value.loss_report) && value.loss_report.length === 0))) {
    return null
  }
  const result = value.result
  if (!isRecord(result) || !isRecord(result.snapshot)) return null
  const snapshot = documentResponse({ api_version: API_VERSION, ...result.snapshot }).document
  if (snapshot.id !== request.documentId || snapshot.revision !== value.document_revision) return null
  if (result.mode !== 'preview' || result.instance !== request.instance || result.scope !== request.scope) return null
  const bondLengthPt = result.bond_length_pt
  if (!isFiniteNumber(bondLengthPt) || bondLengthPt <= 0) return null
  const layout = result.layout
  if (!isRecord(layout)) return null
  const atomIds = new Set(snapshot.atoms.map((atom) => atom.id))
  const bondIds = new Set(snapshot.bonds.map((bond) => bond.id))
  const atoms = layout.atoms
  const bonds = layout.bonds
  if (!isStrings(atoms) || atoms.length === 0 || !distinct(atoms) || !atoms.every((id) => atomIds.has(id))) return null
  if (!isStrings(bonds) || !distinct(bonds) || !bonds.every((id) => bondIds.has(id))) return null
  const components = result.components
  if (!Array.isArray(components) || components.length === 0) return null
  let ringCount = 0
  for (const component of components) {
    if (!isRecord(component) || !Array.isArray(component.rings)) return null
    ringCount += component.rings.length
  }
  const positions = readPositions(result.positions, atoms)
  if (positions === null) return null
  const diagnostics = readLayoutDiagnostics(value.diagnostics, atomIds)
  if (diagnostics === null || !diagnostics.previewOnly || diagnostics.notImplemented === null) return null
  return {
    model: LAYOUT_MODEL,
    snapshot,
    request: { ...request, atoms: [...request.atoms], bonds: [...request.bonds] },
    bondLengthPt,
    atoms,
    bondCount: bonds.length,
    componentCount: components.length,
    ringCount,
    positions,
    overlap: diagnostics.overlap,
    noBonds: diagnostics.noBonds,
    notImplemented: diagnostics.notImplemented,
  }
}

/**
 * Reads the diagnostics of an apply reply against the document it returns. An apply carries
 * `layout_not_implemented` and never `layout_preview_only`.
 */
export function readLayoutApplyDiagnostics(
  diagnostics: unknown,
  document: DocumentView,
): { unchanged: boolean; overlap: LayoutOverlap | null } | null {
  const read = readLayoutDiagnostics(diagnostics, new Set(document.atoms.map((atom) => atom.id)))
  if (read === null || read.notImplemented === null || read.previewOnly) return null
  return { unchanged: read.unchanged, overlap: read.overlap }
}

/** Reads the layout codes. Unknown codes and items without a string code are ignored; the first of each known code counts. */
function readLayoutDiagnostics(diagnostics: unknown, atoms: ReadonlySet<string>): LayoutDiagnostics | null {
  if (!Array.isArray(diagnostics)) return null
  const first = new Map<string, Record<string, unknown>>()
  for (const item of diagnostics) {
    if (isRecord(item) && typeof item.code === 'string' && LAYOUT_CODES.includes(item.code) && !first.has(item.code)) {
      first.set(item.code, item)
    }
  }
  let overlap: LayoutOverlap | null = null
  const overlapItem = first.get('layout_overlap')
  if (overlapItem !== undefined) {
    overlap = readOverlap(overlapItem, atoms)
    if (overlap === null) return null
  }
  let notImplemented: string | null = null
  const notImplementedItem = first.get('layout_not_implemented')
  if (notImplementedItem !== undefined) {
    if (typeof notImplementedItem.message !== 'string') return null
    notImplemented = notImplementedItem.message
  }
  return {
    overlap,
    noBonds: first.has('layout_no_bonds'),
    previewOnly: first.has('layout_preview_only'),
    unchanged: first.has('layout_unchanged'),
    notImplemented,
  }
}

function readOverlap(item: Record<string, unknown>, atoms: ReadonlySet<string>): LayoutOverlap | null {
  if (item.severity !== 'warning' || !isRecord(item.details)) return null
  const rawPairs = item.details.pairs
  const count = item.details.count
  if (!Array.isArray(rawPairs) || !isCount(count)) return null
  const pairs: [string, string][] = []
  for (const pair of rawPairs) {
    if (!Array.isArray(pair) || pair.length !== 2) return null
    const [a, b]: unknown[] = pair
    if (typeof a !== 'string' || typeof b !== 'string' || !atoms.has(a) || !atoms.has(b)) return null
    pairs.push([a, b])
  }
  if (count < pairs.length) return null
  return { pairs, count }
}

/** Reads one point of `before` or `after`, or null when it is not a finite point. */
function readPoint(value: unknown): Position | null {
  if (!isRecord(value)) return null
  const x = value.x
  const y = value.y
  return isFiniteNumber(x) && isFiniteNumber(y) ? { x, y } : null
}

/** One position per layout atom, in the order of `atoms`. */
function readPositions(value: unknown, atoms: readonly string[]): LayoutPreview['positions'] | null {
  if (!Array.isArray(value) || value.length !== atoms.length) return null
  const positions: LayoutPreview['positions'] = []
  for (const [index, atom] of atoms.entries()) {
    const item: unknown = value[index]
    if (!isRecord(item) || item.atom !== atom) return null
    const before = readPoint(item.before)
    const after = readPoint(item.after)
    if (before === null || after === null) return null
    positions.push({ atom, before, after })
  }
  return positions
}
