# 리뷰·CI 이력 (P01–P07, PR #9–#22) 과 정정

이 문서는 GitHub API 기록(REST와 Codex 요약 댓글의 GraphQL 수정 이력, 읽기 전용)으로 P01–P07의 리뷰 회차와 CI 범위를 다시 정리한 것이다. 2026-10-29에 수집했다. 제품 기능의 검증 결과가 아니며, 과거 기록을 조사했다는 사실로 기능 검증이 완료됐다고 보지 않는다. 이후 PR(#23부터)의 회차는 [review-history-batch2.md](review-history-batch2.md)에 둔다.

## 이전 완료 보고에서 바로잡는 점

- **CI 범위.** #9–#13(P01·P02·P03 포함)은 `check` job만 실행했다. 브라우저 검사 job(`browser`)은 #14–#16(병합하지 않은 probe와 종료한 #15)에서 처음 실행됐고, P04(#17, b029e23) 병합 이후의 PR과 main push 실행에는 `check`와 `browser`가 모두 있다. #9·#12·#13 병합 후 main 실행도 `check`만이다.
- **환경 오류.** PR을 열 때의 자동 리뷰(1회차)가 환경 오류로 끝난 PR은 #9–#13, #15, #17–#19다(초안 PR #14·#16에는 Codex 활동이 없다). #20·#21의 자동 리뷰는 정상 완료됐고, #22의 자동 리뷰는 시작됐지만 15분 안에 끝나지 않아 시간 초과였다. 따라서 "자동 리뷰가 계속 환경 오류를 낸다"는 이전 설명은 #19까지에만 맞는다.
- **오류와 실제 리뷰가 함께 있는 회차.** #10 3회차(지적 1건), #11 3회차(리뷰가 시작됐으나 시간 초과), #12 3회차(통과)는 요청 직후 환경 오류 댓글이 달렸지만 같은 회차에 실제 코드 리뷰도 실행됐다. 회차 결과는 실제 리뷰 기준이다.
- **회차가 아닌 오류.** 리뷰 기록·종료 댓글 본문에 요청 문구가 그대로 들어가 봇이 반응한 환경 오류 6건(#9, #10, #11, #12, #15, #18)은 회차로 세지 않는다. 이후 기록 댓글에는 그 문구를 쓰지 않는다.
- **#19 병합 후 main CI 실패**(run 40654403437, browser job)는 테스트 동기화 문제였고 #20(f4d6591)이 고쳤다.


Source: `gh api` (REST + GraphQL comment edit history), read-only. Times UTC (2026-10-27/28).

## PR overview

| PR | Title | State | Created | Closed/Merged | Merge SHA | CI jobs | Rounds (class) | Replaced by |
|---|---|---|---|---|---|---|---|---|
| #9 | fix: report the source session closed when recover-as-copy evicts it | merged | 10-27T22:36 | 10-28T02:05 | db09dbf | check | R1 environment_error; R2 passed | - |
| #10 | fix(web): say when save as copy leaves the original open with its changes | closed | 10-27T22:51 | 10-28T02:31 | - | check | R1 environment_error; R2 findings; R3 findings | #12 |
| #11 | Fix the restart-step flake (#8) and an in-process draft ownership race | closed | 10-27T23:46 | 10-28T02:31 | - | check | R1 environment_error; R2 findings; R3 timeout | #13 |
| #12 | fix(web): tell when save as copy leaves the original open | merged | 10-28T02:40 | 10-28T02:58 | e76bef3 | check | R1 environment_error; R2 findings; R3 passed | - |
| #13 | Fix the browser smoke's restart-step flake (#8) and an in-process draft lock race | merged | 10-28T05:15 | 10-28T05:18 | 4a8f02e | check | R1 environment_error; R2 passed | - |
| #14 | Probe: deliberate regression to check the CI browser job (do not merge) | closed (draft) | 10-28T05:25 | 10-28T05:35 | - | browser, check | R1 no_codex_activity | probe (by design) |
| #15 | Run the browser checks in CI and upload their records | closed | 10-28T05:26 | 10-28T06:01 | - | browser, check | R1 environment_error; R2 findings; R3 timeout | #17 |
| #16 | Probe: deliberate regressions to check the CI browser job (do not merge) | closed (draft) | 10-28T06:05 | 10-28T06:11 | - | browser, check | R1 no_codex_activity | probe (by design) |
| #17 | Run the browser checks in CI and keep their records | merged | 10-28T06:06 | 10-28T06:19 | b029e23 | browser, check | R1 environment_error; R2 passed | - |
| #18 | Say why storage failed, bound lock waits, and confirm saves whose reply was lost | closed | 10-28T06:27 | 10-28T07:00 | - | browser, check | R1 environment_error; R2 findings; R3 findings | #19 |
| #19 | Say why storage failed, bound waits, and confirm requests whose reply was lost | merged | 10-28T07:28 | 10-28T08:06 | e02010b | browser, check | R1 environment_error; R2 findings; R3 passed | - |
| #20 | test: wait for the page to take in a drawn bond before checking its draft | merged | 10-28T08:15 | 10-28T08:18 | f4d6591 | browser, check | R1 passed | - |
| #21 | feat: search and sort the document list, rename, and a trash with restore | merged | 10-28T08:53 | 10-28T08:57 | 0cc92ea | browser, check | R1 passed | - |
| #22 | feat: draft unsaved work in the service, without a client | merged | 10-28T09:18 | 10-28T09:45 | 5aaf6c6 | browser, check | R1 timeout; R2 findings; R3 passed | - |

## Rounds

| PR | Round | Trigger | Requested | Head | Codex review commit | Outcome | End | Min | Class | Error+review co-exist |
|---|---|---|---|---|---|---|---|---|---|---|
| #9 | 1 | automatic_on_open | 22:36:52 | 42720bb | - | environment_error | 22:37:00 | 0.13 | **environment_error** | error only |
| #9 | 2 | re-review comment | 02:02:34 | 42720bb | 42720bb | completed, findings=False | 02:04:13 | 1.65 | **passed** | no |
| #10 | 1 | automatic_on_open | 22:51:42 | 1b86581 | - | environment_error | 22:51:55 | 0.22 | **environment_error** | error only |
| #10 | 2 | re-review comment | 02:02:36 | 1b86581 | 1b86581 | completed, findings=True (1 inline {'P2': 1}) | 02:04:53 | 2.28 | **findings** | no |
| #10 | 3 | re-review comment | 02:16:10 | b421587 | b421587 | completed, findings=True (1 inline {'P2': 1}) | 02:18:11 | 2.02 | **findings** | YES (error 02:16:12) |
| #11 | 1 | automatic_on_open | 23:46:45 | 3b00665 | - | environment_error | 23:46:57 | 0.2 | **environment_error** | error only |
| #11 | 2 | re-review comment | 02:02:33 | 5a9194b | 5a9194b | completed, findings=True (1 inline {'P2': 1}) | 02:05:07 | 2.57 | **findings** | no |
| #11 | 3 | re-review comment | 02:16:08 | 6e5b060 | 6e5b060 | running, never completed in this round | 02:31:52 | 15.73 | **timeout** | YES (error 02:16:11) |
| #12 | 1 | automatic_on_open | 02:40:35 | ded063a | - | environment_error | 02:40:43 | 0.13 | **environment_error** | error only |
| #12 | 2 | re-review comment | 02:48:06 | ded063a | ded063a | completed, findings=True (1 inline {'P3': 1}) | 02:50:56 | 2.83 | **findings** | no |
| #12 | 3 | re-review comment | 02:52:02 | edda026 | edda026 | completed, findings=False | 02:53:34 | 1.53 | **passed** | YES (error 02:52:12) |
| #13 | 1 | automatic_on_open | 05:15:32 | 8438951 | - | environment_error | 05:15:45 | 0.22 | **environment_error** | error only |
| #13 | 2 | re-review comment | 05:16:02 | 8438951 | 8438951 | completed, findings=False | 05:18:06 | 2.07 | **passed** | no |
| #14 | 1 | automatic_on_open | 05:25:37 | 2e3e302 | - | no Codex activity (draft PR) | 05:35:05 | 9.47 | **no_codex_activity** | no |
| #15 | 1 | automatic_on_open | 05:26:04 | 18ab8f5 | - | environment_error | 05:26:13 | 0.15 | **environment_error** | error only |
| #15 | 2 | re-review comment | 05:37:45 | 2de78e9 | 2de78e9 | completed, findings=True (1 inline {'P2': 1}) | 05:39:34 | 1.82 | **findings** | no |
| #15 | 3 | re-review comment | 05:45:44 | 33eafa7 | 33eafa7 | running, never completed in this round | 06:01:10 | 15.43 | **timeout** | no |
| #16 | 1 | automatic_on_open | 06:05:48 | e5a9081 | - | no Codex activity (draft PR) | 06:11:00 | 5.2 | **no_codex_activity** | no |
| #17 | 1 | automatic_on_open | 06:06:08 | 5cd14ae | - | environment_error | 06:06:18 | 0.17 | **environment_error** | error only |
| #17 | 2 | re-review comment | 06:16:47 | 5d1e4cd | 5d1e4cd | completed, findings=False | 06:18:43 | 1.93 | **passed** | no |
| #18 | 1 | automatic_on_open | 06:27:09 | cb243da | - | environment_error | 06:27:22 | 0.22 | **environment_error** | error only |
| #18 | 2 | re-review comment | 06:30:09 | cb243da | cb243da | completed, findings=True (1 inline {'P2': 1}) | 06:33:30 | 3.35 | **findings** | no |
| #18 | 3 | re-review comment | 06:56:37 | 9f2fb08 | 9f2fb08 | completed, findings=True (1 inline {'P2': 1}) | 06:59:16 | 2.65 | **findings** | no |
| #19 | 1 | automatic_on_open | 07:28:29 | 5dfcd18 | - | environment_error | 07:28:41 | 0.2 | **environment_error** | error only |
| #19 | 2 | re-review comment | 07:30:56 | 5dfcd18 | 5dfcd18 | completed, findings=True (1 inline {'P2': 1}) | 07:34:44 | 3.8 | **findings** | no |
| #19 | 3 | re-review comment | 08:01:16 | 06271e6 | 06271e6 | completed, findings=False | 08:05:14 | 3.97 | **passed** | no |
| #20 | 1 | automatic_on_open | 08:15:32 | 9e40635 | 9e40635 | completed, findings=False | 08:17:13 | 1.68 | **passed** | no |
| #21 | 1 | automatic_on_open | 08:53:52 | 058c991 | 058c991 | completed, findings=False | 08:57:23 | 3.52 | **passed** | no |
| #22 | 1 | automatic_on_open | 09:18:11 | e6aa703 | e6aa703 | running, never completed in this round | 09:33:48 | 15.62 | **timeout** | no |
| #22 | 2 | re-review comment | 09:33:48 | e6aa703 | e6aa703 | completed, findings=True (1 inline {'P2': 1}) | 09:36:35 | 2.78 | **findings** | no |
| #22 | 3 | re-review comment | 09:41:43 | 12ada4d | 12ada4d | completed, findings=False | 09:44:20 | 2.62 | **passed** | no |

## Environment errors outside review rounds

| PR | Error at | Preceding human comment mentioning the review bot |
|---|---|---|
| #9 | 02:16:07 | 02:15:59 Review log, round 2 (re-review comment at 02:02:35Z), head 42720bb: revi |
| #10 | 02:32:06 | 02:31:54 Closing after three failed Codex review rounds, as the review rules re |
| #11 | 02:32:01 | 02:31:52 Closing after three failed Codex review rounds, as the review rules re |
| #12 | 02:58:51 | 02:58:38 Review log, round 3 (re-review comment at 02:52:04Z), head edda026: comp |
| #15 | 06:01:19 | 06:01:09 Closing after three failed Codex review rounds, as the review rules re |
| #18 | 07:00:17 | 07:00:06 Closing after three review rounds without a pass, as the review rules  |

## Closures

- #10: Closing after three failed Codex review rounds, as the review rules require. Replaced by #12.
- #11: Closing after three failed Codex review rounds, as the review rules require. Replaced by #13.
- #14: Probe finished; closing without merging, as planned.
- #15: Closing after three failed Codex review rounds, as the review rules require. Replaced by #17.
- #16: Probe finished; closing without merging, as planned. Both runs are recorded in `docs/verification/browser-ci.json` in #17.
- #18: Closing after three review rounds without a pass, as the review rules require. P05 will be rewritten from the latest `main` on a new branch, without reusing these commits, and the new PR will link here. Replaced by #19.

## Main push runs after each merge

| After PR | Merge SHA | Run | Conclusion | Jobs |
|---|---|---|---|---|
| #9 | db09dbf | [40618915865](https://github.com/EunhakChoi/chemifree/actions/runs/40618915865) | success | check=success |
| #12 | e76bef3 | [40624016359](https://github.com/EunhakChoi/chemifree/actions/runs/40624016359) | success | check=success |
| #13 | 4a8f02e | [40637230234](https://github.com/EunhakChoi/chemifree/actions/runs/40637230234) | success | check=success |
| #17 | b029e23 | [40643116016](https://github.com/EunhakChoi/chemifree/actions/runs/40643116016) | success | browser=success, check=success |
| #19 | e02010b | [40654403437](https://github.com/EunhakChoi/chemifree/actions/runs/40654403437) | failure | browser=failure, check=success |
| #20 | f4d6591 | [40655846104](https://github.com/EunhakChoi/chemifree/actions/runs/40655846104) | success | browser=success, check=success |
| #21 | 0cc92ea | [40660586352](https://github.com/EunhakChoi/chemifree/actions/runs/40660586352) | success | browser=success, check=success |
| #22 | 5aaf6c6 | [40666694453](https://github.com/EunhakChoi/chemifree/actions/runs/40666694453) | success | browser=success, check=success |

## Notes

- Head SHA at a given time is inferred from pull_request CI run creation times (push-time proxy, allowing a 10 s push-to-run lag; e.g. #12 edda026 committed 02:51:59, request 02:52:02, CI run created 02:52:05). The Codex summary commit_sha is listed per round as a cross-check; it matches the inferred head in every round where a review ran.
- Summary comment states over time come from GraphQL IssueComment.userContentEdits; REST only shows the final state.
- Rounds: 1 = PR opening (automatic), then one per human comment whose body is exactly the re-review request. Timeout = no completed outcome within 15 minutes of the request.
- Environment-error comments that follow human log/closing comments containing the text the re-review request (not exact requests) are listed as stray_environment_errors, not rounds; the trigger is inferred from timing.
- Draft PRs #14 and #16 have no Codex activity at all; their single round is classed no_codex_activity (not one of the requested classes).
- check-runs exist only for SHAs that were a PR head when pushed; intermediate commits pushed together show no runs.
