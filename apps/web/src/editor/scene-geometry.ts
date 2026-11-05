import type { ArrangementMode, Command, DocumentView, Position, SelectionTransform } from '@/lib/document-contract'
import { selectionTransform } from '@/editor/selection-geometry'

interface GeometryBounds {
  left: number
  top: number
  right: number
  bottom: number
}

interface SceneGeometry {
  objects: string[]
  bounds: GeometryBounds | null
  hasGraphTargets: boolean
  hasEmptyUnits: boolean
  hasMissingTargets: boolean
}

export type SceneGeometryAction =
  | { kind: 'arrange'; mode: ArrangementMode }
  | { kind: 'transform'; transform: SelectionTransform }

/** Geometry controls act on whole scene objects, with each selected ancestor counted once. */
export function sceneGeometry(document: DocumentView, selected: ReadonlySet<string>): SceneGeometry {
  const objects = document.scene?.objects ?? []
  const byId = new Map(objects.map((object) => [object.id, object]))
  const known = new Set([...byId.keys(), ...document.atoms.map((atom) => atom.id), ...document.bonds.map((bond) => bond.id)])
  const hasMissingTargets = [...selected].some((id) => !known.has(id))
  const hasGraphTargets = document.atoms.some((atom) => selected.has(atom.id)) || document.bonds.some((bond) => selected.has(bond.id))
  const roots = objects.filter((object) => {
    if (!selected.has(object.id)) return false
    let parent = object.parent
    while (parent !== null) {
      if (selected.has(parent)) return false
      parent = byId.get(parent)?.parent ?? null
    }
    return true
  })
  let bounds: GeometryBounds | null = null
  let hasEmptyUnits = false
  for (const root of roots) {
    const pending = [root]
    const points: Position[] = []
    while (pending.length > 0) {
      const object = pending.pop()
      if (!object) continue
      switch (object.kind) {
        case 'graphic':
          points.push(...object.points)
          break
        case 'instance': {
          const instance = document.scene?.instances.find((instance) => instance.id === object.id)
          if (instance) points.push(...instance.positions)
          break
        }
        case 'group':
          for (const child of object.children) {
            const descendant = byId.get(child)
            if (descendant) pending.push(descendant)
          }
          break
        default: {
          const unexpected: never = object
          throw new Error(`Unsupported scene object: ${String(unexpected)}`)
        }
      }
    }
    if (points.length === 0) hasEmptyUnits = true
    for (const point of points) {
      if (bounds === null) {
        bounds = { left: point.x, top: point.y, right: point.x, bottom: point.y }
      } else {
        bounds.left = Math.min(bounds.left, point.x)
        bounds.top = Math.min(bounds.top, point.y)
        bounds.right = Math.max(bounds.right, point.x)
        bounds.bottom = Math.max(bounds.bottom, point.y)
      }
    }
  }
  return { objects: roots.map((object) => object.id), bounds, hasGraphTargets, hasEmptyUnits, hasMissingTargets }
}

/** Rebuild a captured action against the queued document; missing targets lapse as a whole. */
export function sceneGeometryCommands(document: DocumentView, selected: ReadonlySet<string>, action: SceneGeometryAction): Command[] | null {
  const geometry = sceneGeometry(document, selected)
  if (geometry.hasMissingTargets || geometry.hasGraphTargets || geometry.bounds === null) return null
  if (action.kind === 'arrange') {
    const required = action.mode.startsWith('distribute_') ? 3 : 2
    if (geometry.hasEmptyUnits || geometry.objects.length < required) return null
    return [{ op: 'arrange_objects', objects: geometry.objects, mode: action.mode }]
  }
  return selectionTransform(document, new Set(geometry.objects), null, action.transform)
}
