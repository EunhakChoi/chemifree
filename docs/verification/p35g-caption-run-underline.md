# P35g caption run underline

P35 remains `in_progress`. Native v15 adds strict boolean underline in current and all history states; v8–14 migrate false without writing. The existing exact-byte document/draft archive protocol includes v14. Metadata increases from136 to154 bytes, within the160-byte reservation.

SVG decoration is an explicit filled rect per run, using actual SVG character start/end positions. The renderer unions its bounds with glyph ink before fit/selection/hit; it does not assume text getBBox includes native decoration. Decoration top is baseline+rendered size/12 and thickness rendered size/16. No scientific behavior or atom/Et label change. External caption roundtrip remains unsupported and existing whole-caption export losses apply.

Written regressions: application omission/invalid input rollback/underline-only undo/replay; provider current/history/document/draft strict bool matrix, v8–14 migration/archive, positive underline save/draft/undo/redo; HTTP=CLI positive Unicode runs and reopen; browser UI toggle and native save/reopen, actual rect ink inside hit/fit, identical advance/anchor on/off for combining/emoji/ligature/RTL/edge spaces/zero-advance, script/family/size/color combinations. Client rejects malformed old/new replies and accepts omitted=false.

Implementation agent read Rust/TypeScript design/testing and shadcn skills; official Base UI Toggle Group docs informed the existing controlled Off/On composition. Build, tests, cargo check, browser and skill gates were intentionally not executed here: parent performs isolated verification. No passing test or mutation result is claimed.

Codex review and independent fresh-context review: pending.
