import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ringSource } from './lib/ring-fixture.mjs'
import { sceneCutSource, sceneCutCopy } from './lib/scene-cut-fixture.mjs'

const { atomLabel, labelStyleOf } = await import('../apps/web/src/editor/label.ts')
const { ethylLabel } = await import('../apps/web/src/editor/abbreviation.ts')
const { DEFAULT_SETTINGS, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { setSettings, preferenceChange } = await import('../apps/web/src/editor/settings.ts')
const { sceneClipboardCopyMatches } = await import('../apps/web/src/lib/scene-clipboard-contract.ts')

test('chemical label style is a checked complete setting and a queued preference/document edit', () => {
  const reply = ringSource()
  assert.equal(isDocumentResponse(reply), true)
  assert.deepEqual(setSettings({ label_style: 'italic' })(reply.document), [{ op: 'update_settings', label_style: 'italic' }])
  reply.document.settings.label_style = 'italic'
  assert.equal(setSettings({ label_style: 'italic' })(reply.document), null)
  assert.deepEqual(preferenceChange({ ui: { theme: 'system' }, new_document_settings: DEFAULT_SETTINGS }, { label_style: 'italic' }),
    { ui: { theme: 'system' }, new_documents: { label_style: 'italic' } })
  for (const value of [undefined, null, 'oblique', 500, [], {}]) {
    reply.document.settings.label_style = value
    assert.equal(isDocumentResponse(reply), false, JSON.stringify(value))
  }
})

test('italic retains 400/500 or 700 chemical weights and normal marks for every family/size', () => {
  const atom = { ...ringSource().document.atoms[0], element: 'N', charge: -2, isotope: 15,
    hydrogens: { mode: 'specified', count: 3 }, radical: 'unknown', attachment_points: [1] }
  for (const font of ['sans-serif', 'serif', 'monospace']) {
    for (const size of [4, 72]) {
      for (const weight of ['normal', 'bold']) {
        const normal = labelStyleOf({ ...DEFAULT_SETTINGS, label_font: font, label_size_pt: size, label_weight: weight })
        const italic = { ...normal, style: 'italic' }
        for (const left of [false, true]) {
          const before = atomLabel(atom, 1, left, normal)
          const after = atomLabel(atom, 1, left, italic)
          assert.deepEqual(after.chunks.flatMap((chunk) => chunk.runs.map((run) => run.weight)), before.chunks.flatMap((chunk) => chunk.runs.map((run) => run.weight)))
          assert.ok(after.chunks.every((chunk) => chunk.runs.every((run) => run.style === 'italic')))
          assert.ok(after.marks.every((chunk) => chunk.runs.every((run) => run.style === 'normal' && run.weight === 400)))
        }
        assert.equal(ethylLabel(italic).chunks[0].runs[0].style, 'italic')
        assert.equal(ethylLabel(italic).chunks[0].runs[0].weight, weight === 'bold' ? 700 : 500)
      }
    }
  }
})

test('actual older graphics payloads omit style and inherit normal while explicit invalid styles stay refused', () => {
  const source = sceneCutSource()
  const copied = sceneCutCopy(source)
  const payload = JSON.parse(copied.payload)
  delete payload.settings.label_style
  const requested = { objects: ['s4'], atoms: [], bonds: [] }
  const matches = (value, settings = source.document.settings) => sceneClipboardCopyMatches(JSON.stringify(value), copied.copied, copied.roots, requested, source.document, settings)
  assert.equal(matches(payload), true)
  assert.equal(matches(payload, { ...source.document.settings, label_style: 'italic' }), false)
  for (const value of [null, 'oblique', 500, [], {}]) {
    assert.equal(matches({ ...payload, settings: { ...payload.settings, label_style: value } }), false)
  }
  delete payload.settings.label_weight
  assert.equal(matches(payload), true, 'pre-weight/pre-style payloads retain both normal defaults')
})
