import type { DocumentView, SelectionView } from '@/lib/document-contract'

/** Capture only IDs that exist on the document to which a queued edit will apply. */
export function selectionSnapshot(selection: ReadonlySet<string>, document: DocumentView): SelectionView {
  const selected = (ids: string[]) => ids.filter((id) => selection.has(id))
  return {
    objects: selected(document.scene?.objects?.map((object) => object.id) ?? []),
    atoms: selected(document.atoms.map((atom) => atom.id)),
    bonds: selected(document.bonds.map((bond) => bond.id)),
  }
}

export function selectionIds(selection: SelectionView | undefined): Set<string> {
  return new Set(selection ? [...selection.objects, ...selection.atoms, ...selection.bonds] : [])
}

/** A grouped selection replaces its members with the group created by command zero. */
export function selectionAfterGroup(before: SelectionView, grouped: readonly string[]): SelectionView {
  const members = new Set(grouped)
  return { ...before, objects: [...before.objects.filter((id) => !members.has(id)), '$0'] }
}

/** Merge the result into the live selection, preserving unrelated changes made while waiting. */
export function mergeGroupSelection(
  live: ReadonlySet<string>,
  grouped: readonly string[],
  before: DocumentView,
  after: DocumentView,
): Set<string> {
  const previous = new Set(before.scene?.objects?.map((object) => object.id) ?? [])
  const created = after.scene?.objects?.filter((object) => object.kind === 'group' && !previous.has(object.id)) ?? []
  if (created.length !== 1) throw new Error('The group response does not identify one new group.')
  const selection = new Set(live)
  for (const id of grouped) selection.delete(id)
  selection.add(created[0].id)
  return selectionIds(selectionSnapshot(selection, after))
}
