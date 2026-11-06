# P35a: independent Unicode caption candidate

Current evidence: this record preserves implementation-session observations. Its candidate/pending wording is historical; merged PRs, passing CI, current scope and remaining P35 work are reconciled in [the completion audit](p35-completion-audit.md). No checks were rerun for that documentation audit.

Status: implemented candidate, execution verification pending. P35 remains `in_progress`.
Branch: `feat/p35a-unicode-caption`, isolated at `chemifree-p35a`, started from
`origin/main` `80e3204`. This record does not claim a merged delivery or successful CI.

## Delivered boundary

- Independent Caption scene kind: anchor and ordered complete Unicode run values, with
  explicit baseline/subscript/superscript. No atom/abbreviation/chemical interpretation.
- Single line, existing document label font/size, fixed script geometry, 1–32 nonempty runs,
  16384 UTF-8 text bytes, Unicode White_Space and control policy documented in CONTRACT.
  Text, normalization form and run boundaries remain verbatim; no substring index protocol.
- Shared transaction create/update, strict DTO/snapshot validation, stable scene IDs,
  no-op/rollback/history/replay, selection, group/order/reference/delete and subtree-union
  translation. Caption-containing arrange/transform and graphic clipboard refuse whole.
- Native v8 document/draft/history encoding. V1–7 read without rewriting; first overwrite or
  recovery preserves old bytes. Old-version caption states, including history-only entries,
  are refused. Typed decoded parts are inspected without a second full JSON tree allocation.
- Real caption form, normal text inputs/IME gate and script controls, captured target and
  queued whole-run edits. Document/process changes invalidate the mounted form. Actual SVG
  text and font-completion measurements provide ink bounds, hit area and view-box inclusion.
- MOL/SDF, SMILES and bounded CDXML/CDX drawing export report consent-required caption losses.
  Existing rich-tree codec contracts remain separate.

Not delivered: general text styling, arbitrary alignment/wrapping/rotation/reflection,
caption clipboard, atom label overrides, abbreviation entities or collapse metadata,
external caption roundtrip, portable fonts or shaping across run boundaries. Marquee still
has its previous atom-center/two-point-graphic scope; caption selection uses Scene/click.

## Written regression paths

- `crates/domain/tests/caption.rs`: scalar/run preservation, scripts, no graph change, no-op,
  one Undo step, overlapping subtree translation, whole-union transform refusal, atomic
  invalid-content rollback, UTF-8 byte/run limits and FEFF/White_Space policy.
- `crates/application/tests/caption.rs`: real application operation boundary, exact request
  replay, save/reopen, real draft recovery through a replacement application instance,
  Undo/Redo, malformed script types, command-index refusals and five export consent paths.
- `crates/providers/tests/caption_format.rs`: pre-caption v7 file/history and draft fixtures
  built from the unchanged pre-caption schema, read without writes, original byte archives,
  first v8 write, history-only caption refusal and corrupt v8 script refusal.
- `scripts/web-caption-client-check.mjs`, imported by `test:client`: accepted/rejected runtime
  snapshots, Unicode and byte/run boundaries, script array/object refusal, stale targets,
  caption-only movement and refusal of groups containing captions in geometric actions.
- `scripts/contract-smoke.mjs`: HTTP=CLI caption creation, replay, strict malformed/content
  refusals, selection move, Undo/Redo and save/close/reopen. Existing catalog assertions now
  explicitly list 39 command and 11 capability rows (50 total).
- `npm run test:browser:caption`: real backend/store plus production CaptionPanel,
  StructureCanvas and useDocumentEditor; form run creation/editing, Unicode, script offsets,
  actual glyph/view-box bounds, canvas selection, Undo/Redo and native v8/page-reload
  preservation. Added to browser CI. This is written coverage, not an executed result.

## Review and finding dispositions

One independent fresh-context review was requested and completed on 2026-11-06 UTC against
the local implementation diff based on `80e3204`, while regression/document writing was
still underway. Reviewer: `/root/p35_decision/caption_review`, `fork_turns: none`. The reviewer
read the task, AGENTS and Rust/TypeScript patterns/testing skills. No implementation history
was inherited. No tests/build/browser were run. No further review round was requested.

1. P1: transaction catalog array still declared 48 after two caption rows. Fixed declaration
   to 50 and synchronized exact command lists/count assertions to 39+11.
2. P2: JavaScript `\s` disagreed with Rust `char::is_whitespace` for U+FEFF, allowing a native
   document the browser would reject. Fixed browser policy to Unicode `White_Space` and wrote
   explicit FEFF/White_Space regressions.
3. P2: `String(run.script)` admitted an array such as `["baseline"]`. Fixed the boundary to
   require a string and the closed vocabulary; wrote array/object/missing/unknown regressions.

Codex author static review: checked scene-kind consumers and found the graphics clipboard
client's previous `Exclude<instance>` type would now include captions. Changed it to the
explicit graphic/group union and made its source traversal reject captions. Reviewed the
native migration scan, caption loss consent, scope movement and stale form boundaries.
This local author review is not a claim that the required GitHub Codex PR review happened.
That PR review, its reviewed SHA/deadline and required CI remain pending the parent workflow.

## Actual execution and remaining verification

Ran `cargo fmt --all` and `git diff --check` only. Read source, docs, installed component source and official shadcn
component documentation; inspected diffs. No Rust or JavaScript tests, mutation probes,
lint/type gates, builds, browser sessions or push were executed, following the user's
explicit instruction. Do not report these written regressions as passing.

The verifier must run the targeted caption Rust/client/HTTP=CLI/browser paths, the normal
`npm run check`, strict route and no-skips browser smoke, plus existing clipboard/template
regressions that traverse scene kinds. Record first failures and fixes honestly. Native
text preservation and platform-specific glyph accuracy are separate judgments; neither
establishes external-application compatibility or broader scientific completeness.

## Main integration

After implementation commit `9c76c36`, integrated `origin/main` `8edcfeb` (P34h #183).
Kept the caption catalog size 50 and the newer attach-ring pattern description, retained
both handoff sections, and synchronized embedded roadmap data from the merged JSON.
The P34h pattern/gesture/recovery changes and caption scene changes are both preserved.
No tests/build/browser were run for the integrated tree; its execution verification is pending.

## First parent-run targeted failure and DTO conversion fix

The parent verifier ran the integrated `ed52406` snapshot in isolation. Its
`/tmp/chemifree-p29d-verify-j4KjPa/p35a-targeted.log` reports domain caption **5 passed**, then
application compilation failed with three E0277 errors: the `caption` helper returned
`ApplicationError`, but both command conversion callers require `Refusal`, while `point`
inside the helper returns `String`. Targeted exit was 101; application tests did not run.
This was an implementation error, not an environment or test failure.

Changed the helper to return `Result<Caption, String>`, matching its sole fallible operation
and the existing graphic/atom point-conversion helpers. The existing `From<String> for
Refusal` and indexed `to_commands` mapping now apply. No broad error conversion or string
mapping of typed caption-content refusals was added: Domain validation retains
`caption_invalid` and the command index. Inspected both caption callers, the adjacent
graphic point conversions, and other `ApplicationError` helpers in `contract.rs`; the other
helper is the intentional top-level transaction-selection boundary.

Added an application regression comparing add/update caption coordinate errors with the
existing graphic-command error, including complete diagnostics, index and atomic snapshot
preservation; a content-invalid case separately pins `caption_invalid`. This follow-up has
not run tests/build/browser or push. Only formatting and diff inspection were performed.

## Second parent-run targeted result and fixture corrections

The parent verifier's `p35a-targeted2.log` for `c28da14` reports Domain 5/5 and application
2/4. Replay/save/reopen/crash-draft and coordinate error parity passed. The other two tests
failed because the new test used the wrong existing contracts: MOL/SDF export requires
`records: [{document_id}]`, SMILES additionally requires `style`, and clipboard domain
refusals use `invalid_command` plus `details.reason`, not a new top-level code.

Compared the actual CT/SMILES/interchange parameter DTOs, existing export fixtures and
consent code. Corrected and expanded the table to MOL/SDF V2000 and V3000, recorded/canonical
SMILES, CDXML drawing and CDX drawing (eight cases). Each case now pins no content before
consent, the exact caption loss/source path, rejected wrong digest, accepted content and
same loss after valid digest, plus unchanged document/session. Corrected clipboard checks
pin the existing envelope, reason, current revision and null result. No product mapping was
changed: source inspection confirms all these paths already use the caption-aware loss
collectors and common consent flow. Corrected fixtures have not been run by this agent.

## Codex PR #184 P2: first fit before caption bounds

The parent relayed [Codex P2](https://github.com/EunhakChoi/chemifree/pull/184#discussion_r4490810286):
reopening could fit the default frame before the caption's requestAnimationFrame measurement,
then retain that incomplete viewport after the actual ink bounds arrived. This was a product
bug. Added an explicit bounds-readiness input to the viewport hook. Its null first viewport
now waits for positive dimensions plus a measurement for the current caption snapshot after
font loading settles. A document switch resets this pending state and stale measurement
callbacks are discarded. Caption anchors are also included in provisional content bounds.

Explicit Fit/pan/zoom creates a viewport from currently available bounds even while waiting,
so late measurements cannot overwrite user navigation. Once fitted, later edits, resize and
font completion preserve center/scale. No repeated auto-fit effect was added.

The real-backend browser regression now edits a long caption to a remote anchor, saves and
reopens it, and checks actual SVG viewBox plus CSS pixel visibility, not only the stored
content-bound calculation. It switches between two remote-caption documents. Two cold-page
cases hold actual woff2 font replies at HTTP: one switches documents while the first fit is
pending and verifies the latest caption fits on completion; the other pans/zooms while fonts
are pending and pins the chosen viewBox after completion. No React/layout/viewport code is
mocked. These review fixes do not trigger another review round. This agent has not executed
tests, builds, browsers or push for them; only static source/diff inspection was performed.

## Third parent-run targeted result and already-drafted recovery semantics

For `20aa4c0`, the parent verifier's `p35a-targeted3.log` reports Domain 5/5, application
4/4, provider caption format 2/3. The remaining provider failure expected a v8 rewrite after
recovering a v7 draft and immediately autosaving it without any new edit; the file correctly
remained v7. Source inspection shows `recovered_session` records the recovered revision as
already drafted, and `DraftTask::for_session` returns no write for an already-drafted dirty
session. `prepare_draft_for_recovery` archives the earlier exact bytes before adopting it;
it does not eagerly replace the original file. Existing migration tests explicitly write a
new draft when expecting a newer format. This is intended persistence behavior, not a
missing caption migration.

Renamed and corrected the test to verify both boundaries: unchanged autosave retains the
original v7 bytes and current drafted revision, then an explicit caption edit advances the
revision and the next autosave writes v8 while preserving the archive. After releasing the
application, the real repository claims that v8 draft; assertions check the exact caption
and Undo/Redo across both the new caption and the original v7 title history. CONTRACT now
states the already-drafted no-rewrite behavior explicitly. No product code changed. This
corrected regression has not been executed by this agent; only formatting/diff inspection
was performed, with tests/build/browser/push still prohibited.

## Fourth parent-run targeted result and caption-box reference stability

For `4c2cd72`, the parent verifier's `p35a-targeted4.log` reports Domain 5/5, application
4/4 and provider caption format 3/3. Lint then failed on one exhaustive-deps warning: the
unmeasured/stale-caption branch allocated a new empty Map on every canvas render, changing
the content-bounds memo dependency even for unrelated hover/gesture updates.

Memoized the selected caption-box map by measurement state and document snapshot, exposing
it as ReadonlyMap. Pending/stale measurements now retain the same empty-map reference until
one of those inputs changes; current measured bounds retain their own map reference.
This fixes the allocation/dependency issue without weakening lint or suppressing the rule.
No extra implementation-mirroring test was added. This agent inspected the diff only;
lint/tests/build/browser/push of the corrected snapshot remain unexecuted here.

## Fifth parent-run targeted result and empty-group movement regression

For `38cd85e`, the parent verifier's `p35a-targeted5.log` reports lint/type checks passed
and client tests 596/599. Two existing exact geometry-result fixtures omitted the added
`hasCaptions: false` field; updated those expected objects while retaining all previous
bounds/scope/empty-state assertions. The third failure was a product regression: allowing a
move whenever the scope contained any scene object made an empty group generate a move.

Changed the additional movement criterion to an actual caption leaf in the resolved object
union. Positions and two-point graphics retain their previous criteria, empty groups still
lapse, and caption-only objects/groups retain their movable anchor. Added regressions with
valid scene snapshots for an empty group referencing an outside caption (no movement and no
reference traversal) and a group containing only a caption (one move command). Existing
empty-group and caption-only movement assertions remain intact. Corrected tests have not
been executed by this agent; only diff inspection was performed, with execution prohibitions
unchanged.

## Sixth parent-run targeted result and CLI caption rendering

For `c713289`, the parent verifier's `p35a-targeted6.log` reports lint/type/client passed,
then the CLI workspace build failed with E0004: the human-readable scene match did not
include `SceneObjectView::Caption`. This was a missed entry-adapter consumer, not a fixture
or environment error.

Added an explicit CLI scene branch showing caption ID, parent, point anchor and every
ordered script/text run. Text uses ASCII scalar/quote/backslash escaping to avoid emitting
user-supplied directional/format characters to the terminal; JSON retains original text.
The script match is also exhaustive. Added tests through the existing real-application
document-text fixture for grouped captions with all scripts, exact escaped Unicode/bidi
and quoting output, and unchanged Unicode in the document's JSON view.

Statically searched CLI/API/conformance source for `SceneObjectView`, domain `SceneObject`
and `ObjectKind` matches; the CLI scene printer was the only scene-kind match in those
entry adapters. The previously audited domain/application/provider matches remain explicit
or use their intentional unsupported guards. This agent has not executed tests, build,
browser or push for the fix; only formatting and source/diff inspection were performed.

## Seventh parent-run targeted result and initial caption measurement fixture

For `5ffbba6`, the parent verifier's `p35a-targeted7.log` reports lint/type/client/web and
workspace build passed. The real caption browser then timed out in its initial combined
ink/hit/view-box assertion. Temp artifacts were removed by the isolation wrapper, so the
failed predicate's individual values were unavailable.

The fixture created the caption with its baseline at (0,0) in an already-fitted empty
canvas, then required the whole caption to be inside that unchanged viewport. Ascenders and
superscript extend above y=0, while the existing empty frame starts at y=0. The documented
edit rule preserves an existing/chosen viewport; only opening a new document auto-fits once.
The fixture therefore assumed an edit-triggered auto-fit the product deliberately does not
provide. It now first pins a user-chosen empty viewport, waits for actual caption/font bounds,
asserts creation preserved that viewport, presses the real Fit drawing control, and retains
all exact ink/hit/view-box and script-offset checks. Remote-caption reopening and font-race
checks still require automatic first fit without pressing Fit.

Added failure diagnostics printed to stderr before browser teardown, including font status,
editor snapshot/error, actual SVG viewBox/readiness/pixel bounds, caption ink/hit/pixel boxes
and each script run's position/size/ink. Cold-font pages also dump their state before closing.
This makes future failures reviewable even if the sandbox removes temp files. No product
behavior, timeouts or assertions were weakened. This agent has not run the browser or other
execution checks for the corrected fixture; only static source/diff inspection was performed.

## Browser rerun: realized metrics outgrew the initial fitted bounds

The parent's `p35a-browser2.log` for `1e62ac7` reaches save/reopen but fails strict visibility.
Its diagnostic proves a product defect: actual caption ink ends at x=2010.982 and the current
hit box at x=2012.982, while the retained viewport ends at x=2002.425. Fonts and the readiness
flag are loaded/true. Thus current hit geometry already reflects newer metrics, but the
viewport was fixed from an earlier measurement. Font readiness alone did not establish that
the bounds were measured in the viewport being finalized. Browser realized glyph metrics or
layout settling across the fit can change the measured extent; the exact lower-level cause
is not established by this log.

Measurements now record their actual SVG viewBox. A viewport-attribute observer schedules a
new real SVG measurement after fitting changes scale. The opening fit remains pending until
bounds measured in that resulting viewport fit inside it. Only this opening phase can widen
the accumulated observed bounds and correct the fit; observed bounds never shrink during
correction. A current-view measurement confirming containment ends that authority. Explicit
Fit/pan/zoom, a changed edit snapshot or a document switch prevents obsolete corrections;
a scale-clamped unchanged fit also terminates rather than render-looping.

Kept the strict actual SVG/pixel visibility checks and cold-font/document/user-navigation
races. Visibility additionally requires measurement/viewBox provenance agreement. Added a
deterministic fixture around the actual viewport hook: supply bounds measured before the
first fit, then larger bounds measured in the resulting viewport, confirm that viewport,
and verify later changes cannot refit it. A second sequence pans during the pending phase
and verifies post-fit measurements cannot overwrite the chosen viewport. These are explicit
measurement-boundary inputs, not mocked hook/layout code. Failure diagnostics now include
the measured viewBox. No tests/build/browser/push were executed by this agent; the corrected
snapshot requires the parent's isolated rerun.

## Codex PR #184 P1: caption payload escaped the entity-count history budget

The parent relayed [Codex P1](https://github.com/EunhakChoi/chemifree/pull/184#discussion_r4491092284).
A caption state can contain 16 KiB of text, but counted as one history entity. Repeated bulk
updates could therefore be accepted and later exceed the provider's 64 MiB document/draft
write/read cap. This was a product persistence defect. No further review was requested.

### Policy and upper bound

Caption-bearing content or history now activates a conservative reservation: current state
48 MiB plus combined undo/redo 8 MiB, at most 56 MiB. The existing count limits remain. The
budget counts every kind of history payload, not only caption strings; otherwise a maximal
legacy graph history plus large captions could still overflow. Oldest undo entries trim
first, then farthest redo on restore. A single latest transaction exceeding 8 MiB refuses
whole with `caption_persistence_limit`, preserving IDs/content/selection/history and keeping
accepted latest edits undoable. A small group move can duplicate many complete caption
states, so adapter request-byte limits and the 500-command cap do not replace this guard.

`document/persistence.rs` owns serializer-independent conservative reservations. The native
v8 writer is checked against them in provider tests. Caption/title control characters are
already forbidden; UTF-8 passes through native serde JSON, and quote/backslash escaping costs
at most twice the UTF-8 bytes. Each caption reserves 256 bytes plus 96 per run plus twice
text bytes, so a maximal 32-run/16384-byte caption reserves 36096 bytes. Run script names,
keys, delimiters and state/ID wrappers fit that metadata reservation. IDs need at most 11
ASCII bytes; list entries reserve 16, floats reserve room for 32-byte representations, and
entity/change reservations include key/identifier overhead. Both selection snapshots and
both sides of every entity change are included. The 4096-byte current-state base covers
native file/draft format wrappers, list keys, document ID, counters, revision, base revision
and a maximal u64 timestamp; actual saved and draft encodings must both fit the same weight.

A deliberately loose simultaneous current-state maximum is 48370560 bytes, below 48 MiB:
4096 envelope +1728 title +256 settings +2000×512 atoms +4000×256 bonds +40000×192 positions
+1000×36096 scene objects +4096×128 references +6000×256 stereo records +352000 stereo-group
reservation +112160 full selection +16032 roots. All 1000 objects cannot actually be captions
when placements exist, so this bound overstates valid content. Group children/graphics fit
below the maximum object reservation; stereo groups partition at most 2000 centers. The
separate 8 MiB history reservation is independent of the current Undo/Redo boundary, so
moving entries across the boundary cannot violate the native cap. A current-state check
also refuses any future shape that exceeds its reservation.

### Written regression and compatibility coverage

- Domain: 116 maximal 32-run caption updates fit a latest history entry; 117 refuse whole;
  500 maximal creations cannot bypass the byte budget. Older history makes room while the
  accepted latest edit stays undoable. Undo/Redo and restore keep the same budgeted history.
- Domain: restoring looser undo/redo histories keeps nearest steps in the documented order;
  equal UTF-8 byte counts receive equal conservative text reservations despite different
  code-point counts, and the existing per-caption byte refusal still applies first.
- Application: one short move of a group with 117 maximal captions reports the new typed
  reason without a command index and rolls back an earlier rename plus the entire move.
- Provider: twelve rounds of 100 maximal quote/backslash captions save and reopen identically,
  then Undo/save/reopen/Redo/save and draft write/claim preserve the nearest boundary. Without
  trimming, these escaped history strings alone would exceed 64 MiB.
- Provider: maximum graph/placements plus 980 maximal escaped captions, stored centers/groups,
  references, title and selections, with nearly full caption history, encode/read back below
  the reservation and native cap in both document and maximal-metadata draft forms. Existing
  widest-state provider checks also assert actual encoded bytes do not exceed the reservation.

No storage schema/version change is needed: looser v8 histories trim in memory as existing
history-limit handling already permits. Reading does not rewrite files. Caption-free v1–7
migration paths and their previous count-based retention remain unchanged; their existing
fixtures and preservation checks remain in the suite. The corrected P1 snapshot has not run
any tests/build/browser/push in this agent; only formatting and source/diff review were done.
The parent reports the preceding cbb0d05 targeted run passed; these new budget checks still
require isolated execution.

## Parent full check: unnecessary cloned one-element test slice

The parent's `p35a-final-full.log` for `67bd893` stopped at clippy with
`cloned_ref_to_slice_refs` in `crates/domain/tests/caption.rs:157`: the refusal check cloned
the delete command solely to construct a borrowed one-element slice. Replaced it with
`std::slice::from_ref(&delete)`. The same command remains available for the subsequent
successful transaction and every behavioral assertion is unchanged. No lint suppression
was added. This agent did not rerun clippy/tests/build/browser or push; the parent owns
execution verification.

## PR #184 CI: stale future-version and migration expectations

[CI run 42201768786](https://github.com/EunhakChoi/chemifree/actions/runs/42201768786)
failed in `a_newer_format_version_is_refused_and_left_untouched`: the test changed a
v4 fixture header to v8, which is now the supported current format. The provider therefore
correctly attempted current-body decoding and reported `unknown field atoms`, rather than
the future-version refusal that the test expected. This is a stale test input, not evidence
that the future-version guard accepts an unsupported version.

Source inspection confirms `file_format::version` checks the header before document or draft
body decoding. Current document/draft versions remain 8; only versions 1–8 are readable.
`decode_revision` also invokes that guard before probing a revision, protecting save's
read-before-overwrite path. The corrected regression uses unsupported v9 and expects the
supported list 1–8. Existing load/revision/save-with-and-without-base refusal assertions,
exact-byte preservation and absence of a migration copy remain intact. v9 is deliberately
only the future fixture value; no new schema or migration is introduced.

A source-wide version-expectation search also found the same stale v8 future input and
1–7 refusal message in `scripts/contract-smoke.mjs`, plus three migration save assertions
still expecting v7. These now expect future v9/current v8 respectively, retaining the
HTTP/CLI, listing, unchanged-byte and original-migration-copy checks. The recovered-draft
comment now agrees with its existing v8 assertion. Other explicit v7 inputs in caption
provider tests deliberately exercise older-format refusal/migration and stay v7; current
v8 roundtrip assertions and the store's unsupported v99 corruption input remain unchanged.

Applied the Rust patterns/testing skills to the existing boundary regressions. No additional
review was requested: the PR's completed reviews remain the applicable review record.
Per the user's cleanup instruction, this agent ran no tests, builds, browser checks, clippy,
mutation checks or push. Validation is limited to source inspection and `git diff --check`;
the corrected provider regression and HTTP/CLI contract suite still require execution after
cleanup, followed by the required CI jobs. No passing result is claimed for this snapshot.
