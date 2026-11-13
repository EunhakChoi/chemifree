# P41 web display of stop replies (T0, fourth pull request)

Status: bounded. The web shows the server's `time_limit` and `cancelled` replies for the seven operations it calls and shows. The server is unchanged (one comment). The web still sends no `time_limit_ms`.

## What the web does

- `readComputeStop` (`apps/web/src/lib/compute-stop.ts`) reads a stop only from `outcome: "error"` with exactly one `error` diagnostic coded `time_limit` (with `details.limit_ms`, a non-negative integer, since a request may ask for 0) or `cancelled`, `path` null, `result` null, an empty `loss_report`, the operation's `model_id` and an integer `document_revision` (null accepted for `chem.aromaticity_convert`). A stop code in another shape is `incompatible_response`; other refusals are not stops.
- `ComputeStopped` (in `document-contract.ts`, after `ApiError`) is thrown by `inspectProperties`, `inspectAromaticity`, `convertAromaticity`, `inspectIdentity`, `searchSubstructure`, `exportSmiles`, `inspectCip` and (since #106, 2026-11) `inspectStereo` before their own outcome checks. The automatic stereo check shows a stop as its failed check (the stop's message and "Check again"), not the stopped notice; `scripts/web-compute-stop-client-check.mjs` covers the `chem.stereo` time-limit, cancelled, missing-limit and wrong-model cases.
- Display: `ComputeStopNotice` (default Alert variant, retry button) in the properties, aromaticity, identity, substructure and SMILES panels; the page-level notice for the convert stop (Dismiss; the Convert buttons retry); the Stereo panel's CIP sentence. Results are cleared; no partial result is shown.

## Tests

- `scripts/web-compute-stop-client-check.mjs` (node:test, loaded by `web-client-check.mjs`; `npm run test:client`): the reader (valid stops, a limit of 0, convert revision null/integer, every malformed change, refusals that are not stops), each client method (stop, malformed stop, refusals unchanged), `cipView`/`cipFailureSentence` (a stop after an ok result shows no label).
- `scripts/web-compute-stop-check.mjs` (`npm run test:browser:compute-stop`, in CI): the five panels with stubbed replies: ok, then a time-limit stop (result gone, notice, no destructive title), retry (one more request, result back); identity also cancelled; SMILES retry carries no accepted losses and a style change clears the notice.
- `scripts/browser-smoke.mjs`: the CIP stop with a real retry, and the convert stop on the page-level notice (not destructive, Dismiss, inspection still shown, Convert still enabled).

## Mutations (private copies)

Killed: the stop-code condition of the reader replaced (refusal cases fail: they become incompatible instead of keeping their own errors); the `limit_ms` check dropped (the missing, string, negative and fractional rows fail); `cipView` returning the earlier ready view after a stop (the no-label case fails); the substructure panel not storing its stop (the earlier result stays; the harness times out waiting for it to go).
Survivors: the not-a-stop rows for `ok`/`unsupported` outcomes and a missing diagnostic return null before the changed condition, so the first mutation does not touch them (they are pinned by their own rows).

## Not covered

- MOL/SDF export has no time limit, so it is not part of this.
- A real stop of a slow server is not forced in a browser; replies are mocked (the server side is covered by the Rust tests and the contract smoke).
- The SMILES stop notice is not keyed to the drawing (like the SMILES error).
