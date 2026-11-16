// Checks the format list of the web Export card (P48 S5) in apps/web/src/lib/export-formats.ts: the
// six formats in the Import card's order with SVG picture last, the strict parse of a form value,
// and the download name of each file. Expected values are written out here, never derived from the
// module. The reopen cases check that the Import card detects each written file name as the format it
// was written from, and that an SVG picture is not detected as an import format.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { EXPORT_FORMATS, exportFileName, parseExportFormat } from '../apps/web/src/lib/export-formats.ts'
import { formatFromExtension } from '../apps/web/src/lib/import-detect.ts'

const FORMAT_IDS = ['smiles', 'mol', 'sdf', 'cdxml', 'cdx', 'svg']
const FORMAT_LABELS = ['SMILES', 'MOL', 'SDF', 'CDXML', 'CDX', 'SVG picture']
const REFUSED_VALUES = ['', 'MOL', 'mol-v3000', 'png', 'inchi', ' smiles']
const FILE_NAMES = [
  ['mol', 'chemifree.mol'],
  ['sdf', 'chemifree.sdf'],
  ['cdxml', 'chemifree.cdxml'],
  ['cdx', 'chemifree.cdx'],
  ['svg', 'chemifree.svg'],
]
const REOPENED_FORMATS = ['mol', 'sdf', 'cdxml', 'cdx']

test('the Export card lists the six formats in Import card order, with SVG picture last', () => {
  assert.deepEqual(
    EXPORT_FORMATS.map((format) => format.id),
    FORMAT_IDS,
  )
  assert.deepEqual(
    EXPORT_FORMATS.map((format) => format.label),
    FORMAT_LABELS,
  )
})

for (const id of FORMAT_IDS) {
  test(`parseExportFormat accepts the form value ${id}`, () => {
    assert.equal(parseExportFormat(id), id)
  })
}

for (const value of REFUSED_VALUES) {
  test(`parseExportFormat refuses ${JSON.stringify(value)}`, () => {
    assert.equal(parseExportFormat(value), null, JSON.stringify(value))
  })
}

for (const [format, name] of FILE_NAMES) {
  test(`exportFileName gives ${name} for ${format}`, () => {
    assert.equal(exportFileName(format), name)
  })
}

for (const format of REOPENED_FORMATS) {
  test(`the Import card reads the ${format} export name back as ${format}`, () => {
    assert.equal(formatFromExtension(exportFileName(format)), format)
  })
}

test('the Import card does not read an SVG picture name as an import format', () => {
  assert.equal(formatFromExtension(exportFileName('svg')), null, 'SVG cannot be imported')
})
