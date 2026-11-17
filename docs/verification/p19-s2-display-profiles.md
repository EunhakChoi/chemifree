# P19 S2: display edges, profile distinction, radical mass

Tests only; no behaviour change. New crate-internal `crates/application/src/operations/property_closure_tests.rs` (registered in `operations.rs`; `TestCapabilities` and `TestRepository` in `property_tests.rs` are now `pub(super)` to share them). Synthetic and public fixtures only.

| Test | Acceptance term | Checked |
|---|---|---|
| `display_rounding_edges` | display rounding | NIST representative carbon (12 × 0.9893 + 13.00335483507 × 0.0107 = 12.010735896735…): 0 places `"12"`, 8 places `"12.01073590"` (ties-to-even; the value is not near a tie). 0 and 8 are the ends of the accepted range |
| `profiles_are_told_apart` | public data vs observed-compatible profile | both fixtures registered: the public NIST profile (carbon) answers `partial` with `chemifree-nist-representative-1` and its charge convention; the observed mass-input profile (Ga) answers `ok` with `chemifree-mass-input-1`, its charge convention and its own values; Ga under the NIST profile is `unsupported` (`missing_element_data`); no profile id with both registered is `mass_data_ambiguous` |
| `radical_does_not_change_exact_mass` | radical | 13C with 3 stated H, doublet and no radical: equal exact mass 16.02682993176 (a synthetic H-1 row is added in the test) |

The precision setting's non-retroactivity (stored document unchanged after a precision change) is already checked by `crates/application/tests/settings.rs` `mass_display_precision_is_persisted_and_old_writes_preserve_it`, which compares the stored file bytes before and after; a crate-internal copy could not use the file repository (two copies of the crate in one test build), so none was added.

## Mutations

| # | Mutation | Caught by |
|---|---|---|
| M1 | display formatting writes one extra decimal | `display_rounding_edges` (`"120"` for `"12"`) |
| M2 | profile selection ignores the requested id and takes the first registration | `profiles_are_told_apart` (`unsupported` where `ok` is expected) |
| M3 | a doublet subtracts one hydrogen in the mass composition | `radical_does_not_change_exact_mass` (15.019… vs 16.026…) |

Commands: `cargo test -p chemifree-application property_closure_tests` (3 passed), `cargo test -p chemifree-application --lib` (377 passed), fmt, clippy.
