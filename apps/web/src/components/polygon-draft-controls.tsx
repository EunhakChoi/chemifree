import { Button } from '@/components/ui/button'
import { MAX_POLYGON_VERTICES, MIN_POLYGON_VERTICES } from '@/lib/document-contract'

export function PolygonDraftControls({ count, available, onFinish, onRemove, onCancel }: { count: number; available: boolean; onFinish: () => void; onRemove: () => void; onCancel: () => void }) {
  return <div role="group" aria-label="Polygon draft" className="flex flex-col gap-1">
    <p className="text-xs" role="status">Polygon draft: <output aria-label="Polygon draft vertices">{count}</output> vertices. Click to add; Finish closes the outline. No document edit occurs before Finish.</p>
    {count === MAX_POLYGON_VERTICES && <p className="text-xs text-muted-foreground">64 vertices reached. Remove the last vertex or finish.</p>}
    <div className="flex flex-wrap gap-1">
      <Button size="sm" variant="outline" disabled={!available || count < MIN_POLYGON_VERTICES} onClick={onFinish}>Finish polygon</Button>
      <Button size="sm" variant="outline" disabled={count === 0} onClick={onRemove}>Remove last polygon vertex</Button>
      <Button size="sm" variant="outline" disabled={count === 0} onClick={onCancel}>Cancel polygon</Button>
    </div>
  </div>
}
