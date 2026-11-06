# P35f: caption run color

P35 remains in progress. Independent caption runs gain nullable `color`: null or request
omission dynamically inherits theme foreground; explicit opaque sRGB `#RRGGBB` with
uppercase ASCII hex digits stays fixed through theme changes. Black is an explicit override,
distinct from inheritance. Alpha, lowercase/noncanonical spellings, CSS names/expressions,
arbitrary palette registration, atom/Et labels and external caption roundtrip are excluded.

Domain validation protects complete edits and restored/history states. Application DTOs,
HTTP/CLI, whole-run UI, SVG fill, replay, undo/redo, native save and crash drafts share this
value. Malformed strings give caption_invalid; wrong JSON types give invalid_request.
Complete-run omission resets inheritance. Text, script/font/size/alignment and existing
loss reporting remain unchanged. Older replies may omit color; older clients may ignore
or reset it, so client/service upgrades should be coordinated.

Native v14 requires explicit nullable color in current and all undo/redo caption states.
V8–13 migrate to null; all older headers refuse color even when null or history-only.
Reads do not rewrite. First overwrite and pre-recovery archives preserve exact old bytes,
including v13. Future fixtures now use v15 and diagnostics list supported versions 1–14.

Recounted actual maximum metadata is 136 bytes: 89 for text/script/weight/style/font
(longest: sans-serif, superscript, normal, italic), 29 for size_pt (18-character bounded
float plus key/comma), and 18 for `,"color":"#RRGGBB"`, exceeding 128. Reservation is now 160 per run;
64 MiB files and 48/8 MiB content/history caps remain. A maximal 32-run before/after update
reserves 76,352 bytes. With 576 entry bytes, 109 reserve 8,322,944 (accepted) and 110
reserve 8,399,296 (refused). With maximal 7,000-entity before/after selections, entry
overhead is 224,576: 106 updates total 8,317,888 (accepted), 107 total 8,394,240 (refused).
The maximal provider fixture uses 106. Repeated 100-caption updates reserve 7,635,776
(one fits, two do not); other refusal and text-byte checks retain their meaning.

Written coverage includes Domain validation/rollback/undo; Application color-only replay,
request-ID reuse, omission reset and rollback; HTTP=CLI mixed inherited/black/color runs
and malformed values; native v8–14 current/history document/draft field matrices, v8–13
exact-byte migration archives and colored save/reopen/draft undo/redo. Web checks cover
boundary replies, malformed/CSS input, inherited versus fixed SVG fills across theme
changes, unchanged ink geometry, reset/undo, existing measured alignment and save/reload.
CLI text renders inheritance or the canonical override per run.

Rust/TypeScript patterns/testing and shadcn skills guided the change. Existing Input/Field
components are reused. No dependencies were added. Per parent instruction the implementation
agent runs formatting and diff inspection only: no tests, builds, cargo check or browsers.
Parent integration owns isolated resource-bounded execution, current-head CI and both reviews.
No passing execution outcome is claimed here.
