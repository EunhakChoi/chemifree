import assert from 'node:assert/strict'
import { test } from 'node:test'

import { ringSource } from './lib/ring-fixture.mjs'

const { isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { abbreviationTargets, compactView, ethylLabel } = await import('../apps/web/src/editor/abbreviation.ts')
const { atomIndex, hitTest, drawingStrokes } = await import('../apps/web/src/editor/geometry.ts')
const { marqueeSelection, selectionMove } = await import('../apps/web/src/editor/selection-geometry.ts')
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')

function source() {
  const response = ringSource()
  response.document.atoms = ['O', 'C', 'C'].map((element, index) => ({ id: `a${index + 1}`, kind: 'element', element, query: null,
    charge: 0, isotope: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [],
    position: [{ x: 0, y: 0 }, { x: 30, y: 0 }, { x: 45, y: 25 }][index] }))
  response.document.bonds.push({ id: 'b2', begin: 'a2', end: 'a3', order: 'single', query: null, mark: null })
  response.document.scene.instances[0].positions = response.document.atoms.map((atom) => ({ atom: atom.id, ...atom.position }))
  response.document.abbreviations = [{ instance: 's1', anchor: 'a2', terminal: 'a3', display_label: 'Et' }]
  return response
}

test('compact replies retain and validate the exact three-atom graph', () => {
  const value = source()
  assert.equal(isDocumentResponse(value), true)
  const invalid = structuredClone(value)
  invalid.document.abbreviations[0].terminal = 'a1'
  assert.equal(isDocumentResponse(invalid), false, 'oxygen cannot become hidden Et carbon')
  const duplicate = structuredClone(value)
  duplicate.document.abbreviations.push(duplicate.document.abbreviations[0])
  assert.equal(isDocumentResponse(duplicate), false)
  const boundary = structuredClone(value)
  boundary.document.bonds[1].order = 'double'
  assert.equal(isDocumentResponse(boundary), false)
  const charged = structuredClone(value)
  charged.document.atoms[2].charge = 1
  assert.equal(isDocumentResponse(charged), false)
  const marked = structuredClone(value)
  marked.document.bonds[0].mark = { kind: 'wavy' }
  assert.equal(isDocumentResponse(marked), false)
  const malformed = structuredClone(value)
  malformed.document.abbreviations = null
  assert.equal(isDocumentResponse(malformed), false)
})

test('visible projection has no hidden atom or internal bond hit but retains the full source', () => {
  const document = source().document
  const original = structuredClone(document)
  const view = compactView(document, 's1')
  assert.deepEqual(view.atoms.map((atom) => atom.id), ['a1', 'a2'])
  assert.deepEqual(view.bonds.map((bond) => bond.id), ['b1'])
  const label = ethylLabel({ font: 'sans-serif', size: 12, carbons: 'default', weight: 'normal', style: 'normal', underline: false, outline: false, shadow: false })
  const labels = new Map([['a2', label]])
  assert.equal(hitTest(view, { x: 45, y: 25 }, new Set(), atomIndex(view), labels), null)
  assert.equal(hitTest(view, { x: 38, y: 14 }, new Set(), atomIndex(view), labels), null)
  assert.deepEqual(hitTest(view, { x: 30, y: 0 }, new Set(), atomIndex(view), labels), { kind: 'atom', id: 'a2' })
  const strokes = drawingStrokes(view, labels, atomIndex(view), new Map(), 2, 1.2)
  assert.equal(strokes.has('b2'), false)
  const line = strokes.get('b1')[0]
  assert.equal(line.shape, 'line')
  assert.ok(line.x2 < 30 + label.box.x, 'boundary stops outside the compact label box')
  assert.deepEqual(document, original, 'the projection never replaces graph/document state')
})

test('compact click and marquee select the whole pair; hidden centers alone cannot select it', () => {
  const document = source().document
  assert.deepEqual(abbreviationTargets(document, 's1', 'a2'), ['a2', 'a3', 'b2'])
  assert.deepEqual([...marqueeSelection(document, 's1', { x: 28, y: -2 }, { x: 32, y: 2 }, new Set(), false, true)], ['a2', 'a3', 'b2'])
  assert.deepEqual([...marqueeSelection(document, 's1', { x: 43, y: 23 }, { x: 47, y: 27 }, new Set(), false, true)], [])
  assert.deepEqual(selectionMove(document, new Set(abbreviationTargets(document, 's1', 'a2')), 's1', { x: 7, y: -2 }), [{ op: 'move_selection', instance: 's1', delta: { x: 7, y: -2 }, selection: { objects: [], atoms: ['a2', 'a3'], bonds: ['b2'] } }])
})

test('queued explicit commands lapse if the placement or a retained atom disappears', () => {
  const document = source().document
  const command = { op: 'collapse_ethyl', instance: 's1', anchor: 'a2', terminal: 'a3' }
  assert.deepEqual(stillApplicable([command], document), [command])
  const missing = structuredClone(document)
  missing.atoms = missing.atoms.filter((atom) => atom.id !== 'a3')
  assert.equal(stillApplicable([command], missing), null)
  assert.equal(stillApplicable([{ op: 'expand_ethyl', instance: 's99', anchor: 'a2' }], document), null)
})
