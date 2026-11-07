# P36k embedded PNG image (merged in PR #216)

This record describes the evolving P36k source and its bounded contract. The
documentation pass inspected source, consulted the Rust/web owners and directly
read the seven execution logs listed below; it ran no product tests, builds or
browser checks. Focused execution passed, but independent review of `eb40cfa`
found a P2 history-replay pixel-limit bypass. The shared-state fix and regression
now address that finding, with follow-up clippy and 52 focused tests passing.
Codex review of `5ea2107` in PR #216 completed without findings. PR #216 was
later merged as `7bec5ea`; its merge-commit main CI failed in a spline cancellation
browser test and was followed by PR #217, and a later Codex
review of `f69f8de` left an unresolved P2 inline finding. See the
[closing record](#merge-and-main-ci-closing-record) below, which supersedes the
"pending" statements in the sections that follow. No latest-head renderer/format
equivalence is claimed. P36 stays `in_progress`; earlier slice results are not
P36k evidence.

## Implemented source boundary

- Domain: `crates/domain/src/image.rs` owns immutable verified PNG assets,
  decoder/cache interfaces and positive rectangle geometry. `scene.rs`,
  `document.rs` and `document/persistence.rs` integrate image scene identity,
  reference/selection/history behavior and pixel/byte limits.
- Application: `add_image` and `update_image_geometry` use the common transaction
  boundary. Snapshots expose source MIME/base64, origin/size and derived pixels.
  CLI/API configure the provider decoder; no original application DLL executes
  this behavior. External exports report omitted scene rendering.
- Provider: `crates/providers/src/png_image.rs` uses canonical base64, explicit
  chunk/CRC/profile validation, exact zlib checks and the PNG decoder's full-frame
  read. Native30 serializes image source/geometry in current and history states;
  all old headers reject images before retention.
- Web: the embedded-image panel reads a bounded local file and submits only on
  explicit Add. Numeric geometry edits retain source bytes. `SceneImage` renders
  a stretched axis-aligned PNG and a complete transparent rectangle hit target.
  Existing Scene movement, grouping, ordering and alignment use rectangle bounds.
  Captured document/revision/process and existing uncertainty guards remain.

These statements describe current authored code, not successful execution. The
source may still change while compile/test feedback is addressed.

## Bounded PNG and geometry policy

The only accepted format is static noninterlaced 8-bit RGB/RGBA PNG. Source is
at most 128 KiB, each edge at most 2048 pixels and each image at most 1,048,576
pixels. Current image occurrences total at most 4,194,304 pixels; distinct image
assets retained in history are limited to 67,108,864 pixels. Decoder memory is
bounded to 8 MiB and one output frame to 4 MiB; raster pixels are temporary.
The encoded bytes remain immutable and can be shared by geometry/history states.

Canonical padded base64, every chunk CRC, complete compressed row data and PNG
decode, and IEND/EOF must validate. Only IHDR/IDAT/IEND and valid pre-IDAT
sRGB/gAMA/pHYs metadata are admitted. Coexisting sRGB/gAMA requires gAMA 45455
regardless of chunk order. APNG, palette/grayscale/16-bit/interlaced
profiles, ICC/text or unknown chunks, truncation and trailing bytes are refused.
No metadata-to-physical-size conversion or color-equivalence guarantee is made.

Origin and the opposite corner are within inclusive ±100000 pt; size is finite
and positive, at most 200000 pt per axis. Aspect ratio is free. Bounds, Fit,
marquee, arrangement and pointer hit use the entire stored rectangle, including
transparent pixels. Selection underlay is editor UI, not source raster content.

Shared 48 MiB current / 8 MiB history byte admission, normal undo retention and
64 MiB native physical limits still apply. Old current/history profiles retain
their prior shape cardinalities, label/style requirements and byte-preservation
archive protocol. Native30 admits images; v1–29 forbid them, future31 refuses.

## Refusals and non-goals

Image-containing rotation/reflection refuses atomically as `image_unsupported`,
even at angle zero. Scene Copy/Cut preparation refuses an image subtree as
`image_clipboard_unsupported`, without silently dropping it; no image clipboard
profile is introduced. Existing graph-selection clipboard refusals remain.
Size/source/pixel/byte errors retain typed reasons and transaction atomicity.
Groups, references, movement, order, geometry edits and ordinary persistence keep
the embedded bytes and chemical graph independent.

No URL fetch, JPEG/GIF/SVG/APNG import, raster replacement, cropping, interactive
resize/rotation handles, image clipboard transport, OCR, chemistry recognition or
general scientific/visual equivalence is supplied. Supported external structure
exports report omitted scene rendering under existing loss rules.

## Verification status and handoff

The following logs were read directly under `/tmp/chemifree-work/`. Counts refer
to the commands actually logged, not complete workspace coverage. The logs do
not embed a source SHA for every command; the web owner identifies the final
image browser's backend as `eb40cfa`, with web source/test commits `691a03f` and
`69f35eb`. Those original five logs precede the replay-limit correction; the two
follow-up logs below establish its reproduction and focused verification.

| Log | Confirmed result and scope |
| --- | --- |
| `p36k-rust-final.log` | Workspace/all-targets clippy with warnings denied passed. There were 73 distinct focused tests: image API1/application4/CLI1/domain6; provider image_format5/png_image6; operations11; domain caption_budget5/polygon6/scene23/spline5. Six intentional mutations were detected: full raster validation, exact zlib, current pixels, history pixels, native29 image admission and immutable discarded history. The restored image/provider group of 23 tests passed again. Final sandbox exit0. These are focused tests, not the full workspace test suite or proof against the later review finding. |
| `p36k-web-gate.log` | Lint with zero-warning policy, typecheck, the complete registered client command (including image7/7) and production web build passed; sandbox exit0. The build retained its large-chunk warning. |
| `p36k-web-browser-3.log` | Image, scene-order, native clipboard integration, graphic-point editing, graphic creation and general smoke browsers passed; sandbox exit0. General smoke recorded no skipped steps and no uncaught browser errors. This run preceded the final color-metadata browser extension. |
| `p36k-image-final.log` | Three intentional client mutations were detected: reading an oversized file, accepting an oversized IHDR and doubling the current pixel budget. Restored image client7/7 and the image browser passed; sandbox exit0. The owner identifies this browser as covering both sRGB/gAMA mismatch orders and ordinary graphic Copy when an image is present but unselected. |
| `p36k-ts-skill-gate.log` | Helper lint and types passed. Format failed because npm returned `ENOTCACHED`; helper exit1. No successful formatter result is claimed. |
| `p36k-image-history-repro.log` | Before the fix, `native_history_cannot_replay_more_pixels_than_the_current_image_budget` failed because a native file with 4 Mi current pixels admitted history that could replay to 5 Mi pixels. Result: zero passed, one failed, five filtered, sandbox exit101. This is the intentional pre-fix reproduction, not an unresolved failure in the fixed run. |
| `p36k-image-history-fix.log` | Follow-up clippy completed and all 52 focused tests passed: provider image_format6/png_image6; image API1/application4/CLI1/domain6; domain caption_budget5/scene23. The replay-limit regression passed, with final sandbox exit0. This is focused verification of the correction, not a full integrated or CI run. |

Independent review of `eb40cfa` found P2: image pixel limits could be bypassed
through history replay. The original fix `b6cd0e4` is now rebased as
`2c140fd116e99e386e12050e190296e74c407a4b` ("Validate image pixel budget in every
replayed history state"). It adds the current image-pixel check to the common
`Content::validate`, covering restored/replayed states instead of checking only
the initially opened scene. The native regression builds a valid 4 Mi current
scene with a 5 Mi undo-only or redo-only state and requires both to be refused
without changing the exact file bytes. The reproducer failed before the fix;
the corrected test and the 52-test follow-up passed. Finding disposition:
**addressed by fix and regression evidence**. Earlier pixel-budget tests and
mutations had missed this path; their passes alone are not the disposition.
The independent review did find an issue; no new independent approval is claimed.

The parent recorded the completed Codex review in
[PR #216](https://github.com/EunhakChoi/chemifree/pull/216): reviewed commit
`5ea2107b023ea1b99858b0b17ab53d8efac97a77`, no findings. This result is separate from
the earlier independent review and its addressed P2.

The parent owned final integrated validation under the shared 6 GiB sandbox lock;
its outcome and the CI results are in the closing record below. The existing native
fixture commit `a9c49f2` alone supplies no execution evidence. This documentation
adds no new feature authorization: the user requested stopping after this image
slice is finished, so further feature work needs a new request.

## Merge and main CI closing record

Facts below were read from GitHub (`gh pr view`, `gh run list`, `gh run view`,
review and review-thread APIs) on 2026-11-07 UTC; this documentation pass ran no
product tests, builds or browsers.

- **Merge:** [PR #216](https://github.com/EunhakChoi/chemifree/pull/216) "Add
  bounded embedded PNG scene images" merged at 07:11 UTC as `7bec5ea`. The PR
  head was `f69f8de` ("Correct future native version contract assertions"), whose
  change since `5ea2107` is limited to docs and `scripts/contract-smoke.mjs`.
- **PR CI:** run 42349141149 on `f69f8de` succeeded (check and browser). The two
  earlier runs for `5ea2107` and `2ce2681` were cancelled by newer pushes. The PR
  body also records that the local full isolated check ran in two parts: the first
  passed lint, types, client, build and Rust but exposed two stale future-version
  contract assertions, which were corrected before the remaining checks passed.
- **Main CI failure:** run 42350215647 on `7bec5ea` failed. The `check` job
  passed; the `browser` job failed with an `AssertionError` in the spline browser
  at "shape/tool/revision/service/document/escape changes discard drafts without
  creation". The log does not say which cancellation case failed.
- **Fix PR:** [PR #217](https://github.com/EunhakChoi/chemifree/pull/217) "Wait
  for spline draft cancellation to render" merged at 07:54 UTC as `4198438`. Its
  body states there is no product code change; the test now waits for the actual
  rendered context and preview removal, then asserts no document change or
  transaction, and names each cancellation case. It records that the precise
  trigger of the main failure is unconfirmed and that a service-generation setter
  can return before React commits the new generation. Main CI run 42353217355 on
  `4198438` succeeded (check and browser).
- **Caption shadow pixel failure (cause not determined):** the first attempt of
  PR #217's CI run 42351443496 passed the spline check but its browser job failed
  at the step "Unicode caption form, canvas and native roundtrip"; the PR body
  records the assertion as `scripts/web-caption-check.mjs:513`, the existing
  caption shadow pixel check. Re-running the job (attempt 2) passed, as did the
  final main run above. **The cause of the first failure is not determined.** A
  passing rerun is not an explanation, and this record offers none.
- **Reviews of #216:** (1) the independent fresh-context review of `eb40cfa`
  found one P2, addressed by `2c140fd` and its regression as recorded above;
  (2) Codex review of `5ea2107` completed without findings; (3) a later Codex
  review of `f69f8de` (07:00 UTC) completed with one P2 inline finding on
  `apps/web/src/editor/structure-canvas.tsx`: with label shadow enabled, an image
  ordered before a molecule instance is still emitted after the global
  chemical-shadow group, so an opaque overlapping image covers that instance's
  shadow while its foreground paints above the image, and First/Last reorders the
  instance only partially. The review thread was unresolved when #216 merged and
  the PR has no reply or recorded disposition; no code in the PR after the review
  touches that file. Main's `structure-canvas.tsx` still draws the shadow group
  before scene objects (source inspection only, not a render check). **This P2 is
  therefore recorded as an unaddressed known limitation, not as fixed or
  accepted.**
- **#217 reviews:** the PR body records an independent fresh-context review of
  `3530168` with no issues and a Codex review of the same commit with no findings.
  Those reviews concern the test-only fix, not image behavior.
- **Status:** P36 stays `in_progress` and the roadmap graphics row is not marked
  done. P36 was stopped after this PNG image at the user's instruction: no further
  P36 slice starts without an explicit request.
