# P36k embedded PNG image candidate

This record describes the evolving P36k source and its bounded contract. The
documentation pass inspected source and consulted the Rust/web owners; it ran no
product tests, builds or browser checks. Compilation corrections and authored
checks are still in progress. Rust/frontend execution, mutations, full isolated
validation, independent/Codex review and PR/main CI are pending. No pass count,
merged delivery or renderer/format equivalence is claimed. P36 stays
`in_progress`; earlier P36 slice results are not P36k evidence.

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
sRGB/gAMA/pHYs metadata are admitted. APNG, palette/grayscale/16-bit/interlaced
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

At documentation time the Rust owner reported first-pass source and five PNG
validation tests authored, with exhaustive-match/clippy corrections in progress;
that is not a compile or test pass. The web owner is implementing client/browser
coverage. The existing native/version fixtures were adjusted separately in
`a9c49f2`; that commit alone establishes no executed result.

All execution results and review dispositions remain pending until actual logs
and exact tested/reviewed commits are supplied. The parent owns final integrated
validation under the shared 6 GiB sandbox lock. This documentation adds no new
feature authorization: the user requested stopping after this image slice is
finished, so further feature work needs a new request.
