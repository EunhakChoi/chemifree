// A one-atom native file joined to a destination whose ordinary bond edit clears stereo.
import { attachSource, attachAfter } from './attach-ring-fixture.mjs'
export function templatePayload() {
  return JSON.stringify({ format: 'chemifree.clipboard', version: 1, settings: attachSource().document.settings,
    atoms: [{ id: 'a10', kind: { type: 'element', symbol: 'Cl' }, charge: 0, isotope: null,
      hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x: 100, y: 100 } }], bonds: [], stereo: [] })
}
export function templateParameters() {
  return { document_id: 'drawing', expected_revision: 3, payload: templatePayload(), instance: 's1', source_anchor: 'a10',
    atom: 'a1', direction: { dx: 1, dy: 0 }, selection_before: { objects: [], atoms: [], bonds: [] } }
}
export function templateResult(plan = false) {
  const source = attachSource()
  const snapshot = structuredClone(source)
  snapshot.document.revision = 4
  snapshot.document.atoms.push({ ...structuredClone(snapshot.document.atoms[0]), id: 'a5', element: 'Cl', position: { x: 30, y: 0 } })
  snapshot.document.scene.instances[0].positions.push({ atom: 'a5', x: 30, y: 0 })
  snapshot.document.bonds.push({ id: 'b4', begin: 'a1', end: 'a5', order: 'single', query: null, mark: null })
  snapshot.document.selection = { objects: [], atoms: ['a5'], bonds: [] }
  snapshot.document.stereo = { atoms: [], bonds: [], groups: [] }
  snapshot.session.unsaved_changes = true
  snapshot.session.undo_depth = 1
  snapshot.notes = attachAfter().notes
  return { ...(plan ? { source } : {}), snapshot, mapping: { atoms: [{ source: 'a10', target: 'a5' }], bonds: [] }, connection: 'b4' }
}
export function templateReply(body, result) {
  const losses = result.snapshot?.notes ?? []
  return { contract: 1, envelope: 'operation_result', request_id: body.request_id, operation: body.operation,
    profile: 'preserve-semantics', outcome: losses.length ? 'partial' : 'ok', document_revision: result.source?.document.revision ?? result.snapshot?.document.revision ?? null,
    result, diagnostics: [], loss_report: losses }
}

export function templateStereoCase(kind) {
  const source = attachSource()
  const atom = (id, symbol, x, y) => ({ id, kind: { type: 'element', symbol }, charge: 0, isotope: null,
    hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x, y } })
  const bond = (id, begin, end, order = 'single') => ({ id, begin, end, order, query: null, mark: null })
  // Deliberately non-sorted atom arrays force a representation-preserving parity/reference change.
  const payload = kind === 'center' ? {
    atoms: [atom('a20', 'C', 130, 100), atom('a30', 'F', 130, 130), atom('a10', 'C', 100, 100), atom('a40', 'Cl', 160, 100), atom('a50', 'Br', 130, 70)],
    bonds: [bond('b10', 'a20', 'a10'), bond('b20', 'a20', 'a30'), bond('b30', 'a20', 'a40'), bond('b40', 'a20', 'a50')],
    stereo: [{ type: 'atom', atom: 'a20', configuration: { kind: 'tetrahedral', neighbors: ['a10', 'a30', 'a40', 'a50'], turn: 'clockwise' } }],
  } : {
    atoms: [atom('a50', 'Cl', 130, 130), atom('a10', 'C', 100, 100), atom('a20', 'C', 130, 100), atom('a30', 'C', 160, 100), atom('a40', 'F', 190, 100), atom('a60', 'Br', 160, 130)],
    bonds: [bond('b10', 'a10', 'a20'), bond('b20', 'a20', 'a30', 'double'), bond('b30', 'a30', 'a40'), bond('b40', 'a20', 'a50'), bond('b50', 'a30', 'a60')],
    stereo: [{ type: 'bond', bond: 'b20', configuration: { kind: 'relative', begin_neighbor: 'a10', end_neighbor: 'a40', relation: 'opposite_sides' } }],
  }
  const text = JSON.stringify({ format: 'chemifree.clipboard', version: 1, settings: source.document.settings, ...payload })
  const parameters = { ...templateParameters(), payload: text }
  const snapshot = structuredClone(source)
  const mapping = { atoms: payload.atoms.map((atom, index) => ({ source: atom.id, target: `a${index + 5}` })),
    bonds: payload.bonds.map((bond, index) => ({ source: bond.id, target: `b${index + 4}` })) }
  const ids = new Map(mapping.atoms.map(({ source, target }) => [source, target]))
  const added = payload.atoms.map((atom) => ({ ...structuredClone(source.document.atoms[0]), id: ids.get(atom.id), element: atom.kind.symbol,
    position: { x: atom.position.x - 70, y: atom.position.y - 100 } }))
  const edges = payload.bonds.map((bond, index) => ({ ...bond, id: mapping.bonds[index].target, begin: ids.get(bond.begin), end: ids.get(bond.end) }))
  const connection = `b${payload.bonds.length + 4}`
  snapshot.document.atoms.push(...added)
  snapshot.document.scene.instances[0].positions.push(...added.map(({ id, position }) => ({ atom: id, ...position })))
  snapshot.document.bonds.push(...edges, { id: connection, begin: 'a1', end: ids.get('a10'), order: 'single', query: null, mark: null })
  snapshot.document.selection = { objects: [], atoms: added.map((atom) => atom.id), bonds: edges.map((bond) => bond.id) }
  snapshot.document.stereo = kind === 'center'
    ? { atoms: [{ atom: 'a5', configuration: { kind: 'tetrahedral', neighbors: ['a6', 'a7', 'a8', 'a9'], turn: 'anticlockwise' } }], bonds: [], groups: [] }
    : { atoms: [], bonds: [{ bond: 'b5', configuration: { kind: 'relative', begin_neighbor: 'a5', end_neighbor: 'a9', relation: 'same_side' } }], groups: [] }
  snapshot.document.revision = 4
  snapshot.session.unsaved_changes = true
  snapshot.session.undo_depth = 1
  snapshot.notes = attachAfter().notes
  return { parameters, result: { source, snapshot, mapping, connection } }
}
