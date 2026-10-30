import { writeFileSync } from 'node:fs'

function decimal(source) {
  const number = Number(source)
  if (!Number.isFinite(number)) throw new Error('Fixture must be finite')
  const bytes = Buffer.alloc(8)
  bytes.writeDoubleLE(number)
  return { source_decimal: source, reference_f64: number, reference_f64_bits_le: bytes.toString('hex') }
}
const hash = '0'.repeat(64)
const documents = {
  'elements.json': { elements: [
    { element_row_ordinal: 0, atomic_number_u32: 24, symbol: 'X', natural_atomic_weight: decimal('14.1250'), permitted_valences_u32: [1, 2], first_isotope_row_ordinal: 0, isotope_rows: 2 },
    { element_row_ordinal: 1, atomic_number_u32: 37, symbol: 'Y', natural_atomic_weight: decimal('20'), permitted_valences_u32: [], first_isotope_row_ordinal: 2, isotope_rows: 1 },
  ] },
  'isotopes.json': { isotopes: [
    { isotope_row_ordinal: 0, element_row_ordinal: 0, atomic_number_u32: 24, mass_number_u32: 14, mass: decimal('14.0'), abundance_percent: decimal('60.0001'), abundance_token_present: true, nickname: 'x' },
    { isotope_row_ordinal: 1, element_row_ordinal: 0, atomic_number_u32: 24, mass_number_u32: 15, mass: decimal('15'), abundance_percent: decimal('40.0000'), abundance_token_present: true, nickname: null },
    { isotope_row_ordinal: 2, element_row_ordinal: 1, atomic_number_u32: 37, mass_number_u32: 20, mass: decimal('20'), abundance_percent: decimal('0'), abundance_token_present: false, nickname: null },
  ] },
  'defaults.json': { selection_is_static_table_analysis_not_runtime_capture: true, rows: [
    { element_row_ordinal: 0, atomic_number_u32: 24, selected_isotope_row_ordinal: 0, mass_number_u32: 14, mass: decimal('14.0'), all_abundances_zero: false },
    { element_row_ordinal: 1, atomic_number_u32: 37, selected_isotope_row_ordinal: 2, mass_number_u32: 20, mass: decimal('20'), all_abundances_zero: true },
  ] },
  'aliases.json': { aliases: [{ symbol: 'x', atomic_number_u32: 24, mass_number_u32: 14, source_isotope_row_ordinal: 0 }] },
  'constants.json': { signed_charge_mass_coefficient: { value: -0.125, bits_le: '000000000000c0bf', unit: 'fixture-unit' } },
  'features.json': { schema_id: 'invented-schema', source_decimal_tokens_authoritative: true, reference_f64_decode_is_independent_not_original_runtime_capture: true, element_rows: 2, isotope_rows: 3, alias_rows: 1, element_key_type: 'u32', isotope_key_type: 'u32', row_order: 'source', mass_unit: 'fixture-unit', natural_atomic_weight_unit: 'fixture-unit', abundance_unit: 'fixture-percent', native_selected_input_secondary: { value: 0, unit: 'fixture-unit' }, source_mass_number_field_explicit_in_text: false, mass_number_values_independently_checked_against_source_constructor_contract: true, original_parser_or_selection_code_included: false, original_naming_grammar_or_module_dispatch_included: false },
  'bounds.json': { binding_id: 'invented-source', inert_source_lineage_only: true, text_source: { file_name: 'invented.txt', bytes: 100, sha256: hash }, installed_equivalent_copy_sha256: hash, row_source_spans: [
    { kind: 'element', row_ordinal: 0, source_line_1based: 1, source_file_offset: 0, source_bytes_including_newline: 4, source_line_sha256: hash },
    { kind: 'isotope', row_ordinal: 1, source_line_1based: 2, source_file_offset: 4, source_bytes_including_newline: 5, source_line_sha256: hash },
  ], consumer_source: { file_name: 'invented-consumer', bytes: 100, sha256: hash }, consumer_evidence: [], signed_charge_coefficient: { source_file_offset: 20, bytes: 8, sha256: hash }, source_offsets_are_not_native_dispatch: true, original_source_or_CFG_included: false },
}
for (const [name, object] of Object.entries(documents)) {
  writeFileSync(new URL(name, import.meta.url), `${JSON.stringify(object)}\n`)
}
