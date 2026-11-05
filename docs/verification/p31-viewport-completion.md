# P31 bounded viewport completion

[PR #150](https://github.com/EunhakChoi/chemifree/pull/150) merged as `7fb7ae99a5dff9c8e70f16f0aecdd3f6e1ab1258`. Its final head was `c3b80d8a6ab5c95ea626837b0a8ca745c5e1331f`. [Final PR CI](https://github.com/EunhakChoi/chemifree/actions/runs/42082757225) and [that merge's main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42084338264) passed both `check` and `browser` jobs.

The delivered scope is a transient browser canvas viewport: pan, anchored zoom, fit/reset, inverse SVG coordinate conversion and screen-scale selection/drag behavior. View changes create no document command, revision, dirty state or undo entry and are not saved. A document switch or reload starts fitted. One-pointer Pan view touch is supported; pinch, multi-touch navigation, rotation, momentum and viewport persistence are not implemented. P32's further selection, alignment and document transformations remain follow-up work. OPS-01 and RENDER-01 remain partial overall; only their bounded viewport parts are implemented.

## Verification

The final local lint, typecheck, 351 client checks, web build, 99 conformance cases, traceability, 42 checker tests and browser smoke passed in the required 6 GiB/no-swap isolation. The Rust/contract portion of `npm run check` passed on the merged dependency. The TypeScript skill gate passed for all five changed source files. The [PR description](https://github.com/EunhakChoi/chemifree/pull/150) records these runs and the focused regression evidence.

The browser smoke covered pointer-anchored modifier-wheel zoom, keyboard controls and input exclusion, pan over a graphic without document edits, zoomed atom creation and save/reopen coordinates, pointer cancellation without an edit, and a distant drawing fitted at 320px width. Numeric checks cover transforms, fit, clamping and scale-aware atom/bond picking. Marked and parallel bonds and selection halos have dedicated low/high zoom checks; five focused checks were observed failing before their fixes, and reverting graphic halo scaling also made its regression checks fail in isolation.

The initial wheel-anchor assertion compared fractional requested coordinates with Chromium's integer wheel-event coordinates. It was corrected to use the actual integer anchor while preserving the world-coordinate invariant. One brief data-page Chromium diagnostic ran outside the required sandbox, with no server or network requests. It is excluded from constrained verification; the full browser smoke was subsequently rerun in the required isolation and passed.

The follow-up browser regression also passed in the same isolation: after zoom and pan, it moves an existing atom by a known screen delta, checks the resulting document coordinates and unchanged neighbors, then saves and reopens the drawing.

## Reviews and dispositions

| Reviewer | Reviewed state and result | Disposition |
| --- | --- | --- |
| Codex | Automatic review of `135b5e044c8ac1d0a7c740ff0ff2a0d322b8f457`, completed 2026-11-05 18:14:19 UTC: [one P2 graphic selection halo finding](https://github.com/EunhakChoi/chemifree/pull/150#discussion_r4484225295). | Fixed in `c3b80d8` with screen-scaled graphic underlays and low/high zoom checks. |
| Independent subagent | Fresh-context local P31 implementation review: two P2 findings, parallel-bond picking at high zoom and a single-bond selection halo hidden by the foreground. | Both fixed with rendered-stroke picking and visible screen-sized halos, including wedge/hash/wavy/crossed bond cases; regressions added. |

The [final Codex summary](https://github.com/EunhakChoi/chemifree/pull/150#issuecomment-6665573261) completed on `c3b80d8` at 2026-11-05 18:20:35 UTC with no findings or unresolved findings. These records establish the bounded viewport delivery, not scientific completeness or rendering equivalence with another application.
