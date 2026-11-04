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
import { samePropertySnapshot, type CipInspection, type DocumentView } from '@/lib/document-contract'
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
  | { kind: 'off' }
  | { kind: 'checking' }
  | { kind: 'failed'; message: string }
  | { kind: 'ready'; inspection: CipInspection }

/** The last reply or failure, and the service process it came from. */
export type CipOutcome =
  | { kind: 'ok'; generation: number; inspection: CipInspection }
  | { kind: 'failed'; generation: number; snapshot: DocumentView; message: string }

/**
 * Labels are shown only while a switch is on, only for the exact drawing the reply is about
 * (the same snapshot rule as the agreement indicator), and only from the service process that
 * gave them; anything else is "checking" and draws nothing. A failure is shown only for the
 * drawing it was asked about and while no retry runs, and then no label is shown. A reply stays
 * shown while the same drawing is asked about again.
 */
export function cipView(document: DocumentView, outcome: CipOutcome | null, generation: number, busy: boolean, wanted: boolean): CipView {
  if (!wanted) return { kind: 'off' }
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
 * The cell for a panel row, or null while labels are off. An item the reply does not list shows
 * "not examined": that is what the stored scope leaves out, never "no label".
 */
export function cipCell(row: AgreementRow, view: CipView): CipCell | null {
  switch (view.kind) {
    case 'off':
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

/** What was last asked about, for deciding whether a change needs a new request. */
export interface CipAsked {
  document: DocumentView
  generation: number
  wanted: boolean
}

/**
 * What a change of source means for the scheduler: `stop` when labels are not wanted (nothing
 * is asked, and a wait that has not started is dropped), `keep` for a repeat of what was
 * already asked about (a reread, say), else `ask`.
 */
export function cipAsk(before: CipAsked | null, next: CipAsked): 'stop' | 'keep' | 'ask' {
  if (!next.wanted) return 'stop'
  const repeat = before !== null && before.wanted && before.generation === next.generation && samePropertySnapshot(before.document, next.document)
  return repeat ? 'keep' : 'ask'
}
