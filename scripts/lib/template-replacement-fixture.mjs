import { attachSource } from './attach-ring-fixture.mjs'
import { templateStereoCase } from './template-attachment-fixture.mjs'

/** Non-sorted native IDs bind a10 to existing a3; expected stereo below is hand-stated. */
export function replacementCase(kind = 'center') {
  const source = attachSource()
  source.document.scene.references.push({ source: 's1', target: { kind: 'atom', id: 'a3' } },
    { source: 's1', target: { kind: 'bond', id: 'b2' } })
  const example = templateStereoCase(kind)
  const raw = JSON.parse(example.parameters.payload)
  const { direction: _direction, ...parameters } = { ...example.parameters, atom: 'a3' }
  const snapshot = structuredClone(source)
  const anchor = raw.atoms.find((atom) => atom.id === 'a10')
  const target = source.document.atoms.find((atom) => atom.id === 'a3')
  let nextAtom = 5
  const mapping = { atoms: raw.atoms.map((atom) => ({ source: atom.id, target: atom.id === 'a10' ? 'a3' : `a${nextAtom++}` })),
    bonds: raw.bonds.map((bond, index) => ({ source: bond.id, target: `b${index + 4}` })) }
  const ids = new Map(mapping.atoms.map(({ source, target }) => [source, target]))
  for (const atom of raw.atoms) {
    const copied = { ...structuredClone(source.document.atoms[0]), id: ids.get(atom.id), element: atom.kind.symbol,
      position: atom.id === 'a10' ? target.position : {
        x: atom.position.x + target.position.x - anchor.position.x,
        y: atom.position.y + target.position.y - anchor.position.y,
      } }
    if (copied.id === 'a3') snapshot.document.atoms[2] = copied
    else {
      snapshot.document.atoms.push(copied)
      snapshot.document.scene.instances[0].positions.push({ atom: copied.id, ...copied.position })
    }
  }
  const bonds = raw.bonds.map((bond, index) => ({ ...bond, id: mapping.bonds[index].target, begin: ids.get(bond.begin), end: ids.get(bond.end) }))
  snapshot.document.bonds.push(...bonds)
  snapshot.document.selection = { objects: [], atoms: mapping.atoms.map((entry) => entry.target).sort((a, b) => Number(a.slice(1)) - Number(b.slice(1))), bonds: bonds.map((bond) => bond.id) }
  if (kind === 'center') snapshot.document.stereo.atoms.push({ atom: 'a5', configuration: {
    kind: 'tetrahedral', neighbors: ['a3', 'a6', 'a7', 'a8'], turn: 'clockwise',
  } })
  else snapshot.document.stereo.bonds.push({ bond: 'b5', configuration: {
    kind: 'relative', begin_neighbor: 'a3', end_neighbor: 'a8', relation: 'opposite_sides',
  } })
  snapshot.document.revision = 4
  snapshot.session.unsaved_changes = true
  snapshot.session.undo_depth = 1
  snapshot.notes = []
  return { parameters, result: { source, snapshot, mapping, boundary: 'b2' } }
}
