# P35 completion evidence audit

P35 remains `in_progress` at main `f29195a` (#199). Caption and chemical-label
appearance slices have shipped; that does not prove the original row's whole
atom/abbreviation/caption label editing scope complete. This audit changes no
product behavior and ran no tests, builds, browser checks or cargo checks.

The candidate/pending statements in the P35a–n and P34i implementation records,
and their copied README/CONTRACT/HANDOFF notes, describe those implementation
sessions. The merged PR and CI evidence below supersedes that execution status;
it does not turn unsupported behavior into implemented behavior.

## Requirement and decision

The unchanged P35 scope is “다중 스타일 Unicode run, 위·아래 첨자, 정렬,
원자·약어·caption 레이블을 편집하고 저장한다.” Its acceptance is “한글·결합
문자·긴 텍스트·run 경계와 codec 인덱스를 검사한다. 텍스트 수정이 원자 의미를
바꾸는 경우를 명시한다.” See [the roadmap JSON](../roadmap/implementation-roadmap.json).

The approved public packet was consulted outside Git: its AGENTS.md,
README.ko.md, `spec/phase7-implementation-entry.ko.md`, readiness requirements v2,
`phase3-design-v3/semantics.ko.md`, `phase3-design-v3/document.schema.json`,
`phase7-analysis-closure-checklist.json`, `deep-rendering-contracts.md`,
`phase7-original-functional-supplement.md` and
`integration-long-caption-contracts.json`. The packet is not copied here.
The semantics require ordered Unicode runs and explicit codec index/encoding
mapping failures; external byte/UTF-16 positions must not become internal string
indices. The Unicode supplement separates stored strings from platform-dependent
serialization and visual appearance. The 66,000-character integration observation
is one authored carrier case, explicitly not a universal length guarantee.

The remaining scope discrepancy is general abbreviation label editing: the
product offers only fixed `Et` display over an existing terminal ethyl graph,
with no abbreviation text/run editor or general abbreviation kind. The public
`document.schema.json` atom union explicitly includes `nickname` and `fragment`
kinds carrying `label: string` and `expression: object | null`; a fixed Et display
map does not implement those label-bearing atom kinds. Atom editing
uses typed state fields, and chemical typography is document-wide rather than
arbitrary rich runs on an atom label. These are real implementation boundaries.
The inspected specifications do not settle the required additional abbreviation
vocabulary, per-label editing model or whether arbitrary text must parse into a
graph. Do not invent those semantics or narrow the original row to mark it done.
A subsequent approved scope decision and implementation must resolve that gap.

## Implemented acceptance evidence

| Requirement | Actual behavior and exact source/regressions |
| --- | --- |
| Unicode, Korean, combining and ZWJ | `crates/domain/src/scene.rs` stores original ordered run strings. `crates/domain/tests/caption.rs::caption_unicode_and_explicit_scripts_are_verbatim_and_one_undo_step` uses Korean, NFD combining text and woman-scientist ZWJ emoji with baseline/subscript/superscript runs. `scripts/web-caption-check.mjs` renders these against the real service and tests edit/Undo/save/reopen. No Unicode normalization or formula interpretation occurs. |
| Multi-style runs and boundaries | `CaptionRun` and caption validation in `scene.rs`, `crates/application/tests/caption.rs`, `crates/providers/tests/caption_format.rs`, and `scripts/web-caption-client-check.mjs` preserve ordered nonempty runs and independently compose script, weight/style, family, size, colour, underline, outline and shadow. `apps/web/src/components/caption-panel.tsx` edits explicit whole runs. Native current/history/draft fields and migrations are strict. This is not a text-cursor/grapheme editor. |
| Long text and limits | `caption.rs::byte_and_run_limits_are_not_character_or_utf16_limits` and client checks cover 1–32 runs and 16,384 UTF-8 bytes, including multibyte Korean boundaries and rejection beyond limits. `crates/domain/tests/caption_budget.rs` and provider caption migration fixtures cover admission/history and physical persistence bounds. These explicit refusals do not claim unlimited text or the external 66,000-character caption profile is admitted to the editor. |
| Alignment | `caption.rs::whole_caption_alignment_keeps_anchor_runs_graph_and_one_undo_step`, `apps/web/src/editor/scene-caption.tsx`, and the caption browser script cover whole-single-line left/center/right alignment around a fixed anchor with natural advance including edge spaces. Script/style changes preserve the anchor; alignment is physical rather than logical bidi start/end. |
| Atom semantics | `apps/web/src/editor/atom-edits.ts` builds `update_atom` commands for element/isotope/charge/H/radical/query/attachment state. These change recorded chemical state and can change graph identity/calculations; the visible label is derived from state by `apps/web/src/editor/label.ts`. Document settings font/size/carbon display and P35j–n weight/style/underline/outline/shadow change presentation only. They do not parse arbitrary atom label text or assign chemistry to captions. |
| Et semantics | `crates/domain/src/document/abbreviation.rs` accepts only its explicit neutral C2/single-boundary profile. `crates/domain/tests/abbreviation.rs`, application/provider tests and `scripts/web-abbreviation-check.mjs` check graph/IDs/state/coordinates, selection, automatic unfolding, native history and save. Collapse/expand changes display metadata only. `Et` is fixed text, not a general substituent parser or editable abbreviation vocabulary. |
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

Arbitrary atom text, per-atom rich-text runs, abbreviation kinds beyond fixed Et,
and external caption roundtrip remain absent. Caption transforms/arrange/graphics
clipboard refuse the whole unsupported request. Drawing exports report caption
loss and require consent; chemical label styles use `drawing_settings` loss and
Et uses `abbreviation_display` loss while retaining the complete chemical graph.
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

The final #199 run passed `check` at 2026-11-06 22:31:13 UTC and `browser` at
22:33:54 UTC. Its body records a fresh isolated full check, smoke and the five
chemical-label, caption and abbreviation matrices; the final contract-only fix
also received passing CI. Earlier failing runs and fixes are retained in their
PRs and implementation records; a final pass does not erase those findings.

## Documentation integration checks still required

The parent integration should run `npm run test:traceability` and
`npm run test:checks`, validate that the roadmap HTML embeds the same JSON, and
review the new `STYLE-01.4` evidence links. This audit intentionally performs no
execution verification. Product checks need rerunning only if integration changes
product code or exposes a concrete concern; this commit changes documentation.
