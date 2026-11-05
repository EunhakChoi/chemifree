import type { Command, DocumentView, Position, SelectionTransform } from '@/lib/document-contract'
import { selectionSnapshot } from '@/editor/scene-selection'

/** Unique scene and per-placement geometry targeted by a selection. */
export interface SelectionGeometry {
  objects: Set<string>
  positions: Map<string | null, Set<string>>
  graphics: Set<string>
}

/** A selected graph ID has one movement placement; scene objects name their own placements. */
export function selectionGeometry(document: DocumentView, selected: ReadonlySet<string>, instance: string | null): SelectionGeometry | null {
  const objects = document.scene?.objects ?? []
  const byId = new Map(objects.map((object) => [object.id, object]))
  const atomIds = new Set(document.atoms.map((atom) => atom.id))
  const bonds = new Map(document.bonds.map((bond) => [bond.id, bond]))
  if ([...selected].some((id) => !byId.has(id) && !atomIds.has(id) && !bonds.has(id))) return null
  const instances = document.scene?.instances ?? []
  if (instance !== null && !instances.some((item) => item.id === instance)) return null
  const explicit = selectionSnapshot(new Set(selected), document)
  const graphAtoms = new Set(explicit.atoms)
  for (const id of explicit.bonds) {
    const bond = bonds.get(id)
    if (bond) { graphAtoms.add(bond.begin); graphAtoms.add(bond.end) }
  }
  const active = instance
  if (graphAtoms.size > 0 && document.scene && active === null) return null
  const scope: SelectionGeometry = { objects: new Set(), positions: new Map(), graphics: new Set() }
  const add = (placement: string | null, atoms: Iterable<string>) => {
    const entries = scope.positions.get(placement) ?? new Set<string>()
    for (const atom of atoms) entries.add(atom)
    scope.positions.set(placement, entries)
  }
  if (graphAtoms.size > 0) add(active, graphAtoms)
  const pending = [...explicit.objects]
  while (pending.length > 0) {
    const id = pending.pop()
    if (id === undefined || scope.objects.has(id)) continue
    scope.objects.add(id)
    const object = byId.get(id)
    if (!object) return null
    switch (object.kind) {
      case 'group': pending.push(...object.children); break
      case 'graphic': scope.graphics.add(id); break
      case 'instance': {
        const placement = instances.find((item) => item.id === id)
        if (!placement) return null
        add(id, placement.positions.map((position) => position.atom))
        break
      }
      default: {
        const unexpected: never = object
        throw new Error(`Unsupported scene object: ${String(unexpected)}`)
      }
    }
  }
  return scope
}

/** Switching movement placement clears graph targets, retaining independently selected scene objects. */
export function sceneOnlySelection(document: DocumentView, selected: ReadonlySet<string>): Set<string> {
  return new Set(document.scene?.objects?.filter((object) => selected.has(object.id)).map((object) => object.id) ?? [])
}

/** Inclusive containment of active-placement atom centers and complete two-point graphic geometry. */
export function marqueeSelection(document: DocumentView, instance: string | null, start: Position, end: Position, pressed: ReadonlySet<string>, additive: boolean, dragged: boolean): Set<string> {
  const selected = additive ? new Set(pressed) : new Set<string>()
  if (!dragged) return selected
  const left = Math.min(start.x, end.x)
  const right = Math.max(start.x, end.x)
  const top = Math.min(start.y, end.y)
  const bottom = Math.max(start.y, end.y)
  const inside = (point: Position) => point.x >= left && point.x <= right && point.y >= top && point.y <= bottom
  const positions = document.scene
    ? document.scene.instances.find((placement) => placement.id === instance)?.positions ?? []
    : document.atoms.map((atom) => ({ ...atom.position, atom: atom.id }))
  for (const position of positions) if (inside(position)) selected.add(position.atom)
  for (const object of document.scene?.objects ?? []) {
    if (object.kind === 'graphic' && object.points.every((point) => inside(point))) selected.add(object.id)
  }
  return selected
}

/** Build one captured union move; unknown targets or an expired placement lapse as a whole. */
export function selectionMove(document: DocumentView, selected: ReadonlySet<string>, instance: string | null, delta: Position): Command[] | null {
  const scope = selectionGeometry(document, selected, instance)
  if (!scope || (scope.positions.size === 0 && scope.graphics.size === 0)) return null
  const selection = selectionSnapshot(new Set(selected), document)
  if (!document.scene?.objects) {
    return [{ op: 'move_atoms', atoms: [...(scope.positions.get(instance) ?? [])], delta, ...(instance === null ? {} : { instance }) }]
  }
  return [{ op: 'move_selection', selection, delta, ...(instance === null ? {} : { instance }) }]
}


/** Transform a captured mixed union about its current geometry-bounds center. */
export function selectionTransform(document: DocumentView, selected: ReadonlySet<string>, instance: string | null, transform: SelectionTransform): Command[] | null {
  if (!document.scene?.objects) return null
  const selection = selectionSnapshot(new Set(selected), document)
  const hasGraphTargets = selection.atoms.length > 0 || selection.bonds.length > 0
  const context = hasGraphTargets ? instance : null
  const scope = selectionGeometry(document, selected, context)
  if (!scope) return null
  const points: Position[] = []
  for (const placement of document.scene.instances) {
    const atoms = scope.positions.get(placement.id)
    if (atoms) points.push(...placement.positions.filter((position) => atoms.has(position.atom)))
  }
  for (const object of document.scene.objects) {
    if (object.kind === 'graphic' && scope.graphics.has(object.id)) points.push(...object.points)
  }
  const first = points[0]
  if (!first) return null
  let left = first.x
  let right = first.x
  let top = first.y
  let bottom = first.y
  for (const point of points) {
    left = Math.min(left, point.x)
    right = Math.max(right, point.x)
    top = Math.min(top, point.y)
    bottom = Math.max(bottom, point.y)
  }
  return [{
    op: 'transform_selection', selection, transform,
    pivot: { x: (left + right) / 2, y: (top + bottom) / 2 },
    ...(context === null ? {} : { instance: context }),
  }]
}
