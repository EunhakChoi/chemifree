# P48 S3: web Import card verification

Scope: one web Import card replaces the import halves of the SMILES, MOL and SDF, and CDXML/CDX panels (client policy "Web Import card (P48 S3; no service change)" in docs/CONTRACT.md). No service, reader, loss or consent change.

## Commands (local, worktree head before the PR)

| command | result |
|---|---|
| `npm run lint` (ESLint, outside the sandbox) | pass |
| `npm run check` | pass |
| `CHEMIFREE_SMOKE_NO_SKIPS=1 npm run test:browser` (Chrome) | pass, no skipped steps |
| `npm run test:browser:smiles-clipboard` | pass |
| `npm run test:browser:mol-clipboard` | pass |
| `npm run test:browser:cdxml-import` | pass |
| `npm run test:browser:cdxml-clipboard` | pass (unchanged file) |
| `npm run test:browser:clipboard-integration` | pass |
| `npm run test:traceability`, `npm run test:roadmap` | pass; totals unchanged |

## New checks

- `scripts/web-import-detect-client-check.mjs`: extension table, sniff order, conflicts, pasted text never CDX, bounded scan.
- `scripts/web-import-decode-client-check.mjs`: per-format decoding (BOM kept only for CDXML, invalid UTF-8 refused), base64 for CDX at chunk edges, detection sentences.
- Browser smoke step "Import card" (`scripts/lib/import-card-smoke.mjs`): results keys `import_card_single_entry`, `import_pick_smiles_file`, `import_drop_mol_file`, `import_paste_sdf_text`, `import_ambiguous_sends_nothing`, `import_override_clears_preview`, `import_empty_screen_needs_document`, `import_drop_cdx_empty_screen`, `import_paste_cdxml_text_open_document`; screenshot `editor-import-card.png`.
- `scripts/web-cdxml-import-check.mjs`: an open-document layout (the card next to the workspace's creation notice without `onClear`); after a restart and clearing the attempt, the reviewed preview is not offered again.

## Migrated checks

`web-smiles-clipboard-check.mjs`, `web-mol-clipboard-check.mjs`, `web-cdxml-import-check.mjs` and the smoke steps `smiles_panel`, `ctfile_preview_import_export`, `ctfile_v3000_version_consent_roundtrip`, `cdxml_projection_with_loss_consent`, `cdxml_unsafe_projection_refused`, `cdx_import_with_consent`, `cdxml_delayed_preview_is_stale` now find import controls in the card. Results keys and assertions are kept; a picked CDXML/CDX file now needs the explicit read click (lead decision Q1), and the CDXML check asserts that a pick alone sends nothing.

## Mutation proofs

Each mutation was applied in a sandbox copy only, the named check failed, and the restored copy matched the worktree.

| # | mutation | caught by |
|---|---|---|
| M1–M4 (detect) | swap SDF/MOL sniff; drop the conflict rule; strong SMILES; CDX from text | detect client check cases 4, 2/7, 7, 10 |
| decode M1–M11 | BOM handling, fatal decoding, base64 padding/chunk edge, CDX subarray, sentence wording, conflict condition | decode client check |
| M5 | flow key without the effective format | smoke `import_override_clears_preview`: "a changed format drops the CT preview" |
| M6 | clipboard read keeps the format choice | cdxml check: "a clipboard read resets the format choice and the fixture is detected" |
| M7 | SMILES/MOL/SDF mounted with no document (needs-document branch made dead) | smoke `import_empty_screen_needs_document`: the `import-needs-document` wait times out |
| M8 | auto-read on CDXML file pick | cdxml check: "a pick sends nothing" |
| M9 | no flow reset on typed edit | smoke `smiles_panel`: a wait times out (the stale-request notice step, attributed by step order) |
| M10 | SMILES recovery alert also rendered inside the flow | smiles check: recovery button count (includeHidden) |
| M11 | CDXML preview not bound to the service generation | cdxml check: "clearing an attempt in an open document drops the reviewed preview" |

## Review dispositions (workflow review before the PR)

20 candidates were judged; 19 were rejected as spec-mandated, pre-existing or out of scope. One was judged real: after clearing an uncertain CDXML/CDX creation attempt in an open drawing the reviewed preview would stay creatable. A fix was tried and then reverted: the clear button appears only after a detected service restart, and the restart moves the service generation, which already hides a preview made for the old service (`InterchangeImportFlow` compares `prepared.generation`). Removing the fix left the new open-document assertion passing, and M11 shows that this assertion depends on the generation check. The fix would also have dropped the preview after a definitive refusal, a behavior change outside this slice.

## PR review fixes (#280)

- Pasted SDF text had no client cap: it now gets the 200 KiB UTF-8 cap of an SDF file (smoke `import_paste_sdf_text` asserts the alert and that nothing is sent).
- While a replacement file was being read, the previous file's flow stayed mounted and importable (Codex P2 and the independent review): the route is now hidden while a file is read.
- README no longer says the web formats are used without detection.

Both reviews and every disposition are recorded in the PR.
