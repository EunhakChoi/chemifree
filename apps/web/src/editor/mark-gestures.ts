/**
 * The commands a stereo-mark gesture makes, built from the document on screen: the bond (when
 * the gesture draws one), its mark, and the configuration the drawing then shows. One gesture is
 * one transaction, so it is one undo step. The configuration is stored with the mark because the
 * mark was drawn to say it; that replaces what was stored for the atom (the status line says so,
 * and undo restores it). Whether the service can read a configuration off the picture is its
 * answer to give: this module only decides when asking cannot succeed (the atom could never be a
 * center), and `sendMark` keeps the mark when the service refuses.
 */
import { drawBond, type BondEnd } from '@/editor/bond-gestures'
import { stillApplicable } from '@/editor/commands'
import { degrees } from '@/editor/geometry'
import {
  ApiError,
  type BondView,
  type Command,
  type DocumentResponse,
  type DocumentView,
} from '@/lib/document-contract'
import { MARK_NAMES, stereoOf, type StereoMark } from '@/lib/stereo-contract'

/** The marks the bond tool draws. Crossed is set only in the inspector. */
export type MarkKind = 'wedge' | 'hash' | 'wavy'

export const isMarkKind = (kind: string): kind is MarkKind => kind === 'wedge' || kind === 'hash' || kind === 'wavy'

/**
 * What a gesture sends: `commands` end with the derive command when one is attached, and
 * `markOnly` is the same without it (null when none is attached). `why` says in words why no
 * derive command is attached, when it is not. A gesture the editor declines on its own (the bond
 * cannot carry the mark, or already shows it) is only its text; nothing is sent.
 */
export type MarkGesture =
  | { kind: 'send'; commands: Command[]; markOnly: Command[] | null; why: string | null }
  | { kind: 'refused'; text: string }

/** Where a gesture reads positions: one scene instance (null: the service has none). */
export type Where = { instance: string | null } | { why: string }

/**
 * Where the inspector reads positions: the one placement, or none for a service without
 * placements. With several, which one a configuration is read from is the user's to choose by
 * drawing the mark on the canvas, so the mark is set and nothing is stored.
 */
export function inspectorWhere(document: DocumentView): Where {
  const instances = document.scene?.instances ?? []
  if (instances.length === 0) return { instance: null }
  if (instances.length === 1) return { instance: instances[0].id }
  return { why: 'the drawing has several placements; draw the mark on the canvas to store the configuration of the placement you draw in.' }
}

const bonded = (document: DocumentView, atom: string, extra: number) => (degrees(document).get(atom) ?? 0) + extra

/**
 * Why `atom` could not hold a stereocenter once the gesture has added `extra` bonds to it, or
 * null when it could: an element atom with three or four bonds (the service's rule).
 */
function notCenter(document: DocumentView, atom: string, extra: number): string | null {
  const found = document.atoms.find((candidate) => candidate.id === atom)
  if (found?.kind === 'query') return `${atom} is a query atom, which cannot hold a configuration.`
  const count = bonded(document, atom, extra)
  return count === 3 || count === 4 ? null : `${atom} has ${count} bond${count === 1 ? '' : 's'}; a stereocenter needs 3 or 4.`
}

/**
 * The gesture setting `mark` on a bond (the reference `bond`, an ID or `$N`) and, for the atoms
 * that could be centers (`ends`: atom ID or `$N`, with how many bonds the gesture adds to each),
 * storing the configuration the drawing shows.
 */
function assemble(
  document: DocumentView,
  where: Where,
  before: Command[],
  bond: string,
  mark: StereoMark,
  ends: Array<{ atom: string; extra: number }>,
): MarkGesture {
  const commands: Command[] = [...before, { op: 'set_bond_mark', bond, mark }]
  if ('why' in where) return { kind: 'send', commands, markOnly: null, why: where.why }
  const reasons = ends.map(({ atom, extra }) => (atom.startsWith('$') ? `${atom === '$0' ? 'the new atom' : 'a new atom'} has one bond; a stereocenter needs 3 or 4.` : notCenter(document, atom, extra)))
  const centers = ends.filter((_, index) => reasons[index] === null).map(({ atom }) => atom)
  if (centers.length === 0) {
    const why = reasons.find((reason) => reason !== null) ?? null
    return { kind: 'send', commands, markOnly: null, why: why && ends.length > 1 ? `neither end could be a stereocenter (${reasons.join(' ')})` : why }
  }
  const derive: Command = {
    op: 'set_configuration_from_drawing',
    atoms: centers,
    ...(where.instance === null ? {} : { instance: where.instance }),
  }
  return { kind: 'send', commands: [...commands, derive], markOnly: commands, why: null }
}

const bondBetween = (document: DocumentView, a: string, b: string) =>
  document.bonds.find((bond) => (bond.begin === a && bond.end === b) || (bond.begin === b && bond.end === a))

/** The text of a refusal to mark a bond that cannot carry the mark. */
function refusal(kind: StereoMark['kind'], bond: BondView): MarkGesture {
  const needs = kind === 'crossed' ? 'double' : 'single'
  return {
    kind: 'refused',
    text: `A ${MARK_NAMES[kind]} mark needs a ${needs} bond; ${bond.id} is ${bond.order === 'unknown' ? 'a bond of unknown kind' : `a ${bond.order} bond`}. Change it to ${needs} first.`,
  }
}

/**
 * Sets `mark` on `bond` (null clears it) and stores the configuration the drawing shows for
 * the atoms it bears on: the narrow end of a wedge or hashed wedge, the ends of a wavy bond, the
 * bond itself for a crossed mark; none of that for a clear.
 */
export function setMark(document: DocumentView, where: Where, bond: BondView, mark: StereoMark | null): MarkGesture {
  if (mark === null) return { kind: 'send', commands: [{ op: 'set_bond_mark', bond: bond.id, mark: null }], markOnly: null, why: null }
  const needs = mark.kind === 'crossed' ? 'double' : 'single'
  if (bond.order !== needs) return refusal(mark.kind, bond)
  if (mark.kind === 'crossed') {
    const commands: Command[] = [{ op: 'set_bond_mark', bond: bond.id, mark }]
    if ('why' in where) return { kind: 'send', commands, markOnly: null, why: where.why }
    const derive: Command = { op: 'set_configuration_from_drawing', bonds: [bond.id], ...(where.instance === null ? {} : { instance: where.instance }) }
    return { kind: 'send', commands: [...commands, derive], markOnly: commands, why: null }
  }
  const ends = 'narrow' in mark ? [mark.narrow === 'begin' ? bond.begin : bond.end] : [bond.begin, bond.end]
  return assemble(document, where, [], bond.id, mark, ends.map((atom) => ({ atom, extra: 0 })))
}

/**
 * What a bond tool click on a marked or plain single bond does with `kind` selected, in one
 * cycle: a wedge or hashed wedge goes plain, narrow end at the begin, narrow end at the end,
 * plain; the other of the two keeps its narrow end; wavy goes plain, wavy, plain, and a wedge or
 * hashed wedge becomes wavy.
 */
export function cycleMark(document: DocumentView, where: Where, bond: BondView, kind: MarkKind): MarkGesture {
  const current = bond.mark
  if (bond.order !== 'single') return refusal(kind, bond)
  if (kind === 'wavy') return setMark(document, where, bond, current?.kind === 'wavy' ? null : { kind: 'wavy' })
  if (current && 'narrow' in current) {
    if (current.kind !== kind) return setMark(document, where, bond, { kind, narrow: current.narrow })
    return setMark(document, where, bond, current.narrow === 'begin' ? { kind, narrow: 'end' } : null)
  }
  return setMark(document, where, bond, { kind, narrow: 'begin' })
}

/**
 * What drawing a mark over a bond that already shows exactly that mark does: nothing to change,
 * so nothing is sent and the text says so. The one thing such a drawing can still do is store
 * the configuration of a center that has none (the mark was set in the inspector, or the
 * service would not derive it then): that is how the user asks for it, so the gesture is sent
 * as it is. The service treats the unchanged mark as no change (no revision, no undo step).
 */
function alreadyDrawn(document: DocumentView, bond: BondView, gesture: MarkGesture): MarkGesture {
  if (gesture.kind === 'send' && gesture.markOnly !== null) {
    const derive = gesture.commands[gesture.commands.length - 1]
    const recorded = new Set(stereoOf(document).atoms.map((record) => record.atom))
    if (derive.op === 'set_configuration_from_drawing' && (derive.atoms ?? []).some((atom) => !recorded.has(atom))) return gesture
  }
  return { kind: 'refused', text: `${bond.id} is already drawn that way.` }
}

/**
 * The gesture of a drag, or a click on an atom or empty space, with a mark kind selected. The
 * bond begins where the gesture starts and that end is the narrow one of a wedge. Over an
 * existing single bond it marks that bond; over another kind of bond it is refused.
 */
export function markBondGesture(
  document: DocumentView,
  instance: string | null,
  from: BondEnd,
  to: BondEnd,
  kind: MarkKind,
): MarkGesture | null {
  const where = { instance }
  if ('atom' in from && 'atom' in to) {
    const existing = bondBetween(document, from.atom, to.atom)
    if (existing) {
      const narrow = existing.begin === from.atom ? 'begin' : 'end'
      const mark: StereoMark = kind === 'wavy' ? { kind } : { kind, narrow }
      const gesture = setMark(document, where, existing, mark)
      return JSON.stringify(existing.mark) === JSON.stringify(mark) ? alreadyDrawn(document, existing, gesture) : gesture
    }
  }
  const drawn = drawBond(document, instance, from, to, 'single')
  if (drawn.length === 0) return null
  // The command that makes the bond is the last; a new atom made first is `$0`.
  const bond = `$${drawn.length - 1}`
  const start = 'atom' in from ? { atom: from.atom, extra: 1 } : { atom: '$0', extra: 1 }
  const finish = 'atom' in to ? [{ atom: to.atom, extra: 1 }] : []
  const mark: StereoMark = kind === 'wavy' ? { kind } : { kind, narrow: 'begin' }
  return assemble(document, where, drawn, bond, mark, kind === 'wavy' ? [start, ...finish] : [start])
}

/**
 * What a mark gesture means, kept apart from the commands it makes: the commands are built from
 * the document the gesture finally runs on, not the one on screen when it was drawn, since an
 * edit queued before it (undo during a drag, say) can change a bond's order or an atom's bonds
 * without changing any ID.
 */
export type MarkIntent =
  | { kind: 'draw'; instance: string | null; from: BondEnd; to: BondEnd; mark: MarkKind }
  | { kind: 'cycle'; instance: string | null; bond: string; mark: MarkKind }

/**
 * The gesture of `intent` on `document` (the latest): null when it no longer applies (an atom
 * or bond it names is gone, a placement is gone, or it draws nothing). A refusal or a
 * declined gesture is worded from this document too.
 */
export function buildMark(document: DocumentView, intent: MarkIntent): MarkGesture | null {
  let gesture: MarkGesture | null
  if (intent.kind === 'draw') {
    gesture = markBondGesture(document, intent.instance, intent.from, intent.to, intent.mark)
  } else {
    const bond = document.bonds.find((candidate) => candidate.id === intent.bond)
    gesture = bond ? cycleMark(document, { instance: intent.instance }, bond, intent.mark) : null
  }
  if (gesture?.kind === 'send' && stillApplicable(gesture.commands, document) === null) return null
  return gesture
}

/** The part of a service refusal's message after its "command N: " prefix. */
const refusalText = (failure: ApiError) => failure.message.replace(/^command \d+: /, '')

/** What became of the derive command of a gesture, for the status line. */
export type DeriveOutcome =
  | { kind: 'none'; why: string | null }
  | { kind: 'sent' }
  | { kind: 'refused'; reason: string; message: string }

/**
 * The reasons the derive command is refused with: it cannot read a configuration off the
 * drawing (`stereo_not_derivable_*`), or the atom or bond cannot hold one. The reasons of
 * `set_bond_mark` (`stereo_mark_bond_order`) and of a stored record that is malformed are not
 * among them.
 */
const DERIVE_REASONS = new Set([
  'stereo_center_unsupported',
  'stereo_center_neighbors',
  'stereo_double_bond_required',
  'stereo_reference_invalid',
  'stereo_geometry_unsupported',
])
const isDeriveReason = (reason: string | undefined) =>
  reason !== undefined && (reason.startsWith('stereo_not_derivable_') || DERIVE_REASONS.has(reason))

/**
 * Sends a mark gesture with `transact`. When the service refuses the derive command (the one
 * after the mark commands, with a reason that command gives), the mark is sent alone: the
 * refused transaction changed nothing, so the mark is kept and the reason goes to the status
 * line. Any other failure is the caller's, and so is a failure of the second send: no reason is
 * returned then, since the mark was not stored.
 */
export async function sendMark(
  transact: (commands: Command[]) => Promise<DocumentResponse>,
  gesture: Extract<MarkGesture, { kind: 'send' }>,
): Promise<{ response: DocumentResponse; outcome: DeriveOutcome }> {
  if (gesture.markOnly === null) return { response: await transact(gesture.commands), outcome: { kind: 'none', why: gesture.why } }
  try {
    return { response: await transact(gesture.commands), outcome: { kind: 'sent' } }
  } catch (failure) {
    const refused =
      failure instanceof ApiError &&
      failure.code === 'invalid_command' &&
      failure.commandIndex === gesture.markOnly.length &&
      isDeriveReason(failure.stateReason)
    if (!(failure instanceof ApiError) || !refused) throw failure
    return {
      response: await transact(gesture.markOnly),
      outcome: { kind: 'refused', reason: failure.stateReason ?? '', message: refusalText(failure) },
    }
  }
}
