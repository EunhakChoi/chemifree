import assert from 'node:assert/strict'
import { test } from 'node:test'

import { ringSource } from './lib/ring-fixture.mjs'

const { validCaptionRuns, isDocumentResponse } = await import('../apps/web/src/lib/document-contract.ts')
const { selectionMove, selectionTransform } = await import('../apps/web/src/editor/selection-geometry.ts')
const { sceneGeometryCommands } = await import('../apps/web/src/editor/scene-geometry.ts')
const { stillApplicable } = await import('../apps/web/src/editor/commands.ts')

const runs = () => [{ text: '한글 e\u0301 👩‍🔬 H', script: 'baseline', weight: 'normal', style: 'normal' }, { text: '2', script: 'subscript', weight: 'normal', style: 'italic' }, { text: '+', script: 'superscript', weight: 'bold', style: 'italic' }]
function source() {
  const result = ringSource()
  result.document.scene.roots.push('s8')
  result.document.scene.objects.push({ kind: 'caption', id: 's8', parent: null, anchor: { x: 100, y: 40 }, runs: runs() })
  return result
}

test('Unicode captions preserve code points, run boundaries and the native White_Space policy', () => {
  assert.equal(isDocumentResponse(source()), true)
  const feff = source()
  feff.document.scene.objects.at(-1).runs = [{ text: '\ufeff', script: 'baseline', weight: 'normal', style: 'normal' }]
  assert.equal(isDocumentResponse(feff), true)
  assert.equal(validCaptionRuns([{ text: '\ufeff', script: 'baseline', weight: 'normal', style: 'normal' }]), true)
  assert.equal(validCaptionRuns([{ text: '\u2003', script: 'baseline', weight: 'normal', style: 'normal' }]), false)
  assert.equal(validCaptionRuns([{ text: '한'.repeat(5461), script: 'baseline', weight: 'normal', style: 'normal' }]), true)
  assert.equal(validCaptionRuns([{ text: '한'.repeat(5462), script: 'baseline', weight: 'normal', style: 'normal' }]), false)
  assert.equal(validCaptionRuns(Array.from({ length: 32 }, () => ({ text: 'A', script: 'baseline', weight: 'normal', style: 'normal' }))), true)
  assert.equal(validCaptionRuns(Array.from({ length: 33 }, () => ({ text: 'A', script: 'baseline', weight: 'normal', style: 'normal' }))), false)
})

for (const [name, run] of [
  ['array script', { text: 'H', script: ['baseline'] }],
  ['object script', { text: 'H', script: {} }],
  ['unknown script', { text: 'H', script: 'auto' }],
  ['missing script', { text: 'H' }],
  ['empty run', { text: '', script: 'baseline', weight: 'normal', style: 'normal' }],
  ['newline', { text: 'a\nb', script: 'baseline', weight: 'normal', style: 'normal' }],
  ['control', { text: 'a\u0085b', script: 'baseline', weight: 'normal', style: 'normal' }],
  ['line separator', { text: 'a\u2028b', script: 'baseline', weight: 'normal', style: 'normal' }],
  ['unpaired UTF-16', { text: '\ud800', script: 'baseline', weight: 'normal', style: 'normal' }],
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

test('an empty group does not become movable because it references an outside caption', () => {
  const result = source()
  const { document } = result
  document.scene.roots.push('s9')
  document.scene.objects.push({ kind: 'group', id: 's9', parent: null, children: [] })
  document.scene.references.push({ source: 's9', target: { kind: 'object', id: 's8' } })
  assert.equal(isDocumentResponse(result), true)
  assert.equal(selectionMove(document, new Set(['s9']), null, { x: 3, y: 4 }), null)
})

test('a group containing only a caption still has a movable anchor', () => {
  const result = source()
  const { document } = result
  const caption = document.scene.objects.pop()
  document.scene.roots = document.scene.roots.filter((id) => id !== 's8')
  document.scene.roots.push('s9')
  document.scene.objects.push({ kind: 'group', id: 's9', parent: null, children: ['s8'] }, { ...caption, parent: 's9' })
  assert.equal(isDocumentResponse(result), true)
  assert.deepEqual(selectionMove(document, new Set(['s9']), null, { x: 3, y: 4 }), [{
    op: 'move_selection', selection: { objects: ['s9'], atoms: [], bonds: [] }, delta: { x: 3, y: 4 },
  }])
})

for (const [field, value] of [['weight', 'heavy'], ['weight', 700], ['weight', null], ['style', 'oblique'], ['style', null], ['style', undefined]]) {
  test(`caption response rejects ${field}=${String(value)}`, () => {
    const result = source()
    result.document.scene.objects.at(-1).runs[0][field] = value
    assert.equal(isDocumentResponse(result), false)
  })
}
