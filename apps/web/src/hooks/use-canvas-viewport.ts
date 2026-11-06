import { useCallback, useEffect, useEffectEvent, useMemo, useState } from 'react'

import {
  fitViewport,
  panViewport,
  svgViewBox,
  zoomViewport,
  type ViewBox,
  type Viewport,
  type ViewportPoint,
  type ViewportSize,
} from '@/editor/viewport'

interface CanvasState {
  documentId: string
  size: ViewportSize
  /** Null means the initial fit still awaits positive size and complete bounds. */
  viewport: Viewport | null
  initialFitPending: boolean
  automaticBounds: ViewBox | null
  fitContent: object | null
}

interface FitReadiness {
  boundsReady?: boolean
  /** The actual SVG viewBox used for this text measurement. */
  measuredViewBox?: string
  /** An edit must not gain authority to refit the opening snapshot. */
  contentIdentity?: object
}

export interface CanvasViewport {
  containerRef: (element: Element | null) => void
  size: ViewportSize
  viewport: Viewport
  viewBox: string
  fit: () => void
  pan: (delta: ViewportPoint) => void
  zoom: (factor: number, anchor?: ViewportPoint) => void
}

function positive(size: ViewportSize): boolean {
  return size.width > 0 && size.height > 0
}

function resized(current: CanvasState, documentId: string, width: number, height: number): CanvasState {
  if (current.documentId !== documentId) return current
  const size = {
    width: Number.isFinite(width) && width > 0 ? width : 0,
    height: Number.isFinite(height) && height > 0 ? height : 0,
  }
  return current.size.width === size.width && current.size.height === size.height ? current : { ...current, size }
}

function viewBoxText(viewport: Viewport, size: ViewportSize): string {
  const box = svgViewBox(viewport, size)
  return `${box.x} ${box.y} ${box.width} ${box.height}`
}

function contains(outer: ViewBox, inner: ViewBox): boolean {
  return outer.x <= inner.x && outer.y <= inner.y && outer.x + outer.width >= inner.x + inner.width && outer.y + outer.height >= inner.y + inner.height
}

/** Expand only: fitting at a different scale may change the browser's realized glyph metrics. */
function enclosing(first: ViewBox, second: ViewBox): ViewBox {
  const x = Math.min(first.x, second.x)
  const y = Math.min(first.y, second.y)
  return { x, y, width: Math.max(first.x + first.width, second.x + second.width) - x,
    height: Math.max(first.y + first.height, second.y + second.height) - y }
}

/**
 * Owns transient presentation state only. Resize preserves scale and center; edits preserve
 * the viewport. The opening fit is confirmed using bounds measured in its resulting SVG
 * viewport, expanding only if needed. Explicit Fit/pan/zoom ends that initial authority.
 */
export function useCanvasViewport(documentId: string, bounds: ViewBox, readiness: FitReadiness = {}): CanvasViewport {
  const { boundsReady = true, measuredViewBox, contentIdentity } = readiness
  const fitBounds = useMemo(() => ({ x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height }),
    [bounds.x, bounds.y, bounds.width, bounds.height])
  const [element, setElement] = useState<Element | null>(null)
  const [state, setState] = useState<CanvasState>(() => ({ documentId, size: { width: 0, height: 0 }, viewport: null,
    initialFitPending: true, automaticBounds: null, fitContent: null }))
  // Adjust this component's own identity state during render, so a new drawing never paints
  // with the previous drawing's viewport while waiting for an effect.
  let active: CanvasState = state.documentId === documentId ? state : {
    documentId, size: state.size, viewport: null, initialFitPending: true, automaticBounds: null, fitContent: null,
  }
  if (active.viewport === null && boundsReady && positive(active.size)) {
    const pending = active.initialFitPending && measuredViewBox !== undefined
    active = { ...active, viewport: fitViewport(fitBounds, active.size), initialFitPending: pending,
      automaticBounds: pending ? fitBounds : null, fitContent: contentIdentity ?? null }
  } else if (active.viewport !== null && active.initialFitPending) {
    if (active.fitContent !== (contentIdentity ?? null)) {
      active = { ...active, initialFitPending: false, automaticBounds: null }
    } else if (boundsReady && measuredViewBox === viewBoxText(active.viewport, active.size)) {
      if (contains(svgViewBox(active.viewport, active.size), fitBounds)) {
        active = { ...active, initialFitPending: false, automaticBounds: null }
      } else {
        const expanded = enclosing(active.automaticBounds ?? fitBounds, fitBounds)
        const fitted = fitViewport(expanded, active.size)
        // A clamped scale can make larger content impossible to fit. Do not render-loop.
        const moved = fitted.scale !== active.viewport.scale || fitted.center.x !== active.viewport.center.x || fitted.center.y !== active.viewport.center.y
        active = { ...active, viewport: fitted, initialFitPending: moved, automaticBounds: moved ? expanded : null }
      }
    }
  }
  if (active !== state) setState(active)

  const containerRef = useCallback((node: Element | null) => {
    setElement(node)
    if (node !== null) {
      const rect = node.getBoundingClientRect()
      setState((current) => resized(current, documentId, rect.width, rect.height))
    }
  }, [documentId])

  const recordSize = useEffectEvent((width: number, height: number) => {
    setState((current) => resized(current, documentId, width, height))
  })
  useEffect(() => {
    if (element === null) return
    if (typeof ResizeObserver === 'undefined') {
      const measure = () => {
        const rect = element.getBoundingClientRect()
        recordSize(rect.width, rect.height)
      }
      window.addEventListener('resize', measure)
      return () => window.removeEventListener('resize', measure)
    }
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (entry.target === element) recordSize(entry.contentRect.width, entry.contentRect.height)
      }
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [element])

  const fit = useCallback(() => {
    setState((current) => current.documentId === documentId ? {
      ...current, viewport: positive(current.size) ? fitViewport(fitBounds, current.size) : null,
      initialFitPending: false, automaticBounds: null,
    } : current)
  }, [documentId, fitBounds])
  const pan = useCallback((delta: ViewportPoint) => {
    setState((current) => current.documentId === documentId && positive(current.size) ? {
      ...current, viewport: panViewport(current.viewport ?? fitViewport(fitBounds, current.size), delta),
      initialFitPending: false, automaticBounds: null,
    } : current)
  }, [documentId, fitBounds])
  const zoom = useCallback((factor: number, anchor?: ViewportPoint) => {
    setState((current) => {
      if (current.documentId !== documentId || !positive(current.size)) return current
      const at = anchor ?? { x: current.size.width / 2, y: current.size.height / 2 }
      return { ...current, viewport: zoomViewport(current.viewport ?? fitViewport(fitBounds, current.size), factor, at, current.size),
        initialFitPending: false, automaticBounds: null }
    })
  }, [documentId, fitBounds])
  const viewport = active.viewport ?? fitViewport(fitBounds, active.size)
  return { containerRef, size: active.size, viewport, viewBox: viewBoxText(viewport, active.size), fit, pan, zoom }
}
