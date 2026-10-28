# 리뷰·CI 이력 (선행 A·B, P08–P15)

회차 규칙: PR을 열 때의 자동 리뷰가 1회차이고, 이후 회차는 정확한 재리뷰 요청 댓글 하나다. PR당 최대 3회, 회차마다 요청 시각부터 15분 제한이다(대기 포함). 결과는 최신 head SHA에 대한 완료된 리뷰로만 판정한다. 시각은 UTC다. 각 PR의 상세 지적과 처리 근거는 PR 댓글의 회차 표에도 있다.

## #23 선행 A — 초안 디스크 I/O를 세션 잠금 밖으로 (A1)

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 17:18:11 | 5a14f7f | 17:21:52 | 지적 2건: P1 주기 초안이 문서를 차례로 써서 멈춘 쓰기가 나머지를 막음, P2 세션 표를 잡은 채 프로세스 전역 소유권 확인을 기다림. 둘 다 수용, 6819ce4에서 수정 | [review](https://github.com/EunhakChoi/chemifree/pull/23#pullrequestreview-5686852185) |
| 2 | 17:39:54 | 6819ce4 | 17:43:03 | 지적 1건: P1 종료가 진행 중인 서버 초안 쓰기를 기다리지 않음. 수용, ae4267b에서 수정(DraftWriter가 쓰기를 기다린 뒤 마지막 초안) | [review](https://github.com/EunhakChoi/chemifree/pull/23#pullrequestreview-5687132662) |
| 3 | 17:47:45 | ae4267b | 17:51:12 | 통과(지적 없음) | [receipt](https://github.com/EunhakChoi/chemifree/pull/23#issuecomment-6476461533) |

CI(ae4267b): check·browser 성공(run 40739098437). 병합 530aa16, 병합 후 main CI 성공([run 40739733773](https://github.com/EunhakChoi/chemifree/actions/runs/40739733773), check·browser).

## #24 선행 B — 웹이 서비스의 초안 실패 기록을 표시 (A2)

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 17:57:09 | b426819 | 17:59:40 | 지적 1건: P2 서비스 재시도 뒤 결과 읽기가 실패하면 만료된 실패가 남고 다시 읽지 않음. 수용, 6da262c에서 수정(실패한 읽기를 backoff로 다시 읽음, smoke 단계 추가) | [review](https://github.com/EunhakChoi/chemifree/pull/24#pullrequestreview-5687349771) |
| 2 | 18:06:47 | 6da262c | 18:11:41 | 지적 1건: P2 페이지의 첫 실패 직후 기록 읽기가 실패하면 페이지가 자체 재시도로 돌아가 서비스 재시도와 겹침(시도 2회 계산, backoff 건너뜀). 수용, ac65f11에서 수정(기록하는 서비스면 읽기만 다시 함). 수정 전 코드에서 새 smoke 단계가 실패함을 확인 | [review](https://github.com/EunhakChoi/chemifree/pull/24#pullrequestreview-5687499897) |
| 3 | 18:18:29 | ac65f11 | 18:21:28 | 지적 1건: P1 초안 실패를 표시하는 동안 API가 재시작되면 결과 읽기(GET)가 서비스 세션을 확인하지 않아 새 프로세스의 저장본으로 화면을 바꿔, 초안에 쓰이지 못한 그림을 잃을 수 있음. 3회차까지 통과하지 못함 | [review](https://github.com/EunhakChoi/chemifree/pull/24#pullrequestreview-5687620057) |

CI(ac65f11): check·browser 성공(run 40743886617). 그러나 3회 모두 통과하지 못해 규칙대로 PR을 닫았다(병합 없음). 세 지적을 모두 반영해 최신 main에서 새 브랜치로 다시 구현한다(실패 브랜치의 코드 재사용·cherry-pick 없음). 후속 PR은 아래에 기록한다.

## #25 선행 B 재구현 — 서비스의 초안 실패 기록 표시와 재시도 따르기 (A2)

#24를 닫은 뒤 최신 main(530aa16)에서 새 브랜치 `feat/web-draft-failure-record`로 다시 구현했다(#24 코드 재사용 없음). #24의 세 지적(실패한 결과 읽기 재시도, 기록된 실패에 대해 페이지가 옆에서 다시 쓰지 않음, 결과 읽기의 서비스 재시작 확인)을 설계 요건으로 넣고, 각각을 되살린 변이가 브라우저 smoke에서 실패함을 PR 전에 확인했다.

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 18:38:10 | e544601 | 18:41:37 | 지적 1건: P1 새로 연 페이지는 토큰을 아직 모르므로, 결과 읽기 전에 API가 재시작되면 새 토큰으로 비교해 재시작을 놓침. 수용: 문서를 읽을 때 토큰을 먼저 확인해 화면의 view를 그 프로세스에 묶고, 재시작을 본 뒤에는 다음 문서 읽기에서 새 토큰을 배움. 같은 head의 CI browser job도 실패: 새 재시작 단계에서 API가 내려간 사이 페이지 읽기가 Vite 프록시의 502를 받아 콘솔 오류 검사에 걸림(제품 결함 아님, 테스트 동기화). 502를 허용하지 않고 API가 다시 뜰 때까지 문서 읽기를 route로 붙잡도록 수정 | [review](https://github.com/EunhakChoi/chemifree/pull/25#pullrequestreview-5687877425), [CI](https://github.com/EunhakChoi/chemifree/actions/runs/40746966000) |
| 2 | 18:49:22 | 08e72d8 | 18:52:34 | 통과(지적 없음) | [receipt](https://github.com/EunhakChoi/chemifree/pull/25#issuecomment-6477763220) |

CI(08e72d8): check·browser 성공(run 40748675913). 병합 a785059, 병합 후 main CI 성공([run 40749278988](https://github.com/EunhakChoi/chemifree/actions/runs/40749278988), check·browser).

## #26 P08 — 요구사항·공개 선언 추적과 conformance 실행 기반

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 18:59:03 | f156f29 | 19:01:45 | 지적 3건(P2). ① CLEANROOM-01 부분의 근거가 추적 검사 자신이라 실제 파일 내용을 보지 않음 → 수용: scripts/clean-room-check.mjs(로컬 전용 경로·실행 파일 서명·1 MiB 초과·대상 프로그램 이름·명세 사본)를 만들어 근거로 바꾸고 npm run check에 넣음. `check:` 근거는 npm run check가 실행하는 스크립트여야 한다는 검사 추가. 임시 저장소에서 각 위반이 잡힘을 확인 ② 기록된 보고서를 ID와 대략의 상태로만 비교 → 수용: 결과마다 현재 사례 메타데이터와 같은지, 개수 재계산, 새 실행과 정확히 같은지 확인. 개수·사례 축을 바꾼 변이가 잡힘을 확인 ③ `--out` 뒤 경로가 없으면 stdout으로 바뀜 → 수용: 사용법 오류(종료 2), 알 수 없는 옵션·두 번째 묶음도 거절, 단위 테스트 3개 | [review](https://github.com/EunhakChoi/chemifree/pull/26#pullrequestreview-5688154050) |
