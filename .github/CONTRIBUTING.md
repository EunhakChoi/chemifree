# Contributing to Chemifree

Chemifree is an independently authored chemical drawing workspace for humans and
agents. Contributions should make the product easier to use, understand, and
maintain. Describe the behavior you deliver and the evidence that supports it.

## Start with the current contract

Read [AGENTS.md](../AGENTS.md), [the handoff](../docs/HANDOFF.ko.md),
[README.md](../README.md), and [the API and document contract](../docs/CONTRACT.md).
For planned work, use the IDs, dependencies, scope, and acceptance criteria in
[the roadmap](../docs/roadmap/implementation-roadmap.json). A roadmap ID such as
P16 is a planning unit, not a GitHub PR number or a promise that one PR is enough.

Use the current source and contract to resolve stale status summaries. Report a
material disagreement rather than silently broadening the feature. Reading a
roadmap or handoff does not authorize implementation of every remaining item.

Bug fixes and focused documentation improvements can proceed without a new issue.
For an unplanned feature, agree on its scope before undertaking a large change.
An already authorized task does not need repeated approval for routine decisions.

## Prepare an isolated change

- Start a descriptive branch from the latest `origin/main`. Keep one coherent
  outcome per PR; split a large roadmap item into independently verifiable PRs.
- Inspect the worktree and preserve existing edits, untracked files, local data,
  and other contributors' branches. Use a separate worktree when needed.
- Keep unrelated refactors, generated artifacts, caches, credentials, and local
  tooling out of the diff. Include lockfile changes when dependencies change.
- Use the toolchain versions declared in `.nvmrc`, `package.json`,
  `rust-toolchain.toml`, and the lockfiles. Follow the setup in README.md.

## Design for maintenance

Keep dependencies pointing inward. Domain rules belong in Rust domain modules;
application modules coordinate use cases and own infrastructure interfaces;
providers implement those interfaces. HTTP and CLI are entry adapters, and the
web UI presents the same application behavior. Avoid duplicating scientific or
document rules in an adapter.

Give modules clear responsibilities and interfaces that hide their implementation.
Prefer explicit state and error types over ambiguous booleans, unchecked casts,
or incidental string conventions. Add an abstraction when it owns a real rule or
hides complexity. Do not introduce speculative frameworks or unrelated cleanup.

Decide routine policy questions as part of the implementation. Record significant
choices, assumptions, and tradeoffs next to the affected contract or code. If a
review exposes an ambiguity, settle it consistently across code, tests, and docs
instead of repeatedly switching policy to satisfy individual comments.

When using coding agents, read applicable installed skills before the relevant
work and explain their actual application in the PR. Rust design/testing,
TypeScript design/testing, API design, and software design skills are relevant
where those areas change. Use the shadcn skill for UI components and Python skills
for Python tools or independent fixture generators. Skills are local tools, not
dependencies to copy into the repository or substitutes for engineering judgment.

## Preserve contracts and user work

- Keep versioned API behavior, request replay, revision checks, structured errors,
  and capability reporting consistent across applicable entry adapters.
- Make multi-object edits atomic. A rejected operation must preserve the prior
  document; selection, references, ID allocation, and Undo/Redo must remain valid.
- Treat a file format change as a migration. Test old documents and drafts,
  preserve original bytes as required by the contract, and reject unsupported
  versions without overwriting them.
- Report conversion losses explicitly. Do not silently discard stereo, groups,
  text, coordinates, unknown fields, or other unsupported semantics.
- Distinguish data availability, representation, parsing, and executable chemistry.
  Expose only capabilities that actually work through their application path.

## Scientific behavior

Do not use original application DLLs as the execution engine.

Scientific results must identify the relevant method, model/data version, units,
precision policy, and limitations. Preserve raw representations where the contract
requires them. Do not substitute plausible numbers for an unavailable model or
claim full legacy equivalence from a small set of examples.

Use independent expected results, published examples, or a separately implemented
reference to verify algorithms. Self round-trips are useful but cannot by
themselves establish compatibility. Compare the relevant graph, stereo, scene,
numeric, text, and file semantics separately.

## Verification proportional to the change

For product code, run `npm run check`. This includes lint, type checks, client
tests, the web build, rustfmt, clippy, Rust tests, HTTP/CLI contracts, conformance,
and traceability checks. For changes to browser behavior or its server contract,
also run:

```bash
npm run test:browser:routes
CHEMIFREE_SMOKE_NO_SKIPS=1 npm run test:browser
```

Use the browser setup in README.md. Use isolated temporary data for tests that
kill processes, simulate permission failures, or exercise recovery and deletion.
Install workspace dependencies normally from the lockfile; sharing another
worktree's `node_modules` through symlinks can change Vite's file access behavior.

For a bug, add a regression that fails before the fix when practical. Test
observable rules, boundaries, and failure atomicity. Use real process or browser
checks where a unit test cannot establish the behavior. Independent reference
checks, property tests, fuzzing, or targeted mutations are useful when they resolve
a specific correctness risk; they are not blanket quotas for every PR.

Do not add tests that only mirror the implementation. A documentation-only change
normally needs diff, link, and content checks rather than a new test or an
expensive local product run. State exactly what ran and what did not. Existing CI
requirements still apply to documentation PRs.

Do not weaken assertions, skip required cases, add blind retries, or add arbitrary
sleeps to turn a failure green. Re-run after a fix or a documented environment
correction and retain the original failure evidence. Synchronize on observable
completion. Distinguish passing cases, expected rejections, failures, and cases
that were not run.

Respect the execution budget for the environment. On the project's shared
automation host, builds, tests, browsers, fixture generators, and their child
processes share an aggregate 16 GiB memory ceiling with swap disabled. This is
not a separate allowance per worker. Verify the effective limit, serialize heavy
work when necessary, and record resource failures honestly. Other development
environments should document their applicable budget.

## Open a reviewable PR

Use [the PR template](PULL_REQUEST_TEMPLATE.md), scaling detail to the change.
Explain the concrete problem, resulting behavior, significant design choices,
compatibility impact, and observed validation. Mark irrelevant sections as
not applicable with a brief reason. A simple docs change can be short.

Reference roadmap IDs and related issues when relevant. Include screenshots for
visible UI changes and reproducible commands or CI links for verification. If
behavior changes, update the contract, capability status, traceability, conformance
cases, roadmap, and handoff where affected. Do not mark a whole feature complete
when only its representation, loader, or one sub-operation is implemented.

## PR review policy

These are repository workflow rules, not guarantees about review service behavior.

Each PR receives exactly two reviews:

1. **One completed Codex review.** The automatic review on PR creation counts.
   If none was triggered, request it with a standalone PR comment containing
   exactly `@codex review`. Avoid the bot mention in PR descriptions and
   ordinary progress comments. Keep the 15-minute deadline, including queue
   time, measured from PR creation or the actual manual request. A reaction,
   silence, a service error, or a pending job is not a completed review. If the
   service fails or times out, run one independent local `codex review` instead;
   record its result and the service failure. Do not send a second GitHub review
   request while an earlier one may still be running. A late service result after
   the fallback is supplementary, not another required review.
2. **One independent subagent review.** Use a fresh context without the
   implementer's conversation. Give the reviewer the user request, applicable
   rules, relevant requirements and decisions, and the diff. Focus on abstract or
   undecided requirements that the implementation may have filled in arbitrarily,
   unsupported assumptions, and compliance with the applicable code-quality skills.
   For documentation changes, check that the policy matches the user request and
   is consistent across the affected documents.

Address findings from both reviews with fixes or an evidenced explanation, and
run the checks appropriate to those fixes. Do not request another review solely
because findings were fixed or the head changed. The former three-round workflow,
forced PR closure, and requirement to discard and reimplement a reviewed change
no longer apply.

Record the reviewer, reviewed full head SHA (or local diff for the subagent),
request/completion times, result, and finding disposition in the PR. Record the
Codex deadline and any failure without claiming a completed review. Review fixes
may follow the recorded reviewed SHA; identify those fixes and their verification
in the same record. A final PR comment can hold this evidence without a recursive
documentation commit just to describe its own review. Earlier review histories
describe the policy in force at the time, not the current workflow.

## Merge and handoff

Merge after the one Codex review and one independent subagent review are
completed, their findings are addressed, and the current head has successful
required CI jobs (`check` and `browser` in the current workflow). Review fixes do
not need a further review. These are merge rules; a checked template box does not
enforce them.
Do not bypass checks or treat a canceled, skipped, or running job as successful.

After merging, verify CI for that actual main merge commit. A failed main run is
unfinished work: investigate and restore the baseline before dependent merges.
Report the delivered scope, PR/commit/CI links, actual verification, remaining
limitations, and next dependencies. Complete the cleanup below as part of handoff.

## Clean up completed work

- When testing is finished, retain required results and failure evidence outside
  disposable environments, then remove all completed disposable test environments
  and their task-owned processes, temporary data, and worktrees. The sandbox
  runner's shared build cache is kept between runs; it is capped at 50 GiB and the
  runner trims it itself, so do not purge it per task.
  Confirm no test, build, browser, server, or other task still uses them before
  stopping processes or deleting files. A pending rerun or investigation is ongoing
  work, not a completed environment.
- After a PR is merged, verify its merged state and exact topic branch/head,
  including squash or rebase merges, and confirm no later unmerged commits or
  dependent work remain. Remove its completed disposable worktrees, local topic
  branch, and remote topic branch if still present. Preserve `main` and other
  long-lived branches. Do not force deletion to bypass an unresolved merge check.
- Preserve user data, existing edits and untracked files, dirty or active
  worktrees, and ongoing work. Directory names, age, or a branch name alone do not
  establish ownership or completion. Do not sweep unrelated paths or branches.
  If ownership, use, or completion is uncertain, leave the resource in place and
  record its path/branch and the reason cleanup is deferred in the handoff.
