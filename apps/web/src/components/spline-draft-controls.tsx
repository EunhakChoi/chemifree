import { Button } from '@/components/ui/button'
import { SPLINE_POINT_NAMES } from '@/editor/spline'

export function SplineDraftControls({ count, available, onFinish, onRemove, onCancel }: { count: number; available: boolean; onFinish: () => void; onRemove: () => void; onCancel: () => void }) {
  return <div role="group" aria-label="Spline draft" className="flex flex-col gap-1">
    <p className="text-xs" role="status">Spline draft: <output aria-label="Spline draft points">{count}</output>/4 points. {count < 4 ? `Click ${SPLINE_POINT_NAMES[count]} next.` : 'All four points entered; Finish to create.'} No document edit occurs before Finish.</p>
    <div className="flex flex-wrap gap-1">
      <Button size="sm" variant="outline" disabled={!available || count !== 4} onClick={onFinish}>Finish spline</Button>
      <Button size="sm" variant="outline" disabled={count === 0} onClick={onRemove}>Remove last spline point</Button>
      <Button size="sm" variant="outline" disabled={count === 0} onClick={onCancel}>Cancel spline</Button>
    </div>
  </div>
}
