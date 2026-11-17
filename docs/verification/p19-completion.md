# P19 completion (bounded): formula, mass and isotope analysis

Roadmap acceptance: "염·동위원소·라디칼·명시/추론 H와 표시 반올림을 각각 대조한다. 공개 데이터 기반 결과와 관찰 호환 프로필을 구분한다. 계산 정밀도 설정의 저장·적용과 기존 문서 비소급을 검사한다." The last sentence was narrowed when P19 closed (2026-11-17) to "질량 표시 자릿수 설정(계산 결과에 적용되는 유일한 정밀도 설정)의 저장·적용과 기존 문서 비소급": CONTRACT computes with a fixed arithmetic policy, the only precision setting is the display decimals of new mass results, and the scope asks for the setting "needed for the first calculation results"; the original wording is kept in the roadmap note, and P19 reopens if it should stand. Closed within the scope documented in docs/CONTRACT.md (`chem.properties`, `chemifree-properties-1`), delivered by #81, #288 (S1) and #289 (S2).

| Criterion | Evidence |
|---|---|
| Salt | `crates/application/tests/properties_acceptance.rs` `salt_formula_by_scope` (sodium acetate by molecule and components scope, charges, Na+ hydrogens inferred); `crates/domain/tests/mass_public_examples.rs` (sodium acetate composition) |
| Isotope | `properties_acceptance.rs` `isotope_hill_order` (`C[13C]H6`); `crates/application/src/operations/property_tests.rs` (13C exact mass, labelled isotope rows); `crates/domain/tests/mass_public_examples.rs`; conformance `formula-and-mass` |
| Radical | `properties_acceptance.rs` `radical_hydrogen_reduction` (C doublet/singlet/triplet, N doublet `H2N`), `charged_radical_is_refused_without_result` (`hydrogens_unavailable`, no partial result); `crates/application/src/operations/property_closure_tests.rs` `radical_does_not_change_exact_mass` (with stated hydrogens) |
| Explicit / inferred H | `properties_acceptance.rs` `hydrogen_modes` (specified count, explicit H vertex counted once, selection formulas); `crates/domain/tests/mass_scopes.rs`; `crates/application/tests/properties_operations.rs` |
| Display rounding | `property_closure_tests.rs` `display_rounding_edges` (0 places `"12"`, 8 places `"12.01073590"`); `property_tests.rs` (5 then 2 places); `crates/numerics/tests/display.rs` |
| Public data vs observed-compatible profile | `property_closure_tests.rs` `profiles_are_told_apart` (public NIST and the synthetic mass-input fixture registered together answer with their own `calculation_profile` and charge convention; no id is `mass_data_ambiguous`); `property_tests.rs` (each profile alone) |
| Precision setting stored and applied; existing documents unchanged | `crates/application/tests/settings.rs` `mass_display_precision_is_persisted_and_old_writes_preserve_it` (persisted, invalid values refused, old clients keep it, stored file bytes unchanged); `property_tests.rs` ("a previous mass result is unchanged") |

Formula notation (Hill order, `[13C]`, charge suffix) is now written in CONTRACT's `chem.properties` reply sentence; the tests pin it.

## Documented limits (not done)

- Compatibility with the separately approved personal mass registration is not verified: it is refused locally as `content_invalid`. The observed-compatible profile kind is tested only with the synthetic mass-input fixture.
- No calculation-precision setting exists (the acceptance was narrowed to the mass display decimals, 0–8); general `numeric_precision` stays `not_implemented` under STYLE-01.rest (its follow-up names P60).
- No uncertainty propagation (CHEM-01.rest); the web settings offer 0–5 places of the 0–8 the service accepts.
- The synthetic conformance fixture covers carbon only.
- Further property models and data conventions stay under CHEM-01.rest with its other follow-ups.

## Traceability

No part had P19 as its only follow-up, so no status changes; P19 is removed from CHEM-01.rest and the `chem_properties` declaration route, which keep their other follow-ups.
