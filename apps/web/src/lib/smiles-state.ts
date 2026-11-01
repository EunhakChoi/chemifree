import { ApiError, sameConversionSnapshot, type CtLoss, type DocumentView, type SmilesExport, type SmilesForm } from '@/lib/document-contract'

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

/** A refusal that needs the losses accepted: the export is repeated with this digest. */
export interface Consent {
  form: SmilesForm
  losses: CtLoss[]
  digest: string
}

/**
 * The consent offer to show after a write failed. A consent refusal replaces the offer with the
 * losses it lists; any other failure ends it, because the digest it carries belongs to a write
 * that no longer stands (the drawing changed, the service restarted).
 */
export function consentAfterFailure(form: SmilesForm, failure: unknown): Consent | null {
  if (failure instanceof ApiError && failure.consent) {
    return { form, losses: failure.consent.losses, digest: failure.consent.digest }
  }
  return null
}
