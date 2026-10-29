/**
 * The scene panel's edits and the selection sent with every edit. Each builder takes the
 * document the request will apply to (the latest, after everything queued before it) and
 * returns null when the edit no longer applies, so quick presses are never refused.
 */
import {
  sceneObjects,
  type Command,
  type DeleteMode,
  type DocumentView,
  type Position,
  type SceneObjectView,
  type SelectionView,
  type Shape,
} from '@/lib/document-contract'

/** An object of the page's tree and how deep it is contained (0 for a root object). */
export interface SceneRow {
  object: SceneObjectView
  depth: number
}

/** The page's objects in tree order with their depth. */
export function sceneRows(document: DocumentView): SceneRow[] {
  const depth = new Map<string, number>()
  // Objects come depth first, so a group's depth is known before its children's.
  return sceneObjects(document).map((object) => {
    const level = depth.has(object.parent) ? (depth.get(object.parent) as number) + 1 : 0
    depth.set(object.id, level)
    return { object, depth: level }
  })
}

/** The selected objects that `document` still holds, in tree order. */
export function selectedObjects(document: DocumentView, selected: ReadonlySet<string>): SceneObjectView[] {
  return sceneObjects(document).filter((object) => selected.has(object.id))
}

/** `id` and everything it contains. */
function subtree(objects: ReadonlyMap<string, SceneObjectView>, id: string, into: Set<string>) {
  const stack = [id]
  while (stack.length > 0) {
    const current = stack.pop() as string
    if (into.has(current)) continue
    into.add(current)
    const object = objects.get(current)
    if (object?.kind === 'group') stack.push(...object.children)
  }
}

/** A selected object and its parent when a button was pressed. */
export interface PressedObject {
  id: string
  parent: string
}

/** The selected objects of `document` with their parents, in tree order, as a press saw them. */
export function pressedObjects(document: DocumentView, selected: ReadonlySet<string>): PressedObject[] {
  return selectedObjects(document, selected).map((object) => ({ id: object.id, parent: object.parent }))
}

/**
 * The group command for objects pressed together, built from the latest document: null unless
 * there are at least two, each still has the parent it had when pressed, they share it, and
 * they are not already exactly all the children of a group. So a press queued behind another
 * Group, or made again after it, does not nest the same objects once more.
 */
export function groupCommand(latest: DocumentView, pressed: readonly PressedObject[]): Command | null {
  if (pressed.length < 2) return null
  const objects = new Map(sceneObjects(latest).map((object) => [object.id, object]))
  const parent = pressed[0].parent
  if (pressed.some((item) => item.parent !== parent || objects.get(item.id)?.parent !== parent)) return null
  const container = objects.get(parent)
  if (container?.kind === 'group' && container.children.length === pressed.length) return null
  const ids = new Set(pressed.map((item) => item.id))
  return { op: 'group', objects: sceneObjects(latest).flatMap((object) => (ids.has(object.id) ? [object.id] : [])) }
}

/**
 * The live selection after the reply to an edit whose selection after (`sent`) named an object
 * the edit creates (`$N`, as a group does): the objects the reply selects, with their
 * identifiers, take the place of the live objects, and the live atoms and bonds stay; otherwise
 * null, keeping the live selection.
 */
export function selectionAfterReply(
  live: ReadonlySet<string>,
  sent: SelectionView | undefined,
  before: DocumentView,
  after: DocumentView,
): ReadonlySet<string> | null {
  if (!after.selection || !sent?.objects.some((id) => id.startsWith('$'))) return null
  const objects = new Set([...sceneObjects(before), ...sceneObjects(after)].map((object) => object.id))
  return new Set([...after.selection.objects, ...[...live].filter((id) => !objects.has(id))])
}

/** One ungroup command per selected group, or null when no group is selected. */
export function ungroupCommands(document: DocumentView, selected: ReadonlySet<string>): Command[] | null {
  const groups = selectedObjects(document, selected).filter((object) => object.kind === 'group')
  return groups.length > 0 ? groups.map((group) => ({ op: 'ungroup', group: group.id })) : null
}

/**
 * The delete command for the selected objects, or null when nothing selected remains or it
 * would delete every instance (the service keeps the last one).
 */
export function deleteObjectsCommand(document: DocumentView, selected: ReadonlySet<string>, mode: DeleteMode): Command | null {
  const objects = selectedObjects(document, selected)
  if (objects.length === 0) return null
  const all = new Map(sceneObjects(document).map((object) => [object.id, object]))
  const removed = new Set<string>()
  for (const object of objects) {
    if (mode === 'subtree') subtree(all, object.id, removed)
    else removed.add(object.id)
  }
  const instances = document.scene?.instances ?? []
  if (instances.every((instance) => removed.has(instance.id))) return null
  return { op: 'delete_objects', objects: objects.map((object) => object.id), mode }
}

/**
 * The move command for the selected objects, sent as selected: a group and its child may both
 * be listed, and the service moves every leaf once. Null when nothing selected remains.
 */
export function moveObjectsCommand(document: DocumentView, selected: ReadonlySet<string>, offset: Position): Command | null {
  const objects = selectedObjects(document, selected)
  return objects.length > 0 ? { op: 'move_objects', objects: objects.map((object) => object.id), offset } : null
}

/** A new graphic's size, and how far below the drawing it goes. */
export const GRAPHIC_SIZE = { width: 60, height: 30 }
const GRAPHIC_GAP = 40
const EMPTY_PAGE_AT: Position = { x: 40, y: 40 }

/**
 * A graphic of `shape` at a free spot: below everything drawn (every instance's atoms and every
 * graphic), at the drawing's left edge, so repeated presses step down the page.
 */
export function addGraphicCommand(document: DocumentView, shape: Shape): Command {
  const points: Position[] = [
    ...(document.scene?.instances.flatMap((instance) => instance.positions) ?? document.atoms.map((atom) => atom.position)),
    ...sceneObjects(document).flatMap((object) => (object.kind === 'graphic' ? object.points : [])),
  ]
  const at =
    points.length === 0
      ? EMPTY_PAGE_AT
      : { x: Math.min(...points.map((point) => point.x)), y: Math.max(...points.map((point) => point.y)) + GRAPHIC_GAP }
  const end = { x: at.x + GRAPHIC_SIZE.width, y: at.y + GRAPHIC_SIZE.height }
  const middle = at.y + GRAPHIC_SIZE.height / 2
  return {
    op: 'add_graphic',
    shape,
    points: shape === 'rectangle' ? [at, end] : [{ x: at.x, y: middle }, { x: end.x, y: middle }],
  }
}

/**
 * The selections sent with `commands`: the live one before the edit, less what `document` (the
 * document the edit applies to) no longer holds, and after it (see selectionAfter).
 */
export function selections(selected: ReadonlySet<string>, commands: Command[], document: DocumentView) {
  return { before: selectionAfter(selected, [], document), after: selectionAfter(selected, commands, document) }
}

/** Why a document is shown: an edit's reply, or a reply that replaces what was shown. */
export type ShownBy = 'edit' | 'open' | 'undo' | 'redo' | 'recover' | 'discard' | 'reload'

/**
 * The live selection once `next` is shown in place of the document `shownId`: an edit's reply
 * for the same document keeps the live selection; anything else (another document, undo, redo,
 * a recovered draft, a discarded one, a reload) brings the selection the reply holds.
 */
export function selectionOnShow(
  live: ReadonlySet<string>,
  shownId: string | undefined,
  next: DocumentView | null,
  by: ShownBy,
): ReadonlySet<string> {
  if (next && next.id === shownId && by === 'edit') return live
  return selectionSet(next?.selection)
}

/** What a row of the scene tree says about its object. */
export function objectText(object: SceneObjectView) {
  switch (object.kind) {
    case 'molecule':
      return 'placement'
    case 'group':
      return `group of ${object.children.length}`
    case 'graphic':
      return object.shape
    default: {
      const unhandled: never = object
      return unhandled
    }
  }
}

/** An object's accessible name: its ID, its kind, and the group containing it, if any. */
export function objectName(object: SceneObjectView) {
  const inGroup = object.parent.startsWith('s') ? `, in group ${object.parent}` : ''
  return `${object.id}, ${objectText(object)}${inGroup}`
}

/** The canvas's accessible name: the title and how many atoms, bonds, graphics, and groups. */
export function canvasName(document: DocumentView) {
  const count = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`
  const objects = sceneObjects(document)
  const graphics = objects.filter((object) => object.kind === 'graphic').length
  const groups = objects.filter((object) => object.kind === 'group').length
  const parts = [count(document.atoms.length, 'atom'), count(document.bonds.length, 'bond')]
  if (graphics > 0) parts.push(count(graphics, 'graphic'))
  if (groups > 0) parts.push(count(groups, 'group'))
  return `${document.title}: ${parts.join(', ')}`
}

/** Every identifier a selection names, as the editor keeps it. */
export function selectionSet(selection: SelectionView | undefined): Set<string> {
  return new Set(selection ? [...selection.objects, ...selection.atoms, ...selection.bonds] : [])
}

/**
 * The selection sent with `commands`: what is selected and still in `document`, less what the
 * commands delete (atoms with their bonds, bonds, instances, objects with their contents when
 * deleted as a subtree, ungrouped groups), so it names only what the edited document holds. A
 * group takes the place of the objects it contains, named `$N` by its command's index, since
 * its identifier is not known until the reply.
 */
export function selectionAfter(selected: ReadonlySet<string>, commands: Command[], document: DocumentView): SelectionView {
  const objects = new Map(sceneObjects(document).map((object) => [object.id, object]))
  const removed = new Set<string>()
  const created: string[] = []
  for (const [index, command] of commands.entries()) {
    switch (command.op) {
      case 'delete_atoms': {
        const atoms = new Set(command.atoms)
        command.atoms.forEach((id) => removed.add(id))
        for (const bond of document.bonds) {
          if (atoms.has(bond.begin) || atoms.has(bond.end)) removed.add(bond.id)
        }
        break
      }
      case 'delete_bonds':
        command.bonds.forEach((id) => removed.add(id))
        break
      case 'delete_instance':
        removed.add(command.instance)
        break
      case 'ungroup':
        removed.add(command.group)
        break
      case 'group':
        command.objects.forEach((id) => removed.add(id))
        created.push(`$${index}`)
        break
      case 'delete_objects':
        for (const id of command.objects) {
          if (command.mode === 'subtree') subtree(objects, id, removed)
          else removed.add(id)
        }
        break
      default:
        break
    }
  }
  const atoms = new Set(document.atoms.map((atom) => atom.id))
  const bonds = new Set(document.bonds.map((bond) => bond.id))
  const kept = [...selected].filter((id) => !removed.has(id))
  return {
    objects: [...kept.filter((id) => objects.has(id)), ...created],
    atoms: kept.filter((id) => atoms.has(id)),
    bonds: kept.filter((id) => bonds.has(id)),
  }
}
