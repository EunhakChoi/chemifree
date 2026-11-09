# P26 chem.cip time limit: verification record

Scope: `chem.cip` takes the optional integer parameter `time_limit_ms`, resolved by the same compute limits as `chem.aromaticity_convert` (P41): default 5000 ms, maximum 8000 ms, host-settable and capped at one hour. One deadline covers the whole request: the scan, the centers and the bonds. A value above the maximum is `invalid_request` at `parameters.time_limit_ms` with `details.maximum_ms`, and the reply names the model `chemifree-cip-1`. A stop by the clock is `outcome` `error` with the diagnostic `time_limit` and `details: { limit_ms }` only, with no elapsed time and no result. A host cancel is still `cancelled`, and it wins over a passed deadline. This is a behaviour change: a request without `time_limit_ms` now runs under the 5000 ms default, where before it had no clock. The model id, the node caps and the labels are unchanged. The web client sends no `time_limit_ms`. P26 stays in progress. Issue #127 stays open: its time-limit part is resolved, and the dense-graph cap and the nodes-used figure remain.

## Run after the last edit

`npm run check` ran to the end in the sandbox after the last code edit of the first push: 2436 cargo tests passed, 2 skipped, client tests passed, conformance 118/118 after the conformance report was recorded again (`compute-time-limit-refused` has a new hash), traceability ok. After the review round (checkpoint after the graph snapshot, `Application::start_time_limit` shared by `chem.aromaticity_convert` and `chem.cip`, three tests) `npm run check` ran to the end again, exit 0: 2439 cargo tests passed, 2 skipped, client tests 0 failed, conformance 118/118, traceability ok; `npm run check` also builds the web. The browser smoke was not run locally. A first run of the gate failed once on the conformance case `stereo-cip-limits-and-errors` (`time_limit` for `resource_limit`, see Decisions) and was fixed before the push. The review-round changes (the checkpoint, the shared helper for the limit resolution, the tests) are verified by a second full run, which the lead records in the PR comment. The web build and the browser smoke are left to CI.

## Tests

In `crates/application/tests/cip_operation.rs`, as found by the last check (re-check these names if the file changes):

- `a_zero_time_limit_stops_the_labeling_and_changes_nothing`: a zero limit stops a `stored` request (the labeling) with `outcome` `error`, `result` null, `model_id` `chemifree-cip-1`, a `time_limit` diagnostic whose details are only `limit_ms` 0, and an unchanged document.
- `a_zero_time_limit_stops_the_scan_and_changes_nothing`: the same for a `stereogenic` request (the scan).
- `a_time_limit_one_above_the_maximum_is_refused_with_the_maximum`: with a 1000 ms maximum, 1001 ms is `invalid_request` at `parameters.time_limit_ms`, with `details.maximum_ms` 1000 and the model id.
- `a_time_limit_equal_to_the_maximum_is_accepted`: with the one-hour cap as the maximum, a request for exactly that value is `ok`.
- `the_default_maximum_time_limit_is_eight_seconds`: on an application with default limits, 8,000 ms is accepted and 8,001 ms is refused as `invalid_request` at `parameters.time_limit_ms` with `details.maximum_ms` 8000.
- `the_default_time_limit_applies_until_a_request_states_its_own`: with a 20 ms default, a request without a limit stops at `limit_ms` 20; a request that states 30 ms stops at 30.
- `a_cancel_wins_over_a_time_limit_that_has_passed`: a cancelled flag with a zero limit gives `cancelled` with empty details and an unchanged document.
- `the_catalog_advertises_chem_cip_with_its_limits_and_its_time_limit`: the catalog note of `chem.cip` names the time limit (the phrases in `TIME_LIMIT_NOTE`).
- (added in the review round of this PR; `a_zero_time_limit_stops_a_document_with_no_stored_stereo_and_changes_nothing`) `time_limit_ms: 0` on a document without stored stereo: the request stops with a `time_limit` diagnostic whose details are only `limit_ms` 0, and no result.
- (added in the review round of this PR; `a_zero_time_limit_stops_an_empty_document_and_changes_nothing`) `time_limit_ms: 0` on an empty document: the same stop, with `limit_ms` 0 and no result.
- (added in the review round of this PR; `a_cancel_wins_over_a_zero_time_limit_on_a_document_with_no_stored_stereo`) A cancel still wins over a zero limit: the request is `cancelled`, not `time_limit`.

## Decisions

- Shared limits. `chem.cip` resolves its limit through `ComputeLimits::resolve`, the function the convert uses, so the default, the maximum and the one-hour cap are one setting for both operations.
- Refused before the document is read. The limit is resolved before the document lookup, so a value above the maximum is refused whatever the document is.
- Whole-request deadline. The scan, the centers and the bonds share one `Budget`, so a stop in any stage ends the whole request with `time_limit` and no partial result.
- Budget after the snapshot (added in the review round of this PR). The budget is checked once, after the graph snapshot, so a zero limit, or a clock that expired during preparation, stops the request even when there is nothing to label.
- Cancel first. `Budget` asks the host's interrupt before the deadline, so a cancel that comes after the deadline has passed is still `cancelled`.
- No commit, no replay. `chem.cip` changes nothing and its result is never replayed. The convert's revision-conflict and replay rules do not apply, and a repeated `request_id` is computed again with its own `time_limit_ms`.
- The 5 s default exposed #127 in tests. The `too-big` and `one-atom` steps of the conformance case `stereo-cip-limits-and-errors`, and the Rust test `a_request_over_the_node_limit_is_refused_whole_with_the_numbers`, reach their work limits only after more than 8 s in a debug build, so under the default clock they ended in `time_limit` instead of `resource_limit` (an explicit `time_limit_ms: 8000` was not enough in the conformance run). The conformance runner and that Rust test now set the clock to its cap (`ComputeLimits::CAP_MS`, a host setting), so a work-limit case does not depend on the speed of the build. The conformance case `compute-time-limit-refused` therefore asks 3,600,001 ms and expects the maximum 3,600,000; the 8 s maximum of default limits is pinned instead by `the_default_maximum_time_limit_is_eight_seconds` in `crates/application/tests/cip_operation.rs`. The cost of reaching those work limits is the open part of #127.
- Behaviour change. Every `chem.cip` request without `time_limit_ms` now runs under the 5000 ms default, where before it had no clock. The web client's own 10 s timeout is unchanged.

## Not run or not covered

- The browser smoke was not run locally (the web change is comments only; CI runs it). The web build ran inside `npm run check`.
- The longest gap between CIP checkpoints is unmeasured.
- The release-build timing of the 12-17 s circulant cases of issue #127 is not measured.
- No lower node caps and no nodes-used figure (#127 stays open for them).
