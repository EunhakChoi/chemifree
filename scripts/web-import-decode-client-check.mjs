// Pure decoding and card sentences of the Import module (apps/web/src/lib/import-detect.ts): the bytes
// each reader receives, the UTF-8 refusal, base64 for CDX, and the line the card shows for a detection.
// Expected values are written out here, never recomputed with the module under test.
import assert from 'node:assert/strict'
import { test } from 'node:test'

// The module has no imports and no `@/` alias, so it loads without the alias hooks.
const { decodeImportFile, detectImportFile, detectImportText, detectionSentence, toBase64 } = await import(
  '../apps/web/src/lib/import-detect.ts'
)

const BOM = [0xef, 0xbb, 0xbf]
const NOT_UTF8 = 'The selected file is not valid UTF-8.'
const MOL_BLOCK = 'mol\n  test\n\n  0  0  0  0  0  0  0  0  0  0999 V2000\nM  END\n'
const encode = (text) => new TextEncoder().encode(text)
const cyclicBytes = (length) => Uint8Array.from({ length }, (_, index) => index % 256)
const standardBase64 = (bytes) => Buffer.from(bytes).toString('base64')

// Only cdxml keeps a leading byte-order mark. mol, sdf and smiles drop it, as their readers did before.
for (const [format, verb, source] of [
  ['cdxml', 'keeps', '﻿<CDXML/>'],
  ['mol', 'drops', '<CDXML/>'],
  ['sdf', 'drops', '<CDXML/>'],
  ['smiles', 'drops', '<CDXML/>'],
]) {
  test(`decoding ${format} ${verb} a leading byte-order mark`, () => {
    assert.deepEqual(decodeImportFile(new Uint8Array([...BOM, ...Buffer.from('<CDXML/>')]), format), { ok: true, source })
  })
}

for (const format of ['cdxml', 'mol', 'sdf', 'smiles']) {
  test(`decoding ${format} refuses bytes that are not UTF-8`, () => {
    assert.deepEqual(decodeImportFile(new Uint8Array([0xc3, 0x28]), format), { ok: false, message: NOT_UTF8 })
  })
}

test('toBase64 of 0x00 to 0xFF repeated 300 times equals standard base64', () => {
  const bytes = cyclicBytes(256 * 300)
  assert.equal(toBase64(bytes), standardBase64(bytes))
})

test('decoding a CDX file sends the same canonical padded base64', () => {
  const bytes = cyclicBytes(256 * 300)
  assert.deepEqual(decodeImportFile(bytes, 'cdx'), { ok: true, source: standardBase64(bytes) })
})

// toBase64 encodes in 32 KiB chunks, so the lengths on either side of a chunk edge are checked, and so
// is each padding shape (none, one '=', two '=').
for (const length of [0, 1, 2, 3, 32767, 32768, 32769]) {
  test(`toBase64 of a ${length}-byte input equals standard base64`, () => {
    const bytes = cyclicBytes(length)
    assert.equal(toBase64(bytes), standardBase64(bytes))
  })
}

for (const [label, detect, chosen, expected] of [
  ['a detected file name', () => detectImportFile('a.mol', encode(MOL_BLOCK)), null, 'Detected MOL from the .mol file name.'],
  [
    'a conflict between a file name and its content',
    () => detectImportFile('x.mol', encode('<CDXML></CDXML>')),
    null,
    'Could not tell: the .mol name says MOL but the content looks like CDXML.',
  ],
  ['a choice that overrides the detected format', () => detectImportText('CCO'), 'cdxml', 'Detected SMILES; you chose CDXML.'],
  ['a detected element in pasted text', () => detectImportText('<CDXML/>'), null, 'Detected CDXML from its <CDXML> element.'],
]) {
  test(`detectionSentence explains ${label}`, () => {
    assert.equal(detectionSentence(detect(), chosen), expected)
  })
}
