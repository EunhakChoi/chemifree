/** Local canvas coordinates: center in document points and scale in CSS pixels per point. */
export interface Viewport {
  center: ViewportPoint
  scale: number
}

export interface ViewportPoint {
  x: number
  y: number
}

export interface ViewportSize {
  width: number
  height: number
}

/** The bounds supplied by geometry.viewBox, including its existing empty drawing frame. */
export interface ViewBox extends ViewportSize {
  x: number
  y: number
}

export const MIN_SCALE = 1 / 4096
export const MAX_SCALE = 32

function finitePoint(point: ViewportPoint): boolean {
  return Number.isFinite(point.x) && Number.isFinite(point.y)
}

function measured(size: ViewportSize): boolean {
  return Number.isFinite(size.width) && size.width > 0 && Number.isFinite(size.height) && size.height > 0
}

/** Invalid scale values use one CSS pixel per document point. */
export function clampScale(scale: number): number {
  return Number.isFinite(scale) && scale > 0 ? Math.max(MIN_SCALE, Math.min(MAX_SCALE, scale)) : 1
}

function normalized(viewport: Viewport): Viewport {
  return {
    center: finitePoint(viewport.center) ? viewport.center : { x: 0, y: 0 },
    scale: clampScale(viewport.scale),
  }
}

/** Fits the complete supplied bounds without adding another margin or stretching either axis. */
export function fitViewport(bounds: ViewBox, size: ViewportSize): Viewport {
  const width = Number.isFinite(bounds.width) && bounds.width > 0 ? bounds.width : 1
  const height = Number.isFinite(bounds.height) && bounds.height > 0 ? bounds.height : 1
  const x = Number.isFinite(bounds.x) ? bounds.x : 0
  const y = Number.isFinite(bounds.y) ? bounds.y : 0
  const center = { x: x + width / 2, y: y + height / 2 }
  const scale = measured(size) ? Math.min(size.width / width, size.height / height) : 1
  return {
    center: finitePoint(center) ? center : { x, y },
    scale: Math.max(MIN_SCALE, Math.min(MAX_SCALE, scale)),
  }
}

/** Positive pixel deltas move the drawing right/down, hence move the document center left/up. */
export function panViewport(viewport: Viewport, delta: ViewportPoint): Viewport {
  const current = normalized(viewport)
  if (!finitePoint(delta)) return current
  const center = { x: current.center.x - delta.x / current.scale, y: current.center.y - delta.y / current.scale }
  return finitePoint(center) ? { center, scale: current.scale } : current
}

/** Converts a CSS pixel point relative to the canvas's top left into document points. */
export function clientToWorld(viewport: Viewport, point: ViewportPoint, size: ViewportSize): ViewportPoint {
  const current = normalized(viewport)
  if (!measured(size) || !finitePoint(point)) return current.center
  const world = {
    x: current.center.x + (point.x - size.width / 2) / current.scale,
    y: current.center.y + (point.y - size.height / 2) / current.scale,
  }
  return finitePoint(world) ? world : current.center
}

/** Converts document points into CSS pixels relative to the canvas's top left. */
export function worldToClient(viewport: Viewport, point: ViewportPoint, size: ViewportSize): ViewportPoint {
  const current = normalized(viewport)
  if (!measured(size) || !finitePoint(point)) return { x: 0, y: 0 }
  const client = {
    x: (point.x - current.center.x) * current.scale + size.width / 2,
    y: (point.y - current.center.y) * current.scale + size.height / 2,
  }
  return finitePoint(client) ? client : { x: 0, y: 0 }
}

/** Keeps the document point beneath the CSS pixel anchor fixed, including at scale limits. */
export function zoomViewport(viewport: Viewport, factor: number, anchor: ViewportPoint, size: ViewportSize): Viewport {
  const current = normalized(viewport)
  if (!measured(size) || !finitePoint(anchor) || !Number.isFinite(factor) || factor <= 0) return current
  const scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, current.scale * factor))
  if (scale === current.scale) return current
  const world = clientToWorld(current, anchor, size)
  const center = {
    x: world.x - (anchor.x - size.width / 2) / scale,
    y: world.y - (anchor.y - size.height / 2) / scale,
  }
  return finitePoint(center) ? { center, scale } : current
}

/** The SVG viewBox has the measured aspect ratio, so its points use exactly viewport.scale. */
export function svgViewBox(viewport: Viewport, size: ViewportSize): ViewBox {
  const current = normalized(viewport)
  const width = measured(size) ? size.width / current.scale : 1 / current.scale
  const height = measured(size) ? size.height / current.scale : 1 / current.scale
  const safeWidth = Number.isFinite(width) ? width : 1 / current.scale
  const safeHeight = Number.isFinite(height) ? height : 1 / current.scale
  const x = current.center.x - safeWidth / 2
  const y = current.center.y - safeHeight / 2
  return {
    x: Number.isFinite(x) ? x : current.center.x,
    y: Number.isFinite(y) ? y : current.center.y,
    width: safeWidth,
    height: safeHeight,
  }
}
