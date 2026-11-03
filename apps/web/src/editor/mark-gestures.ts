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
import { degrees } from '@/editor/geometry'
import {
  ApiError,
  type BondView,
  type Command,
  type DocumentResponse,
  type DocumentView,
} from '@/lib/document-contract'
import { MARK_NAMES, type StereoMark } from '@/lib/stereo-contract'

/** The marks the bond tool draws. Crossed is set only in the inspector. */
export type MarkKind = 'wedge' | 'hash' | 'wavy'

export const isMarkKind = (kind: string): kind is MarkKind => kind === 'wedge' || kind === 'hash' || kind === 'wavy'

/**
 * What a gesture sends: `commands` end with the derive command when one is attached, and
 * `markOnly` is the same without it (null when none is attached). `why` says in words why no
 * derive command is attached, when it is not. A gesture the editor refuses on its own is only
 * its text; nothing is sent.
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
      return setMark(document, where, existing, kind === 'wavy' ? { kind } : { kind, narrow })
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

/** The part of a service refusal's message after its "command N: " prefix. */
const refusalText = (failure: ApiError) => failure.message.replace(/^command \d+: /, '')

/** What became of the derive command of a gesture, for the status line. */
export type DeriveOutcome =
  | { kind: 'none'; why: string | null }
  | { kind: 'sent' }
  | { kind: 'refused'; reason: string; message: string }

/**
 * Sends a mark gesture with `transact`. When the service refuses its last command, the
 * configuration (a `stereo_*` reason), the mark is sent alone: the refused transaction changed
 * nothing, so the mark is kept and the reason goes to the status line. Any other failure is the
 * caller's.
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
      failure.commandIndex === gesture.commands.length - 1 &&
      failure.stateReason?.startsWith('stereo_') === true
    if (!(failure instanceof ApiError) || !refused) throw failure
    return {
      response: await transact(gesture.markOnly),
      outcome: { kind: 'refused', reason: failure.stateReason ?? '', message: refusalText(failure) },
    }
  }
}
