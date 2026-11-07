# P35o retained-graph plain display label record

Current evidence: this is an implementation-session record. Its pending execution/review wording is historical: merged #201/#202 and passing final-head check/browser CI are reconciled in [the P35 completion audit draft](p35-completion-audit.md). The #203 script-run follow-up is now merged with passing final-head PR CI; post-merge main CI and the final P35 state change remain pending there. No checks were rerun for this documentation draft.

The bounded candidate edits one plain display string on an existing collapsed
ethyl graph. `update_abbreviation_label` names the explicit instance and anchor;
required `text` is either a string or null to reset the effective label to `Et`.
Strings must contain a non-whitespace character, fit 128 UTF-8 bytes and remain
one Unicode line without control characters or line separators. The original
text is preserved without chemical parsing or normalization.

Text does not define chemistry. The two underlying carbon atoms and their bonds,
IDs, coordinates, states and stereo remain unchanged, as do identity and formula.
No formula, nickname, element or fragment definition is inferred from a label.
Existing contraction eligibility and automatic expansion rules remain binding.
General multistyle atom/abbreviation labels, fragment editing and external label
roundtrip remain unsupported; P35 stays `in_progress`.

The actual web form exposes Apply, Reset to Et and explicit expansion, with the
chemical distinction beside the field. The one plain string inherits document
label typography. Actual mounted SVG metrics feed shared trimming, fit, hit and
selection bounds, including existing underline/outline/shadow combinations.
Chemical exports retain the full graph and report `abbreviation_display` rendering
loss. Native clipboard/Cut/template restrictions remain in force.

Native v23 document/draft requires separate cosmetic `display_labels` current and
before/after history maps. Missing metadata means default Et; old v1–22 headers
forbid new fields even if empty or only in history. Migration preserves prior
graph membership and histories. Reading writes nothing; first overwrite/recovery
archives exact original document/draft bytes. Future v24 is refused untouched.
Existing current/history admission and the provider 64 MiB physical limit apply.

Authored or in-progress regression scope: domain validation/atomicity/no-op and
undo/redo; application snapshot, replay and chemistry invariance; client command
and response contracts; strict native old/current/future headers, history-only
fields and exact-byte document/draft archives; HTTP/CLI agreement; real-backend
production browser text/reset/history/save/reopen and measured SVG bounds.
`npm run test:browser:display-label` is registered in npm and CI.

Execution status: tests, builds, cargo check, browser and skill gates have not
been run for this candidate. Source/static documentation inspection only; no
scientific validation or renderer-equivalence claim is made. Required Codex and
fresh independent subagent PR reviews, findings and dispositions are pending the
parent workflow. The main P35 documentation audit `4ab1e84` (#200) is integrated;
its merged P35a–n evidence and original-scope gaps remain intact. That evidence
does not establish execution or completion of this additional P35o candidate.
