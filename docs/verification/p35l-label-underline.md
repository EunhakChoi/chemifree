# P35l chemical label underline

Current evidence: this record preserves implementation-session observations. Its candidate/pending wording is historical; merged PRs, passing CI, current scope and remaining P35 work are reconciled in [the completion audit](p35-completion-audit.md). No checks were rerun for that documentation audit.

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


## Codex PR review findings and dispositions

Source: Codex review of [PR #197](https://github.com/EunhakChoi/chemifree/pull/197).
The complete review identity, reviewed head and timing are recorded in the PR.

- P1 — stale `labelStyleOf` expected object omitted `underline: false`.
  Addressed by `aaf8bd8` (`Complete chemical label style fixtures for underline`).
  Static inspection confirms the expected object now includes the false default;
  the directly constructed Et style fixture also names all current style fields.
- P2 — the primary native file-format section still described v19 and a stale
  migration/archive range. Addressed by the accompanying documentation change:
  document and draft headers now name v20, migration/first-save archival covers
  v1–19, and the current/history underline, weight/style introduction boundaries,
  and future-v21 refusal agree with `crates/providers/src/file_format.rs`.
  Historical descriptions of individual earlier format additions remain.

Verification of this review fix: source/document comparison and `git diff --check`
only. No tests or builds were run for this documentation-only change.


## Browser failure diagnosis: 4pt multi-run measurement keys

The isolated browser log reported zero underline rectangles for a five-run label
on its first sans-serif/normal/4pt case. Static arithmetic tracing establishes a
product measurement lookup failure: the isotope/charge absolute shift is
`-5 * (4/12) = -1.6666666666666665`, while the rendered relative SVG dy values
reconstruct its last run as `1.3333333333333333 - 3 = -1.6666666666666667`.
Exact JSON keys therefore differed despite identical intended layout. The whole
chunk measurement was missed, producing no rectangles; caption-count-only
completeness nevertheless reported ready.

The fix passes each exact source chunk identity through `data-label-metric-key`
to its SVG measurement. It includes font, anchor and each run's text, size,
absolute shift, weight and style. Measurement no longer reconstructs identity by
summing SVG dy values; there is no rounding or merging of distinct source shifts.
Actual measured and painted coordinates are retained. Every measured chemical
chunk must also have actual SVG evidence before bounds are reported ready;
existing document identity, loaded-font and viewBox provenance checks still apply.
The existing strict 4pt/72pt browser matrix and rectangle count assertions remain
unchanged. Diagnosis used the supplied failure log, source inspection and numeric
arithmetic tracing. No test, build or browser execution was run for this fix.
