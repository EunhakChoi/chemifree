# I129 web reader tolerance: verification record

Scope: issue #129. Within contract 1 (CONTRACT, "Version transitions"), every web reader of a server reply ignores fields it does not know. An unknown kind, a wrong type or value, a missing required field, or a record that does not fit the graph still refuses the whole reply as incompatible. The closed native clipboard and template format (CONTRACT P33a) and the native graphic-subtree clipboard (P33e) keep refusing unknown fields. The page builds its requests from the fields it knows, because the service refuses unknown request fields. The change covers the web contracts (`apps/web/src/lib/stereo-contract.ts`, `cip-contract.ts`, `document-contract.ts`), the editor helpers (`editor/settings.ts`, `atom-edits.ts`, `graphic-points.ts`), the request builders in `components/` (caption panel, fragment display panel, document workspace, style settings), the CONTRACT wording, the HANDOFF section and the tests in `scripts/web-client-check.mjs`.

## Run after the last edit

These are the test agent's results for this change. They were not re-run for this record.

- `scripts/web-client-check.mjs` on the host, after the last edit: 770 tests, 770 passed (after the review-round tests for a foreign narrow end and an extra key inside a stored record).
- `scripts/web-client-check.mjs` in the sandbox: 758 passed and 11 cancelled. The caption suite's Vite hook could not write `node_modules/.vite-temp` on the read-only bind. The test agent records this as an environment limit, not a failure.
- `scripts/web-styled-clipboard-client-check.mjs` in the sandbox: 81 of 81 passed.
- Full `npm run check`: the lead runs it and records the result in the PR comment. This record does not include it.

## Tests

Titles are as they appear in `scripts/web-client-check.mjs`. The number is the line where each test starts in the current file.

New tests:

- 1330 `a document reply with an inferred hydrogen state that carries a count is accepted, and the count is ignored`
- 1729 `a list query negated from a reply is sent with only the fields the client knows, and known fields give the same JSON`
- 1745 `a graphic endpoint moved from a reply is sent with only the coordinates the client knows`
- 1993 `a setting outside the schema is ignored by a reply reader but refused by the closed clipboard reader`
- 2224 `a preference write sends the theme and the new-document settings it knows, whatever else the reply carried`
- 4948 `a CIP reply is identified end to end: a stored record must be the snapshot's own, key for key, and an unknown field outside it is ignored`

Changed in place:

- The hydrogen refusal loop, `test(\`a document reply with ${name} is refused\`)`, around line 1299, was edited.
- 1970 `document settings are checked at the boundary and an older reply has none`: assertions changed.
- 2018 `a preferences reply is checked whole, computation included`: a stored field the client does not know is now accepted (line 2041).
- 2548 `the notes a transaction reply carries are read, and a malformed note is refused`: assertions changed.
- 2612 `a snapshot whose enhanced stereo groups are inconsistent is refused as a whole`: refusal cases added, including an unknown group field that does not excuse a wrong number (line 2634).
- 2771 `a malformed mark or stereo table is refused, a kind the client does not know yet is refused, and an unknown field is ignored` (was `... and so is a kind the client does not know yet`). Its ignored cases are a wavy mark with a narrow end, an unknown center that names a turn, and an unknown field in the stereo table (lines 2801 to 2806).
- 2857 `an identity answer that ignored stereo is accepted only as partial, with the flags and the loss; an unknown flag is ignored` (was `... with the flags and the loss`).
- 3834 `the reply reader accepts the documented reply, ignores fields it does not know, and refuses each way it can be made untrue` (was `... and refuses each way it can be made untrue`). Its refusal list includes a stored configuration that differs from the snapshot's (line 3841).
- 3888 `a double bond item must be a double bond of the snapshot with its own stored record, drawn entries must fit it, and a field a double bond lacks is ignored` (was `..., and drawn entries must fit it`).

Existing and unchanged: 4899 `a reply with fields the reader does not know, at every level, still shows its labels; a missing or wrong required value does not`.

## Mutation proofs

Recorded by the test agent for this change. Not re-run for this record.

| Mutation | Recorded result |
| --- | --- |
| A key-set check re-added to a notes or mark reader | Fails the flipped tests |
| An unknown kind accepted | Fails the kind test |
| A wrong setting value accepted | Fails the settings tests |
| `knownSettingsEdit` returns its input | Fails the send-back test |
| `isDocumentSettings` made open | Fails the new closed-reader test (1993) and two styled-clipboard client tests |

## Decisions

- Readers of server replies are open. An unknown field is ignored. An unknown kind, a wrong type or value, a missing required field, or a record that does not fit the graph refuses the reply.
- The closed formats stay closed. The P33a payload structs in the service (`crates/application/src/clipboard.rs`) use `deny_unknown_fields`. The page checks the top-level keys of a P33a payload (`clipboardPayloadIds` in `document-contract.ts`) and the keys and settings of a P33e payload (`payloadOf` in `scene-clipboard-contract.ts`, which uses the closed `isDocumentSettings`).
- `isSettings(value, complete, closed)` takes the `closed` flag. `isDocumentSettings` is closed. The reply readers (`document.settings`, `new_documents`, `new_document_settings`) are open.
- Identity checks stay exact. A CIP `stored` record (`storedFits` in `cip-contract.ts`) and a `chem.stereo` `stored` entry (`readStereoInspection` in `stereo-contract.ts`, with `sameValue`) must equal the snapshot's own record, key for key. `sameJson` in `document-contract.ts` is unchanged; it still compares reply and snapshot values in full.
- Requests carry known fields only. The service's request DTOs refuse unknown fields. The builders that copied reply objects now send the known fields: `setQueryNegated` (`editor/atom-edits.ts`), the graphic endpoint move (`editor/graphic-points.ts`), the caption and fragment-display runs, `remove_reference` (`components/document-workspace.tsx`), and the style settings and preference writes (`knownSettingsEdit` and `knownUiPreferences` in `editor/settings.ts`).
- A known field of another variant is ignored by the reader: `narrow` on a wavy mark and `turn` on an unknown center (test 2771), and `count` on an inferred hydrogen state (test 1330). The editor helpers do not follow this; see below.

## Not run or not covered

- Browser smoke: not run for this record.
- `remove_reference` (`components/document-workspace.tsx`) has no browser flow and no unit test. The caption and fragment-display builders have browser flows (`scripts/web-caption-check.mjs`, `scripts/web-fragment-display-check.mjs`) that create runs through the production forms. They do not feed a reply that carries an unknown field, so they do not show that such a field is dropped.
- The known-fields rule is unit-tested for `setQueryNegated` (1729), the graphic endpoint move (1745) and the preference write (2224). No test added in this change sends an extra field to the live service.
- No test puts an extra key inside a `chem.stereo` `stored` entry. The value mismatch is tested (line 3841). The CIP `stored` record is tested with an extra key (4948).
- `narrowAtom` (`apps/web/src/editor/stereo-status.ts`) and `cycleMark` (`apps/web/src/editor/mark-gestures.ts`) take a narrow end from `narrow` whenever the key is present, whatever the kind. A wavy or crossed mark that carries `narrow` would get a narrow end in those helpers. The service does not send that field, and no test covers the editor path. The reader ignores the field.
- The sandbox cannot run `scripts/web-client-check.mjs` to the end because of the read-only bind (see above).
