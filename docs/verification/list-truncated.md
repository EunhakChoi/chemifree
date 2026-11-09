# Document list `truncated` flag (#104): verification record

Scope: the document list and the trash list reply with an additive boolean `truncated`, `true` exactly when the cap of 1,000 documents left stored (resp. trashed) documents unread. The CLI prints one `incomplete: ...` line to stderr (stdout is unchanged; `--json` output gains the key) and the web shows a "List is incomplete" notice (`documents-truncated`, `trash-truncated`). A web client reads a reply without the field as not truncated. Not changed: the cap, the order, the filter and the sort.

## Tests

- `crates/application/tests/document_list_cap.rs` (8 tests, public `Application` over the file repository): exactly 1,000 and 1,001 stored; exactly 1,000 and 1,001 trashed (the newest 1,000 are kept, in order); three documents; a title search on a truncated list still reports `truncated`; the cap literal 1,000. The trash cases pause 1 ms after each delete because the store stamps trash time in whole milliseconds and ties break by id; this is a clock tick, not a wait for completion.
- `scripts/web-client-check.mjs`: 12 cases through `DocumentClient.list()` and `.trash()` (flag kept when true, accepted when false or absent, `"yes"`, `null` and `1` refused as `incompatible_response`).
- The browser smoke does not render the notices; the CLI line is not asserted by a test (it is one `eprintln!` after the `unreadable` loop).

## Mutations (each applied alone in a private copy)

| Mutation | Caught by |
| --- | --- |
| list comparison `>` to `>=` | `stored_documents_at_the_cap_are_all_listed` |
| trash comparison `>` to `>=` | `trashed_documents_at_the_cap_are_all_listed` |
| list `truncated` forced to false | `stored_documents_one_past_the_cap_are_truncated`, `a_title_search_on_a_truncated_list_still_reports_truncated` |
| `isList` ignores the field check | the three document-list refusal cases |
| `isTrashList` ignores the field check | the three trash-list refusal cases |

## Process notes

- The sandbox lint gap reported by one worker (nested `@eslint/js` under `apps/web/node_modules`) was not reproduced by the full `npm run check` runs of earlier slices; it is left as an observation.
