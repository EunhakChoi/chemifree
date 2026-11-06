# P35j chemical label weight

Base: origin/main f63f23a (#194). P35 remains in progress.

Document `label_weight` is normal/bold. Normal preserves existing 400/500 runs,
bold is 700 for atom/query/H/count/isotope/charge and the existing graph-preserving
Et label. Radical dots/marks, attachment marks and independent caption run weights
retain their existing policy. No graph or scientific model changes.

The existing 400/500 metric tables are not 700 evidence. After mounting and after
font/viewBox changes, the canvas measures actual SVG 700 text advances, horizontal
glyph overhang and vertical ink, then rebuilds its shared label layout. This also
measures unchanged 400 radical/attachment text within bold labels: their actual
small-font ink must fit, while their weight, size and placement policy stay unchanged.
Normal document labels retain the previous static metrics. Drawing,
bond trimming, fitting, picking and selection highlighting consume that layout.
Normal typography and normal Et geometry retain the prior path.

The existing settings edit, reset/import, preferences copied once into new docs,
HTTP/CLI/replay/history, save/copy and recovery paths carry the setting. Native
clipboard/template exact-style comparison includes weight. Earlier graph-v1 and
graphics-v2 payloads lacking it read as normal; explicit null/unknown fail. New
payloads include it. Document/draft v18 requires the field in current settings and
all history settings states; v1–17 forbid it and migrate to normal. Existing
versioned archival paths preserve exact original bytes. Future v19 is refused.
The existing 256-byte settings reservation covers the field; no caption admission
weights or physical 64 MiB provider boundary change.

Written regression coverage:

- Domain appearance-only change, full settings edit, no-op and undo/redo.
- Application preferred defaults copied once, replay, persistence, scientific
  inspections/formula and identical external export graph payloads with rendering
  loss; old normal template acceptance and bold mismatch; malformed payload weight.
- Native document/draft current/history strictness, v16/v17 admission/normal
  migration, exact document/draft archives and complete history after recovery.
  Existing maximum 109-caption regression now covers both v16 and v17, undo and redo.
- HTTP/CLI settings/preferences parity and future-v19 fixtures.
- Client runtime settings boundary, queued edit, preference edit and run weights.
- `npm run test:browser:label-weight` exercises the actual canvas, editor queue,
  style selector and backend: 3 generic families × 4/72pt, H/count, isotope/charge,
  long list query, Et, actual SVG ink/bounds, trim, fit, edge picking and highlight,
  independent caption weight, graph invariance, undo/redo and native reopen.
  A cold-page regression holds actual bundled-font HTTP replies while 700 labels
  render, switches to a distant bold document, pans/zooms before font release,
  and checks changed natural advances/bounds, current-document identity, preserved
  user viewport and post-load fit/ink provenance. It uses production font loading,
  canvas and document-open paths. The browser CI job includes it.

Applied skills: Rust patterns/testing, TypeScript patterns/testing and shadcn
existing settings controls. No original application DLL is an execution engine.

Implementation agent execution: formatting and diff inspection only. No tests,
builds, cargo check/clippy, browser runs, mutations or skill gates were executed,
as requested by the parent. All behavioral verification is pending isolated
parent-owned execution. Codex review and independent fresh-context subagent review,
and dispositions of their findings, are pending; record them before PR delivery.

External label-style roundtrip, arbitrary fonts, italic chemical labels, legacy
rendering equivalence and P35 completion are not claimed.
