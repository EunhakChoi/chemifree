// User graph template file checks that run without a browser: the 64 KiB read limit, strict
// UTF-8 decoding, the download filename and the crossing-bond refusal. The real-backend browser
// flow (download, reselect, insert and the request-ordering guards) is test:browser:user-template.
import assert from 'node:assert/strict'
import { test } from 'node:test'

const { MAX_TEMPLATE_BYTES, checkTemplateFileSize, decodeTemplateFile, templateDownloadName, templateSaveRefusal } =
  await import('../apps/web/src/lib/user-template-file.ts')

test('template files are read up to exactly 64 KiB', () => {
  assert.equal(MAX_TEMPLATE_BYTES, 65536)
  assert.doesNotThrow(() => checkTemplateFileSize(65536))
  assert.throws(() => checkTemplateFileSize(65537), { message: 'Template files must be at most 64 KiB.' })
})

test('template bytes decode as strict UTF-8 and refuse blank files', () => {
  assert.throws(() => decodeTemplateFile(new Uint8Array([0xc3, 0x28])), TypeError)
  for (const blank of ['', ' \n']) {
    assert.throws(() => decodeTemplateFile(new TextEncoder().encode(blank)), { message: 'Choose a non-empty Chemifree graph template file.' })
  }
  // A leading byte order mark is kept so the service refuses it, rather than being dropped here.
  assert.equal(decodeTemplateFile(new Uint8Array([0xef, 0xbb, 0xbf, 0x7b, 0x7d])), '﻿{}')
  assert.equal(decodeTemplateFile(new Uint8Array([0x7b, 0x22, 0xed, 0x95, 0x9c, 0x22, 0x7d])), '{"한"}')
})

test('download names keep letters and digits and fall back to template', () => {
  assert.equal(templateDownloadName('  my fragment/x  '), 'my_fragment_x.chemifree-fragment.json')
  assert.equal(templateDownloadName('...'), 'template.chemifree-fragment.json')
  assert.equal(templateDownloadName('..hidden'), 'hidden.chemifree-fragment.json')
  assert.equal(templateDownloadName('벤젠-1'), '벤젠-1.chemifree-fragment.json')
})

test('a selection with crossing bonds is refused before saving', () => {
  assert.equal(templateSaveRefusal(0), null)
  assert.equal(templateSaveRefusal(1),
    'Select complete graph components before saving a template. Crossing bonds are not template attachment instructions.')
})
