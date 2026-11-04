/**
 * What the MOL and SDF panel decides on its own account, kept apart from the component so each
 * rule can be checked without a browser: which MOL dialect a request asks for, the steps of the
 * export consent flow, the plain sentence for each loss the service reports and for each reason
 * it refuses a file or a drawing. The service decides every chemical fact; none is decided here.
 * See docs/CONTRACT.md, "Export version and consent (P28b1)" and "Enhanced stereo collections".
 */

/** The dialect the person asks for. `auto` is the service's choice, made once per request. */
export type CtabChoice = 'v2000' | 'v3000' | 'auto'

export const DEFAULT_CTAB: CtabChoice = 'v2000'

export const CTAB_OPTIONS: ReadonlyArray<{ value: CtabChoice; label: string }> = [
  { value: 'v2000', label: 'V2000 (default; no stereo groups)' },
  { value: 'v3000', label: 'V3000 (carries stereo groups)' },
  { value: 'auto', label: 'Auto (V3000 only when the drawing needs it)' },
]

/** The form value as a choice; anything else is the default, never passed to the service. */
export function parseCtabChoice(value: string): CtabChoice {
  return value === 'v3000' || value === 'auto' ? value : DEFAULT_CTAB
}

/** The version is client policy: it only ever appears as the `ctab` key of one export request. */
export function ctabParameters(choice: CtabChoice, acceptedLosses?: string): { ctab: CtabChoice; accepted_losses?: string } {
  return acceptedLosses === undefined ? { ctab: choice } : { ctab: choice, accepted_losses: acceptedLosses }
}

/** What a reply may say it wrote: an explicit choice must come back as itself. */
export function writtenDialectMatches(choice: CtabChoice, written: unknown): written is 'v2000' | 'v3000' {
  return (written === 'v2000' || written === 'v3000') && (choice === 'auto' || choice === written)
}

/** What the page does after an export request failed. */
export type ExportStep =
  | { kind: 'offer'; digest: string; changed: boolean }
  | { kind: 'ask-again' }
  | { kind: 'error' }

/**
 * `loss_consent_required` with a digest shows the losses to accept. `loss_consent_changed`
 * means the digest sent no longer matches what would be written: the old consent is void, so
 * the page asks again without it and shows the new losses marked as changed. Anything else is
 * an error to show.
 */
export function afterExportFailure(code: string, digest: string | undefined, sentDigest: string | undefined): ExportStep {
  if (code === 'loss_consent_required' && digest !== undefined) return { kind: 'offer', digest, changed: sentDigest !== undefined }
  if (code === 'loss_consent_changed' && sentDigest !== undefined) return { kind: 'ask-again' }
  return { kind: 'error' }
}

/** One plain sentence per loss id the page explains. An id not listed is shown as it is, never hidden. */
export const LOSS_SENTENCES: Readonly<Record<string, string>> = {
  stereo_configuration_not_drawn: 'A stored atom configuration that the wedges, hashes and positions do not show; a reader of the file gets another configuration or none.',
  stereo_bond_configuration: 'A stored double-bond configuration that neither a crossed mark nor the positions show.',
  stereo_group: 'Enhanced stereo groups, which a V2000 file or SMILES cannot carry.',
  stereo_ungrouped_centers: 'Stored stereo centers that are in no group: the file states nothing about them.',
  stereo_group_identity: 'The group names (e1, e2, ...) are not written; the groups themselves are.',
  chiral_flag: 'The chiral flag: whether the file states absolute stereochemistry.',
  registry_number: 'A registry number in the counts line, which the drawing does not keep.',
  isotope_validation: 'An isotope mass that was not checked against known isotopes.',
  coordinate_precision: 'Coordinates rounded to four decimals.',
  z_coordinate: 'A nonzero z coordinate, which the editor does not store.',
  stereo_unreadable: 'Wedges or hashes whose configuration could not be derived and were kept as marks only.',
  stereo_group_merged: 'A file group joined the absolute group the document already has.',
  stereo_group_number: 'A group number the document already uses was replaced by the smallest unused one.',
  extra_placement: 'Extra placements of the same structure, which the file cannot hold.',
}

/** The heading for a loss: the plain sentence when the page has one, else the id itself. */
export function lossHeading(feature: string): string {
  return Object.hasOwn(LOSS_SENTENCES, feature) ? LOSS_SENTENCES[feature] : feature
}

const NOTHING = ' Nothing was changed.'

/** Sentences for the refusals a person can reach from this page. */
export const CT_REFUSAL_SENTENCES: Readonly<Record<string, string>> = {
  v3000_collection_member_not_stereocenter: `A stereo group in the file lists an atom whose wedges or hashes do not give it a stored tetrahedral configuration. Dropping it would change which centers belong together, so the record was refused.${NOTHING}`,
  v3000_collection_drawing_unreadable: `The file holds stereo groups, but its wedges and hashes could not all be read into configurations, so the groups cannot be checked.${NOTHING}`,
  v3000_group_member_not_drawn: `A stereo group holds atoms that the wedges or hashes would not derive a configuration for as the file would be written; draw the marks or remove those atoms from the group. Nothing was written.${NOTHING}`,
  v3000_empty_collection: `A stereo group in the file has no atoms.${NOTHING}`,
  v3000_duplicate_collection: `The file states the same stereo group twice.${NOTHING}`,
  v3000_collection_atom_shared: `An atom is in two stereo groups of the file.${NOTHING}`,
  v3000_duplicate_index: `An atom is listed twice in one group, or an index is repeated.${NOTHING}`,
  v3000_invalid_collection: `A stereo group in the file is malformed or its number does not fit 32 bits.${NOTHING}`,
  v3000_unknown_atom_index: `The file names an atom that its atom block does not define.${NOTHING}`,
  v3000_collection_not_supported_yet: `The file holds a collection other than the three stereo kinds, which this page does not read.${NOTHING}`,
  v3000_sgroup_not_supported: `S-groups (superatoms, repeat units, data) are not read; the file was refused rather than partly imported.${NOTHING}`,
  v3000_unsupported_rgroup: `R-groups are not read; the file was refused rather than partly imported.${NOTHING}`,
  v3000_unsupported_query: `Query features are not read; the file was refused rather than partly imported.${NOTHING}`,
  v3000_unsupported_atom: `The file holds an atom this page does not read (a query, pseudo or list atom).${NOTHING}`,
  v3000_unsupported_atom_property: `The file holds an atom property this page does not read.${NOTHING}`,
  v3000_unsupported_bond: `The file holds a bond type this page does not read.${NOTHING}`,
  v3000_unsupported_bond_property: `The file holds a bond property this page does not read.${NOTHING}`,
  v3000_unsupported_3d: `3D blocks are not read.${NOTHING}`,
  v3000_unsupported_reaction: `Reaction blocks are not read.${NOTHING}`,
  v3000_unsupported_block: `The file holds a block this page does not read.${NOTHING}`,
  v3000_multiple_structures: `The record holds more than one connection table.${NOTHING}`,
  unsupported_atom: `The drawing or file holds an atom the MOL format here cannot carry.${NOTHING}`,
  unsupported_bond: `The drawing or file holds a bond the chosen MOL version cannot carry; an aromatic bond needs V3000.${NOTHING}`,
  unsupported_version: `This MOL version is not supported.${NOTHING}`,
  resource_limit: `The drawing or file exceeds a size limit (V2000 holds 999 atoms or bonds, V3000 2000 atoms and 4000 bonds).${NOTHING}`,
  format_ctab_conflict: `The format id and the MOL version disagree.${NOTHING}`,
}

/**
 * Reasons the page words only generically: each is a malformed-file or malformed-framing
 * reason, and the service message names the line. Listed so the contract's table and this
 * module cannot drift apart unnoticed.
 */
export const CT_FALLBACK_REASONS: readonly string[] = [
  'v3000_missing_prefix', 'v3000_unterminated_continuation', 'v3000_invalid_counts_line', 'v3000_count_mismatch',
  'v3000_missing_block', 'v3000_duplicate_block', 'v3000_block_mismatch', 'v3000_block_not_closed',
  'v3000_invalid_atom', 'v3000_invalid_bond', 'v3000_invalid_number', 'v3000_invalid_property',
  'v3000_duplicate_property', 'v3000_invalid_index', 'v3000_duplicate_bond', 'v3000_invalid_charge',
  'v3000_invalid_radical', 'v3000_invalid_isotope',
  'empty_input', 'truncated', 'invalid_utf8', 'invalid_line', 'invalid_columns', 'invalid_header', 'reserved_title',
  'truncated_counts', 'invalid_counts', 'missing_m_end', 'invalid_m_end', 'trailing_content', 'invalid_coordinate',
  'invalid_bond', 'duplicate_bond', 'invalid_charge', 'invalid_isotope', 'invalid_count',
]

const GENERIC_MALFORMED = `The MOL text is not valid, or the drawing holds a value the file cannot carry; the service message names where.${NOTHING}`

/** The plain sentence for a refusal code, or null for a code the page does not know (show the service's message). */
export function ctRefusalSentence(code: string): string | null {
  if (Object.hasOwn(CT_REFUSAL_SENTENCES, code)) return CT_REFUSAL_SENTENCES[code]
  return CT_FALLBACK_REASONS.includes(code) ? GENERIC_MALFORMED : null
}
