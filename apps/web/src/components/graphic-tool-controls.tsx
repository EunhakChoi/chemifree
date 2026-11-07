import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { GraphicShape } from '@/lib/document-contract'

/** A creation gesture starts only after an explicit shape choice. */
export function GraphicToolControls({ shape, onShapeChange, disabled }: { shape: GraphicShape | null; onShapeChange: (shape: GraphicShape) => void; disabled: boolean }) {
  return <>
    <ToggleGroup aria-label="Graphic shape" variant="outline" size="sm" value={shape ? [shape] : []} disabled={disabled} onValueChange={(values) => {
      const value = values[0]
      if (value === 'line' || value === 'arrow' || value === 'rectangle') onShapeChange(value)
    }}>
      <ToggleGroupItem value="line">Line</ToggleGroupItem>
      <ToggleGroupItem value="arrow">Arrow</ToggleGroupItem>
      <ToggleGroupItem value="rectangle">Rectangle</ToggleGroupItem>
    </ToggleGroup>
    <p className="text-xs text-muted-foreground">{shape ? `Drag two points to add a ${shape} at the page root.` : 'Choose Line, Arrow or Rectangle before drawing.'} Graphics preserve the chemical graph. Hold Space or use the middle button to pan; Escape cancels the preview.</p>
  </>
}
