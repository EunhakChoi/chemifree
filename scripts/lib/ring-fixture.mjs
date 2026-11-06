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

export function ringAfter(size = 6) {
  const result = ringSource()
  result.document.revision = 4
  const positions = size === 5
    ? [[125.5195242505612, 100], [107.885966681787, 124.27050983124842],
      [79.3542711929324, 115], [79.3542711929324, 85], [107.885966681787, 75.72949016875158]]
    : [[130, 100], [115, 125.98076211353316], [85, 125.98076211353316],
    [70, 100], [85, 74.01923788646684], [115, 74.01923788646684]]
  const atoms = positions.map(([x, y], index) => ({ id: `a${index + 3}`, element: 'C', kind: 'element', query: null,
    charge: 0, isotope: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } }))
  result.document.atoms.push(...atoms)
  result.document.bonds.push(...atoms.map((atom, index) => ({ id: `b${index + 2}`, begin: atom.id,
    end: atoms[(index + 1) % size].id, order: 'single', query: null, mark: null })))
  result.document.scene.instances[0].positions.push(...atoms.map(({ id, position }) => ({ atom: id, ...position })))
  result.document.selection = ringSelection()
  result.session.unsaved_changes = true
  result.session.undo_depth = 1
  return result
}
