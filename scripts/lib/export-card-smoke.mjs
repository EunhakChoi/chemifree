import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { openSavedDocument } from './open-document-smoke.mjs'

import { exportCardReopenSmoke } from './export-card-reopen-smoke.mjs'

// The "Export card" step of browser-smoke.mjs (P48 S5). The one Export card of an open drawing is driven
// the way a user drives it: choose a format, then preview, download or write. Every export is a
// document.export request, so requests are counted, and the drawing with its history must be unchanged
// after each export.

const DRAWING = [
  { op: 'add_atom', element: 'C', position: { x: 100, y: 100 } },
  { op: 'add_atom', element: 'C', position: { x: 126, y: 115 }, bond_to: { atom: '$0', order: 'single' } },
  { op: 'add_atom', element: 'O', position: { x: 152, y: 100 }, bond_to: { atom: '$1', order: 'single' } },
]

// Each file format's preview and download buttons, and the name its download must carry.
const FILE_EXPORTS = {
  mol: { preview: 'Preview MOL export', download: 'Download MOL', name: 'chemifree.mol' },
  sdf: { preview: 'Preview SDF export', download: 'Download SDF', name: 'chemifree.sdf' },
  cdxml: { preview: 'Preview CDXML export', download: 'Download CDXML', name: 'chemifree.cdxml' },
  cdx: { preview: 'Preview CDX export', download: 'Download CDX', name: 'chemifree.cdx' },
  svg: { preview: 'Preview SVG drawing', download: 'Download SVG', name: 'chemifree.svg' },
}

// The four panels that the single Export card replaces.
const REPLACED_ASIDES = ['SMILES', 'MOL and SDF', 'CDXML and CDX export', 'SVG drawing export']

/**
 * Chooses a format in the Export card and returns the card. The chosen format's flow mounts after
 * the change, so callers wait for that flow before they look inside it.
 */
export async function exportCard(page, format) {
  const card = page.getByRole('region', { name: 'Export', exact: true })
  await card.getByRole('combobox', { name: 'Export format', exact: true }).selectOption(format)
  return card
}

export async function exportCardSmoke({ page, api, apiWrite, outDir, work, results, setStep }) {
  setStep('Export card')
  const counts = { exports: 0, imports: 0, importCommits: 0 }
  const countRequest = (request) => {
    if (request.method() !== 'POST' || new URL(request.url()).pathname !== '/api/v1/operations') return
    const body = request.postDataJSON()
    if (body?.operation === 'document.export') counts.exports += 1
    if (body?.operation === 'document.import') {
      counts.imports += 1
      if (body.parameters?.preview !== true && body.parameters?.mode === undefined) counts.importCommits += 1
    }
  }
  page.on('request', countRequest)
  try {
    const created = await apiWrite('/api/v1/documents', { request_id: 'export-card-create', title: 'Export card' })
    const sourceId = created.body.document.id
    const drawn = await apiWrite(`/api/v1/documents/${sourceId}/transactions`, {
      request_id: 'export-card-draw', expected_revision: 0, commands: DRAWING,
    })
    assert.equal(drawn.status, 200)

    const snap = async (id) => {
      const r = await api(`/api/v1/documents/${id}`)
      return {
        document: r.document,
        session: {
          revision: r.document.revision,
          undo_depth: r.session.undo_depth,
          redo_depth: r.session.redo_depth,
          unsaved_changes: r.session.unsaved_changes,
        },
      }
    }
    const openDocument = (id) => openSavedDocument(page, api, id)
    const region = page.getByRole('region', { name: 'Export', exact: true })
    const flowOf = (format) => region.locator(`[data-testid="export-flow"][data-format="${format}"]`)
    // Every format change goes through the select, and the flow it mounts is awaited before its controls are used.
    const choose = async (format) => {
      await exportCard(page, format)
      await flowOf(format).waitFor()
    }

    setStep('export_card_single_entry')
    const exportsBefore = counts.exports
    await openDocument(sourceId)
    await region.waitFor()
    const original = await snap(sourceId)
    assert.equal(await region.count(), 1, 'one Export card serves every format')
    for (const name of REPLACED_ASIDES) {
      assert.equal(await page.getByRole('complementary', { name, exact: true }).count(), 0, `the "${name}" panel is gone`)
    }
    const select = region.getByRole('combobox', { name: 'Export format', exact: true })
    assert.deepEqual(
      await select.locator('option').evaluateAll((options) => options.map((option) => option.value)),
      ['', 'smiles', 'mol', 'sdf', 'cdxml', 'cdx', 'svg'],
      'the format list starts with an empty choice',
    )
    assert.equal(await select.inputValue(), '', 'no format is chosen by default')
    assert.equal(await region.getByTestId('export-choose').isVisible(), true, 'the card asks for a format before anything shows')
    assert.equal(await region.getByRole('button', { name: /^(Preview|Write|Download)/ }).count(), 0, 'no export control shows before a format is chosen')
    assert.equal(counts.exports, exportsBefore, 'opening the card sends no export')
    await page.emulateMedia({ colorScheme: 'light' })
    await choose('mol')
    await region.screenshot({ path: join(outDir, 'editor-export-card.png') })
    results.export_card_single_entry = true

    setStep('export_format_switch_drops_result')
    await choose('mol')
    assert.equal(await region.getByRole('button', { name: 'Preview MOL export', exact: true }).count(), 1, 'MOL offers its preview')
    assert.equal(await region.getByRole('button', { name: 'Preview SDF export', exact: true }).count(), 0, 'MOL does not offer the SDF preview')
    const version = region.getByRole('combobox', { name: 'MOL version to write', exact: true })
    await version.selectOption('v3000')
    await region.getByRole('button', { name: 'Preview MOL export', exact: true }).click()
    await region.getByRole('button', { name: 'Download MOL', exact: true }).waitFor()
    await choose('sdf')
    assert.equal(await region.getByRole('button', { name: 'Download MOL', exact: true }).count(), 0, 'a format switch drops the MOL result')
    assert.equal(await region.getByRole('button', { name: 'Download SDF', exact: true }).count(), 0, 'no SDF result exists before a preview')
    assert.equal(await version.inputValue(), 'v3000', 'the MOL version is kept for the drawing')
    await choose('mol')
    assert.equal(await version.inputValue(), 'v3000', 'the MOL version is still v3000 after switching back')
    assert.equal(await region.getByRole('button', { name: 'Download MOL', exact: true }).count(), 0, 'switching back does not restore the MOL result')
    await choose('cdxml')
    assert.equal(await region.getByRole('button', { name: 'Preview CDXML export', exact: true }).count(), 1, 'CDXML offers its preview')
    assert.equal(await region.getByRole('button', { name: 'Preview CDX export', exact: true }).count(), 0, 'CDXML does not offer the CDX preview')
    await choose('smiles')
    const style = region.getByRole('combobox', { name: 'Write the drawing as', exact: true })
    await style.selectOption('canonical')
    await choose('svg')
    await choose('smiles')
    assert.equal(await style.inputValue(), 'canonical', 'the SMILES form is kept for the drawing')
    // Reset the choices that the next steps rely on.
    await style.selectOption('recorded')
    await choose('mol')
    await version.selectOption('v2000')
    results.export_format_switch_drops_result = true

    setStep('export_cancel_by_switch_sends_no_file')
    await choose('mol')
    let arrive
    const reached = new Promise((resolve) => { arrive = resolve })
    let release
    const released = new Promise((resolve) => { release = resolve })
    let finish
    const answered = new Promise((resolve) => { finish = resolve })
    const hold = async (route) => {
      if (route.request().postDataJSON()?.operation !== 'document.export') return route.continue()
      try {
        const response = await route.fetch()
        arrive()
        await released
        await route.fulfill({ response })
      } finally {
        finish()
      }
    }
    let downloads = 0
    const countDownload = () => { downloads += 1 }
    page.on('download', countDownload)
    await page.route('**/api/v1/operations', hold)
    try {
      await region.getByRole('button', { name: 'Preview MOL export', exact: true }).click()
      await reached
      await choose('sdf')
      release()
      await answered
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      assert.equal(downloads, 0, 'the MOL reply that arrives after the switch is not downloaded')
      assert.equal(await region.getByTestId('ctfile-offer').count(), 0, 'the MOL reply that arrives after the switch offers nothing')
      assert.equal(await region.getByRole('button', { name: /^Download / }).count(), 0, 'no download button appears after the switch')
      assert.deepEqual(await snap(sourceId), original, 'the drawing and its history are unchanged by the cancelled export')
    } finally {
      release()
      await page.unroute('**/api/v1/operations', hold)
    }
    // A download already clicked re-checks the export first; switching the format while that
    // re-check is held drops it, so no file is saved for a format the user left.
    await choose('mol')
    await region.getByRole('button', { name: 'Preview MOL export', exact: true }).click()
    const downloadMol = region.getByRole('button', { name: 'Download MOL', exact: true })
    await downloadMol.waitFor()
    let recheckArrive
    const recheckReached = new Promise((resolve) => { recheckArrive = resolve })
    let recheckRelease
    const recheckReleased = new Promise((resolve) => { recheckRelease = resolve })
    let recheckFinish
    const recheckAnswered = new Promise((resolve) => { recheckFinish = resolve })
    const holdRecheck = async (route) => {
      if (route.request().postDataJSON()?.operation !== 'document.export') return route.continue()
      try {
        const response = await route.fetch()
        recheckArrive()
        await recheckReleased
        await route.fulfill({ response })
      } finally {
        recheckFinish()
      }
    }
    await page.route('**/api/v1/operations', holdRecheck)
    try {
      await downloadMol.click()
      await recheckReached
      await choose('sdf')
      recheckRelease()
      await recheckAnswered
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      assert.equal(downloads, 0, 'a MOL download whose re-check returns after the switch saves no file')
      assert.deepEqual(await snap(sourceId), original, 'the drawing and its history are unchanged by the dropped download')
    } finally {
      recheckRelease()
      await page.unroute('**/api/v1/operations', holdRecheck)
      release()
      await page.unroute('**/api/v1/operations', hold)
      page.off('download', countDownload)
    }
    results.export_cancel_by_switch_sends_no_file = true

    setStep('export_files_leave_history_unchanged')
    const saved = {}
    for (const [format, { preview, download, name }] of Object.entries(FILE_EXPORTS)) {
      await choose(format)
      await region.getByRole('button', { name: preview, exact: true }).click()
      const offer = region.getByRole('button', { name: download, exact: true })
      await offer.waitFor()
      const ready = page.waitForEvent('download')
      await offer.click()
      const file = await ready
      assert.equal(file.suggestedFilename(), name, `the ${format} export downloads as ${name}`)
      saved[format] = join(work, `export-card-${file.suggestedFilename()}`)
      await file.saveAs(saved[format])
      assert.deepEqual(await snap(sourceId), original, `the ${format} export leaves the drawing and its history unchanged`)
    }
    await choose('smiles')
    assert.equal(await region.getByRole('combobox', { name: 'Write the drawing as', exact: true }).inputValue(), 'recorded', 'the SMILES form starts as recorded')
    await region.getByRole('button', { name: 'Write SMILES', exact: true }).click()
    const written = region.getByRole('textbox', { name: 'Written SMILES', exact: true })
    await written.waitFor()
    const smilesText = await written.inputValue()
    assert.notEqual(smilesText, '', 'the written SMILES text is not empty')
    assert.deepEqual(await snap(sourceId), original, 'writing SMILES leaves the drawing and its history unchanged')
    assert.equal(readFileSync(saved.cdx).subarray(0, 8).toString('ascii'), 'VjCD0100', 'the CDX download is binary CDX, not base64 text')
    assert.ok(readFileSync(saved.svg, 'utf8').startsWith('<?xml'), 'the SVG download is an XML document')
    results.export_files_leave_history_unchanged = true

    await exportCardReopenSmoke({
      page, api, apiWrite, work, results, setStep,
      sourceId, original, snap, openDocument, saved, smilesText, counts,
    })

    setStep('export_card_absent_on_empty_screen')
    // Closing a drawing with unsaved changes asks first; this step discards them, as the Import card step does.
    await page.getByRole('button', { name: 'Close document', exact: true }).click()
    const discard = page.getByRole('button', { name: 'Discard and close', exact: true })
    const empty = page.getByText('No document open')
    await empty.or(discard).waitFor()
    if (await discard.isVisible()) await discard.click()
    await empty.waitFor()
    assert.equal(await region.count(), 0, 'the empty screen has no Export card')
    results.export_card_absent_on_empty_screen = true
  } finally {
    page.off('request', countRequest)
  }
}
