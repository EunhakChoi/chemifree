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

function resized(current: CanvasState, documentId: string, bounds: ViewBox, width: number, height: number, boundsReady: boolean): CanvasState {
  if (current.documentId !== documentId) return current
  const size = {
    width: Number.isFinite(width) && width > 0 ? width : 0,
    height: Number.isFinite(height) && height > 0 ? height : 0,
  }
  const viewport = current.viewport ?? (boundsReady && positive(size) ? fitViewport(bounds, size) : null)
  if (current.size.width === size.width && current.size.height === size.height && current.viewport === viewport) return current
  return { documentId, size, viewport }
}

/**
 * Owns transient presentation state only. Resize preserves scale and center; edits preserve
 * the viewport. A new document fits once when its size and bounds are ready. Explicit Fit,
 * pan or zoom use currently available bounds and cancel any deferred first fit.
 */
export function useCanvasViewport(documentId: string, bounds: ViewBox, boundsReady = true): CanvasViewport {
  const fitBounds = useMemo(() => ({ x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height }),
    [bounds.x, bounds.y, bounds.width, bounds.height])
  const [element, setElement] = useState<Element | null>(null)
  const [state, setState] = useState<CanvasState>(() => ({ documentId, size: { width: 0, height: 0 }, viewport: null }))
  // Adjust this component's own identity state during render, so a new drawing never paints
  // with the previous drawing's viewport while waiting for an effect.
  const active = state.documentId === documentId ? state : {
    documentId, size: state.size, viewport: boundsReady && positive(state.size) ? fitViewport(fitBounds, state.size) : null,
  }
  // Only the still-pending first fit can adopt later text/font bounds. Edits, resize and
  // font completions cannot overwrite a viewport already fitted or chosen by the user.
  const ready = active.viewport === null && boundsReady && positive(active.size)
    ? { ...active, viewport: fitViewport(fitBounds, active.size) } : active
  if (ready !== state) setState(ready)

  const containerRef = useCallback((node: Element | null) => {
    setElement(node)
    if (node !== null) {
      const rect = node.getBoundingClientRect()
      setState((current) => resized(current, documentId, fitBounds, rect.width, rect.height, boundsReady))
    }
  }, [documentId, fitBounds, boundsReady])

  const recordSize = useEffectEvent((width: number, height: number) => {
    setState((current) => resized(current, documentId, fitBounds, width, height, boundsReady))
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
    } : current)
  }, [documentId, fitBounds])
  const pan = useCallback((delta: ViewportPoint) => {
    setState((current) => current.documentId === documentId && positive(current.size) ? {
      ...current, viewport: panViewport(current.viewport ?? fitViewport(fitBounds, current.size), delta),
    } : current)
  }, [documentId, fitBounds])
  const zoom = useCallback((factor: number, anchor?: ViewportPoint) => {
    setState((current) => {
      if (current.documentId !== documentId || !positive(current.size)) return current
      const at = anchor ?? { x: current.size.width / 2, y: current.size.height / 2 }
      return { ...current, viewport: zoomViewport(current.viewport ?? fitViewport(fitBounds, current.size), factor, at, current.size) }
    })
  }, [documentId, fitBounds])
  const viewport = ready.viewport ?? fitViewport(fitBounds, ready.size)
  const box = svgViewBox(viewport, ready.size)
  return { containerRef, size: ready.size, viewport, viewBox: `${box.x} ${box.y} ${box.width} ${box.height}`, fit, pan, zoom }
}
