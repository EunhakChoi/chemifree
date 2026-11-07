# P35 completion evidence audit

P35 remains `in_progress`. This draft audits source at `e21b47b`, based on merged
main `2b35e6c` (#202), while [#203](https://github.com/EunhakChoi/chemifree/pull/203)
is open and its current CI run is pending. #201 and #202 have shipped editable
Et text and explicit retained-fragment displays, so the earlier `f29195a` audit's
"fixed Et only, no abbreviation text editor" conclusion is superseded. #203 adds
display script runs in this branch; this draft does not claim it is merged or
fully CI-verified, or declare the original row complete. It changes no product
behavior and ran no tests, builds, browser checks or cargo checks.

The candidate/pending statements in the P35a–p and P34i implementation records,
and their copied README/CONTRACT/HANDOFF notes, describe those implementation
sessions. The merged PR and CI evidence below supersedes that execution status;
it does not turn unsupported behavior into implemented behavior.

## Requirement and decision

The unchanged P35 scope is “다중 스타일 Unicode run, 위·아래 첨자, 정렬,
원자·약어·caption 레이블을 편집하고 저장한다.” Its acceptance is “한글·결합
문자·긴 텍스트·run 경계와 codec 인덱스를 검사한다. 텍스트 수정이 원자 의미를
바꾸는 경우를 명시한다.” See [the roadmap JSON](../roadmap/implementation-roadmap.json).

The prior `f29195a` audit consulted the approved public packet outside Git: its AGENTS.md,
README.ko.md, `spec/phase7-implementation-entry.ko.md`, readiness requirements v2,
`phase3-design-v3/semantics.ko.md`, `phase3-design-v3/document.schema.json`,
`phase7-analysis-closure-checklist.json`, `deep-rendering-contracts.md`,
`phase7-original-functional-supplement.md` and
`integration-long-caption-contracts.json`. The packet is not copied here. This
follow-up uses that recorded specification comparison and the current
implementation; it does not claim a new packet review.
The semantics require ordered Unicode runs and explicit codec index/encoding
mapping failures; external byte/UTF-16 positions must not become internal string
indices. The Unicode supplement separates stored strings from platform-dependent
serialization and visual appearance. The 66,000-character integration observation
is one authored carrier case, explicitly not a universal length guarantee.

The recorded [user decision](p34i-ethyl-display.md#authorized-boundary) is to retain
the actual chemical graph and contract its display. [The current contract](../CONTRACT.md#explicit-retained-graph-fragment-display-p35p-candidate)
and subsequent P35o–q slices apply that decision to explicit text, member selection
and script runs. Display strings and scripts never define chemistry. Editing
typed atom state can change chemical meaning; editing display text cannot. No
nickname dictionary or text-to-graph interpretation is inferred to fill a gap.

Merged #201 supplies an existing Et display's plain-text editor/reset. Merged
#202 supplies explicit eligible fragment membership, plain-text editing and
expansion. #203's candidate supplies ordered baseline/subscript/superscript runs
on that display. These address the former missing abbreviation editing path and
must be assessed as graph-preserving display editing, rather than described as
fixed Et. The public packet's `nickname`/`fragment` atom union carrying `label`
and `expression` is still not implemented as those graph atom kinds: this product
stores separate display metadata over real atoms. That distinction remains an
explicit compatibility boundary, not a new arbitrary atom union or parser added
by this audit. The parent must reconcile the approved display-only decision with
the unchanged roadmap acceptance after #203 merges and its final checks pass;
this draft makes no final completion decision.

## Implemented acceptance evidence

| Requirement | Actual behavior and exact source/regressions |
| --- | --- |
| Unicode, Korean, combining and ZWJ | `crates/domain/src/scene.rs` stores original ordered run strings. `crates/domain/tests/caption.rs::caption_unicode_and_explicit_scripts_are_verbatim_and_one_undo_step` uses Korean, NFD combining text and woman-scientist ZWJ emoji with baseline/subscript/superscript runs. `scripts/web-caption-check.mjs` renders these against the real service and tests edit/Undo/save/reopen. No Unicode normalization or formula interpretation occurs. |
| Multi-style runs and boundaries | `CaptionRun` and caption validation in `scene.rs`, `crates/application/tests/caption.rs`, `crates/providers/tests/caption_format.rs`, and `scripts/web-caption-client-check.mjs` preserve ordered nonempty runs and independently compose script, weight/style, family, size, colour, underline, outline and shadow. `apps/web/src/components/caption-panel.tsx` edits explicit whole runs. Native current/history/draft fields and migrations are strict. This is not a text-cursor/grapheme editor. |
| Long text and limits | `caption.rs::byte_and_run_limits_are_not_character_or_utf16_limits` and client checks cover 1–32 runs and 16,384 UTF-8 bytes, including multibyte Korean boundaries and rejection beyond limits. `crates/domain/tests/caption_budget.rs` and provider caption migration fixtures cover admission/history and physical persistence bounds. These explicit refusals do not claim unlimited text or the external 66,000-character caption profile is admitted to the editor. |
| Alignment | `caption.rs::whole_caption_alignment_keeps_anchor_runs_graph_and_one_undo_step`, `apps/web/src/editor/scene-caption.tsx`, and the caption browser script cover whole-single-line left/center/right alignment around a fixed anchor with natural advance including edge spaces. Script/style changes preserve the anchor; alignment is physical rather than logical bidi start/end. |
| Atom semantics | `apps/web/src/editor/atom-edits.ts` builds `update_atom` commands for element/isotope/charge/H/radical/query/attachment state. These change recorded chemical state and can change graph identity/calculations; the visible label is derived from state by `apps/web/src/editor/label.ts`. Document settings font/size/carbon display and P35j–n weight/style/underline/outline/shadow change presentation only. They do not parse arbitrary atom label text or assign chemistry to captions. |
| Et text semantics (#201, merged) | `crates/domain/src/document/abbreviation.rs` retains its neutral C2/single-boundary eligibility. `update_abbreviation_label` edits a nonempty single-line string up to128 UTF-8 bytes; explicit null resets Et. Domain/application/provider display-label checks and `scripts/web-display-label-check.mjs` cover literal Unicode, history/replay/reset, graph/identity/formula preservation and measured SVG bounds. This is now editable display text, without a substituent parser. |
| Fragment membership/text (#202, merged) | `crates/domain/src/document/fragment_display.rs`, `crates/domain/tests/fragment_display.rs` and `crates/application/tests/fragment_display.rs` cover explicit member/anchor contraction, text edits, expansion, exact graph/ID/state/coordinate/identity preservation, refusals, history/replay and full-graph scientific/export behavior. `crates/providers/tests/fragment_display_format.rs` covers strict v24/current/history/draft and old-byte archives; `scripts/web-fragment-display-check.mjs` exercises the actual form, hidden members, whole-member dragging and native roundtrip. Bounds and eligible shapes remain explicit below. |
| Fragment script runs (#203, pending) | `explicit_scripts_are_preserved_in_history_and_plain_text_resets_to_one_baseline_run`, `run_budget_uses_total_utf8_bytes_and_nonempty_runs_without_an_independent_count_limit` and provider v24/v25 migration/strict-history tests preserve complete ordered runs. Application `rich_run_edit_replays_as_one_transaction_and_plain_text_resets_scripts`, `scripts/web-fragment-display-client-check.mjs` and the production browser script cover Korean, combining/ZWJ text, exact run/text agreement, baseline reset, IME, sub/sup positions and native reopen. This branch adds these checks; final PR CI and merge are pending, as detailed below. |
| Measured rendering | `scene-caption.tsx`, `label.ts` and `scripts/web-label-{weight,style,underline,outline,shadow}-check.mjs` cover mounted SVG ink/advance, fit, trimming, picking and selection, family/size/style combinations, font completion and viewport/document provenance. Stored Unicode and bounds checks are distinct from universal glyph or legacy pixel equivalence. |
| CDXML rich runs | `crates/providers/tests/cdxml.rs` and `cdxml_golden.rs` exercise the separate typed interchange tree with style-run Unicode text, XML entities, properties and ordered children. CDXML `<s>` child runs have text content rather than editor byte offsets. This tree does not import/export editor captions. |
| CDX indices and strict encoding | `crates/providers/src/cdx/text.rs` converts encoded byte ranges into independent strings; `crates/providers/tests/cdx_reader.rs::style_runs_must_cover_the_text_in_order_with_known_fonts_and_colors` checks zero first index, ordering, range and known tables. `unstyled_text_is_latin_one_and_refuses_the_ambiguous_range`, `styled_text_applies_the_same_encoding_rules`, `font_charsets_and_platforms_are_never_guessed`, and writer `text_that_cannot_be_encoded_is_refused_not_approximated` keep the bounded Latin-1/charset restrictions explicit. Unsupported Korean/emoji encodings and ambiguous 0x80–0x9f are not guessed. |
| CDX long framing | Reader `extended_lengths_carry_long_text_and_may_spell_short_values` and writer `long_text_uses_the_extended_length_and_reads_back` use 70,000 ASCII bytes and the extended property length; the writer asserts the 70,002-byte string payload and rereads it. This exercises binary framing separately from the editor's 16 KiB caption bound. |

## Boundaries retained

Single-line captions reject line breaks, line separators, invalid controls, empty
runs, all-White_Space content and malformed strings; FEFF is not Unicode
White_Space. The existing renderer exercises RTL/mixed-script samples, ligatures,
combining marks and ZWJ, but provides no explicit bidi editing/direction controls,
paragraph layout/wrapping or cross-run grapheme shaping guarantee. Browser/font
shaping of an individual sample does not prove every script or run boundary.
These limits are recorded without introducing new P35 acceptance requirements.
General logical text/line-break rendering in public rendering matrix R13 remains
part of incomplete RENDER-01, not evidence of P35 completion.

Arbitrary atom text, per-atom rich-text runs, nickname dictionaries, text-to-graph
parsing and external caption/display-label roundtrip remain absent. Fragment
displays require one explicit placement, neutral/default element members forming
an induced connected acyclic unmarked-single-bond tree and exactly one unmarked
single external boundary at the anchor. Query/nondefault states, rings, multiple
boundaries/placements, member overlap with Et/another fragment and local
marks/stereo/groups touching members or their outside neighbor refuse. Partial
member geometry and affected chemistry reveal the display in the same Undo step;
whole-member rigid transforms and outside-neighbor-only coordinate moves retain
it. Expansion performs no layout. Native clipboard/Cut/template operations refuse
member intersections until expansion.

#203's display runs inherit document label typography, vary only script and keep
their total128 UTF-8 byte single-line limit. They do not add per-run font, colour,
weight/style overrides or alignment controls to fragments. Caption alignment and
multi-style runs remain independently implemented as above. Native v25 requires
runs without top-level text, v24 requires text without runs and migrates every
current/history/draft display to one baseline run; exact-byte archives precede
replacement/recovery, and future v26 refuses untouched. These source-inspected
candidate rules are not a final CI claim.

Caption transforms/arrange/graphics
clipboard refuse the whole unsupported request. Drawing exports report caption
loss and require consent; chemical label styles use `drawing_settings` loss and
Et and fragment displays use `abbreviation_display` loss while retaining the
complete chemical graph.
The separate CDXML/CDX rich-tree carrier cannot be cited as native editor-caption
roundtrip. No original application DLL is used as an execution engine.

## Merged delivery and CI

Read-only GitHub PR metadata was checked on 2026-11-07 KST. Every row below has
successful `check` and `browser` jobs at its final PR head. CI links identify PR
runs; they are not a claim of a newly executed audit run or a distinct main-branch
run. PR bodies record both the independent fresh-context review and GitHub Codex
review, their exact reviewed heads/times where retained, and finding dispositions.
Those historical reviews are not reviews of this documentation change.

| Slice | Merged PR | Passing CI run |
| --- | --- | --- |
| Unicode caption | [#184](https://github.com/EunhakChoi/chemifree/pull/184) | [42217895331](https://github.com/EunhakChoi/chemifree/actions/runs/42217895331) |
| Fixed Et display | [#185](https://github.com/EunhakChoi/chemifree/pull/185) | [42222883519](https://github.com/EunhakChoi/chemifree/actions/runs/42222883519) |
| Caption weight/style | [#187](https://github.com/EunhakChoi/chemifree/pull/187) | [42228057737](https://github.com/EunhakChoi/chemifree/actions/runs/42228057737) |
| Caption alignment | [#188](https://github.com/EunhakChoi/chemifree/pull/188) | [42233018423](https://github.com/EunhakChoi/chemifree/actions/runs/42233018423) |
| Caption font | [#189](https://github.com/EunhakChoi/chemifree/pull/189) | [42236925347](https://github.com/EunhakChoi/chemifree/actions/runs/42236925347) |
| Caption size | [#190](https://github.com/EunhakChoi/chemifree/pull/190) | [42245005601](https://github.com/EunhakChoi/chemifree/actions/runs/42245005601) |
| Caption colour | [#191](https://github.com/EunhakChoi/chemifree/pull/191) | [42251326854](https://github.com/EunhakChoi/chemifree/actions/runs/42251326854) |
| Caption underline | [#192](https://github.com/EunhakChoi/chemifree/pull/192) | [42255514410](https://github.com/EunhakChoi/chemifree/actions/runs/42255514410) |
| Caption outline | [#193](https://github.com/EunhakChoi/chemifree/pull/193) | [42263530510](https://github.com/EunhakChoi/chemifree/actions/runs/42263530510) |
| Caption shadow | [#194](https://github.com/EunhakChoi/chemifree/pull/194) | [42270405347](https://github.com/EunhakChoi/chemifree/actions/runs/42270405347) |
| Chemical weight | [#195](https://github.com/EunhakChoi/chemifree/pull/195) | [42279426529](https://github.com/EunhakChoi/chemifree/actions/runs/42279426529) |
| Chemical style | [#196](https://github.com/EunhakChoi/chemifree/pull/196) | [42285589480](https://github.com/EunhakChoi/chemifree/actions/runs/42285589480) |
| Chemical underline | [#197](https://github.com/EunhakChoi/chemifree/pull/197) | [42292016972](https://github.com/EunhakChoi/chemifree/actions/runs/42292016972) |
| Chemical outline | [#198](https://github.com/EunhakChoi/chemifree/pull/198) | [42298128975](https://github.com/EunhakChoi/chemifree/actions/runs/42298128975) |
| Chemical shadow | [#199](https://github.com/EunhakChoi/chemifree/pull/199) | [42304228461](https://github.com/EunhakChoi/chemifree/actions/runs/42304228461) |
| Editable Et display text | [#201](https://github.com/EunhakChoi/chemifree/pull/201) | [42311153640](https://github.com/EunhakChoi/chemifree/actions/runs/42311153640) |
| Explicit retained-fragment display | [#202](https://github.com/EunhakChoi/chemifree/pull/202) | [42316586336](https://github.com/EunhakChoi/chemifree/actions/runs/42316586336) |

The final #199 run passed `check` at 2026-11-06 22:31:13 UTC and `browser` at
22:33:54 UTC. Its body records a fresh isolated full check, smoke and the five
chemical-label, caption and abbreviation matrices; the final contract-only fix
also received passing CI. Earlier failing runs and fixes are retained in their
PRs and implementation records; a final pass does not erase those findings.

The #201 metadata confirms merge `00332b5` at 2026-11-06 23:37:06 UTC and
successful final-head `bfb54d7` CI `42311153640`; #202 confirms merge `2b35e6c` at
2026-11-07 00:21:21 UTC and successful final-head `8d7efbe` CI `42316586336`.
Both runs' `check` and `browser` jobs were read directly for this draft. These
are PR runs, not new executions by this audit. #202's browser includes the actual
retained-fragment form/Unicode/native-roundtrip scenario. PR bodies report full
isolated checks and relevant browser matrices separately.

#201 records an independent fresh-context review at `3a9a43e`, catalog-count and
pre-retention history fixes, and Codex review at `7348123`; the missing supported
v22 diagnostic was fixed in `bfb54d7`. #202 records independent review at
`8fc2889` with no actionable finding and Codex's missing supported-v23 diagnostic,
fixed in `411a773`. Its off-screen browser click failure was fixed in the test.
These dispositions come from the merged PR bodies; they are not new reviews of
this audit draft.

## #203 pending evidence and final decision gate

At the metadata inspection for this draft, #203 is open with head `e21b47b` and
[CI run 42319966036](https://github.com/EunhakChoi/chemifree/actions/runs/42319966036)
is `in_progress`. There is no confirmed merge SHA or complete successful current
PR CI claim here. Its PR body and parent handoff report successful isolated full
`npm run check`, routes, no-skip general smoke, fragment/display-label/Et/caption
browser checks and focused Rust/frontend checks. The body records that the
previous `9b2e150` browser job passed, while `check` reached the 15-minute limit
during conformance; `e21b47b` changes that job's limit to 25 minutes and is being
rerun. A timeout is not a full passing run. No check was rerun by this audit.

The #203 body records Codex and independent fresh-context review at `9be4dd8`;
both identified the wrong post-reopen browser ink assertion label, fixed in
`9b2e150` with an exact saved/reopened script-position comparison. It reports no
other actionable independent finding and no repeat review request. Final merge,
current-head CI and any later findings still need reconciliation by the parent.

Before a final P35 state decision, replace this pending snapshot with verified
final head/merge/run evidence and reconcile the unchanged scope and acceptance
table against the approved graph-preserving decision. Keep unsupported atom text,
per-atom rich runs, external roundtrip and bounded display eligibility explicit.
Do not infer dictionary/parser requirements, count an unexecuted check as passed,
or promote all RENDER-01/STYLE-01 work merely because the bounded P35 path ships.
P35 remains `in_progress` in this draft; the parent owns the later conclusion.

## Documentation integration checks still required

The parent integration should run `npm run test:traceability` and
`npm run test:checks`, validate that the roadmap HTML embeds the same JSON, and
review the updated `STYLE-01.4` evidence links. This audit intentionally performs no
execution verification. Product checks need rerunning only if integration changes
product code or exposes a concrete concern; this commit changes documentation.

Historical original-audit integration follow-up: the first isolated `test:traceability` invocation
reported stale HANDOFF/TRACEABILITY summary totals after adding `STYLE-01.4`
(109/60 rather than 110/61), and a missing fresh `target/conformance/report.json`.
Both summary lines now use the checker-provided exact 110-part/61-implemented
text. Conformance generation and verification remain parent-owned; this correction
ran no tests and does not claim the traceability check has passed.
