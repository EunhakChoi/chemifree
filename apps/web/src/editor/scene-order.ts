import type { Command, DocumentView } from '@/lib/document-contract'

export interface SceneOrderCapture {
  documentId: string
  object: string
  parent: string | null
}

export type SceneOrderBoundary = 'first' | 'last'

/** Like Earlier/Later, order the single selected scene object without targeting graph IDs. */
export function captureSceneOrder(document: DocumentView, selected: ReadonlySet<string>): SceneOrderCapture | null {
  const objects = document.scene?.objects?.filter((object) => selected.has(object.id)) ?? []
  if (objects.length !== 1) return null
  return { documentId: document.id, object: objects[0].id, parent: objects[0].parent }
}

/** Keep the pressed parent, but compute its boundary in the sibling list reached by the queue. */
export function sceneOrderCommands(document: DocumentView, captured: SceneOrderCapture, boundary: SceneOrderBoundary): Command[] | null {
  if (document.id !== captured.documentId) return null
  const object = document.scene?.objects?.find((item) => item.id === captured.object)
  if (!object || object.parent !== captured.parent) return null
  const parent = captured.parent === null ? undefined : document.scene?.objects?.find((item) => item.id === captured.parent)
  const siblings = captured.parent === null ? document.scene?.roots : parent?.kind === 'group' ? parent.children : undefined
  if (!siblings) return null
  const current = siblings.indexOf(captured.object)
  const target = boundary === 'first' ? 0 : siblings.length - 1
  if (current < 0 || current === target) return null
  return [{ op: 'reorder', object: captured.object, index: target }]
}
