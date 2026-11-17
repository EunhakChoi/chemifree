import assert from 'node:assert/strict'
import { join } from 'node:path'

import { openSavedDocument } from './open-document-smoke.mjs'

// The "Import card" step of browser-smoke.mjs (P48 S3). It drives the one Import entry, in the open
// workspace and on the empty screen, the way a user does, and checks each outcome on the service.
// A refused or ambiguous source must send no document.import operation, so requests are counted.

const SMILES_SOURCE = 'CCO'
const CARBON_MOL = 'Carbon\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n' +
  '    0.0000    1.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n'
const NITROGEN_MOL = 'Nitrogen\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n' +
  '    2.0000    1.0000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n'
const CDXML_TEXT = '<CDXML><page id="1"><fragment id="2"><n id="3" p="1 2"/></fragment></page></CDXML>'
const DESTINATION_WITH_DOCUMENT = 'SMILES, MOL and SDF are added to this drawing. CDXML and CDX create a new drawing and switch to it.'
const DESTINATION_EMPTY = 'CDXML and CDX create a new drawing. SMILES, MOL and SDF are added to an open drawing.'
const SWITCH_NOTE = 'This creates a new drawing and switches to it, like New document. Unsaved changes to this drawing are written to its recovery draft first.'
const TOO_LARGE = 'This web form reads files up to 200 KiB (SMILES 16 KiB, CDXML and CDX 120 KiB). The operation API and CLI have their own limits.'

/** The CDX file of the CDX step in browser-smoke.mjs: one oxygen node at (20, 10) pt. */
function cdxFixture() {
  const u16 = (value) => Buffer.from([value & 0xff, value >> 8])
  const i32 = (value) => {
    const buffer = Buffer.alloc(4)
    buffer.writeInt32LE(value)
    return buffer
  }
  const obj = (tag, id, ...parts) => Buffer.concat([u16(tag), i32(id), ...parts, u16(0)])
  const prop = (tag, data) => Buffer.concat([u16(tag), u16(data.length), data])
  const node = obj(0x8004, 3, prop(0x0200, Buffer.concat([i32(20 * 65536), i32(10 * 65536)])), prop(0x0402, u16(8)))
  const document = obj(0x8000, 0, obj(0x8001, 1, obj(0x8003, 2, node)))
  const header = Buffer.concat([Buffer.from('VjCD0100'), Buffer.from([4, 3, 2, 1]), Buffer.alloc(16)])
  return Buffer.concat([header, document, u16(0)])
}

/** True for a document.import operation request, whether it previews, reads, creates or commits. */
function isImportRequest(request) {
  return request.method() === 'POST' && new URL(request.url()).pathname === '/api/v1/operations' &&
    request.postDataJSON()?.operation === 'document.import'
}

/** A commit into the open document: neither a preview nor a read nor a create. */
function isCommit(request) {
  const parameters = request.postDataJSON()?.parameters
  return parameters?.preview !== true && parameters?.mode === undefined
}

function isCreate(request) {
  return request.postDataJSON()?.parameters?.mode === 'create'
}

/** A response predicate for a document.import operation that `matches` accepts. */
function importResponse(matches) {
  return (response) => response.url().endsWith('/api/v1/operations') && isImportRequest(response.request()) && matches(response.request())
}

/** The Format select of the card, which the user changes to override the detected format. */
function formatSelect(card) {
  return card.getByRole('combobox', { name: 'Format', exact: true })
}

function button(scope, name) {
  return scope.getByRole('button', { name, exact: true })
}

function countElement(atoms, element) {
  return atoms.filter((atom) => atom.element === element).length
}

/** A DataTransfer holding the given files, as a drag or drop carries them. */
function transferOf(page, files) {
  return page.evaluateHandle((entries) => {
    const transfer = new DataTransfer()
    for (const entry of entries) transfer.items.add(new File([new Uint8Array(entry.bytes)], entry.name))
    return transfer
  }, files.map(({ name, bytes }) => ({ name, bytes: [...bytes] })))
}

/** Picks a file with the card's file input. */
function pickFile(card, name, buffer) {
  return card.getByLabel('Import file', { exact: true }).setInputFiles({ name, mimeType: 'text/plain', buffer })
}

export async function importCardSmoke({ page, api, apiWrite, outDir, results, setStep }) {
  setStep('Import card')
  let imports = 0
  const countImport = (request) => {
    if (isImportRequest(request)) imports += 1
  }
  page.on('request', countImport)
  try {
    const created = await apiWrite('/api/v1/documents', { request_id: 'import-card-create', title: 'Import card' })
    const docId = created.body.document.id
    const atoms = async () => (await api(`/api/v1/documents/${docId}`)).document.atoms
    const openDocument = (id) => openSavedDocument(page, api, id)
    await openDocument(docId)
    const card = page.getByRole('region', { name: 'Import', exact: true })
    // Imports leave unsaved changes, so closing asks first; this step discards them.
    const closeDocument = async () => {
      await page.getByRole('button', { name: 'Close document', exact: true }).click()
      const discard = page.getByRole('button', { name: 'Discard and close', exact: true })
      const empty = page.getByText('No document open')
      await empty.or(discard).waitFor()
      if (await discard.isVisible()) await discard.click()
      await empty.waitFor()
    }

    setStep('import_card_single_entry')
    assert.equal(await page.getByLabel('Import file', { exact: true }).count(), 1, 'one file input serves every format')
    assert.equal(await page.getByLabel('Text to import', { exact: true }).count(), 1, 'one text field serves every format')
    const exportRegion = page.getByRole('region', { name: 'Export', exact: true })
    await exportRegion.waitFor()
    for (const name of ['SMILES', 'MOL and SDF', 'CDXML and CDX export', 'SVG drawing export']) assert.equal(await page.getByRole('complementary', { name, exact: true }).count(), 0, `the ${name} panel is gone`)
    const exportFormat = exportRegion.getByRole('combobox', { name: 'Export format', exact: true })
    await exportFormat.selectOption('smiles')
    await exportRegion.locator('[data-format="smiles"]').waitFor()
    for (const label of ['SMILES text', 'Text to import', 'Import file']) assert.equal(await exportRegion.getByLabel(label, { exact: true }).count(), 0, `the SMILES export has no ${label} field`)
    await exportFormat.selectOption('mol')
    await exportRegion.locator('[data-format="mol"]').waitFor()
    assert.equal(await exportRegion.getByLabel('Import file', { exact: true }).count(), 0, 'the MOL and SDF export has no file input')
    assert.equal(await exportRegion.getByRole('button', { name: /Import MOL text/ }).count(), 0, 'the MOL and SDF export has no MOL text import')
    await page.getByText(DESTINATION_WITH_DOCUMENT, { exact: true }).waitFor()
    await page.emulateMedia({ colorScheme: 'light' })
    await card.screenshot({ path: join(outDir, 'editor-import-card.png') })
    results.import_card_single_entry = true

    setStep('import_pick_smiles_file')
    const beforeSmiles = (await atoms()).length
    await pickFile(card, 'x.smi', Buffer.from(SMILES_SOURCE))
    await card.getByTestId('import-detected').waitFor()
    assert.equal(await card.getByTestId('import-detected').textContent(), 'Detected SMILES from the .smi file name.')
    assert.equal(await formatSelect(card).inputValue(), 'smiles')
    await button(card, 'Preview SMILES import').click()
    await card.getByTestId('smiles-preview-summary').waitFor()
    assert.equal(await card.getByTestId('smiles-preview-summary').textContent(), '3 atom(s), 2 bond(s) would be added.')
    const smilesCommit = page.waitForResponse(importResponse(isCommit))
    await button(card, 'Import SMILES').click()
    assert.equal((await smilesCommit).status(), 200)
    assert.equal((await atoms()).length, beforeSmiles + 3, 'the picked SMILES file adds three atoms')
    results.import_pick_smiles_file = true

    setStep('import_drop_mol_file')
    const molFile = await transferOf(page, [{ name: 'carbon.mol', bytes: Buffer.from(CARBON_MOL) }])
    await card.dispatchEvent('dragenter', { dataTransfer: molFile })
    assert.equal(await card.getAttribute('data-dragging'), 'true', 'a file dragged over the card marks it')
    await card.dispatchEvent('dragover', { dataTransfer: molFile })
    await card.dispatchEvent('drop', { dataTransfer: molFile })
    assert.equal(await card.getAttribute('data-dragging'), null, 'the drop ends the drag state')
    await card.getByTestId('import-detected').waitFor()
    assert.equal(await formatSelect(card).inputValue(), 'mol')
    await button(card, 'Preview import').click()
    await card.getByText('1 record(s); importing record 1.', { exact: true }).waitFor()
    const beforeMol = await atoms()
    const molCommit = page.waitForResponse(importResponse(isCommit))
    await button(card, 'Accept losses and import').click()
    assert.equal((await molCommit).status(), 200)
    const afterMol = await atoms()
    assert.equal(afterMol.length, beforeMol.length + 1, 'the dropped MOL file adds one atom')
    assert.equal(countElement(afterMol, 'C'), countElement(beforeMol, 'C') + 1, 'the added atom is carbon')
    const importsBeforeTwoFiles = imports
    const twoFiles = await transferOf(page, [
      { name: 'a.mol', bytes: Buffer.from(CARBON_MOL) },
      { name: 'b.mol', bytes: Buffer.from(CARBON_MOL) },
    ])
    await card.dispatchEvent('drop', { dataTransfer: twoFiles })
    await card.getByTestId('import-error').waitFor()
    assert.equal(await card.getByTestId('import-error').textContent(), 'Drop one file at a time.')
    assert.equal(imports, importsBeforeTwoFiles, 'two dropped files send no request')
    results.import_drop_mol_file = true

    setStep('import_paste_sdf_text')
    const beforeSdf = await atoms()
    const sdfText = `${CARBON_MOL}$$$$\n${NITROGEN_MOL}$$$$\n`
    await card.getByLabel('Text to import', { exact: true }).fill(sdfText)
    await card.getByTestId('import-detected').waitFor()
    assert.equal(await formatSelect(card).inputValue(), 'sdf')
    await card.getByLabel('Record number', { exact: true }).fill('2')
    await button(card, 'Preview import').click()
    await card.getByText('2 record(s); importing record 2.', { exact: true }).waitFor()
    const sdfCommit = page.waitForResponse(importResponse(isCommit))
    await button(card, 'Accept losses and import').click()
    assert.equal((await sdfCommit).status(), 200)
    const afterSdf = await atoms()
    assert.equal(countElement(afterSdf, 'N'), 1, 'the pasted SDF record 2 adds the nitrogen')
    assert.equal(afterSdf.length, beforeSdf.length + 1, 'only the chosen SDF record is added')
    // Pasted SDF text over 200 KiB of UTF-8 is refused in the card, like an SDF file of that size.
    const importsBeforeBigSdf = imports
    await card.getByLabel('Text to import', { exact: true }).fill(`${sdfText}${'> <note>\n'}${'x'.repeat(201 * 1024)}\n\n$$$$\n`)
    await card.getByText('SDF text exceeds 200 KiB of UTF-8, the limit of an SDF file in this web form. Nothing was sent.', { exact: true }).waitFor()
    assert.equal(await button(card, 'Preview import').count(), 0, 'oversized SDF text offers no preview')
    assert.equal(imports, importsBeforeBigSdf, 'oversized SDF text sends nothing')
    results.import_paste_sdf_text = true

    setStep('import_ambiguous_sends_nothing')
    const importsBeforeAmbiguous = imports
    await card.getByLabel('Text to import', { exact: true }).fill('Hello, world!')
    await card.getByTestId('import-ambiguous').waitFor()
    for (const name of ['Preview SMILES import', 'Preview import', 'Preview MOL text']) {
      assert.equal(await button(card, name).count(), 0, `an ambiguous source offers no "${name}"`)
    }
    // Each refused or ambiguous file is named in the card and sends nothing.
    const refusals = [
      { name: 'x.mol', bytes: Buffer.from('<CDXML></CDXML>'), message: 'Could not tell: the .mol name says MOL but the content looks like CDXML.' },
      { name: 'x.smi', bytes: Buffer.alloc(0), message: 'The file is empty.' },
      { name: 'x.smi', bytes: Buffer.alloc(201 * 1024, 0x43), message: TOO_LARGE },
      { name: 'x.mol', bytes: Buffer.from([0xc3, 0x28]), message: 'The selected file is not valid UTF-8.' },
    ]
    for (const { name, bytes, message } of refusals) {
      await pickFile(card, name, bytes)
      await card.getByText(message, { exact: true }).waitFor()
      assert.equal(imports, importsBeforeAmbiguous, `${name} with "${message}" sends nothing`)
    }
    results.import_ambiguous_sends_nothing = true

    setStep('import_override_clears_preview')
    await card.getByLabel('Text to import', { exact: true }).fill(SMILES_SOURCE)
    await button(card, 'Preview SMILES import').click()
    await card.getByTestId('smiles-preview-summary').waitFor()
    await formatSelect(card).selectOption('cdxml')
    await button(card, 'Read CDXML pages and molecules').waitFor()
    assert.equal(await card.getByTestId('smiles-preview-summary').count(), 0, 'a changed format drops the SMILES preview')
    await formatSelect(card).selectOption('smiles')
    await button(card, 'Preview SMILES import').waitFor()
    assert.equal(await card.getByTestId('smiles-preview-summary').count(), 0, 'a SMILES route after a change starts with no preview')
    // A one-record SDF: its name and content agree, so the card detects SDF.
    await pickFile(card, 'x.sdf', Buffer.from(`${CARBON_MOL}$$$$\n`))
    await card.getByTestId('import-detected').waitFor()
    assert.equal(await formatSelect(card).inputValue(), 'sdf')
    await button(card, 'Preview import').click()
    await card.getByText('1 record(s); importing record 1.', { exact: true }).waitFor()
    await formatSelect(card).selectOption('mol')
    await card.getByText('Detected SDF; you chose MOL.', { exact: true }).waitFor()
    assert.equal(await card.getByText('1 record(s); importing record 1.', { exact: true }).count(), 0, 'a changed format drops the CT preview')
    results.import_override_clears_preview = true

    setStep('import_empty_screen_needs_document')
    await closeDocument()
    await page.getByText(DESTINATION_EMPTY, { exact: true }).waitFor()
    const emptyCard = page.getByRole('region', { name: 'Import', exact: true })
    const importsBeforeEmpty = imports
    await emptyCard.getByLabel('Text to import', { exact: true }).fill(SMILES_SOURCE)
    await emptyCard.getByTestId('import-needs-document').waitFor()
    assert.equal(
      await emptyCard.getByTestId('import-needs-document').textContent(),
      'SMILES is added to an open drawing. Create or open a document, then import it there. Nothing was sent.',
    )
    assert.equal(await button(emptyCard, 'Preview SMILES import').count(), 0, 'no SMILES flow runs without a document')
    assert.equal(imports, importsBeforeEmpty, 'the empty screen sends nothing for SMILES')
    await formatSelect(emptyCard).selectOption('cdxml')
    await button(emptyCard, 'Read CDXML pages and molecules').waitFor()
    await button(emptyCard, 'Clear').click()
    assert.equal(await emptyCard.getByLabel('Text to import', { exact: true }).inputValue(), '', 'Clear empties the text field')
    results.import_empty_screen_needs_document = true

    setStep('import_drop_cdx_empty_screen')
    const documentsBeforeCdx = (await api('/api/v1/documents')).documents.length
    const cdxFile = await transferOf(page, [{ name: 'molecule.cdx', bytes: cdxFixture() }])
    await emptyCard.dispatchEvent('drop', { dataTransfer: cdxFile })
    await emptyCard.getByTestId('import-detected').waitFor()
    assert.equal(await emptyCard.getByTestId('import-detected').textContent(), 'Detected CDX from the .cdx file name.', 'a known extension that agrees with the content is the basis (I8)')
    assert.equal(await formatSelect(emptyCard).inputValue(), 'cdx')
    await button(emptyCard, 'Read CDX pages and molecules').click()
    await page.getByLabel('Page and molecule').waitFor()
    await button(emptyCard, 'Preview import').click()
    await emptyCard.getByTestId('cdxml-preview').waitFor()
    const cdxCreate = page.waitForResponse(importResponse(isCreate))
    await button(emptyCard, 'Accept losses and create drawing').click()
    assert.equal((await cdxCreate).status(), 200)
    await page.getByLabel('Document title').waitFor()
    assert.equal(await page.getByLabel('Document title').inputValue(), 'molecule', 'the .cdx extension is stripped from the title')
    assert.equal((await api('/api/v1/documents')).documents.length, documentsBeforeCdx + 1, 'the CDX drop creates one drawing')
    await closeDocument()
    results.import_drop_cdx_empty_screen = true

    setStep('import_paste_cdxml_text_open_document')
    await openDocument(docId)
    const recorded = await atoms()
    const openCard = page.getByRole('region', { name: 'Import', exact: true })
    await openCard.getByLabel('Text to import', { exact: true }).fill(CDXML_TEXT)
    await page.getByText(SWITCH_NOTE, { exact: true }).waitFor()
    await button(openCard, 'Read CDXML pages and molecules').click()
    await page.getByLabel('Page and molecule').waitFor()
    await button(openCard, 'Preview import').click()
    await openCard.getByTestId('cdxml-preview').waitFor()
    const pastedCreate = page.waitForResponse(importResponse(isCreate))
    await button(openCard, 'Accept losses and create drawing').click()
    assert.equal((await pastedCreate).status(), 200)
    await page.waitForFunction(() => document.querySelector('[aria-label="Document title"]')?.value === 'Imported drawing')
    assert.deepEqual(await atoms(), recorded, 'creating a drawing from pasted CDXML leaves the open drawing unchanged')
    results.import_paste_cdxml_text_open_document = true
  } finally {
    page.off('request', countImport)
  }
}
