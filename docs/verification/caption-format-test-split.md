# caption_format: the v16–v21 maximal-edit test split into parallel cases

Scope: `crates/providers/tests/caption_format.rs`, tests only. No production code changed.

## Measurement

`cargo nextest run --package chemifree-providers --test caption_format --no-fail-fast` in the sandbox, one sample each, debug profile:

| | Tests | Wall time | Slowest |
| --- | --- | --- | --- |
| Before | 19 | 117.5 s (just under the 120 s slow period) | `v16_through_v21_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes` 117.5 s; `repeated_maximal_caption_updates_save_reload_and_keep_the_latest_undo` 31.5 s; `v15_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes` 19.8 s |
| After | 42 | 33.3 s | `repeated_maximal_caption_updates_...` 33.3 s (unchanged); `v15_...` 21.6 s (unchanged); the 24 new cases 5.4–6.8 s each |

The first hypothesis (the `repeated_maximal_caption_updates_...` test, 12 rounds) was wrong: it took 31.5 s. The 117 s came from the `v16_through_v21_...` test, which ran 24 independent cases (versions 16–21 × undone × draft) one after another.

## Change

The loop body moved unchanged into `maximal_109_caption_edit_case(version, undone, draft)` (token-identical apart from the failure label, which now names the version). A `case!` macro generates 24 tests, `v{16..21}_{saved_document,undone_saved_document,draft,undone_draft}_maximal_109_caption_edit_keeps_undo_and_original_bytes`. Every assertion is kept; nextest runs the cases in parallel.

## Proof that the cases still guard

With `persistence_weight` in `crates/domain/src/document/persistence.rs` returning `+ 100_000_000` (sandbox private copy, no shared build cache), all 24 new cases fail at `assert!(loaded.persistence_weight() <= bound)`. `cargo fmt --all --check`, clippy `-D warnings` and `npm run check:rust` pass.

## Not changed

`repeated_maximal_caption_updates_...` (12 dependent rounds; the 64 MiB premise in its comment needs about eight rounds, so it was not shortened) and the `v15_...` test (about 20 s) still run sequentially and now set the file's wall time. Splitting the `v15_...` loops the same way is a possible follow-up.
