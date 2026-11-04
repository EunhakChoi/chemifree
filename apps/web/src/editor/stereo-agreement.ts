/**
 * What the editor makes of a `chem.stereo` reply: when the reply may be shown, the rows and
 * sentences that say how each stored configuration relates to the drawing, the one-click edits
 * that resolve a row, and the scheduler that decides when to ask. Nothing here compares a
 * configuration with a drawing: the service does, and these functions read its answer. Nothing
 * is stored unless the user presses a row's button.
 */
import { stored, turnWords } from '@/editor/stereo-status'
import {
  samePropertySnapshot,
  type Command,
  type DocumentView,
  type StereoInspection,
} from '@/lib/document-contract'
import {
  holdsStereo,
  type Agreement,
  type AtomConfiguration,
  type BondConfiguration,
  type DrawnReason,
  type StereoDiagnostic,
} from '@/lib/stereo-contract'

/** Said wherever the check's reach is stated. A stored record is compared; nothing is judged. The CIP sentence is true whether or not labels are switched on (editor/cip-labels.ts). */
export const NOT_JUDGED =
  'Only stored configurations are compared with the drawing. CIP labels describe the stored configuration, not the drawing.'

/** Said where a drawing holds nothing to compare: not "no stereo", for nothing is assessed. */
export const NOTHING_TO_COMPARE =
  'No wedge, hashed wedge or wavy bond is drawn and no configuration is stored. Whether an atom is a stereocenter is not assessed.'

export interface ItemRef {
  kind: 'atom' | 'bond'
  id: string
}

export const itemName = (item: ItemRef) => `${item.kind === 'atom' ? 'Center' : 'Double bond'} ${item.id}`

/** How one stored item and one instance's drawing relate. */
export interface AgreementRow {
  key: string
  item: ItemRef
  instance: string
  agreement: Agreement
  stored: AtomConfiguration | BondConfiguration | null
  drawn: AtomConfiguration | BondConfiguration | null
  reason: DrawnReason | null
  labelDiffers: boolean
}

/** One row per item and inspected instance, atoms first, as the service lists them. */
export function agreementRows(inspection: StereoInspection): AgreementRow[] {
  const rows: AgreementRow[] = []
  for (const entry of inspection.atoms) {
    for (const drawn of entry.drawn) {
      rows.push({
        key: `atom:${entry.atom}:${drawn.instance}`,
        item: { kind: 'atom', id: entry.atom },
        instance: drawn.instance,
        agreement: drawn.agreement,
        stored: entry.stored,
        drawn: drawn.configuration,
        reason: drawn.reason,
        labelDiffers: drawn.label_differs,
      })
    }
  }
  for (const entry of inspection.bonds) {
    for (const drawn of entry.drawn) {
      rows.push({
        key: `bond:${entry.bond}:${drawn.instance}`,
        item: { kind: 'bond', id: entry.bond },
        instance: drawn.instance,
        agreement: drawn.agreement,
        stored: entry.stored,
        drawn: drawn.configuration,
        reason: drawn.reason,
        labelDiffers: false,
      })
    }
  }
  return rows
}

/** What the drawing cannot say, in words. The code follows verbatim. */
const REASON_WORDS: Record<DrawnReason, string> = {
  stereo_not_derivable_no_marks: 'no wedge, hashed wedge or wavy bond bears on it',
  stereo_not_derivable_conflicting_marks: 'a wavy bond and a wedge or hashed wedge bear on the same center',
  stereo_not_derivable_degenerate: 'the drawing is too flat to determine a configuration',
  stereo_not_derivable_coincident_atoms: 'two bonded atoms are drawn on top of each other',
  stereo_not_derivable_fourth_position: 'the fourth position cannot be named from the hydrogens and valence of the atom',
  stereo_not_derivable_no_reference: 'a double-bond end has no neighbor to read a direction from',
  stereo_center_unsupported: 'the model cannot hold a configuration at this atom',
  stereo_center_neighbors: 'the atom does not have three or four neighbors',
  stereo_double_bond_required: 'only a double bond holds a configuration of this kind',
  stereo_reference_invalid: 'a double-bond reference is not valid here',
  stereo_geometry_unsupported: 'the model cannot read this double bond',
}

const reasonText = (reason: DrawnReason) => `${REASON_WORDS[reason]} (${reason})`

/** One sentence for a row: what is stored, what the drawing shows. */
export function rowSentence(row: AgreementRow): string {
  const stored = row.stored && turnWords(row.stored)
  const drawn = row.drawn && turnWords(row.drawn)
  const where = `in ${row.instance}`
  switch (row.agreement) {
    case 'agrees':
      return `Stored ${stored}; the drawing ${where} shows the same${row.labelDiffers ? ' (the hydrogen or lone pair is named differently)' : ''}.`
    case 'disagrees':
      return `Stored ${stored}; the drawing ${where} shows ${drawn}.`
    case 'only_drawn':
      return `The drawing ${where} shows ${drawn}; nothing is stored.`
    case 'only_stored':
      return `Stored ${stored}; the drawing ${where} shows nothing usable: ${row.reason ? reasonText(row.reason) : 'no reason given'}.`
    case 'neither':
      return `Nothing is stored and the drawing ${where} gives no configuration: ${row.reason ? reasonText(row.reason) : 'no reason given'}.`
    default: {
      const unhandled: never = row.agreement
      return unhandled
    }
  }
}

export const AGREEMENT_LABELS: Record<Agreement, string> = {
  agrees: 'agrees',
  disagrees: 'disagrees',
  only_stored: 'stored only',
  only_drawn: 'drawn only',
  neither: 'unreadable',
}

/** What one click on a row does; null where nothing can be done from the row. */
export type RowAction = 'update' | 'store' | 'remove'

export function rowAction(agreement: Agreement): RowAction | null {
  switch (agreement) {
    case 'disagrees':
      return 'update'
    case 'only_drawn':
      return 'store'
    case 'only_stored':
      return 'remove'
    case 'agrees':
    case 'neither':
      return null
    default: {
      const unhandled: never = agreement
      return unhandled
    }
  }
}

export const ACTION_LABELS: Record<RowAction, string> = {
  update: 'Update from drawing',
  store: 'Store from drawing',
  remove: 'Remove stored',
}

/** The name a row's button announces: the action, the item, and the instance it reads. */
export function actionName(row: AgreementRow, action: RowAction): string {
  const id = row.item.id
  switch (action) {
    case 'update':
      return `Update ${id} in ${row.instance} from the drawing`
    case 'store':
      return `Store ${id} in ${row.instance} from the drawing`
    case 'remove':
      return `Remove the stored configuration of ${id} (row ${row.instance})`
    default: {
      const unhandled: never = action
      return unhandled
    }
  }
}

/** Rows "Update all from drawing" would replace: a single instance and two or more disagreements. */
export function updatableRows(inspection: StereoInspection, rows: readonly AgreementRow[]): AgreementRow[] {
  const updatable = rows.filter((row) => row.agreement === 'disagrees')
  return inspection.instances.length === 1 && updatable.length >= 2 ? updatable : []
}

const count = (n: number, noun: string) => `${n} ${noun}${n === 1 ? '' : 's'}`

/**
 * The comparison in one sentence: how many rows agree, disagree and so on, or why there is none.
 * The notice above it already says that whether an atom is a stereocenter is not assessed; where
 * there is nothing to compare, this is the only sentence, so it says so itself.
 */
export function summarySentence(rows: readonly AgreementRow[]): string {
  if (rows.length === 0) {
    return 'Nothing stored or drawn bears on a configuration, so there is nothing to compare. Whether an atom is a stereocenter is not assessed.'
  }
  const tally = (agreement: Agreement) => rows.filter((row) => row.agreement === agreement).length
  const parts: string[] = []
  const add = (n: number, one: string, many: string) => {
    if (n > 0) parts.push(n === 1 ? `1 ${one}` : `${n} ${many}`)
  }
  add(tally('disagrees'), 'disagrees with the drawing', 'disagree with the drawing')
  add(tally('agrees'), 'agrees', 'agree')
  add(tally('only_stored'), 'is stored only', 'are stored only')
  add(tally('only_drawn'), 'is drawn only', 'are drawn only')
  add(tally('neither'), 'is marked but unreadable', 'are marked but unreadable')
  return `Compared with the drawing: ${parts.join(', ')}.`
}

/** The sentence said while a reply is awaited or has not been asked for. */
export const CHECKING = 'Checking the drawing…'
export const UNCHECKED = 'Not compared with the drawing yet. Press Check now.'

/** The diagnostics a panel lists: known codes first, in the service's order, the rest after; the fixed sentence replaces the last. */
const KNOWN_CODES = [
  'drawing_disagrees',
  'drawing_not_derivable',
  'marks_without_configuration',
  'configuration_without_marks',
  'mark_narrow_end_not_a_center',
  'center_has_aromatic_bond',
  'double_bond_substituents_same_side',
  'wavy_between_possible_centers',
  'drawing_weak_evidence',
]

export function listedDiagnostics(diagnostics: readonly StereoDiagnostic[]): StereoDiagnostic[] {
  const shown = diagnostics.filter((item) => item.code !== 'stereo_not_checked')
  const rank = (item: StereoDiagnostic) => {
    const at = KNOWN_CODES.indexOf(item.code)
    return at < 0 ? KNOWN_CODES.length : at
  }
  return shown.map((item, index) => ({ item, index })).sort((a, b) => rank(a.item) - rank(b.item) || a.index - b.index).map(({ item }) => item)
}

/** The sentence a view says about itself: also the summary line's dynamic part. */
export function viewSentence(view: AgreementView): string | null {
  switch (view.kind) {
    case 'none':
      return null
    case 'checking':
      return CHECKING
    case 'unchecked':
      return UNCHECKED
    case 'failed':
      return 'The comparison with the drawing could not be made.'
    case 'ready':
      return summarySentence(agreementRows(view.inspection))
    default: {
      const unhandled: never = view
      return unhandled
    }
  }
}

/** What the page may say about the drawing's stereo against what is stored, for this document. */
export type AgreementView =
  | { kind: 'none' }
  | { kind: 'checking' }
  | { kind: 'unchecked' }
  | { kind: 'failed'; message: string }
  | { kind: 'ready'; inspection: StereoInspection }

/** The last reply or failure, and the service process it came from. */
export type CheckOutcome =
  | { kind: 'ok'; generation: number; inspection: StereoInspection }
  | { kind: 'failed'; generation: number; snapshot: DocumentView; message: string }

/**
 * A reply is shown only for the exact drawing on screen, positions included, and the service
 * process that gave it; anything else is "checking" (or "not compared" when the user turned
 * checking off), and nothing is drawn on the canvas for it. A failure is shown only for the
 * drawing it was asked about, and only while no retry is running; a reply stays shown while the
 * same drawing is checked again.
 */
export function agreementView(
  document: DocumentView,
  outcome: CheckOutcome | null,
  generation: number,
  busy: boolean,
  automatic: boolean,
): AgreementView {
  if (!holdsStereo(document)) return { kind: 'none' }
  if (outcome && outcome.generation === generation) {
    if (outcome.kind === 'ok' && samePropertySnapshot(document, outcome.inspection.snapshot)) return { kind: 'ready', inspection: outcome.inspection }
    // A failure stays shown until a retry is running; then the view says so, which also takes the retry button away.
    if (outcome.kind === 'failed' && !busy && samePropertySnapshot(document, outcome.snapshot)) return { kind: 'failed', message: outcome.message }
  }
  return busy || automatic ? { kind: 'checking' } : { kind: 'unchecked' }
}

/** The ring a row that does not agree gets on the canvas. */
export interface AgreementRing {
  item: ItemRef
  instance: string
  agreement: Exclude<Agreement, 'agrees'>
  name: string
}

const RING_WORDS: Record<Exclude<Agreement, 'agrees'>, (instance: string) => string> = {
  disagrees: (instance) => `the stored configuration disagrees with the drawing in ${instance}`,
  only_drawn: (instance) => `the marks show a configuration in ${instance} that is not stored`,
  only_stored: (instance) => `a stored configuration that the drawing in ${instance} does not show`,
  neither: (instance) => `marked, but the drawing in ${instance} gives no configuration`,
}

export function agreementRings(rows: readonly AgreementRow[]): AgreementRing[] {
  const rings: AgreementRing[] = []
  for (const row of rows) {
    if (row.agreement === 'agrees') continue
    rings.push({
      item: row.item,
      instance: row.instance,
      agreement: row.agreement,
      name: `${itemName(row.item)}: ${RING_WORDS[row.agreement](row.instance)}`,
    })
  }
  return rings
}

/** A click on a row's button, or on "Update all": what the user asked, not yet built into commands. */
export type Resolution =
  | { kind: RowAction; item: ItemRef; instance: string }
  | { kind: 'update_all'; items: ItemRef[]; instance: string }

const exists = (document: DocumentView, item: ItemRef) =>
  (item.kind === 'atom' ? document.atoms : document.bonds).some((candidate) => candidate.id === item.id)

/**
 * The commands for a click, built from the latest document (queued edits can have changed it):
 * null, so that nothing is sent, when an item or the instance it reads is gone or a stored
 * record to remove is. One transaction, so one undo step.
 */
export function resolutionCommands(document: DocumentView, resolution: Resolution): Command[] | null {
  const items = resolution.kind === 'update_all' ? resolution.items : [resolution.item]
  if (!items.every((item) => exists(document, item))) return null
  if (resolution.kind === 'remove') {
    const { item } = resolution
    if (stored(document, item.id) === undefined) return null
    return [item.kind === 'atom'
      ? { op: 'set_atom_configuration', atom: item.id, configuration: null }
      : { op: 'set_bond_configuration', bond: item.id, configuration: null }]
  }
  if (!(document.scene?.instances.some((instance) => instance.id === resolution.instance) ?? false)) return null
  const atoms = items.filter((item) => item.kind === 'atom').map((item) => item.id)
  const bonds = items.filter((item) => item.kind === 'bond').map((item) => item.id)
  return [{
    op: 'set_configuration_from_drawing',
    ...(atoms.length > 0 ? { atoms } : {}),
    ...(bonds.length > 0 ? { bonds } : {}),
    instance: resolution.instance,
  }]
}

const MOST_LISTED = 6

/**
 * What a click did, naming the stated value it replaced or removed (the reply's notes list only
 * what a neighbor change cleared, so this sentence and undo are the record), or null when the
 * configuration did not change.
 */
export function resolutionStatus(before: DocumentView, after: DocumentView, resolution: Resolution): string | null {
  if (resolution.kind === 'update_all') {
    const changed = resolution.items.filter((item) => JSON.stringify(stored(before, item.id)) !== JSON.stringify(stored(after, item.id)))
    if (changed.length === 0) return `Nothing changed: the stored configurations already matched the drawing in ${resolution.instance}.`
    const shown = changed.slice(0, MOST_LISTED).map((item) => {
      const was = stored(before, item.id)
      const now = stored(after, item.id)
      return `${item.id} (${was ? turnWords(was) : 'nothing'} to ${now ? turnWords(now) : 'nothing'})`
    })
    const more = changed.length > shown.length ? ` and ${changed.length - shown.length} more` : ''
    return `Replaced ${count(changed.length, 'stored configuration')} with what the drawing in ${resolution.instance} shows: ${shown.join(', ')}${more}. Undo restores them.`
  }
  const { id } = resolution.item
  const was = stored(before, id)
  const now = stored(after, id)
  if (resolution.kind === 'remove') {
    return was === undefined || now !== undefined ? null : `${id}: removed the stored ${turnWords(was)}. Undo restores it.`
  }
  if (now === undefined) return null
  if (was === undefined) return `${id}: stored ${turnWords(now)} from the drawing in ${resolution.instance}.`
  if (JSON.stringify(was) === JSON.stringify(now)) {
    return `${id}: the stored ${turnWords(now)} already matched the drawing in ${resolution.instance}; nothing changed.`
  }
  return `${id}: replaced stored ${turnWords(was)} with ${turnWords(now)} from the drawing in ${resolution.instance}. Undo restores it.`
}

/** Timers the scheduler uses; a test passes its own clock. */
export interface Timers {
  set: (task: () => void, milliseconds: number) => unknown
  clear: (handle: unknown) => void
}

/** How long a change waits for another before the drawing is asked about. */
export const CHECK_DELAY_MS = 300

/**
 * Decides when to ask the service. A change starts (or restarts) a wait of `delay`; at its end
 * one request runs, unless the user is dragging, in which case it waits for the release. One
 * request is in flight at a time, and any number of changes meanwhile cause exactly one more
 * when it ends. `now` asks without waiting. `stop` forgets a change that has not started.
 * `check` reports its own failure and never rejects; if it does anyway, the scheduler carries on
 * and does not raise it a second time.
 */
export class CheckScheduler {
  private readonly check: () => Promise<void>
  private readonly timers: Timers
  private readonly delay: number
  private timer: unknown = null
  private running = false
  private again = false
  private immediate = false
  private due = false
  private paused = false

  constructor(check: () => Promise<void>, timers: Timers, delay: number = CHECK_DELAY_MS) {
    this.check = check
    this.timers = timers
    this.delay = delay
  }

  /** The drawing changed, or checking was turned on. */
  changed() {
    if (this.running) {
      this.again = true
      return
    }
    this.due = true
    this.wait(this.delay)
  }

  /** Asks now (a request already in flight is followed by one more at once). */
  now() {
    if (this.running) {
      this.again = true
      this.immediate = true
      return
    }
    this.due = true
    this.wait(0)
  }

  /** While paused no request starts; a change that comes due meanwhile waits for the end. */
  pause(paused: boolean) {
    this.paused = paused
    if (paused) this.clearTimer()
    else if (this.due && !this.running) this.wait(this.delay)
  }

  /** Drops what has not started. A request in flight is left to finish. */
  stop() {
    this.clearTimer()
    this.due = false
    this.again = false
    this.immediate = false
  }

  private clearTimer() {
    if (this.timer !== null) this.timers.clear(this.timer)
    this.timer = null
  }

  private wait(milliseconds: number) {
    this.clearTimer()
    if (this.paused) return
    this.timer = this.timers.set(() => {
      this.timer = null
      this.start()
    }, milliseconds)
  }

  private start() {
    if (this.paused || this.running) return
    this.due = false
    this.running = true
    this.again = false
    const finish = () => {
      this.running = false
      if (!this.again) return
      this.again = false
      const wait = this.immediate ? 0 : this.delay
      this.immediate = false
      this.due = true
      this.wait(wait)
    }
    // The check owns its error reporting, so a rejection is not rethrown here (that would be an unhandled one).
    this.check().then(finish, finish)
  }
}
