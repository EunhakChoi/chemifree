import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ringSource } from './lib/ring-fixture.mjs'
import { sceneCutSource, sceneCutCopy } from './lib/scene-cut-fixture.mjs'

const { DEFAULT_SETTINGS, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { setSettings, preferenceChange } = await import('../apps/web/src/editor/settings.ts')
const { sceneClipboardCopyMatches } = await import('../apps/web/src/lib/scene-clipboard-contract.ts')

test('chemical label underline is a checked complete setting and a queued preference/document edit', () => {
  const reply = ringSource()
  assert.equal(isDocumentResponse(reply), true)
  assert.deepEqual(setSettings({ label_underline: true })(reply.document), [{ op: 'update_settings', label_underline: true }])
  reply.document.settings.label_underline = true
  assert.equal(setSettings({ label_underline: true })(reply.document), null)
  assert.deepEqual(preferenceChange({ ui: { theme: 'system' }, new_document_settings: DEFAULT_SETTINGS }, { label_underline: true }),
    { ui: { theme: 'system' }, new_documents: { label_underline: true } })
  for (const value of [undefined, null, 'oblique', 500, [], {}]) {
    reply.document.settings.label_underline = value
    assert.equal(isDocumentResponse(reply), false, JSON.stringify(value))
  }
})

test('actual older graphics payloads omit style and inherit false while explicit invalid styles stay refused', () => {
  const source = sceneCutSource()
  const copied = sceneCutCopy(source)
  const payload = JSON.parse(copied.payload)
  delete payload.settings.label_underline
  const requested = { objects: ['s4'], atoms: [], bonds: [] }
  const matches = (value, settings = source.document.settings) => sceneClipboardCopyMatches(JSON.stringify(value), copied.copied, copied.roots, requested, source.document, settings)
  assert.equal(matches(payload), true)
  assert.equal(matches(payload, { ...source.document.settings, label_underline: true }), false)
  for (const value of [null, 'oblique', 500, [], {}]) {
    assert.equal(matches({ ...payload, settings: { ...payload.settings, label_underline: value } }), false)
  }
  delete payload.settings.label_weight
  assert.equal(matches(payload), true, 'pre-weight/pre-style payloads retain both normal defaults')
})
