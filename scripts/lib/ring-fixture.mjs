// Explicit public-boundary geometry expectations, independent of the Rust implementation.
import { sceneCutSource } from './scene-cut-fixture.mjs'

export const ringSelection = () => ({ objects: ['s7'], atoms: ['a1'], bonds: ['b1'] })

export function ringSource() {
  const source = sceneCutSource()
  source.document.title = 'Independent ring'
  source.document.scene.instances = source.document.scene.instances.slice(0, 1)
  source.document.scene.roots = source.document.scene.roots.filter((id) => id !== 's2')
  source.document.scene.objects = source.document.scene.objects.filter((object) => object.id !== 's2')
  return source
}

export function ringAfter() {
  const result = ringSource()
  result.document.revision = 4
  const positions = [[130, 100], [115, 125.98076211353316], [85, 125.98076211353316],
    [70, 100], [85, 74.01923788646684], [115, 74.01923788646684]]
  const atoms = positions.map(([x, y], index) => ({ id: `a${index + 3}`, element: 'C', kind: 'element', query: null,
    charge: 0, isotope: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } }))
  result.document.atoms.push(...atoms)
  result.document.bonds.push(...atoms.map((atom, index) => ({ id: `b${index + 2}`, begin: atom.id,
    end: atoms[(index + 1) % 6].id, order: 'single', query: null, mark: null })))
  result.document.scene.instances[0].positions.push(...atoms.map(({ id, position }) => ({ atom: id, ...position })))
  result.document.selection = ringSelection()
  result.session.unsaved_changes = true
  result.session.undo_depth = 1
  return result
}
