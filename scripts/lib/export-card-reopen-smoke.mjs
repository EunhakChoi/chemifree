import assert from 'node:assert/strict'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

// Keys 5 to 12 of the "Export card" smoke (P48 S5, E19), called by the export-card step after the
// export keys have saved their files. Each key takes a file the card saved, loads it through the
// Import card the way a user does, and checks the drawing the service holds afterwards. A refused,
// cancelled or unimportable file must leave the source drawing S and the target's history
// (revision, undo and redo depth, content) unchanged. This module drives the page and reads the
// service; the caller counts requests and owns the request listener.

/** Atoms as [element, x, y]: a MOL or SDF reopen must keep the saved positions exactly. */
function atomsOf(doc) {
  return doc.atoms.map((atom) => [atom.element, atom.position.x, atom.position.y])
}

/** The sorted element symbols of a drawing. */
function elementsOf(doc) {
  return doc.atoms.map((atom) => atom.element).sort()
}

/** True for a document.import operation that commits into an open drawing, not a preview, a read or a create. */
function isCommit(request) {
  const parameters = request.postDataJSON()?.parameters
  return parameters?.preview !== true && parameters?.mode === undefined
}

function isCreate(request) {
  return request.postDataJSON()?.parameters?.mode === 'create'
}

/** A response predicate for a document.import operation whose request `matches` accepts. */
function importReply(matches) {
  return (response) =>
    response.url().endsWith('/api/v1/operations') &&
    response.request().postDataJSON()?.operation === 'document.import' &&
    matches(response.request())
}

/**
 * Keys 5 to 12 of the "Export card" smoke. `saved` holds the paths the export keys saved their
 * downloads to, `smilesText` the SMILES the card wrote, `sourceId` and `original` the source
 * drawing S and its snapshot. Sets results keys 5 to 12 in order.
 */
export async function exportCardReopenSmoke({
  page, api, apiWrite, work, results, setStep, sourceId, original, snap, openDocument, saved, smilesText, counts,
}) {
  const card = page.getByRole('region', { name: 'Import', exact: true })
  const button = (name) => card.getByRole('button', { name, exact: true })
  const pick = (path) => card.getByLabel('Import file', { exact: true }).setInputFiles(path)
  // Reading a file is asynchronous, so wait for the detected format rather than reading the select at once.
  // On a timeout, report what the card showed so a failure can be told apart from a slow read.
  const detected = async (format) => {
    try {
      await page.waitForFunction((f) => document.getElementById('import-format')?.value === f, format)
    } catch (error) {
      const state = await page.evaluate(() => {
        const input = document.getElementById('import-file')
        const importCard = document.querySelector('[data-testid="import-card"]')
        return {
          shownTitle: document.querySelector('[aria-label="Document title"]')?.value ?? null,
          inputDisabled: input?.disabled ?? null,
          inputFiles: input?.files?.length ?? null,
          format: document.getElementById('import-format')?.value ?? null,
          importCardAlerts: [...(importCard?.querySelectorAll('[role="alert"], [role="status"]') ?? [])].map((node) => node.textContent),
        }
      }).catch((evaluateError) => ({ evaluateFailed: String(evaluateError) }))
      throw new Error(`the Import card did not detect ${format}: ${JSON.stringify(state)}`, { cause: error })
    }
  }
  const documentOf = async (id) => (await api(`/api/v1/documents/${id}`)).document
  const create = async (key, title) =>
    (await apiWrite('/api/v1/documents', { request_id: `export-card-${key}`, title })).body.document.id
  const sourceAtoms = atomsOf(await documentOf(sourceId))

  /** Loads a saved one-record MOL or SDF file into the open drawing and checks that it reopens as the source. */
  async function reopenOneRecord(target, path, format) {
    await pick(path)
    await detected(format)
    await button('Preview import').click()
    await card.getByText('1 record(s); importing record 1.', { exact: true }).waitFor()
    const commit = page.waitForResponse(importReply(isCommit))
    await button('Accept losses and import').click()
    assert.equal((await commit).status(), 200)
    await button('Accept losses and import').waitFor({ state: 'hidden' })
    const doc = await documentOf(target)
    assert.deepEqual(atomsOf(doc), sourceAtoms, `the saved ${format} reopens with the source atoms at their positions`)
    assert.equal(doc.bonds.length, 2, `the saved ${format} reopens with the source bonds`)
    assert.deepEqual(await snap(sourceId), original, 'the source drawing is unchanged')
  }

  /** Reads a saved CDXML or CDX file into a new drawing, the way the Import card handles these formats. */
  async function reopenAsNewDrawing(path, format, readButton) {
    const listedBefore = new Set((await api('/api/v1/documents')).documents.map((item) => item.id))
    await pick(path)
    await detected(format)
    await button(readButton).click()
    await page.getByLabel('Page and molecule').waitFor()
    await button('Preview import').click()
    await card.getByTestId('cdxml-preview').waitFor()
    const created = page.waitForResponse(importReply(isCreate))
    await button('Accept losses and create drawing').click()
    assert.equal((await created).status(), 200)
    await page.waitForFunction(() => document.querySelector('[aria-label="Document title"]')?.value === 'export-card-chemifree')
    const listed = (await api('/api/v1/documents')).documents
    const fresh = listed.filter((item) => !listedBefore.has(item.id))
    assert.equal(listed.length, listedBefore.size + 1, `the saved ${format} creates one drawing`)
    assert.equal(fresh.length, 1, `the saved ${format} creates exactly one new drawing`)
    const doc = await documentOf(fresh[0].id)
    assert.equal(doc.title, 'export-card-chemifree', 'the new drawing takes the file name without its extension')
    assert.deepEqual(elementsOf(doc), ['C', 'C', 'O'], `the saved ${format} reopens with the three heavy atoms of ethanol`)
    assert.equal(doc.bonds.length, 2, `the saved ${format} reopens with two bonds`)
    assert.deepEqual(await snap(sourceId), original, 'the source drawing is unchanged')
  }

  // Key 5: a saved MOL file reopens into an empty drawing at its saved positions.
  setStep('export_mol_file_reopens')
  const t1 = await create('t1', 'Export card T1')
  await openDocument(t1)
  await reopenOneRecord(t1, saved.mol, 'mol')
  results.export_mol_file_reopens = true

  // Key 6: a MOL file cut before its M  END is refused, and neither the drawing nor its session changes.
  setStep('export_corrupt_file_refused_history_kept')
  const text = readFileSync(saved.mol, 'utf8')
  const end = text.indexOf('M  END')
  assert.notEqual(end, -1, 'the saved MOL file has an M  END line')
  const cut = join(work, 'export-card-cut.mol')
  writeFileSync(cut, text.slice(0, end))
  const t2 = await create('t2', 'Export card T2')
  await openDocument(t2)
  const before = await snap(t2)
  await pick(cut)
  await detected('mol')
  await button('Preview import').click()
  await card.getByRole('alert').first().waitFor()
  assert.equal(await button('Accept losses and import').count(), 0, 'a corrupt MOL file offers no import')
  assert.deepEqual(await snap(t2), before, 'the target drawing and its session are unchanged')
  results.export_corrupt_file_refused_history_kept = true

  // Key 7: a preview cleared while it is in flight is dropped, and its late reply commits nothing.
  setStep('import_clear_cancels_preview_history_kept')
  // Clear first: the format is then detected from the newly read file, not from the refused one.
  await button('Clear').click()
  await pick(saved.mol)
  await detected('mol')
  const commitsBefore = counts.importCommits
  let arrived
  const previewArrived = new Promise((resolve) => { arrived = resolve })
  let release
  const released = new Promise((resolve) => { release = resolve })
  let answer
  const answered = new Promise((resolve) => { answer = resolve })
  let held = false
  let delivered = false
  // Holds the first CT preview until the card has been cleared, then delivers its reply.
  const holdPreview = async (route) => {
    const request = route.request().postDataJSON()
    if (held || request?.operation !== 'document.import' || request.parameters?.preview !== true) return route.continue()
    held = true
    try {
      const response = await route.fetch()
      arrived()
      await released
      await route.fulfill({ response })
      delivered = true
    } finally {
      arrived()
      answer()
    }
  }
  await page.route('**/api/v1/operations', holdPreview)
  try {
    await button('Preview import').click()
    await previewArrived
    await button('Clear').click()
    release()
    await answered
    assert.equal(delivered, true, 'the held preview reply reached the page after Clear')
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
    assert.equal(await button('Accept losses and import').count(), 0, 'a preview cleared in flight offers no import')
    assert.equal(counts.importCommits, commitsBefore, 'a cleared preview commits nothing')
    assert.deepEqual(await snap(t2), before, 'the target drawing and its session are unchanged')
  } finally {
    release()
    await page.unroute('**/api/v1/operations', holdPreview)
  }
  results.import_clear_cancels_preview_history_kept = true

  // Key 8: a saved SDF file reopens as its one record.
  setStep('export_sdf_file_reopens')
  const t3 = await create('t3', 'Export card T3')
  await openDocument(t3)
  await reopenOneRecord(t3, saved.sdf, 'sdf')
  results.export_sdf_file_reopens = true

  // Key 9: a saved CDXML file reads into a new drawing. Positions are not compared: CDXML is not on the MOL scale.
  setStep('export_cdxml_file_reopens')
  await reopenAsNewDrawing(saved.cdxml, 'cdxml', 'Read CDXML pages and molecules')
  results.export_cdxml_file_reopens = true

  // Key 10: a saved CDX file. Reopening T3 first makes the title wait prove the switch to the new drawing.
  setStep('export_cdx_file_reopens')
  await openDocument(t3)
  assert.equal(await page.getByLabel('Document title', { exact: true }).inputValue(), 'Export card T3')
  await reopenAsNewDrawing(saved.cdx, 'cdx', 'Read CDX pages and molecules')
  results.export_cdx_file_reopens = true

  // Key 11: the SMILES the card wrote reads back through the SMILES import as ethanol.
  setStep('export_smiles_text_reopens')
  const t4 = await create('t4', 'Export card T4')
  await openDocument(t4)
  await card.getByLabel('Text to import', { exact: true }).fill(smilesText)
  await detected('smiles')
  await button('Preview SMILES import').click()
  await card.getByTestId('smiles-preview-summary').getByText('3 atom(s), 2 bond(s) would be added.', { exact: true }).waitFor()
  const committed = page.waitForResponse(importReply(isCommit))
  await button('Import SMILES').click()
  assert.equal((await committed).status(), 200)
  await card.getByText(/Imported\./).waitFor()
  assert.deepEqual(elementsOf(await documentOf(t4)), ['C', 'C', 'O'], 'the written SMILES reopens as ethanol')
  results.export_smiles_text_reopens = true

  // Key 12: an SVG file is a picture, not an input. The card asks for a format and sends nothing.
  setStep('export_svg_file_is_not_importable')
  await button('Clear').click()
  const importsBefore = counts.imports
  await pick(saved.svg)
  await card.getByTestId('import-ambiguous').waitFor()
  assert.equal(await card.getByRole('button', { name: /^Preview / }).count(), 0, 'an SVG file offers no preview')
  assert.equal(await card.getByRole('button', { name: /^Read .+ pages and molecules$/ }).count(), 0, 'an SVG file offers no CT read')
  assert.equal(counts.imports, importsBefore, 'an SVG file sends no import operation')
  const formats = await card.getByRole('combobox', { name: 'Format', exact: true }).locator('option')
    .evaluateAll((options) => options.map((option) => option.value))
  assert.equal(formats.includes('svg'), false, 'the Import format select has no svg option')
  results.export_svg_file_is_not_importable = true
}
