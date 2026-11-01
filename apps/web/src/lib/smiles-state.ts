import { sameConversionSnapshot, type DocumentView, type SmilesExport, type SmilesForm } from '@/lib/document-contract'

/** A written SMILES and the service generation it came from. */
export interface WrittenSmiles {
  result: SmilesExport
  generation: number
}

/**
 * The written text is still the answer to what the panel asks: the same drawing snapshot, the
 * same service and the form now selected. A text written in another form is not current, so
 * it is neither shown nor offered for copying.
 */
export function currentSmilesExport(
  written: WrittenSmiles | null,
  form: SmilesForm,
  generation: number,
  document: DocumentView,
): WrittenSmiles | null {
  if (!written || written.generation !== generation || written.result.form !== form) return null
  return sameConversionSnapshot(document, written.result.snapshot) ? written : null
}
