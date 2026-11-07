import { Button } from '@/components/ui/button'
import { captureSceneOrder, sceneOrderCommands, type SceneOrderBoundary } from '@/editor/scene-order'
import type { DocumentEditor } from '@/hooks/use-document-editor'
import type { DocumentView } from '@/lib/document-contract'

/** First/Last share the existing order queue; neither changes an object's parent. */
export function SceneOrderBoundaryButtons({ document, editor }: { document: DocumentView; editor: DocumentEditor }) {
  const captured = captureSceneOrder(document, editor.selection)
  function move(boundary: SceneOrderBoundary) {
    const pressed = editor.selectionAtPress()
    const target = captureSceneOrder(document, pressed)
    if (!target) return
    void editor.apply((latest) => sceneOrderCommands(latest, target, boundary), { kind: 'gesture', before: pressed })
  }
  return <div role="group" aria-label="Scene order boundaries" className="flex flex-wrap gap-1">
    <Button size="sm" variant="outline" disabled={!captured || sceneOrderCommands(document, captured, 'first') === null} onClick={() => move('first')}>First</Button>
    <Button size="sm" variant="outline" disabled={!captured || sceneOrderCommands(document, captured, 'last') === null} onClick={() => move('last')}>Last</Button>
  </div>
}
