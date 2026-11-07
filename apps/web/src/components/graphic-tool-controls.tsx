import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { SceneGraphicShape } from '@/lib/document-contract'

/** A creation gesture starts only after an explicit shape choice. */
export function GraphicToolControls({ shape, onShapeChange, disabled }: { shape: SceneGraphicShape | null; onShapeChange: (shape: SceneGraphicShape) => void; disabled: boolean }) {
  return <>
    <ToggleGroup className="max-w-full flex-wrap" aria-label="Graphic shape" variant="outline" size="sm" value={shape ? [shape] : []} disabled={disabled} onValueChange={(values) => {
      const value = values[0]
      if (value === 'line' || value === 'arrow' || value === 'rectangle' || value === 'ellipse' || value === 'polygon' || value === 'spline') onShapeChange(value)
    }}>
      <ToggleGroupItem value="line">Line</ToggleGroupItem>
      <ToggleGroupItem value="arrow">Arrow</ToggleGroupItem>
      <ToggleGroupItem value="rectangle">Rectangle</ToggleGroupItem>
      <ToggleGroupItem value="ellipse">Ellipse</ToggleGroupItem>
      <ToggleGroupItem value="polygon">Polygon</ToggleGroupItem>
      <ToggleGroupItem value="spline">Spline</ToggleGroupItem>
    </ToggleGroup>
    <p className="text-xs text-muted-foreground">{shape === 'spline' ? 'Click exactly P0, C1, C2, P3, then Finish spline. One open cubic stroke; no controls are inferred. Fit, marquee and transform bounds use the four defining points, not curve extrema.' : shape === 'polygon' ? 'Click 3–64 ordered vertices, then Finish polygon. The outline closes without a fill.' : shape ? `Drag two points to add a ${shape} at the page root.` : 'Choose Line, Arrow, Rectangle, Ellipse, Polygon or Spline before drawing.'} Rectangles and ellipses use opposite corners. Graphics preserve the chemical graph. Hold Space or use the middle button to pan; Escape cancels the preview.</p>
  </>
}
