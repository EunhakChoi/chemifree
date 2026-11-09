# caption_format: the maximal-metadata caption edit tests split into parallel cases

Scope: `crates/providers/tests/caption_format.rs` (tests only, no production code) and `.config/nextest.toml` (one test group).

## Measurement

`cargo nextest run --package chemifree-providers --test caption_format --no-fail-fast` in the sandbox, debug profile, one sample each:

| | Tests | Wall time | Slowest |
| --- | --- | --- | --- |
| Before | 19 | 117.5 s (just under the 120 s slow period) | `v16_through_v21_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes` 117.5 s; `repeated_maximal_caption_updates_save_reload_and_keep_the_latest_undo` 31.5 s; `v15_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes` 19.8 s |
| After | 45 | 35.8 s | `repeated_maximal_caption_updates_...` 31.9 s (unchanged); the 28 split cases 5.3–6.8 s each |

The whole workspace run (`npm run check:rust`, 2465 tests) took 71.6 s after the change.

The first hypothesis, that the 12-round `repeated_maximal_caption_updates_...` test caused the 117 s, was wrong: it takes 31.5 s. The 117 s came from the `v16_through_v21_...` test, which ran 24 independent cases (versions 16–21, undone or not, document or draft) one after another.

## Change

- The loop body of the v16–v21 test and of the v15 test moved unchanged into helpers (`maximal_109_caption_edit_case(version, undone, draft)`, `maximum_metadata_v15_109_caption_edit_case(undone, draft)`); the document id and file names come from the version, and the failure labels name it. `case!` and `v15_case!` generate 28 tests named `v{15..21}_{saved_document,undone_saved_document,draft,undone_draft}_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes` (the v15 test's own naming scheme). Every assertion is kept.
- Each generated test first calls `assert_case_name_matches_arguments`, so a name that disagrees with its version, undone or draft argument fails by name (proved by flipping one flag, one version and one draft argument in a private copy).
- `.config/nextest.toml`: a test group `caption-format-maximal` (`max-threads = 4`) selects the 28 cases with `test(~maximum_metadata_109_caption_edit)`, so a burst of them cannot occupy the whole runner beside the timing-sensitive tests. The override check (`npm run test:nextest-overrides`) still matches its 33 exact names.

## Proof that the cases still guard

With `persistence_weight` in `crates/domain/src/document/persistence.rs` returning `+ 100_000_000` (sandbox private copy, no shared build cache) the 24 v16–v21 cases and the 4 v15 cases all fail at `assert!(loaded.persistence_weight() <= bound)`. `cargo fmt --all --check`, clippy `-D warnings` and `npm run check:rust` pass.

## Not changed

`repeated_maximal_caption_updates_...` (12 dependent rounds, 31.9 s) now sets the file's wall time. Its comment says the 64 MiB premise needs about eight rounds (an estimate, not measured), so it was not shortened.
