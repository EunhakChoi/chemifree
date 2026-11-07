# P36k embedded PNG image candidate

This record describes the evolving P36k source and its bounded contract. The
documentation pass inspected source, consulted the Rust/web owners and directly
read the seven execution logs listed below; it ran no product tests, builds or
browser checks. Focused execution passed, but independent review of `eb40cfa`
found a P2 history-replay pixel-limit bypass. The shared-state fix and regression
now address that finding, with follow-up clippy and 52 focused tests passing.
Full integrated validation, Codex review and PR/main CI remain
pending; no merged delivery, latest-head full pass or renderer/format equivalence
is claimed. P36 stays `in_progress`; earlier slice results are not P36k evidence.

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

The parent owns final integrated validation under the shared 6 GiB sandbox lock.
Codex review, full integrated checks and CI remain pending. The existing native
fixture commit `a9c49f2` alone supplies no execution evidence. This documentation
adds no new feature authorization: the user requested stopping after this image
slice is finished, so further feature work needs a new request.
