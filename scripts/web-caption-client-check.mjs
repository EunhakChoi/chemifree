import assert from 'node:assert/strict'
import { test } from 'node:test'

import { ringSource } from './lib/ring-fixture.mjs'

const { validCaptionRuns, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { selectionMove, selectionTransform } = await import('../apps/web/src/editor/selection-geometry.ts')
const { sceneGeometryCommands } = await import('../apps/web/src/editor/scene-geometry.ts')
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')

const runs = () => [{ text: '한글 e\u0301 👩‍🔬 H', script: 'baseline' }, { text: '2', script: 'subscript' }, { text: '+', script: 'superscript' }]
function source() {
  const result = ringSource()
  result.document.scene.roots.push('s8')
  result.document.scene.objects.push({ kind: 'caption', id: 's8', parent: null, anchor: { x: 100, y: 40 }, runs: runs() })
  return result
}

test('Unicode captions preserve code points, run boundaries and the native White_Space policy', () => {
  assert.equal(isDocumentResponse(source()), true)
  const feff = source()
  feff.document.scene.objects.at(-1).runs = [{ text: '\ufeff', script: 'baseline' }]
  assert.equal(isDocumentResponse(feff), true)
  assert.equal(validCaptionRuns([{ text: '\ufeff', script: 'baseline' }]), true)
  assert.equal(validCaptionRuns([{ text: '\u2003', script: 'baseline' }]), false)
  assert.equal(validCaptionRuns([{ text: '한'.repeat(5461), script: 'baseline' }]), true)
  assert.equal(validCaptionRuns([{ text: '한'.repeat(5462), script: 'baseline' }]), false)
  assert.equal(validCaptionRuns(Array.from({ length: 32 }, () => ({ text: 'A', script: 'baseline' }))), true)
  assert.equal(validCaptionRuns(Array.from({ length: 33 }, () => ({ text: 'A', script: 'baseline' }))), false)
})

for (const [name, run] of [
  ['array script', { text: 'H', script: ['baseline'] }],
  ['object script', { text: 'H', script: {} }],
  ['unknown script', { text: 'H', script: 'auto' }],
  ['missing script', { text: 'H' }],
  ['empty run', { text: '', script: 'baseline' }],
  ['newline', { text: 'a\nb', script: 'baseline' }],
  ['control', { text: 'a\u0085b', script: 'baseline' }],
  ['line separator', { text: 'a\u2028b', script: 'baseline' }],
  ['unpaired UTF-16', { text: '\ud800', script: 'baseline' }],
]) test(`caption response refuses ${name} instead of accepting a malformed whole snapshot`, () => {
  const result = source()
  result.document.scene.objects.at(-1).runs = [run]
  assert.equal(validCaptionRuns([run]), false)
  assert.equal(isDocumentResponse(result), false)
})

test('caption-only moves retain the captured object and stale edits lapse', () => {
  const { document } = source()
  assert.deepEqual(selectionMove(document, new Set(['s8']), null, { x: 5, y: 7 }), [{
    op: 'move_selection', selection: { objects: ['s8'], atoms: [], bonds: [] }, delta: { x: 5, y: 7 },
  }])
  const edit = [{ op: 'update_caption', object: 's8', anchor: { x: 0, y: 0 }, runs: runs() }]
  assert.deepEqual(stillApplicable(edit, document), edit)
  document.scene.objects.pop()
  document.scene.roots.pop()
  assert.equal(stillApplicable(edit, document), null)
})

test('caption-containing groups refuse complete arrange/transform actions without shrinking the union', () => {
  const { document } = source()
  document.scene.roots = document.scene.roots.filter((id) => id !== 's8')
  document.scene.objects.at(-1).parent = 's3'
  document.scene.objects.find((object) => object.id === 's3').children.push('s8')
  assert.equal(selectionTransform(document, new Set(['s3', 'a1']), 's1', { type: 'rotate', degrees: 90 }), null)
  assert.equal(sceneGeometryCommands(document, new Set(['s3', 's1']), { kind: 'arrange', mode: 'align_left' }), null)
  assert.deepEqual(selectionMove(document, new Set(['s3', 's8']), null, { x: 3, y: 4 })[0].selection.objects, ['s3', 's8'])
})
