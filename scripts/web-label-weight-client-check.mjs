import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ringSource } from './lib/ring-fixture.mjs'

const { atomLabel, DEFAULT_LABEL_STYLE, labelStyleOf } = await import('../apps/web/src/editor/label.ts')
const { ethylLabel } = await import('../apps/web/src/editor/abbreviation.ts')
const { DEFAULT_SETTINGS, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { setSettings, preferenceChange } = await import('../apps/web/src/editor/settings.ts')

test('chemical label weight is a checked setting and a queued document/preference edit', () => {
  const reply = ringSource()
  reply.document.settings = { ...DEFAULT_SETTINGS }
  assert.equal(isDocumentResponse(reply), true)
  assert.deepEqual(setSettings({ label_weight: 'bold' })(reply.document), [{ op: 'update_settings', label_weight: 'bold' }])
  reply.document.settings.label_weight = 'bold'
  assert.equal(setSettings({ label_weight: 'bold' })(reply.document), null)
  for (const value of [null, 'heavy', 700, [], {}]) {
    reply.document.settings.label_weight = value
    assert.equal(isDocumentResponse(reply), false, JSON.stringify(value))
  }
  const change = preferenceChange({ ui: { theme: 'system' }, new_document_settings: DEFAULT_SETTINGS }, { label_weight: 'bold' })
  assert.deepEqual(change, { ui: { theme: 'system' }, new_documents: { label_weight: 'bold' } })
})

for (const font of ['sans-serif', 'serif', 'monospace']) {
  for (const size of [4, 72]) {
    test(`${font} ${size}pt chemical runs use 700, normal retains 400/500 and radical marks`, () => {
      const atom = { ...ringSource().document.atoms[0], element: 'N', query: null, kind: 'element', charge: -2, isotope: 15,
        hydrogens: { mode: 'specified', count: 3 }, radical: 'unknown', attachment_points: [1] }
      const normal = labelStyleOf({ ...DEFAULT_SETTINGS, label_font: font, label_size_pt: size })
      const bold = { ...normal, weight: 'bold' }
      const original = structuredClone(atom)
      for (const left of [false, true]) {
        const label = atomLabel(atom, 1, left, bold)
        assert.deepEqual(label.chunks[0].runs.map((run) => run.weight), [700, 700, 700, 700, 700])
        const unchanged = atomLabel(atom, 1, left, normal)
        assert.deepEqual(unchanged.chunks[0].runs.map((run) => run.weight), left ? [500, 400, 400, 500, 400] : [400, 500, 500, 400, 400])
        assert.deepEqual(label.marks.map((chunk) => chunk.runs.map((run) => run.weight)), unchanged.marks.map((chunk) => chunk.runs.map((run) => run.weight)))
        assert.deepEqual(label.dots.map((dot) => dot.radius), unchanged.dots.map((dot) => dot.radius))
      }
      const query = { ...atom, kind: 'query', element: '', query: { kind: 'list', negated: true, elements: ['C', 'Cl', 'Br', 'I'] }, charge: 0, isotope: null, hydrogens: { mode: 'inferred' }, radical: 'none', attachment_points: [] }
      assert.equal(atomLabel(query, 1, false, bold).chunks[0].runs[0].weight, 700)
      assert.equal(ethylLabel(bold).chunks[0].runs[0].weight, 700)
      assert.equal(ethylLabel(normal).chunks[0].runs[0].weight, 500)
      assert.deepEqual(atom, original)
      assert.equal(DEFAULT_LABEL_STYLE.weight, 'normal')
    })
  }
}

test('old graphics payload settings normalize only an omitted weight to normal', async () => {
  const { sceneCutSource, sceneCutCopy } = await import('./lib/scene-cut-fixture.mjs')
  const { sceneClipboardCopyMatches } = await import('../apps/web/src/lib/scene-clipboard-contract.ts')
  const source = sceneCutSource()
  const copied = sceneCutCopy(source)
  const payload = JSON.parse(copied.payload)
  delete payload.settings.label_weight
  const requested = { objects: ['s4'], atoms: [], bonds: [] }
  const matches = (value, settings = source.document.settings) => sceneClipboardCopyMatches(JSON.stringify(value), copied.copied, copied.roots, requested, source.document, settings)
  assert.equal(matches(payload), true)
  assert.equal(matches(payload, { ...source.document.settings, label_weight: 'bold' }), false)
  for (const value of [null, 'heavy', 700]) {
    assert.equal(matches({ ...payload, settings: { ...payload.settings, label_weight: value } }), false)
  }
})
