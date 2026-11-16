# P48 S5: web Export card verification

Scope: one web Export card replaces the four export panels (client policy "Web Export card (P48 S5; no service change)" in docs/CONTRACT.md). No service, writer, loss or consent change.

## Commands (local, worktree before the PR)

| command | result |
|---|---|
| `npm run lint` | pass |
| `npm run check` | pass (the `test:contracts` step failed once on its 13 s "drafted while edits continue" timing bound under load and passed twice on rerun; no backend change in this PR) |
| `CHEMIFREE_SMOKE_NO_SKIPS=1 npm run test:browser` (Chrome) | pass, no skipped steps |
| `test:browser:smiles-clipboard`, `mol-clipboard`, `cdxml-clipboard`, `compute-stop`, `cdxml-import`, `clipboard-integration` | pass |
| `test:conformance` (121), `test:traceability`, `test:roadmap` | pass |

## New checks

- `scripts/web-export-formats-client-check.mjs`: format order and labels, `parseExportFormat`, `exportFileName`, and that each download name is detected as the same format by the Import card (SVG detects nothing).
- Smoke step "Export card" (`scripts/lib/export-card-smoke.mjs`, `scripts/lib/export-card-reopen-smoke.mjs`): `export_card_single_entry`, `export_format_switch_drops_result`, `export_cancel_by_switch_sends_no_file`, `export_files_leave_history_unchanged`, `export_mol_file_reopens`, `export_corrupt_file_refused_history_kept`, `import_clear_cancels_preview_history_kept`, `export_sdf_file_reopens`, `export_cdxml_file_reopens`, `export_cdx_file_reopens`, `export_smiles_text_reopens`, `export_svg_file_is_not_importable`, `export_card_absent_on_empty_screen`; screenshot `editor-export-card.png`.

## Migrated checks

`web-smiles-clipboard-check.mjs`, `web-mol-clipboard-check.mjs`, `web-cdxml-clipboard-check.mjs`, `web-compute-stop-check.mjs`, `scripts/lib/import-card-smoke.mjs` (`import_card_single_entry`) and the smoke steps `ctfile_export_response_race`, `ctfile_preview_import_export`, `ctfile_v3000_version_consent_roundtrip`, `interchange_export_consent_binary_and_groups`, `interchange_export_response_race`, `interchange_export_service_generation`, `svg_export_download`, `svg_export_consent_and_stale_guards`, `smiles_panel` now reach the export controls through the Export card (`exportCard(page, format)`). Results keys and assertions are kept; where a step used both CDXML and CDX buttons of one panel, it now selects the other format first, which drops the previous preview as the card promises.

## P48 acceptance, clause by clause

"손상·다중 레코드·큰 입력·취소·부분 지원에서 원본과 이력을 보존한다. reject/report 정책과 실제 저장 파일의 재개방을 검증한다." Original means the given source and the target drawing; history means revision, undo/redo depth and document content.

| clause | existing evidence | new in S5 |
|---|---|---|
| 손상 | smoke `cdx_import_with_consent` (truncated CDX refused), `cdxml_unsafe_projection_refused`, `ctfile_v3000_version_consent_roundtrip`; `web-import-decode-client-check.mjs`; `ct_operations.rs` bad-record tests | `export_corrupt_file_refused_history_kept` |
| 다중 레코드 | smoke `import_paste_sdf_text` (record 2 of 2); `ct_operations.rs` record tests | `export_sdf_file_reopens` |
| 큰 입력 | `import_ambiguous_sends_nothing` (201 KiB refused before reading), `cdx_import_with_consent` (121 KiB), per-format caps in the clipboard checks, pasted SDF 200 KiB, `cumulative_export_limit_points_to_the_record_that_crosses_it` | none needed: nothing is sent |
| 취소 | `import_override_clears_preview`, mol-clipboard late read after Clear, `cdxml_delayed_preview_is_stale`, compute-stop time-limit and cancel. An import or export request has no cancel signal of its own. | `export_cancel_by_switch_sends_no_file`, `import_clear_cancels_preview_history_kept` |
| 부분 지원 | loss-consent steps (`ctfile_v3000_*`, `interchange_export_consent_binary_and_groups`, `svg_export_consent_and_stale_guards`, `smiles_panel`); S1/S2/S4 layout losses | `export_svg_file_is_not_importable` |
| reject/report 정책 | the refusal and LossList displays above; CONTRACT reason tables | the two refusal keys above |
| 실제 저장 파일의 재개방 | smoke `save`, `close_and_reopen`, `crash_recovery_restores_drawing_and_history`; V3000 download re-imported | `export_{mol,sdf,cdxml,cdx}_file_reopens` (picked from the saved path), `export_smiles_text_reopens`, `export_files_leave_history_unchanged` |

## Mutation proofs

| # | mutation | caught by |
|---|---|---|
| X1 | `exportFileName('cdx')` returns `chemifree.cdxml` | client check (file name and reopen-detection cases) |
| X2 | `parseExportFormat` accepts `'png'` | client check (refuse case) |

X3–X9 (smoke guards) are recorded in the PR.

## Reviews

The workflow review judged 14 candidates; none was real (stale candidates against scripts migrated later, or behaviour moved unchanged as E3 requires). PR reviews (Codex and the independent review) are recorded in the PR.
