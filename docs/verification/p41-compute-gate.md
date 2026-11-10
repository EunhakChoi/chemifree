# P41 concurrency gate for `chem.aromaticity_convert` and `chem.cip`: verification record

Scope: at most N computations run at the same time in one `Application`; N defaults to min(cores, 4) and a host setting is clamped to 1..=64. A request that finds no free slot waits for one inside its own time limit and, if the limit passes first, replies `time_limit` (no new code, no new field). `Application::cancel_computations` ends every running and waiting computation with `cancelled` and refuses new ones; the API calls it on its graceful-shutdown signal, so that `cancelled` reply is delivered over HTTP as 200. The CLI is unchanged. Not changed: the limits, replay rules, results of any computation that finds a free slot, other operations (no gate for them), explicit cancel operation, jobs.

## Tests

- `crates/domain/src/budget.rs` (unit, 3): the second interrupt stops a budget whose caller did not; the order is caller, second, deadline; the caller is asked once per check.
- `crates/application/src/compute_gate.rs` (unit, 7): a full gate asks the waiter's interrupt; a dropped permit lets a waiter in; a cancelled waiter leaves and takes no slot; `close` wakes every waiter; a panic while holding a permit releases it; a free slot is taken at once; the cap is clamped to 1..=64.
- `crates/application/tests/compute_gate.rs` (7): through `Application` — a conversion and `chem.cip` each wait for a slot until their own deadline and reply `time_limit`; the same `request_id` while waiting is `request_in_progress`; `cancel_computations` stops a running conversion, wakes waiters and refuses new computations but not other operations; document unchanged in every case.
- `apps/api/tests/shutdown_cancel.rs` (2): after `ApiState::cancel_computations` a conversion over HTTP is 200 `cancelled` and changes nothing; reads, health and snapshot are still served.

## Mutations (each in a private copy)

| Mutation | Caught by |
| --- | --- |
| `Budget::check` without the `also` check | the three budget tests |
| gate: `running <= cap`; `Permit::drop` without decrement; no interrupt check in the wait loop; `close` without notify; poison-unsafe drop; no clamp | the named gate unit case for each (six mutations) |
| conversion without `gate.enter` | the conversion wait case |
| `chem.cip` without `gate.enter` | the cip wait case |
| `start_time_limit` without `.also(shutdown_flag)` | the cancel-running case |
| `gate.enter` before the in-progress guard | the same-request-id case |
| `ApiState::cancel_computations` emptied | the HTTP shutdown case |

Limits of the proof: a full gate is made in tests by holding permits, so the wait is exercised with the 10 ms slice, not under real load; no fairness is claimed or tested.

## Decisions (kept as built)

These three were listed as awaiting the user's confirmation when PR #248 merged. On 2026-11-10 the user left them to the lead, who kept all three; the decision is recorded on issue #106 and in the roadmap note of P41.

- A full gate replies `time_limit`, not `resource_limit`, so no wire shape is added; the reply does not say whether waiting or computing used the time.
- Server shutdown now delivers `cancelled` as 200 (before, a `cancelled` reply never reached an HTTP client).
- No CLI flag or environment variable sets N; only the host API (`with_compute_concurrency`) does. A user-facing setting belongs to the product server's settings (P87).
