# P35l chemical label underline

Status: implementation and checks authored; execution intentionally deferred.
P35 remains in_progress. No scientific compatibility claim.

- Domain/document setting defaults false; full edits, preferences, reset, history
  and persistence carry the same boolean. HTTP and CLI share application DTOs.
- SVG measurements use actual character starts/ends for every run and union
  explicit underline ink with glyph ink. Radical/attachment/caption styles remain
  independent. Document and viewport/font provenance follow existing measurement.
- Native v20 checks document and both history directions. Earlier formats default
  false and preserve exact archive bytes, including drafts; v21 stays unsupported.
- Clipboard/template omission defaults false; explicit null/nonboolean is invalid.
  Complete style comparisons include underline. External drawing_settings losses
  and existing history/file admission budgets remain.

Authored checks:

- `crates/providers/tests/label_underline_format.rs`: strict current/history,
  old-field refusal, v16–19 migration, exact archives, draft recovery, future v21.
- `crates/application/tests/settings.rs`: preferences copy once, reset, replay,
  undo/redo and persisted settings.
- `scripts/web-label-underline-client-check.mjs`: validators, queued settings,
  preferences and old graphics payload style comparisons.
- `npm run test:browser:label-underline`: production canvas 3 font families ×
  4/72pt × normal/bold × normal/italic; actual per-run rect geometry, shared
  bounds/fit/trim/hit/selection, unchanged graph/Et/captions, Unicode/space/RTL/
  ligature probes, production UI/native history, delayed-font document switch
  and viewBox provenance. Registered in CI.
- Existing complete settings fixtures/native assertions updated to v20, old
  migration helpers strip underline, max v16–19 caption histories retain the
  existing admission/64MiB boundary.

Executed in this session: formatting and diff review only. Tests, builds,
clippy/typecheck and browser runs were not executed by explicit task instruction.
Reviews and finding dispositions are recorded by the PR delivery workflow.
