# Document list `truncated` flag (#104): verification record

Scope: the document list and the trash list reply with an additive boolean `truncated`, `true` exactly when the cap of 1,000 documents left stored (resp. trashed) documents unread. The CLI prints one `incomplete: ...` line to stderr (stdout is unchanged; `--json` output gains the key) and the web shows a "List is incomplete" notice (`documents-truncated`, `trash-truncated`). A web client reads a reply without the field as not truncated. Not changed: the cap, the order, the filter and the sort.

## Tests

- `crates/application/tests/document_list_cap.rs` (8 tests, public `Application` over the file repository): exactly 1,000 and 1,001 stored; exactly 1,000 and 1,001 trashed (the cap, the flag, each listed document once and trashed, and stamps non-increasing in the listed order); three documents; a title search on a truncated list still reports `truncated`; the cap literal 1,000. The file store stamps a trashing with the wall clock, which can step backwards (measured once on a WSL host: 2.9 s within 40 s), so these cases do not check which documents the cap keeps. That rule (the newest 1,000 by stamp, newest first) is checked deterministically over `MemoryStore`'s logical clock in `crates/application/tests/documents.rs` (`the_most_recently_trashed_documents_are_listed_however_full_the_trash_is`); equal stamps (kept and listed in ascending identifier order) by `trashed_documents_with_equal_stamps_are_kept_and_listed_by_identifier` over a frozen `MemoryStore` stamp; the stamp itself (wall-clock milliseconds between the instants just before and after the trashing) by `trash_stamps_are_wall_clock_milliseconds` in `crates/providers/src/file_store.rs`. Until 2026-11 the file-store cases paused 1 ms per delete and compared against trashing order, which failed on such hosts (#106).
- `scripts/web-client-check.mjs`: 12 cases through `DocumentClient.list()` and `.trash()` (flag kept when true, accepted when false or absent, `"yes"`, `null` and `1` refused as `incompatible_response`).
- The browser smoke does not render the notices; the CLI line is not asserted by a test (it is one `eprintln!` after the `unreadable` loop).

## Mutations (each applied alone in a private copy)

| Mutation | Caught by |
| --- | --- |
| list comparison `>` to `>=` | `stored_documents_at_the_cap_are_all_listed` |
| trash comparison `>` to `>=` | `trashed_documents_at_the_cap_are_all_listed` |
| trash selection sort flipped to oldest first (2026-11, #106) | `the_most_recently_trashed_documents_are_listed_however_full_the_trash_is` (`tests/documents.rs`, `MemoryStore`) |
| trash selection tie-break flipped to descending identifier (2026-11, #106) | `trashed_documents_with_equal_stamps_are_kept_and_listed_by_identifier` (`tests/documents.rs`, `MemoryStore`) |
| file-store trash stamp fixed at `0` (2026-11, #106) | `trash_stamps_are_wall_clock_milliseconds` (`file_store.rs` unit test) |
| list `truncated` forced to false | `stored_documents_one_past_the_cap_are_truncated`, `a_title_search_on_a_truncated_list_still_reports_truncated` |
| `isList` ignores the field check | the three document-list refusal cases |
| `isTrashList` ignores the field check | the three trash-list refusal cases |

## Process notes

- The sandbox lint gap reported by one worker (nested `@eslint/js` under `apps/web/node_modules`) was not reproduced by the full `npm run check` runs of earlier slices; it is left as an observation.
