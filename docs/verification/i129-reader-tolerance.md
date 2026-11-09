# I129 web reader tolerance: verification record

Scope: issue #129. Within contract 1 (CONTRACT, "Version transitions"), every web reader of a server reply ignores fields it does not know. An unknown kind, a wrong type or value, a missing required field, or a record that does not fit the graph still refuses the whole reply as incompatible. The closed native clipboard and template format (CONTRACT P33a) and the native graphic-subtree clipboard (P33e) keep refusing unknown fields. The page builds its requests from the fields it knows, because the service refuses unknown request fields. The scene clipboard reply checks (`scene-clipboard-contract.ts`) compare only the fields the closed payload states (Copy settings and points, the Paste mapping reply and pasted objects, the Cut selection sets). The change covers the web contracts (`apps/web/src/lib/stereo-contract.ts`, `cip-contract.ts`, `document-contract.ts`), the editor helpers (`editor/settings.ts`, `atom-edits.ts`, `graphic-points.ts`), the request builders in `components/` (caption panel, fragment display panel, document workspace, style settings), the CONTRACT wording, the HANDOFF section and the tests in `scripts/web-client-check.mjs`.

## Run after the last edit

These are the test agent's results for this change. They were not re-run for this record.

- `scripts/web-client-check.mjs` on the host before the review-fix round: 770 tests, 770 passed.
- `scripts/web-client-check.mjs` in the sandbox after the review-fix round (with `npm ci --ignore-scripts` inside the command, so the caption suite's Vite hook can write): 778 tests, 778 passed. The lead re-ran the file once with the Paste and Cut tolerance edits in place (774/774) and the test agent re-ran it after the last four tests (778/778).
- An earlier sandbox run without `npm ci` inside the command had 11 cancelled tests: the caption suite's Vite hook could not write `node_modules/.vite-temp` on the read-only bind. That was an environment limit of that command line, not a failure.
- `scripts/web-styled-clipboard-client-check.mjs` in the sandbox: 81 of 81 passed.
- Full `npm run check` and the browser checks: the lead runs them once before push and records the result in the PR comment. This record does not include them.

## Tests

Titles are as they appear in `scripts/web-client-check.mjs`. Line numbers are left out because the file keeps changing; search by title.

New tests:

- `a document reply with an inferred hydrogen state that carries a count is accepted, and the count is ignored`
- `a list query negated from a reply is sent with only the fields the client knows, and known fields give the same JSON`
- `a graphic endpoint moved from a reply is sent with only the coordinates the client knows`
- `a setting outside the schema is ignored by a reply reader but refused by the closed clipboard reader`
- `a preference write sends the theme and the new-document settings it knows, whatever else the reply carried`
- `a CIP reply is identified end to end: a stored record must be the snapshot's own, key for key, and an unknown field outside it is ignored`

Changed in place:

- The hydrogen refusal loop, `test(\`a document reply with ${name} is refused\`)` was edited.
- `document settings are checked at the boundary and an older reply has none`: assertions changed.
- `a preferences reply is checked whole, computation included`: a stored field the client does not know is now accepted.
- `the notes a transaction reply carries are read, and a malformed note is refused`: assertions changed.
- `a snapshot whose enhanced stereo groups are inconsistent is refused as a whole`: refusal cases added, including an unknown group field that does not excuse a wrong number.
- `a malformed mark or stereo table is refused, a kind the client does not know yet is refused, and an unknown field is ignored` (was `... and so is a kind the client does not know yet`). Its ignored cases are a wavy mark with a narrow end, an unknown center that names a turn, and an unknown field in the stereo table.
- `an identity answer that ignored stereo is accepted only as partial, with the flags and the loss; an unknown flag is ignored` (was `... with the flags and the loss`).
- `the reply reader accepts the documented reply, ignores fields it does not know, and refuses each way it can be made untrue` (was `... and refuses each way it can be made untrue`). Its refusal list includes a stored configuration that differs from the snapshot's.
- `a double bond item must be a double bond of the snapshot with its own stored record, drawn entries must fit it, and a field a double bond lacks is ignored` (was `..., and drawn entries must fit it`).

Added in the review-fix round:

- `a double bond agreement ignores a key the drawn record lacks, and the reply echoes the snapshot key as stored`
- `a double bond agreement is refused when a known field of the drawn record differs yet the reply reports agreement`
- `scene copy accepts keys the snapshot adds to a copied point, to the settings and to a reference, and the payload stays closed`
- `scene copy refuses a reply whose copied point differs in a known value, even when the point also carries an extra key`
- `scene paste accepts keys the reply adds to a pasted graphic, its point, the mapping and its items`
- `scene paste refuses a reply whose translated point differs in a known value, even when the point also carries an extra key`
- `scene cut accepts keys the reply adds to its selection when the emptied sets are as expected` (client level, `DocumentClient.createSceneClipboardCut` with a scripted reply)
- `scene cut refuses a reply whose selection keeps an object, even when the selection also carries an extra key`

Existing and unchanged: `a reply with fields the reader does not know, at every level, still shows its labels; a missing or wrong required value does not`.

## Mutation proofs

Recorded by the test agent for this change. Not re-run for this record.

| Mutation | Recorded result |
| --- | --- |
| A key-set check re-added to a notes or mark reader | Fails the flipped tests |
| An unknown kind accepted | Fails the kind test |
| A wrong setting value accepted | Fails the settings tests |
| `knownSettingsEdit` returns its input | Fails the send-back test |
| `isDocumentSettings` made open | Fails the new closed-reader test and two styled-clipboard client tests |
| `bondsAgree` replaced by the old strict `sameValue` | Fails the extra-key agreement test (the refusal test passes) |
| `bondsAgree` compares only `kind` | Fails the refusal test, at its `disagrees` control line first |
| Copy uses the raw reply point, or the raw settings | Fails the copy acceptance test |
| Paste compares the raw after-paste object, or `exactKeys` is put back on the mapping | Fails the paste acceptance test |
| Cut compares the whole reply selection again | Fails the cut acceptance test |

## Decisions

- Readers of server replies are open. An unknown field is ignored. An unknown kind, a wrong type or value, a missing required field, or a record that does not fit the graph refuses the reply.
- The closed formats stay closed. The P33a payload structs in the service (`crates/application/src/clipboard.rs`) use `deny_unknown_fields`. The page checks the top-level keys of a P33a payload (`clipboardPayloadIds` in `document-contract.ts`) and the keys and settings of a P33e payload (`payloadOf` in `scene-clipboard-contract.ts`, which uses the closed `isDocumentSettings`).
- `isSettings` takes a reading mode, `closed` or `open` (`SettingsReading`, threaded through `isSettingValue`). `isDocumentSettings` is closed. The reply readers (`document.settings`, `new_documents`, `new_document_settings`) are open.
- Identity checks stay exact. A CIP `stored` record (`storedFits` in `cip-contract.ts`) and a `chem.stereo` `stored` entry (`readStereoInspection` in `stereo-contract.ts`, with `sameValue`) must equal the snapshot's own record, key for key. `sameJson` in `document-contract.ts` is unchanged; it still compares reply and snapshot values in full.
- Requests carry known fields only. The service's request DTOs refuse unknown fields. The builders that copied reply objects now send the known fields: `setQueryNegated` (`editor/atom-edits.ts`), the graphic endpoint move (`editor/graphic-points.ts`), the caption and fragment-display runs, `remove_reference` (`components/document-workspace.tsx`), and the style settings and preference writes (`knownSettingsEdit` and `knownUiPreferences` in `editor/settings.ts`).
- A known field of another variant is ignored by the reader: `narrow` on a wavy mark and `turn` on an unknown center (the mark and stereo table test), and `count` on an inferred hydrogen state (the hydrogen test). The editor reads a mark's `narrow` end only for a wedge or a hash (`editor/geometry.ts`, `editor/mark-gestures.ts`, `editor/stereo-status.ts`).

## Not run or not covered

- Browser checks run in the sandbox after the last edit, each green: `test:browser:stereo-manual`, `test:browser:scene-clipboard`, `web-caption-check`, `web-fragment-display-check`, `web-graphic-points-check`. The abbreviation and the other browser scripts were not run locally; CI runs the full set. A browser check of `web-abbreviation-check` times out locally even on a clean `origin/main`.
- `remove_reference` (`components/document-workspace.tsx`) has no browser flow and no unit test. The caption and fragment-display builders have browser flows (`scripts/web-caption-check.mjs`, `scripts/web-fragment-display-check.mjs`) that create runs through the production forms. They do not feed a reply that carries an unknown field, so they do not show that such a field is dropped.
- The known-fields rule is unit-tested for `setQueryNegated` (its list query test), the graphic endpoint move and the preference write. No test added in this change sends an extra field to the live service.
- An extra key inside a stored `chem.stereo` center or double-bond record that the snapshot does not hold is refused and tested (the identity exception: `an extra field in a stored center that the snapshot does not hold` and the stored double-bond case). A key both hold is accepted. The CIP `stored` record is tested the same way in the end-to-end CIP test.
- The editor's kind check on a mark's `narrow` end is covered by `a wedge asked of a bond whose mark carries a narrow end it does not read starts at the begin`; the mark-gesture and geometry call sites are checked by the compiler and the stereo browser check, not by a dedicated test.
- Change-detection guards that compare with `JSON.stringify` (`editor/atom-edits.ts`, `editor/mark-gestures.ts`, and the spline, polygon, image and graphic-point forms) were left as they are. An unknown key in a reply only makes such a guard send one redundant request for an unchanged value.
- `onlyKeys` (`document-contract.ts`, a subset check used for the clipboard payload ids) differs from `exactKeys` (the full key set), so it is not a duplicate of it.
