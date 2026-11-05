<!--
Follow .github/CONTRIBUTING.md. Keep simple changes short; use N/A with a reason
where a section does not apply. Replace prompts with observed facts.
Each PR receives one Codex review (automatic on creation) and one independent
subagent review. Do not put bot mentions in this description.
-->

## Problem and resulting behavior

<!-- What concrete problem does this solve? What can the user do after this PR?
For a bug, give a trigger and before/after behavior. -->

## Scope and design

- Roadmap / issue:
- Main changes and significant decisions:
- Dependencies and intentionally deferred work:

<!-- Explain ownership of rules and why any new abstraction is needed. -->

## Compatibility and scientific behavior

<!-- Cover the applicable API/CLI/web contract, file/draft migration, Undo/Redo,
failure atomicity, conversion loss, and capability changes. For scientific work,
identify method/data/model versions, units, precision, references, and limits.
Use N/A with a reason for documentation-only or unrelated changes. -->

## Verification

| Command or scenario | Observed result | Evidence / environment |
| --- | --- | --- |
| | | |

<!-- List checks actually run, regression behavior before the fix when relevant,
and any checks not run with reasons. Link browser screenshots for visible changes.
State independent reference results where relevant; do not equate self round-trip
with external compatibility. State the shared resource budget for heavy runs. -->

## Skills applied

<!-- For agent-assisted work: name the skills actually read and the concrete
decisions or checks they informed. Otherwise use N/A. Do not list unused skills. -->

## Risks and remaining limits

<!-- Describe material limitations and recovery/rollback for risky behavior.
Keep unsupported and untested behavior explicit; link follow-up plan IDs. -->

## Review record

<!-- One Codex review and one independent subagent review per PR. The subagent
checks arbitrary interpretations of abstract or undecided requirements and code
quality (policy consistency for docs). Address findings without another review
round; record later fixes and their checks. The Codex deadline is 15 minutes
including queue time. On a service error or timeout, record it and use one
independent local `codex review` as the fallback.
The evidence may live in a PR comment without another documentation commit. -->

| Reviewer | Trigger / request link | Reviewed full head SHA / local diff | Requested / deadline / completed (UTC) | Elapsed | Result / findings / fixes / evidence |
| --- | --- | --- | --- | --- | --- |
| Codex | Automatic on PR creation | | | | Pending |
| Independent subagent | | | | | Pending |

## Delivery checks

- [ ] The scope, decisions, and compatibility impact match the final diff.
- [ ] Relevant verification is recorded, including failures and checks not run.
- [ ] Affected contracts, capabilities, traceability, roadmap, and docs are updated, or N/A is explained.
- [ ] The diff excludes credentials, local data, generated outputs, caches, and unrelated changes.
- [ ] One Codex review and one independent subagent review are completed, and their findings are addressed.
- [ ] Required CI jobs pass for the current head.

<!-- After merging, record the merge SHA and its main CI result in a PR comment.
Do not check review/CI boxes in advance or describe pending work as completed. -->
