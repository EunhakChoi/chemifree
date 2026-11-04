/**
 * What the editor makes of a `chem.cip` reply: when a reply may be shown, the label text drawn
 * over the canvas, the cell each Stereo panel row gets, and when the service is asked. Nothing
 * here computes or ranks anything: the service does, and these functions read its answer.
 *
 * Cost policy (docs/CONTRACT.md says `chem.cip` has no time limit, and a dense graph can be
 * slow): the page asks ONLY while at least one of the two label switches is on and something is
 * stored that the switch covers, and then always with `scope: "stored"`, never `stereogenic`.
 * With both switches off (the default) it sends no `chem.cip` request at all.
 *
 * A label describes the stored configuration, not the drawing, and is never written to the
 * document, saved, exported or sent back in a command; it never touches a mark. Placement and
 * text form are product choices, not legacy behavior (see `CipLabelLayer`): the atom form
 * `(S)` is the parenthesized descriptor; the bond form `(E)` or `(Z)` is chosen the same way.
 * Only an `assigned` item gets overlay text. The overlay does not mark an unspecified center,
 * and a label carries no enhanced-stereo-group meaning (this build ignores such groups).
 */
import { AGREEMENT_LABELS, itemName, type AgreementRow, type ItemRef } from '@/editor/stereo-agreement'
import type { CipAtom, CipBond } from '@/lib/cip-contract'
import { samePropertySnapshot, type CipInspection, type DocumentView, type Position } from '@/lib/document-contract'
import { stereoOf } from '@/lib/stereo-contract'

/** Said by the switches' group while both are off. */
export const CIP_OFF = 'Switch on atom or bond stereo labels to compute CIP labels for the stored configurations.'

/** The sentence for a failed request or limit: the service's message, and no label. */
export const unavailableSentence = (message: string) => `CIP labels unavailable: ${message}`

/**
 * Whether the page should ask `chem.cip` for this drawing: a label switch is on and the drawing
 * stores a configuration of that kind. A drawing with marks only has nothing to label.
 */
export function wantsCip(document: DocumentView, atomLabels: boolean, bondLabels: boolean): boolean {
  const stereo = stereoOf(document)
  return (atomLabels && stereo.atoms.length > 0) || (bondLabels && stereo.bonds.length > 0)
}

/** What the page may say about CIP labels for this drawing. */
export type CipView =
  /** Both switches are off: nothing is asked and nothing is said but how to turn labels on. */
  | { kind: 'off' }
  /** A switch is on but the drawing stores nothing of that kind: nothing is asked. `message` says what is missing. */
  | { kind: 'nothing'; message: string }
  | { kind: 'checking' }
  | { kind: 'failed'; message: string }
  | { kind: 'ready'; inspection: CipInspection }

/** The last reply or failure, and the service process it came from. */
export type CipOutcome =
  | { kind: 'ok'; generation: number; inspection: CipInspection }
  | { kind: 'failed'; generation: number; snapshot: DocumentView; message: string }

/**
 * Labels are shown only while a switch is on and the drawing stores something it covers, only
 * for the exact drawing the reply is about (the same snapshot rule as the agreement indicator),
 * and only from the service process that gave them; anything else is "checking" and draws
 * nothing. A failure is shown only for the drawing it was asked about and while no retry runs,
 * and then no label is shown. A reply stays shown while the same drawing is asked about again.
 * With a switch on and nothing stored of its kind the view says so (`nothing`): the control is
 * on, so "switch on ..." would be wrong.
 */
export function cipView(
  document: DocumentView,
  outcome: CipOutcome | null,
  generation: number,
  busy: boolean,
  atomLabels: boolean,
  bondLabels: boolean,
): CipView {
  if (!atomLabels && !bondLabels) return { kind: 'off' }
  if (!wantsCip(document, atomLabels, bondLabels)) {
    const stereo = stereoOf(document)
    const missing = [atomLabels && stereo.atoms.length === 0 ? 'atom' : null, bondLabels && stereo.bonds.length === 0 ? 'bond' : null].filter((kind) => kind !== null)
    return { kind: 'nothing', message: `No stored ${missing.join(' or ')} configurations to label.` }
  }
  if (outcome && outcome.generation === generation) {
    if (outcome.kind === 'ok' && samePropertySnapshot(document, outcome.inspection.snapshot)) return { kind: 'ready', inspection: outcome.inspection }
    if (outcome.kind === 'failed' && !busy && samePropertySnapshot(document, outcome.snapshot)) return { kind: 'failed', message: outcome.message }
  }
  return { kind: 'checking' }
}

/** The text drawn for one assigned item. `name` is its accessible name. */
export interface CipMark {
  item: ItemRef
  text: string
  name: string
}

/**
 * The labels to draw: an `assigned` center when the atom switch is on, an `assigned` double
 * bond when the bond switch is on, and nothing for `none`, `unspecified`, `unknown`,
 * `ambiguous` or `unsupported` (the panel says what those are).
 */
export function cipMarks(view: CipView, atomLabels: boolean, bondLabels: boolean): CipMark[] {
  if (view.kind !== 'ready') return []
  const marks: CipMark[] = []
  if (atomLabels) {
    for (const atom of view.inspection.atoms) {
      if (atom.state === 'assigned' && atom.descriptor !== null) marks.push(mark({ kind: 'atom', id: atom.atom }, atom.descriptor))
    }
  }
  if (bondLabels) {
    for (const bond of view.inspection.bonds) {
      if (bond.state === 'assigned' && bond.descriptor !== null) marks.push(mark({ kind: 'bond', id: bond.bond }, bond.descriptor))
    }
  }
  return marks
}

const mark = (item: ItemRef, descriptor: string): CipMark => ({
  item,
  text: `(${descriptor})`,
  name: `${itemName(item)}: CIP label ${descriptor}, of the stored configuration`,
})

/** The panel cell of one row: the label, and how the stored configuration relates to the drawing. */
export interface CipCell {
  /** `CIP: S`, `CIP: not supported - ...`, `CIP: not examined`. */
  text: string
  /** The sentence that shows the agreement next to the label; null where no label was examined. */
  relation: string | null
}

const RELATION: Record<AgreementRow['agreement'], (instance: string) => string> = {
  agrees: (instance) => `which agrees with the drawing in ${instance}`,
  disagrees: (instance) => `which disagrees with the drawing in ${instance}`,
  only_stored: (instance) => `which the drawing in ${instance} does not show`,
  only_drawn: (instance) => `which the drawing in ${instance} does not show as stored`,
  neither: (instance) => `and the drawing in ${instance} gives no configuration`,
}

const NOT_STEREOGENIC_WARNING =
  'A configuration is stored here but makes no difference: the unit is not stereogenic under the rules applied.'

/** The wording of one examined item. */
function itemText(item: CipAtom | CipBond): string {
  switch (item.state) {
    case 'assigned':
      return `CIP: ${item.descriptor ?? ''}`
    case 'none':
      return `CIP: none, the ligands are equivalent${item.storedNotStereogenic ? `. ${NOT_STEREOGENIC_WARNING}` : ''}`
    case 'unspecified':
      return 'CIP: unspecified (nothing is stored)'
    case 'unknown':
      return 'CIP: unknown (stored as either)'
    case 'ambiguous':
      return `CIP: ambiguous - ${item.reasonSentence ?? item.reason ?? ''}`
    case 'unsupported':
      return `CIP: not supported - ${item.reasonSentence ?? item.reason ?? ''}`
    default: {
      const unhandled: never = item.state
      return unhandled
    }
  }
}

/**
 * The cell for a panel row, or null: while labels are off or nothing is stored of the row's
 * kind, when the row's kind has its switch off (a row shows a label under the switch that
 * draws it), and for a row with nothing stored (a drawn-only row has no stored configuration to
 * label). A stored row the reply does not list shows "not examined": that is what the stored
 * scope leaves out, never "no label" (a valid reply for that scope lists every stored item, so
 * this is a robustness case).
 */
export function cipCell(row: AgreementRow, view: CipView, atomLabels: boolean, bondLabels: boolean): CipCell | null {
  if (row.stored === null || !(row.item.kind === 'atom' ? atomLabels : bondLabels)) return null
  switch (view.kind) {
    case 'off':
    case 'nothing':
      return null
    case 'checking':
      return { text: 'CIP: checking…', relation: null }
    case 'failed':
      return { text: 'CIP: unavailable', relation: null }
    case 'ready': {
      const listed: CipAtom | CipBond | undefined = row.item.kind === 'atom'
        ? view.inspection.atoms.find((item) => item.atom === row.item.id)
        : view.inspection.bonds.find((item) => item.bond === row.item.id)
      if (listed === undefined) return { text: 'CIP: not examined', relation: null }
      return { text: itemText(listed), relation: `of the stored configuration, ${RELATION[row.agreement](row.instance)} (${AGREEMENT_LABELS[row.agreement]})` }
    }
    default: {
      const unhandled: never = view
      return unhandled
    }
  }
}

/**
 * Where a label is drawn, in the placement's own coordinates: a point beside its atom, in the
 * widest gap between the bonds at the atom (opposite the one bond of a terminal atom, up and to
 * the right of an atom with none), or beside the middle of a double bond on the side that points
 * up. `around` is the positions of the bonded atoms for an atom, and of the two ends for a bond.
 * Placement is a Chemifree choice, not legacy behavior; it keeps a label off the bonds of an
 * ordinary drawing but a crowded one can still make labels touch each other.
 */
export function labelAt(kind: 'atom' | 'bond', anchor: Position, around: readonly Position[]): Position {
  if (kind === 'bond') {
    const [a, b] = around
    if (a === undefined || b === undefined) return { x: anchor.x, y: anchor.y - BOND_LABEL_DISTANCE }
    const length = Math.hypot(b.x - a.x, b.y - a.y) || 1
    // The normal that points up the screen (y grows downward); a vertical bond takes the right-hand one.
    let normal = { x: -(b.y - a.y) / length, y: (b.x - a.x) / length }
    if (normal.y > 1e-9 || (Math.abs(normal.y) <= 1e-9 && normal.x < 0)) normal = { x: -normal.x, y: -normal.y }
    return { x: anchor.x + normal.x * BOND_LABEL_DISTANCE, y: anchor.y + normal.y * BOND_LABEL_DISTANCE }
  }
  const angles = around
    .map((p) => Math.atan2(p.y - anchor.y, p.x - anchor.x))
    .sort((x, y) => x - y)
  if (angles.length === 0) return { x: anchor.x + FREE_ATOM_LABEL.x, y: anchor.y + FREE_ATOM_LABEL.y }
  let best = 0
  let from = 0
  for (const [index, angle] of angles.entries()) {
    const next = index + 1 < angles.length ? angles[index + 1] : angles[0] + 2 * Math.PI
    if (next - angle > best + 1e-9) {
      best = next - angle
      from = angle
    }
  }
  const middle = from + best / 2
  return { x: anchor.x + Math.cos(middle) * ATOM_LABEL_DISTANCE, y: anchor.y + Math.sin(middle) * ATOM_LABEL_DISTANCE }
}

/** Distance (pt) from an atom to its label: outside an agreement ring (radius 11). */
export const ATOM_LABEL_DISTANCE = 17
/** Distance (pt) from the middle of a double bond to its label. */
export const BOND_LABEL_DISTANCE = 11
/** Where the label of an atom with no bond goes, relative to it. */
export const FREE_ATOM_LABEL = { x: 12, y: -12 }
