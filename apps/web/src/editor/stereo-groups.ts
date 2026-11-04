/**
 * What the editor makes of enhanced stereo groups (ABS, AND, OR): the words for each kind, the
 * table cell and group list built from a snapshot, the one-click actions with the reason one is
 * unavailable, the command batch each builds, the sentence for each refusal the service can
 * give, and where a group label sits on the canvas. Nothing here decides anything about
 * chemistry: a group states which stored centers belong together (docs/CONTRACT.md, Stereo
 * groups), the service enforces every rule, and these checks only keep the page from sending
 * what the contract says it will refuse. Everything is read from the snapshot's `stereo.groups`.
 *
 * No ratio is stated or implied for any kind, AND included. Groups are not drawn into exported
 * files, not compared with the drawing, and do not change CIP labels or identity.
 */
import { labelAt } from '@/editor/cip-labels'
import type { Box } from '@/editor/label'
import type { Command, DocumentView, Position } from '@/lib/document-contract'
import { stereoOf, type StereoGroup, type StereoGroupKind } from '@/lib/stereo-contract'

/** Said once above the group list; every sentence is true of this build (docs/CONTRACT.md). */
export const GROUPS_NOTE =
  'Groups are stored and edited here. They are not drawn into exported files yet (SMILES export reports a loss), not compared with the drawing, and do not change CIP labels or identity.'

/** What is outside the groups, so a reader does not take silence for support. */
export const GROUPS_NOT_EXAMINED =
  'Not examined or unsupported: double bonds cannot be grouped; centers stored unknown cannot be members; group meaning is ignored by CIP labels, identity and substructure search.'

/** The word for each kind. */
export const KIND_WORDS: Record<StereoGroupKind, string> = { absolute: 'absolute', and: 'AND', or: 'OR' }

/** What each kind states, in the CONTRACT's words. None states a ratio. */
export const KIND_MEANINGS: Record<StereoGroupKind, string> = {
  absolute: 'the members are as drawn.',
  and: 'the author means a mixture of the drawn set of members and its all-inverted counterpart. No ratio is stated or implied.',
  or: 'the members invert together or not at all.',
}

export const INT32_MIN = -2147483648
export const INT32_MAX = 2147483647

/** The group an atom belongs to in a snapshot, or undefined. */
export function groupOf(groups: readonly StereoGroup[], atom: string): StereoGroup | undefined {
  return groups.find((group) => group.members.includes(atom))
}

/** The text of a panel row's Group cell for an atom: its group, label and kind, or `no group`. */
export function groupCell(groups: readonly StereoGroup[], atom: string): string {
  const group = groupOf(groups, atom)
  return group ? `${group.id}: ${group.label} (${KIND_WORDS[group.kind]})` : 'no group'
}

/** The number a group states, as stated and as read: `absolute` takes none; an omitted one reads as 0. */
export function numberSentence(group: StereoGroup): string {
  if (group.kind === 'absolute') return 'no number (an absolute group takes none)'
  if (group.stored_number === null) return 'number omitted (reads as 0)'
  return `number ${group.stored_number}`
}

/** One entry of the group list under the table. */
export interface GroupRow {
  id: string
  label: string
  kind: StereoGroupKind
  kindWord: string
  meaning: string
  number: string
  /** The number input's starting text: the stored number, empty when none. */
  numberText: string
  count: number
  members: string[]
}

/** The group list, in group id order as the snapshot gives it. */
export function groupRows(document: DocumentView): GroupRow[] {
  return (stereoOf(document).groups ?? []).map((group) => ({
    id: group.id,
    label: group.label,
    kind: group.kind,
    kindWord: KIND_WORDS[group.kind],
    meaning: KIND_MEANINGS[group.kind],
    number: numberSentence(group),
    numberText: group.stored_number === null ? '' : String(group.stored_number),
    count: group.members.length,
    members: group.members,
  }))
}

/** What the number field holds. */
export type NumberParse =
  | { kind: 'none' }
  | { kind: 'integer'; value: number }
  | { kind: 'invalid'; why: string }

const NUMBER_RANGE = `a whole number from ${INT32_MIN} to ${INT32_MAX}`

/**
 * Reads the text of a number field: empty is no number; an integer in the 32-bit signed range
 * (0 and negatives included) is a number; anything else is refused with the reason, so the page
 * never sends what the wire would call malformed or out of range.
 */
export function parseGroupNumber(text: string): NumberParse {
  const trimmed = text.trim()
  if (trimmed === '') return { kind: 'none' }
  if (!/^-?\d+$/.test(trimmed)) return { kind: 'invalid', why: `A group number must be ${NUMBER_RANGE}, or empty for none.` }
  const value = BigInt(trimmed)
  if (value < BigInt(INT32_MIN) || value > BigInt(INT32_MAX)) {
    return { kind: 'invalid', why: `A group number must be ${NUMBER_RANGE}.` }
  }
  return { kind: 'integer', value: Number(value) === 0 ? 0 : Number(value) }
}

/** The one-click actions on the selected atoms. Deleting a group and setting its number are per group. */
export type GroupAction = 'make_absolute' | 'group_and' | 'group_or' | 'add_to_group' | 'remove_from_group' | 'merge' | 'split'

export const GROUP_ACTIONS: ReadonlyArray<{ action: GroupAction; label: string }> = [
  { action: 'make_absolute', label: 'Make absolute' },
  { action: 'group_and', label: 'Group as AND' },
  { action: 'group_or', label: 'Group as OR' },
  { action: 'add_to_group', label: 'Add to group' },
  { action: 'remove_from_group', label: 'Remove from group' },
  { action: 'merge', label: 'Merge groups' },
  { action: 'split', label: 'Split selected out' },
]

/** What a click asks for; the selection is read when the click is built. */
export type GroupRequest =
  | { action: GroupAction; numberText: string }
  | { action: 'delete_group'; group: string }
  | { action: 'set_number'; group: string; numberText: string }

/** The request's action, for the status sentence. */
export type PlannedAction = GroupRequest['action']

/** Whether a request can be sent now: the commands (one transaction), or why not. */
export type GroupPlan = { ok: true; action: PlannedAction; commands: Command[] } | { ok: false; why: string }

const refuse = (why: string): GroupPlan => ({ ok: false, why })
const joined = (ids: readonly string[]) => ids.join(', ')
const textOf = (group: StereoGroup) => `${group.id} (${group.label})`

/** The selected atoms that exist, in selection order. Bonds and drawn objects in the selection are ignored. */
export function selectedAtoms(document: DocumentView, selection: Iterable<string>): string[] {
  const atoms = new Set(document.atoms.map((atom) => atom.id))
  return [...selection].filter((id) => atoms.has(id))
}

/** The first selected atom that cannot be a member (no stored tetrahedral configuration), as a sentence; null when all can. */
function memberProblem(document: DocumentView, atoms: readonly string[]): string | null {
  const records = stereoOf(document).atoms
  for (const atom of atoms) {
    const record = records.find((candidate) => candidate.atom === atom)
    if (record === undefined) return `Every selected atom must have a stored tetrahedral configuration: ${atom} has none.`
    if (record.configuration.kind !== 'tetrahedral') {
      return `Every selected atom must have a stored tetrahedral configuration: ${atom} is stored unknown, which cannot be a member.`
    }
  }
  return null
}

/** The groups the atoms belong to, in group id order, and the atoms that are in none. */
function touched(groups: readonly StereoGroup[], atoms: readonly string[]) {
  const hit = groups.filter((group) => atoms.some((atom) => group.members.includes(atom)))
  const loose = atoms.filter((atom) => groupOf(groups, atom) === undefined)
  return { hit, loose }
}

/** The interpreted number of a new group of `kind` and the group that already has it, if any. */
function numberTaken(groups: readonly StereoGroup[], kind: StereoGroupKind, number: NumberParse): StereoGroup | undefined {
  if (kind === 'absolute') return undefined
  const read = number.kind === 'integer' ? number.value : 0
  return groups.find((group) => group.kind === kind && group.number === read)
}

const takenSentence = (kind: StereoGroupKind, number: NumberParse, other: StereoGroup) =>
  `${KIND_WORDS[kind]} group ${other.id} already has ${number.kind === 'integer' ? `number ${number.value}` : 'the number 0 (a group with no number reads as 0)'}. Choose another number, or select its members together with these atoms and use Add to group.`

/** A new group over the selected atoms: they must be stored tetrahedral centers in no group, and the kind's own rules must hold. */
function planCreate(
  document: DocumentView,
  groups: readonly StereoGroup[],
  atoms: readonly string[],
  hit: readonly StereoGroup[],
  kind: StereoGroupKind,
  request: { action: PlannedAction; numberText: string },
): GroupPlan {
  const number = parseGroupNumber(request.numberText)
  if (kind === 'absolute' && number.kind !== 'none') return refuse('An absolute group takes no number. Clear the number field.')
  if (number.kind === 'invalid') return refuse(number.why)
  const problem = memberProblem(document, atoms)
  if (problem) return refuse(problem)
  if (hit.length > 0) {
    const group = hit[0]
    const member = atoms.find((atom) => group.members.includes(atom))
    return refuse(`${member} is already in group ${textOf(group)}; an atom belongs to at most one group. Remove it from that group first, or use Add to group to add the other atoms to it.`)
  }
  const absolute = groups.find((group) => group.kind === 'absolute')
  if (kind === 'absolute' && absolute) return refuse(`The document already has an absolute group (${absolute.id}); there is at most one.`)
  const other = numberTaken(groups, kind, number)
  if (other) return refuse(takenSentence(kind, number, other))
  return { ok: true, action: request.action, commands: [{ op: 'create_stereo_group', kind, atoms: [...atoms], number: number.kind === 'integer' ? number.value : null }] }
}


/**
 * What a request would do against the latest snapshot and the atoms selected at the click: the
 * commands, in one transaction, or the reason it is not available. The same function decides
 * whether a control is enabled and builds what the click sends, from whatever document the queue
 * has reached, so a disabled control and a refused click can never disagree.
 */
export function planGroupRequest(document: DocumentView, selection: readonly string[], request: GroupRequest): GroupPlan {
  const groups = stereoOf(document).groups ?? []
  if (request.action === 'delete_group') {
    return groups.some((group) => group.id === request.group)
      ? { ok: true, action: request.action, commands: [{ op: 'delete_stereo_group', group: request.group }] }
      : refuse(`Group ${request.group} no longer exists.`)
  }
  if (request.action === 'set_number') {
    const group = groups.find((candidate) => candidate.id === request.group)
    if (!group) return refuse(`Group ${request.group} no longer exists.`)
    if (group.kind === 'absolute') return refuse('An absolute group takes no number.')
    const number = parseGroupNumber(request.numberText)
    if (number.kind === 'invalid') return refuse(number.why)
    const value = number.kind === 'integer' ? number.value : null
    if (value === group.stored_number) return refuse(value === null ? 'This group has no number.' : `The number is already ${value}.`)
    // A number another group of the kind has is not checked here: the service refuses it, and the refusal offers the merge.
    return { ok: true, action: request.action, commands: [{ op: 'set_stereo_group_number', group: group.id, number: value }] }
  }

  const atoms = selectedAtoms(document, selection)
  if (atoms.length === 0) return refuse('Select atoms with a stored tetrahedral configuration. Bonds in the selection are ignored.')
  const { hit, loose } = touched(groups, atoms)
  switch (request.action) {
    case 'make_absolute':
      return planCreate(document, groups, atoms, hit, 'absolute', request)
    case 'group_and':
      return planCreate(document, groups, atoms, hit, 'and', request)
    case 'group_or':
      return planCreate(document, groups, atoms, hit, 'or', request)
    case 'add_to_group': {
      const problem = memberProblem(document, atoms)
      if (problem) return refuse(problem)
      if (hit.length === 0) return refuse('Select a member of the group to add to, together with the atoms to add.')
      if (hit.length > 1) return refuse(`The selection holds members of ${hit.length} groups (${joined(hit.map((group) => group.id))}). Select members of one group to add to it, or use Merge groups to join two.`)
      if (loose.length === 0) return refuse(`Every selected atom is already in ${hit[0].id}; there is nothing to add.`)
      return { ok: true, action: request.action, commands: [{ op: 'add_stereo_group_members', group: hit[0].id, atoms: loose }] }
    }
    case 'remove_from_group': {
      if (loose.length > 0) return refuse(`${loose[0]} is in no group; select only group members to remove.`)
      for (const group of hit) {
        if (group.members.every((member) => atoms.includes(member))) {
          return refuse(`Removing every member of ${group.id} would leave it empty. Delete the group instead.`)
        }
      }
      return {
        ok: true,
        action: request.action,
        commands: hit.map((group) => ({ op: 'remove_stereo_group_members', group: group.id, atoms: atoms.filter((atom) => group.members.includes(atom)) })),
      }
    }
    case 'merge': {
      if (loose.length > 0) return refuse(`${loose[0]} is in no group; select only members of the two groups.`)
      if (hit.length !== 2) return refuse(`Merging takes members of exactly two groups; the selection holds ${hit.length === 1 ? 'one' : hit.length}.`)
      const [into, from] = hit
      if (into.kind !== from.kind) {
        return refuse(`Only groups of the same kind can be merged: ${into.id} is ${KIND_WORDS[into.kind]} and ${from.id} is ${KIND_WORDS[from.kind]}.`)
      }
      return { ok: true, action: request.action, commands: [{ op: 'merge_stereo_groups', into: into.id, from: from.id }] }
    }
    case 'split': {
      if (loose.length > 0) return refuse(`${loose[0]} is in no group; select only members of one group.`)
      if (hit.length !== 1) return refuse(`Splitting takes members of exactly one group; the selection holds ${hit.length}.`)
      const group = hit[0]
      if (group.kind === 'absolute') return refuse(`${group.id} is the absolute group, which cannot be split; remove members instead.`)
      if (group.members.every((member) => atoms.includes(member))) {
        return refuse(`Every member of ${group.id} is selected; a split moves some but not all members.`)
      }
      const number = parseGroupNumber(request.numberText)
      if (number.kind === 'invalid') return refuse(number.why)
      const other = numberTaken(groups, group.kind, number)
      if (other) return refuse(takenSentence(group.kind, number, other))
      return { ok: true, action: request.action, commands: [{ op: 'split_stereo_group', group: group.id, atoms, number: number.kind === 'integer' ? number.value : null }] }
    }
    default: {
      const unhandled: never = request
      return unhandled
    }
  }
}

/**
 * What a sent request did, in one sentence, from the snapshots before and after. A transaction
 * that changes what a group states also reports it in the reply's notes, which the page shows
 * as it shows the other stereo notes.
 */
export function groupStatus(before: DocumentView, after: DocumentView, request: GroupRequest): string {
  const was = stereoOf(before).groups ?? []
  const now = stereoOf(after).groups ?? []
  const made = now.filter((group) => !was.some((old) => old.id === group.id))
  switch (request.action) {
    case 'make_absolute':
    case 'group_and':
    case 'group_or':
    case 'split': {
      const group = made[0]
      return group ? `Created group ${textOf(group)} over ${joined(group.members)}.` : 'No group was created.'
    }
    case 'add_to_group':
      return 'Added the selected atoms to the group.'
    case 'remove_from_group':
      return 'Removed the selected atoms from their groups; they keep their stored configurations.'
    case 'merge': {
      const gone = was.filter((old) => !now.some((group) => group.id === old.id))
      return gone[0] ? `Merged ${gone[0].id} into the other group, which keeps its kind and number; ${gone[0].id} is gone.` : 'Nothing changed.'
    }
    case 'delete_group':
      return `Deleted group ${request.group}; its members keep their stored configurations.`
    case 'set_number': {
      const group = now.find((candidate) => candidate.id === request.group)
      return group ? `Group ${request.group} is now ${group.label} (${numberSentence(group)}).` : 'Nothing changed.'
    }
    default: {
      const unhandled: never = request
      return unhandled
    }
  }
}

/**
 * The sentence for each refusal the service can give for a group command (`invalid_command`
 * with one of these `reason` codes). Null for any other reason, which the page shows as the
 * service worded it. A refusal changes nothing.
 */
export const REFUSAL_SENTENCES: Record<string, string> = {
  stereo_group_unknown: 'That group no longer exists, so nothing was changed.',
  stereo_group_empty: 'A group needs at least one member, and a split must move some but not all of them. To remove every member, delete the group instead.',
  stereo_group_member_not_center: 'Only atoms with a stored tetrahedral configuration can be members. An atom with no configuration, or one stored unknown, cannot.',
  stereo_group_member_taken: 'An atom is already in a group, and an atom belongs to at most one group. Remove it from its group first.',
  stereo_group_member_absent: 'An atom is not a member of that group.',
  stereo_group_number_not_allowed: 'An absolute group takes no number. Clear the number first.',
  stereo_group_number_taken: 'Another group of this kind already has this number (a group with no number reads as 0). Choose a different number, or merge the two groups: select a member of each and press Merge groups.',
  stereo_group_abs_exists: 'The document already has an absolute group, and there is at most one.',
  stereo_group_same: 'A group cannot be merged into itself.',
  stereo_group_kind_mismatch: 'Only groups of the same kind can be merged.',
  stereo_group_split_abs: 'The absolute group cannot be split. Remove members instead.',
  stereo_group_number_range: `A group number must be ${NUMBER_RANGE}.`,
}

/** The sentence for a service refusal, or null when the reason is not a group reason. */
export function groupRefusalSentence(code: string, reason: string | undefined): string | null {
  if (code !== 'invalid_command' || reason === undefined || !Object.hasOwn(REFUSAL_SENTENCES, reason)) return null
  return `${REFUSAL_SENTENCES[reason]} Nothing was changed.`
}

/** The text drawn beside one group member, and its accessible name. */
export interface GroupMark {
  atom: string
  text: string
  name: string
}

/**
 * The labels the Group labels switch draws: `abs`, or the kind word and the interpreted number
 * (`OR 2`, `AND 0`, `OR -3`) beside each member. Product text, read from the snapshot; it
 * implies no ratio and is never part of the document or an export.
 */
export function groupMarks(document: DocumentView): GroupMark[] {
  return (stereoOf(document).groups ?? []).flatMap((group) => {
    const text = group.kind === 'absolute' ? 'abs' : `${KIND_WORDS[group.kind]} ${group.number ?? group.stored_number ?? 0}`
    return group.members.map((atom) => ({ atom, text, name: `Center ${atom}: member of group ${group.id}, ${text}` }))
  })
}

/** Rounding noise (pt) below which a label counts as level with its atom. */
const LEVEL_TOLERANCE = 1e-6

/** Vertical step (pt) between a CIP label and the group label stacked with it. */
export const GROUP_LABEL_STEP = 12

/**
 * Where a group label is drawn: at the point the CIP label of the same atom takes (`labelAt`,
 * the widest gap between the bonds), moved GROUP_LABEL_STEP pt further from the atom's level, up
 * when that point is above the atom and down otherwise (level counts as otherwise). So the two labels stack, never overlap,
 * and the position depends on nothing but the drawing: not on whether the CIP label is shown.
 */
export function groupLabelAt(anchor: Position, around: readonly Position[]): Position {
  const base = labelAt('atom', anchor, around)
  return { x: base.x, y: base.y + (base.y < anchor.y - LEVEL_TOLERANCE ? -GROUP_LABEL_STEP : GROUP_LABEL_STEP) }
}

/** Estimated advance (pt) of one character of a group label (10 pt semibold) and the halo around the text. */
const LABEL_CHAR_WIDTH = 6.5
const LABEL_HEIGHT = 14
const LABEL_HALO = 6

/**
 * The boxes the drawn group labels occupy in one placement, so the view box can include them:
 * each is centered on the point `groupLabelAt` gives, with an estimated width for its text (the
 * widest, `AND -2147483648`, is about 110 pt). Used only while the Group labels switch is on.
 */
export function groupLabelBoxes(
  marks: readonly GroupMark[],
  bonds: ReadonlyArray<{ begin: string; end: string }>,
  positions: ReadonlyMap<string, Position>,
): Box[] {
  return marks.flatMap((mark) => {
    const anchor = positions.get(mark.atom)
    if (anchor === undefined) return []
    const around = bonds
      .flatMap((bond) => (bond.begin === mark.atom ? [bond.end] : bond.end === mark.atom ? [bond.begin] : []))
      .flatMap((id) => positions.get(id) ?? [])
    const at = groupLabelAt(anchor, around)
    const width = mark.text.length * LABEL_CHAR_WIDTH + LABEL_HALO
    return [{ x: at.x - width / 2, y: at.y - LABEL_HEIGHT / 2, width, height: LABEL_HEIGHT }]
  })
}

/**
 * The text a group's number field holds after the stored number may have changed (an undo, a
 * redo, an edit elsewhere). A field that is not being edited follows the stored number; one the
 * person is typing in, with text that differs from what it last followed, keeps that text, so a
 * change that arrives mid-edit loses neither the text nor the focus.
 */
export function reseedDraft(draft: string, followed: string, stored: string, focused: boolean): string {
  if (stored === followed) return draft
  return focused && draft !== followed ? draft : stored
}
