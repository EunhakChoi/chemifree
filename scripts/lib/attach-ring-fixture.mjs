// Hand-stated public boundary fixture: existing target/configuration/group and fresh cycles.
import { ringSource, ringSelection } from './ring-fixture.mjs'

export function attachSource() {
  const source = ringSource()
  const atom = source.document.atoms[0]
  const added = [{ ...structuredClone(atom), id: 'a3', element: 'F', position: { x: 0, y: 30 } },
    { ...structuredClone(atom), id: 'a4', element: 'Cl', position: { x: -30, y: 0 } }]
  source.document.atoms.push(...added)
  source.document.scene.instances[0].positions.push(...added.map(({ id, position }) => ({ atom: id, ...position })))
  source.document.bonds[0].order = 'double'
  source.document.bonds[0].mark = { kind: 'crossed' }
  source.document.bonds.push({ id: 'b2', begin: 'a1', end: 'a3', order: 'single', query: null, mark: { kind: 'wedge', narrow: 'begin' } },
    { id: 'b3', begin: 'a1', end: 'a4', order: 'single', query: null, mark: null })
  source.document.stereo = { atoms: [{ atom: 'a1', configuration: { kind: 'tetrahedral',
    neighbors: ['a2', 'a3', 'a4', 'implicit_hydrogen'], turn: 'clockwise' } }],
    bonds: [{ bond: 'b1', configuration: { kind: 'unknown' } }],
    groups: [{ id: 'e1', kind: 'absolute', stored_number: null, number: null, label: 'abs', members: ['a1'] }] }
  return source
}

export function attachAfter(size = 6, pattern = 'single') {
  const result = attachSource()
  const positions = size === 5
    ? [[30, 0], [47.6335575687742, -24.27050983124842], [76.1652530576288, -15],
      [76.1652530576288, 15], [47.6335575687742, 24.27050983124842]]
    : [[30, 0], [45, -25.98076211353316], [75, -25.98076211353316],
      [90, 0], [75, 25.98076211353316], [45, 25.98076211353316]]
  const atoms = positions.map(([x, y], index) => ({ ...structuredClone(result.document.atoms[0]), id: `a${index + 5}`, position: { x, y } }))
  result.document.atoms.push(...atoms)
  result.document.bonds.push(...atoms.map((atom, index) => ({ id: `b${index + 4}`, begin: atom.id,
    end: atoms[(index + 1) % size].id, order: pattern === 'kekule' && index % 2 === 0 ? 'double' : 'single', query: null, mark: null })),
  { id: `b${size + 4}`, begin: 'a1', end: 'a5', order: 'single', query: null, mark: null })
  result.document.scene.instances[0].positions.push(...atoms.map(({ id, position }) => ({ atom: id, ...position })))
  result.document.stereo = { atoms: [], bonds: [], groups: [] }
  result.document.selection = ringSelection()
  result.document.revision = 4
  result.session.unsaved_changes = true
  result.session.undo_depth = 1
  result.notes = [['stereo_configuration', 'atoms[a1]'], ['stereo_configuration', 'bonds[b1]'],
    ['stereo_group_member', 'atoms[a1]'], ['stereo_group', 'groups[e1]']].map(([feature, source_path]) => ({
    feature, source_path, disposition: 'dropped', semantic_impact: 'stereo', reason: `${feature} changed ${source_path}; Undo restores the statement.` }))
  return result
}
