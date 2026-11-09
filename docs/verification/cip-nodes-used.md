# `chem.cip` `resource_limit` reports `nodes_used` (#127, item): verification record

Scope: the `resource_limit` reply of `chem.cip` gains `details.nodes_used`, the digraph nodes the stopped stage had counted when it stopped (greater than that stage's `limit`). The domain `Stop::RequestLimit` carries it as `used`. The model id stays `chemifree-cip-1` (no labelling result changes). Not changed: the limits, their defaults, the order of the checks, the dense-graph caps (deferred by the user, #127).

## Tests

- `crates/application/tests/cip_operation.rs`: the existing refusal tests pin `nodes_used`. The centers refusal is 36 and the bonds refusal is 4, each the cost that the same test measures without a limit; a fake labeler that over-reports pins 1001 for every stage; the scan, item-cap and over-limit refusals assert `nodes_used > limit` (their exact count is the built-in's per-unit cost and is not derived in the test).
- `crates/domain/tests/cip/main.rs`: the two `RequestLimit` assertions pin `limit` and the exact running total `used` (2 x one item, 4 x one refused item).

## Mutations (each in a private copy)

| Mutation | Caught by |
| --- | --- |
| `stopped` writes `limit` as `nodes_used` | four tests in `cip_operation.rs` |
| the labeler-report stop sets `used` 0 | the over-reporting labeler test |
| domain centers stop `used` 0 | the limit-reply test (centers) |
| domain double-bond stop `used` 0 | the limit-reply test (bonds) |
| `charge` reports the last item's nodes instead of the running total | both domain request-cap tests |

Limits of the proof: the `nodes_used > limit` lines that follow an exact check in one block, and the bonds, mixed and scan iterations of the fake-labeler loop, were not mutated one by one.

## Decisions

- `nodes_used` is per stage, like `limit`: for the double bonds `limit` is what the centers left and `nodes_used` is what the double bonds counted. It is not the request total (documented in the CONTRACT).
- The scan-stage site can only fire with a replacement labeler that returns an over-budget answer; with the built-in labeler the domain stops inside `charge` first.
