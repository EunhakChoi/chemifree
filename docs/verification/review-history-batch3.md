# Batch 3 review history

This file records completed rounds preceding the current delivery. A PR comment records the final reviewed head without requiring a new commit just to describe its own review.

## P19 formula and mass inspection

[#81](https://github.com/EunhakChoi/chemifree/pull/81) opened on 2026-10-31 at 16:38:59 UTC. Automatic round 1 reviewed `fc84150039019acbf4f998725c846fa35daa7fca`, with a 16:53:59 deadline, and completed at 16:41:23.746969 UTC (2m 24.746969s). It failed with [a finding](https://github.com/EunhakChoi/chemifree/pull/81#discussion_r4432463655): formula-only inspection unnecessarily depended on the availability of mass preferences. The fix and later review rounds are recorded on the PR; [round 1 record](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546230978).

[Round 2](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546329361) was requested at 16:51:19 UTC on `5549404279ba30592ca55b5360892b063cec51d6`, with a 17:06:19 deadline. It completed at 16:54:23.059388 UTC (3m 4.059388s) and failed: [mass suboperations were advertised as executable without compatible data](https://github.com/EunhakChoi/chemifree/pull/81#discussion_r4432505526). The following change reports registry-dependent readiness separately for mass and isotope distribution; [round 2 record](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546382070).

[Round 3](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546546309) was requested at 17:09:47 UTC on `7d3703f8148fff1ba9b81ec963a2ac26e93e9264`, with a 17:24:47 deadline. It completed at 17:12:22.601823 UTC (2m 35.601823s) with no findings or unresolved threads. Current-head CI passed and the PR merged as `3aa8686cc30c0c717bfbe02e94ffd2cc1a3c786d`; the [final review record](https://github.com/EunhakChoi/chemifree/pull/81#issuecomment-6546582777) records its disposition. [Exact-commit main CI](https://github.com/EunhakChoi/chemifree/actions/runs/41223458088) passed both jobs.

## P16 first attempt: closed without merge

[#78](https://github.com/EunhakChoi/chemifree/pull/78) was closed on 2026-10-31 after its third round failed. The replacement is independently authored from main; the closed branch is not reused.

| Round and trigger | Full head | Requested / deadline / completed UTC | Elapsed | Result and disposition |
| --- | --- | --- | --- | --- |
| 1: [automatic on opening](https://github.com/EunhakChoi/chemifree/pull/78) | `fecbb8ebed5296324135884d9070ac37df706baa` | 14:34:58 / 14:49:58 / 14:37:18 | 2m 20s | Failed: [footer denied available inspection](https://github.com/EunhakChoi/chemifree/pull/78#discussion_r4432060435). Replacement copy must agree with the executable operation. |
| 2: [manual request](https://github.com/EunhakChoi/chemifree/pull/78#issuecomment-6544926888) | `4f54f331e54896f7f7a4846301fccbf336d3bd7e` | 14:48:34 / 15:03:34 / 14:50:45 | 2m 11s | Failed: [capability status denied available inspection](https://github.com/EunhakChoi/chemifree/pull/78#discussion_r4432108902). Keep the v1 capability ID and status vocabulary, but label the available scope narrowly and make the operation catalog authoritative. |
| 3: [manual request](https://github.com/EunhakChoi/chemifree/pull/78#issuecomment-6545095890) | `25aeafe7cc48739ea956033da6b401e7328c5c5d` | 15:02:31 / 15:17:31 / 15:05:11.735744 | 2m 40.735744s | Failed: [same revision can name different content after restart](https://github.com/EunhakChoi/chemifree/pull/78#discussion_r4432162319). The replacement returns the inspected snapshot and compares actual graph content; a process-restart browser regression checks the case. |

## P20 first round

[#79](https://github.com/EunhakChoi/chemifree/pull/79) opened at 2026-10-31 15:07:39 UTC on `3f982dc771dd426b57dfb138fc4596f3510d8ef5`. Its automatic round had deadline 15:22:39 and completed at 15:10:33 UTC (2m 54s): failed for [a missing record number on a multi-document export failure](https://github.com/EunhakChoi/chemifree/pull/79#discussion_r4432178963). The PR records the corrective work and later rounds.

The [second request](https://github.com/EunhakChoi/chemifree/pull/79#issuecomment-6545311017) was sent at 15:20:31 UTC on `89e3c8aeed28aaf4a366530418fb3ead89a8a053`, with a 15:35:31 deadline. It completed at 15:23:19.039190 UTC (2m 48.039190s) with [a record-delimiter finding](https://github.com/EunhakChoi/chemifree/pull/79#discussion_r4432222522). The PR tracks the record-context fix and the final round.

The [third request](https://github.com/EunhakChoi/chemifree/pull/79#issuecomment-6545715776) was sent at 15:56:38 UTC on `b9a61506a195298ce7714e77396ca7623a866930`, with a 16:11:38 deadline. It completed at 16:00:53.817665 UTC (4m 15.817665s) with [a case-insensitive literal finding](https://github.com/EunhakChoi/chemifree/pull/79#discussion_r4432340882): mixed-case V2000 property names could lose charge. The PR closed without merging at 16:01:31 UTC. Its independently authored replacement must start from current main and satisfy the record-context, framing and literal-case requirements without reusing the failed implementation. [Final round record](https://github.com/EunhakChoi/chemifree/pull/79#issuecomment-6545784090).

## P16 independent rewrite

[#80](https://github.com/EunhakChoi/chemifree/pull/80) opened on 2026-10-31 at 16:03:23 UTC and triggered automatic round 1 on head `0d4003b2bd0b7054dfe7ee7c3a6a14dbb6674505`; its deadline was 16:18:23 UTC. The [completed review](https://github.com/EunhakChoi/chemifree/pull/80#issuecomment-6545797944) finished at 16:05:35.680026 UTC (2m 12.680026s) with no findings or unresolved threads. The current-head check and browser jobs passed, so it merged as `fb61369aeb456d6e4f7940dc13dc46d86d1c061d`. [Exact-commit main CI](https://github.com/EunhakChoi/chemifree/actions/runs/41218250815) passed both jobs; the [PR evidence comment](https://github.com/EunhakChoi/chemifree/pull/80#issuecomment-6545920244) records the disposition.

## P17 aromaticity and ring inspection

[#82](https://github.com/EunhakChoi/chemifree/pull/82) opened on 2026-10-31 at 17:22:46 UTC. Automatic round 1 reviewed `d9ea212ad9a3d6ce4e7c10eb81ccae792c0acfbe`, with a 17:37:46 deadline, and completed at 17:25:17.290759 UTC (2m 31.290759s) with no findings or unresolved threads. The [final record](https://github.com/EunhakChoi/chemifree/pull/82#issuecomment-6546797496) describes the current-head checks and merge `c4a8e44eafeb5f560743559d4797d0791c463eb2`; [exact-commit main CI](https://github.com/EunhakChoi/chemifree/actions/runs/41225052724) passed.

## P20 independent rewrite

[#83](https://github.com/EunhakChoi/chemifree/pull/83) opened on 2026-10-31 at 17:54:32 UTC. Its automatic first round reviewed `a9af5085e2af656260936f1970c960f46f75f012`, with an 18:09:32 deadline, and completed at 17:58:34.314725 UTC (4m 02.314725s) with no findings or unresolved threads. A separate reference-script quality gate then prompted a correction, so the changed head requires another review; see the [round record](https://github.com/EunhakChoi/chemifree/pull/83#issuecomment-6547104866).
