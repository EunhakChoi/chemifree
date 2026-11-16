// Checks how the Import card detects a pasted text or a picked file. Each case asserts the exact
// ImportDetection object that the pure module returns, so a change to a format, a basis or an
// ambiguity reason fails here. The expected objects are written out, not derived from the module.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { detectImportFile, detectImportText, formatFromExtension } from '../apps/web/src/lib/import-detect.ts'

const OXYGEN_MOL = [
  'Clipboard oxygen',
  '  Chemifree test',
  '',
  '  1  0  0  0  0  0            999 V2000',
  '    1.0000    2.0000    0.0000 O   0  0  0  0  0  0  0  0  0  0  0  0',
  'M  END',
  '',
].join('\n')
const EMPTY_MOL = ['Empty', '  Chemifree test', '', '  0  0  0  0  0  0            999 V2000', 'M  END', ''].join('\n')
// A V3000 counts line and no M  END line, so only the counts rule can match it.
const V3000_WITHOUT_END = 'T\n\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\n'
// A leading U+FEFF byte-order mark, then an XML declaration and CRLF line endings.
const CDXML_WITH_BOM =
  '﻿<?xml version="1.0"?>\r\n<CDXML><page id="7"><fragment id="9"/><fragment id="12"/></page></CDXML>\r\n'
const CDX_HEADER = Buffer.concat([Buffer.from('VjCD0100'), Buffer.alloc(16)])
const NOT_UTF8 = Uint8Array.of(0xc3, 0x28, 0xff)
const encode = (text) => new TextEncoder().encode(text)

const SMILES_TEXT = { kind: 'detected', format: 'smiles', basis: 'smiles-text' }
const CDXML_ELEMENT = { kind: 'detected', format: 'cdxml', basis: 'cdxml-element' }
const SDF_BY_DELIMITER = { kind: 'detected', format: 'sdf', basis: 'sdf-delimiter' }
const MOL_END = { kind: 'detected', format: 'mol', basis: 'mol-end' }
const MOL_COUNTS = { kind: 'detected', format: 'mol', basis: 'mol-counts' }
const NOT_RECOGNIZED = { kind: 'ambiguous', reason: 'not-recognized', extension: null, content: null }

// Case 1: the extension table (I6). Null means the name alone does not decide the format.
const EXTENSION_ROWS = [
  ['a.SMI', 'smiles'],
  ['a.smiles', 'smiles'],
  ['a.mol', 'mol'],
  ['a.SDF', 'sdf'],
  ['a.sd', 'sdf'],
  ['a.cdxml', 'cdxml'],
  ['a.CDX', 'cdx'],
  ['a.xml', null],
  ['a.txt', null],
  ['a', null],
]

// Case 3: pasted CDXML.
const CASE_3_ROWS = [
  { label: 'a BOM, an XML declaration and CRLF endings is cdxml by its element', text: CDXML_WITH_BOM, expected: CDXML_ELEMENT },
  {
    label: 'XML without a CDXML element is ambiguous',
    text: '<svg/>',
    expected: { kind: 'ambiguous', reason: 'xml-not-cdxml', extension: null, content: null },
  },
  { label: 'leading spaces before <CDXML> are cdxml by its element', text: '  <CDXML>', expected: CDXML_ELEMENT },
]

// Case 4: an SDF delimiter wins over the MOL end line that every record also has.
const CASE_4_ROWS = [
  { label: 'two MOL records joined by $$$$ are sdf', text: `${OXYGEN_MOL}$$$$\n${OXYGEN_MOL}`, expected: SDF_BY_DELIMITER },
  { label: 'a $$$$ line ending CRLF is sdf', text: `${OXYGEN_MOL}$$$$\r\n`, expected: SDF_BY_DELIMITER },
  { label: 'a $$$$ line with trailing spaces is sdf', text: `${OXYGEN_MOL}$$$$  \n`, expected: SDF_BY_DELIMITER },
]

// Case 5: pasted MOL.
const CASE_5_ROWS = [
  { label: 'a V2000 record is mol by its M  END line', text: OXYGEN_MOL, expected: MOL_END },
  { label: 'an empty V2000 record is mol by its M  END line', text: EMPTY_MOL, expected: MOL_END },
  { label: 'a V3000 header with no M  END is mol by its counts line', text: V3000_WITHOUT_END, expected: MOL_COUNTS },
]

// Case 6: pasted SMILES, not recognized text, and blank text.
const CASE_6_ROWS = [
  { label: 'CCO with a blank line after it is smiles', text: 'CCO\nCC  \n', expected: SMILES_TEXT },
  { label: 'CC>>CO is smiles', text: 'CC>>CO', expected: SMILES_TEXT },
  { label: '[C@H](F)(Cl)Br is smiles', text: '[C@H](F)(Cl)Br', expected: SMILES_TEXT },
  { label: 'VmpDRDAxMDA= is smiles', text: 'VmpDRDAxMDA=', expected: SMILES_TEXT },
  { label: 'CéO is not recognized', text: 'CéO', expected: NOT_RECOGNIZED },
  { label: 'a leading space is not recognized', text: ' CC', expected: NOT_RECOGNIZED },
  { label: 'Hello, world! is not recognized', text: 'Hello, world!', expected: NOT_RECOGNIZED },
  { label: 'an empty text gives no detection', text: '', expected: null },
  { label: 'a whitespace-only text gives no detection', text: '  \n', expected: null },
]

// Case 10: pasted text that also carries a CDX header-looking start, plus the other pasted inputs above.
const CASE_10_EXTRA_ROWS = [
  { label: 'VjCD0100 pasted as text is smiles', text: 'VjCD0100', expected: SMILES_TEXT },
  { label: 'VjCD pasted as text is smiles', text: 'VjCD', expected: SMILES_TEXT },
  { label: '<CDXML></CDXML> pasted as text is cdxml', text: '<CDXML></CDXML>', expected: CDXML_ELEMENT },
  { label: 'arbitrary text pasted as text is smiles', text: 'arbitrary text', expected: SMILES_TEXT },
  { label: 'an SDF body pasted as text is sdf', text: `${OXYGEN_MOL}$$$$\n`, expected: SDF_BY_DELIMITER },
]

for (const [name, format] of EXTENSION_ROWS) {
  test(`case 1: formatFromExtension(${name}) is ${format}`, () => {
    assert.equal(formatFromExtension(name), format)
  })
}

for (const row of [
  {
    label: 'x.bin with the VjCD0100 header is cdx by its signature',
    name: 'x.bin',
    bytes: CDX_HEADER,
    expected: { kind: 'detected', format: 'cdx', basis: 'cdx-signature' },
  },
  {
    label: 'x.bin with the bare VjCD signature is cdx by its signature',
    name: 'x.bin',
    bytes: Buffer.from('VjCD'),
    expected: { kind: 'detected', format: 'cdx', basis: 'cdx-signature' },
  },
  {
    label: 'x.cdx with CDXML content is a conflict that keeps both formats',
    name: 'x.cdx',
    bytes: encode('<CDXML></CDXML>'),
    expected: { kind: 'ambiguous', reason: 'conflict', extension: 'cdx', content: 'cdxml' },
  },
  {
    label: 'x.cdx with arbitrary text is cdx by its extension',
    name: 'x.cdx',
    bytes: encode('arbitrary text'),
    expected: { kind: 'detected', format: 'cdx', basis: 'extension' },
  },
]) {
  test(`case 2: ${row.label}`, () => {
    assert.deepEqual(detectImportFile(row.name, row.bytes), row.expected)
  })
}

for (const row of CASE_3_ROWS) {
  test(`case 3: ${row.label}`, () => {
    assert.deepEqual(detectImportText(row.text), row.expected)
  })
}

for (const row of CASE_4_ROWS) {
  test(`case 4: ${row.label}`, () => {
    assert.deepEqual(detectImportText(row.text), row.expected)
  })
}

for (const row of CASE_5_ROWS) {
  test(`case 5: ${row.label}`, () => {
    assert.deepEqual(detectImportText(row.text), row.expected)
  })
}

for (const row of CASE_6_ROWS) {
  test(`case 6: ${row.label}`, () => {
    assert.deepEqual(detectImportText(row.text), row.expected)
  })
}

for (const row of [
  {
    label: 'x.mol with an SDF body is a conflict of mol name and sdf content',
    name: 'x.mol',
    bytes: encode(`${OXYGEN_MOL}$$$$\n`),
    expected: { kind: 'ambiguous', reason: 'conflict', extension: 'mol', content: 'sdf' },
  },
  {
    label: 'x.sdf with one MOL record and no $$$$ is sdf by its extension',
    name: 'x.sdf',
    bytes: encode(OXYGEN_MOL),
    expected: { kind: 'detected', format: 'sdf', basis: 'extension' },
  },
  {
    label: 'x.smi with a MOL body is a conflict of smiles name and mol content',
    name: 'x.smi',
    bytes: encode(OXYGEN_MOL),
    expected: { kind: 'ambiguous', reason: 'conflict', extension: 'smiles', content: 'mol' },
  },
  {
    label: 'x.mol with CCO is mol by its extension',
    name: 'x.mol',
    bytes: encode('CCO'),
    expected: { kind: 'detected', format: 'mol', basis: 'extension' },
  },
]) {
  test(`case 7: ${row.label}`, () => {
    assert.deepEqual(detectImportFile(row.name, row.bytes), row.expected)
  })
}

for (const row of [
  {
    label: 'x.bin with non-UTF-8 bytes is binary without a CDX signature',
    name: 'x.bin',
    bytes: NOT_UTF8,
    expected: { kind: 'ambiguous', reason: 'binary-not-cdx', extension: null, content: null },
  },
  {
    label: 'x.mol with the same non-UTF-8 bytes is mol by its extension',
    name: 'x.mol',
    bytes: NOT_UTF8,
    expected: { kind: 'detected', format: 'mol', basis: 'extension' },
  },
]) {
  test(`case 8: ${row.label}`, () => {
    assert.deepEqual(detectImportFile(row.name, row.bytes), row.expected)
  })
}

// Pasted text never reaches format cdx, which only a file's bytes can give.
for (const row of [...CASE_3_ROWS, ...CASE_4_ROWS, ...CASE_5_ROWS, ...CASE_6_ROWS, ...CASE_10_EXTRA_ROWS]) {
  test(`case 10: ${row.label}`, () => {
    const detection = detectImportText(row.text)
    assert.deepEqual(detection, row.expected)
    assert.notEqual(detection?.format, 'cdx', 'pasted text is never detected as cdx')
  })
}

test('case 11: a $$$$ line after unit 256 Ki is not scanned, so the text is smiles and not sdf', () => {
  assert.deepEqual(detectImportText('C'.repeat(300 * 1024 - 6) + '\n$$$$\n'), SMILES_TEXT)
})
