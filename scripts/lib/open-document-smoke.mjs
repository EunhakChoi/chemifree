/**
 * Opens a saved drawing from the document list after a reload and waits until the page shows it with the
 * editor idle. Until then the page may still show the workspace without a drawing, whose Import card is
 * replaced by a new card keyed to the drawing once it opens, so a file picked earlier would be lost.
 */
export async function openSavedDocument(page, api, id) {
  const { title } = (await api(`/api/v1/documents/${id}`)).document
  await page.reload()
  await page.locator(`li[data-document-id="${id}"]`).getByRole('button', { name: 'Open' }).click()
  // The file input is enabled only once the editor is idle, which is after the drawing is shown.
  await page.waitForFunction((expected) =>
    document.querySelector('[aria-label="Document title"]')?.value === expected
      && document.getElementById('import-file')?.disabled === false, title)
}
