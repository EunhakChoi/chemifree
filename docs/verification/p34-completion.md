# P34 completion (bounded): rings, user templates, Et display and attachment editing

Roadmap scope: "독립 고리·치환기와 사용자 템플릿, 약어의 축약·펼침과 attachment를 구현한다. 출처가 다른 템플릿을 provider로 분리한다." Acceptance: "삽입·연결·교체·확장 전후 그래프와 ID를 검사하고 원본 monomer geometry를 복사하지 않는다." Closed within the scope documented in docs/CONTRACT.md by #174, #175, #177, #178, #180, #181, #182, #183, #185 (slices P34a–i), #291 (S1) and #292 (S2). Et display text editing, #201, is P35o and is cited only as a follow-up of P34i.

## How the scope is read (user decision, option A)

- "출처가 다른 템플릿을 provider로 분리" is met by two template sources that use separate code paths, without a template provider port: built-in carbon ring recipes computed in the domain (`insert_ring`, `attach_ring`, sizes 5 and 6, optional Kekulé for 6) and user-authored native v1 fragment files (`document.template_*` operations and native paste). The roadmap scope and acceptance text are unchanged; P34 reopens, or a new row is added, if a provider port or a DATA-backed library is wanted.
- "치환기" is read as rings attached to an existing atom and user template files attached or substituted at an explicit anchor; "약어" as the graph-preserving terminal Et display.
- "원본 monomer geometry" is read as monomer or template geometry from loaded DATA (for example HELM monomer molfiles).

## Criterion → evidence

| Criterion | Evidence |
|---|---|
| Insert, built-in ring: graph and IDs before/after | `crates/domain/tests/insert_ring.rs` `ring_is_one_independent_six_cycle_with_current_bond_length_and_stable_history_ids`, `five_ring_has_independent_geometry_cycle_and_one_step_history`, `kekule_six_cycle_has_explicit_orders_and_ordinary_history`, and the rollback tests (`late_coordinate_failure_rolls_back_prior_commands_counters_and_redo`, `atom_limit_and_identifier_exhaustion_do_not_leave_partial_rings`, `kekule_refusals_rollback_prior_commands_counters_and_redo`); `crates/application/tests/insert_ring.rs` `ring_replay_history_and_reopen_preserve_existing_drawing_and_selection`, `invalid_ring_requests_leave_document_revision_selection_and_redo_unchanged` |
| Insert, user template file | `crates/application/tests/native_clipboard.rs` `paste_allocates_fresh_ids_preserves_states_and_replays_original_mapping_after_undo_redo`, `paste_byte_limit_counts_utf8_and_accepts_the_exact_boundary`; `scripts/web-user-template-client-check.mjs` (64 KiB limit, strict UTF-8, filename, crossing-bond refusal) |
| Connect (attach) | `crates/domain/tests/attach_ring.rs` `attached_cycles_follow_direction_and_current_length_without_reusing_the_target`, `attached_kekule_has_exact_alternating_cycle_single_connector_and_stable_history`, `kekule_attachment_failures_preserve_the_complete_document_and_redo`; `crates/application/tests/attach_ring.rs` `attachment_clears_only_touched_stereo_and_prunes_groups_with_undo_replay_and_reopen`; `crates/domain/tests/template_attachment.rs` `translation_connection_mapping_and_preserved_source_stereo_survive_history`, `connector_counts_toward_the_exact_command_budget_and_late_id_failure_is_atomic`; `crates/application/tests/template_attachment.rs` `inspection_plan_atomic_apply_notes_replay_and_persistence_preserve_both_sources` |
| Replace | `crates/domain/tests/template_attachment.rs` `replacement_binds_a_nonfirst_source_anchor_and_preserves_the_outside_edge`, `replacement_repeated_refusals_do_not_consume_identifiers_or_history`, `replacement_budget_and_late_failure_roll_back_the_bound_atom_too`; `crates/application/tests/template_attachment.rs` `terminal_replacement_retains_boundary_identity_references_and_neighbor_stereo` |
| Expand (Et) | `crates/domain/tests/abbreviation.rs` `collapse_and_expand_change_only_display_and_each_have_one_undo_step`, `edits_to_members_boundary_and_local_marks_expand_atomically_without_stereo_loss`, `unsupported_boundary_states_and_multiple_placements_refuse_without_changing_anything`; `crates/application/tests/abbreviation.rs` `replay_save_reopen_recovery_and_history_keep_the_exact_graph_and_display`, `all_writers_emit_the_full_graph_and_a_rendering_loss_without_mutating_the_document` |
| No original monomer geometry copied | `crates/domain/Cargo.toml` has no dependencies, so the ring and template code cannot read DATA; HELM monomer data is read only by `crates/application/src/phase4_helm.rs` and `active_data.rs`, which no template operation uses. Ring coordinates are computed regular polygons at the document's current bond length (six-ring edges at 40 pt and five-ring edges at 60 pt are asserted in `insert_ring.rs`). User-file geometry is the user's own payload, only translated. |
| Web flows (additional, outside traceability) | CI browser scripts `test:browser:ring`, `test:browser:attach-ring`, `test:browser:user-template`, `test:browser:abbreviation` run on every PR and on main; each slice's merge commit passed them (runs listed in docs/HANDOFF.ko.md, P34 section). |

The common checks (`npm run check`) run the Rust tests above (`check:rust`), the client checks (`test:client`) and the contract checks; traceability parts `OPS-01.template-replacement` and `OPS-01.user-template-files` are now `implemented` on that evidence.

## Documented limits (not done)

- No template provider port and no DATA-backed template or monomer library; DATA is never turned into template geometry.
- Only Et as an abbreviation: no other abbreviations, user-defined abbreviations or text-to-graph abbreviations.
- No general substituents: only 5/6-carbon rings and user files; Kekulé only for the six-ring; no aromaticity inference.
- No fused, spiro or bridged attachment; replacement only at a terminal atom with one single outside bond; one placement only.
- No layout, collision avoidance or conformation.
- User-template request ordering, service-generation guards and the real download/reselect/insert flow are checked only by the CI browser script, not by `npm run check`.
- Broader template, abbreviation and model scope stays under MODEL-02.rest, MODEL-04.rest and OPS-02.rest with their other follow-ups.
