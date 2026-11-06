// Data at the public HTTP boundary; expected deletion keeps s3 as a singleton parent.
export function sceneCutSource() {
  const settings = { bond_length_pt: 30, line_width_pt: 1.2, label_font: 'sans-serif', label_weight: 'normal', label_style: 'normal', label_underline: false, label_outline: false, label_shadow: false, label_size_pt: 12, carbon_labels: 'default', display_unit: 'pt' }
  const atoms = ['C', 'O'].map((element, index) => ({ id: `a${index + 1}`, element, kind: 'element', query: null,
    charge: 0, isotope: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [], position: { x: index * 30, y: 0 } }))
  return { api_version: 'v1', document: { id: 'drawing', title: 'Graphic Cut', revision: 3, atoms,
    bonds: [{ id: 'b1', begin: 'a1', end: 'a2', order: 'single', query: null, mark: null }],
    stereo: { atoms: [], bonds: [], groups: [] }, fragment_displays: [], molecule: { id: 'm1' }, settings,
    scene: { pages: [{ id: 'p1' }], instances: ['s1', 's2'].map((id, index) => ({ id, page: 'p1', molecule: 'm1',
      positions: [{ atom: 'a1', x: index * 100, y: 0 }, { atom: 'a2', x: index * 100 + 30, y: 0 }] })),
      roots: ['s1', 's2', 's3'], objects: [
        { kind: 'instance', id: 's1', parent: null, molecule: 'm1' },
        { kind: 'instance', id: 's2', parent: null, molecule: 'm1' },
        { kind: 'group', id: 's3', parent: null, children: ['s4', 's7'] },
        { kind: 'group', id: 's4', parent: 's3', children: ['s6', 's5'] },
        { kind: 'graphic', id: 's6', parent: 's4', shape: 'line', points: [{ x: 1, y: 2 }, { x: 31, y: 2 }] },
        { kind: 'graphic', id: 's5', parent: 's4', shape: 'arrow', points: [{ x: 1, y: 12 }, { x: 31, y: 12 }] },
        { kind: 'graphic', id: 's7', parent: 's3', shape: 'rectangle', points: [{ x: 50, y: 0 }, { x: 80, y: 30 }] },
      ], references: [
        { source: 's5', target: { kind: 'object', id: 's6' } },
        { source: 's6', target: { kind: 'object', id: 's5' } },
        { source: 's6', target: { kind: 'object', id: 's6' } },
        { source: 's7', target: { kind: 'object', id: 's1' } },
      ] }, selection: { objects: [], atoms: [], bonds: [] } },
    session: { saved_revision: 3, unsaved_changes: false, undo_depth: 0, redo_depth: 0, draft_revision: null }, recovery: null, notes: [] }
}

export function sceneCutCopy(source = sceneCutSource()) {
  const objects = source.document.scene.objects.slice(3, 6).map(({ parent, ...object }) => object)
  return { snapshot: structuredClone(source), copied: { objects: ['s4', 's6', 's5'], atoms: [], bonds: [] }, roots: ['s4'],
    payload: JSON.stringify({ format: 'chemifree.clipboard', version: 2, kind: 'graphics', settings: source.document.settings,
      roots: ['s4'], objects, references: source.document.scene.references.slice(0, 3) }) }
}

export function sceneCutAfter(source = sceneCutSource()) {
  const after = structuredClone(source)
  after.document.revision = 4
  after.document.scene.objects = [after.document.scene.objects[0], after.document.scene.objects[1],
    { kind: 'group', id: 's3', parent: null, children: ['s7'] }, after.document.scene.objects[6]]
  after.document.scene.references = [{ source: 's7', target: { kind: 'object', id: 's1' } }]
  after.document.selection = { objects: [], atoms: [], bonds: [] }
  after.session.unsaved_changes = true
  after.session.undo_depth = 1
  return after
}
