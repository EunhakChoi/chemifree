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
| 2 (요청) | 2026-10-28 19:06:58 | 283c61d | 19:10:08 | 지적 1건(P2). 요구사항 전체 상태가 부분 상태와 어긋나도 통과 → 수용: 부분들에서 상태를 계산해(모두 거절 → intentionally_rejected, 모두 구현·거절 → implemented, 구현·일부 구현이 있으면 partial, 미검증이 있으면 unverified, 그 밖 not_implemented) 적힌 상태와 비교. 문서에 규칙 기재 | [review](https://github.com/EunhakChoi/chemifree/pull/26#pullrequestreview-5688256394) |
| 3 (요청) | 2026-10-28 19:22:34 | 3689d1b | 19:25:47 | 지적 1건(P2): 실행 가능한 accept 사례가 오류를 기대하는 단계만 가져도 불러오기를 통과해, 의도된 거절이 passed/accept로 세어짐. 3회차까지 통과하지 못함 | [review](https://github.com/EunhakChoi/chemifree/pull/26#pullrequestreview-5688475867) |

마지막 회차(3회차)를 요청하기 전에 Codex review와 별개로, 이 PR의 코드만 받은 독립 검토 에이전트가 추가로 6건을 지적했고 모두 고쳤다(Codex 회차로 세지 않음).

1. clean-room 검사가 .claude·.agents를 파일·링크로 추적하거나 analysis/·investigations/·data/private·키·.env.* 경로를 놓침 → 경로 조각·패턴 확장, 링크는 따라가지 않고 대상 경로 검사, 일반 파일이 아니면 문제로 보고. 임시 저장소에서 각 경우가 잡히고 .env.example은 허용됨을 확인
2. `case:` 근거가 대기 중(not_run)인 사례여도 implemented·partial 근거로 통과 → 기록에서 passed인 사례만 근거로 인정. 대기 사례를 근거로 넣은 변이가 잡힘을 확인
3. 저장소 경로에 공백·비ASCII가 있으면 검사 스크립트가 URL 경로를 그대로 써서 실패 → fileURLToPath 사용. 공백 경로에서 실행 확인
4. 실행기가 일부 기대값을 확인하지 않고 통과시킬 수 있음(`$contains` 옆의 다른 키 무시, 기대값 없는 accept 사례, 받지 않는 단계의 `request_id`) → 불러올 때 거절하고 `matches`도 실패, 단위 테스트 2개 추가와 기존 테스트 보강. 각 검사를 끈 변이 4개가 모두 테스트를 실패시킴을 확인
5. 후속 계획이 이미 완료된 로드맵 항목을 가리켜도 통과, implemented 부분에 후속 계획이 남아도 통과 → 둘 다 거절. 완료 항목(P07)을 가리킨 변이가 잡힘을 확인
6. 문서의 현재 합계 문장이 추적표와 어긋나도 통과 → TRACEABILITY.ko.md·HANDOFF.ko.md가 추적표에서 만든 정확한 문장을 담는지 확인

CI(3689d1b): check·browser 성공(run 40753734609). 그러나 3회 모두 통과하지 못해 규칙대로 PR을 닫았다(병합 없음, [닫은 이유](https://github.com/EunhakChoi/chemifree/pull/26#issuecomment-6478530918)). 최신 main(a785059)에서 새 브랜치로 다시 구현한다(#26 코드 재사용·cherry-pick 없음). 후속 PR은 아래에 기록한다.

## #27 P08 재구현 — 요구사항·공개 선언 추적과 버전 있는 적합성 runner

#26을 닫은 뒤 최신 main(a785059)에서 새 브랜치 `feat/requirement-traceability`로 다시 구현했다. #26의 코드를 읽거나 재사용하지 않도록 새 컨텍스트의 에이전트가 작성했고, #26의 모든 지적과 독립 검토 지적을 설계 요건으로 넣었다. PR 전에 별도 독립 검토(지적 P2 3건·P3 1건 등)를 반영했다(c2eb869). 리뷰·CI 기록(review-history*.md)과 로드맵은 GitHub 기록과 로드맵 생성 결과로, 제품 코드가 아니다.

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 20:15:21 | c2eb869 | 20:20:01 | 지적 2건(P2). ① 탭이 든 추적 파일 이름을 첫 탭 뒤에서 자르지 않아 이름이 잘리고 검사를 건너뜀 → 수용: 첫 탭에서만 나눔 ② `#[ignore]` 테스트를 실행되는 근거로 인정 → 수용: 무시·건너뛴 테스트 거절. 각각 실패 테스트를 먼저 작성. dc36cae에서 수정 | [review](https://github.com/EunhakChoi/chemifree/pull/27#pullrequestreview-5689145595) |
| 2 (요청) | 21:12:52 | dc36cae | 21:15:33 | 지적 2건(P2). ① UTF-8이 아닌 추적 파일 이름이 대체 문자로 바뀌어 검사를 건너뜀 ② `/* */` 주석 안 테스트를 실행 근거로 인정 → 수용: 텍스트 추정 대신 원천을 읽도록 바꿈. clean-room은 index의 이름 바이트와 blob 내용을 읽고 UTF-8이 아닌 이름을 거절, 테스트 근거는 cargo가 실제로 나열·실행하는 테스트만 인정 | [review](https://github.com/EunhakChoi/chemifree/pull/27#pullrequestreview-5689798963) |
| 3 (요청) | 21:41:44 | b7dc53e | 21:44:39 | 지적 1건(P2): clean-room 검사가 WebAssembly 실행 파일(`.wasm`, `\0asm` 서명)을 거절하지 않음. 3회차까지 통과하지 못함 | [review](https://github.com/EunhakChoi/chemifree/pull/27#pullrequestreview-5690094601) |

3회차 요청 전에 두 번째 독립 사전 검토(P2 5건·P3 4건: catch-all의 의도적 거절, CI 주석·비활성 단계, `cargo test`를 실행하지 않는 check, `test = false` 대상, CI에서의 명세 참조 누락, 깊은 곳의 빈 기대값 등)를 반영했다(9d5e935). CI(b7dc53e): check·browser 성공(run 40774065830). 그러나 3회 모두 통과하지 못해 규칙대로 PR을 닫았다(병합 없음). 세 회차의 지적이 모두 "저장소 검사가 주장하는 성질이 거짓인데도 통과할 수 있다"는 종류였으므로, 다음 구현은 검사가 확인할 수 있는 것만 주장하도록 범위를 좁혀 최신 main에서 새 브랜치로 다시 구현한다(#27 코드 재사용·cherry-pick 없음).

## #28 P08 세 번째 구현 — 검사가 확인할 수 있는 것만 주장하는 추적·적합성 runner

#27을 닫은 뒤 최신 main(a785059)에서 새 브랜치 `feat/requirement-tracing`으로 다시 구현했다. #26·#27의 코드를 읽거나 재사용하지 않도록 새 컨텍스트의 에이전트가 작성했다. 앞선 리뷰 지적이 모두 "검사가 주장하는 성질이 거짓인데도 통과"하는 종류였으므로 범위를 좁혔다. 근거는 통과한 적합성 사례와 `npm run check`가 부르는 npm 스크립트 두 종류뿐이고, CI·Rust 소스·cargo 출력·git 내부를 해석하지 않는다. 공개 저장소에 비공개 자료가 없다는 부분은 자동 검사 없이 `unverified`로 기록한다. PR 전에 독립 사전 검토(P2 2건·P3 3건)를 반영했다(c85de26). 회차 기록은 이 PR이 끝난 뒤 다음 PR에서 덧붙인다.

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 22:31:59 | a41f34a | 22:34:28 | 통과: 이 head의 코드 리뷰 완료, 지적 없음(요약 댓글의 review state `completed`, `has_any_findings: false`, 👍 반응) | [summary](https://github.com/EunhakChoi/chemifree/pull/28#issuecomment-6482371868) |

CI(a41f34a): check·browser 성공(run 40780684248). 병합 ede7230, 병합 후 main CI 성공([run 40781143752](https://github.com/EunhakChoi/chemifree/actions/runs/40781143752), check·browser).

## #29 P13 — 연산 계약 1과 연산별 capability 카탈로그

최신 main(ede7230) 위의 브랜치 `feat/operation-contract-1`. PR을 연 뒤 받은 독립 사전 검토(P1 없음, P2 판단 1건·P3 9건)는 1회차 지적과 함께 반영했다.

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-28 22:48:18 | 6e82fc6 | 22:51:06 | 지적 1건(P2): 정확히 1 MiB인 요청이 CRLF로 끝나면 CLI가 `\r`까지만 읽어 `resource_limit`으로 거절(LF는 통과) → 수용: 실패하는 계약 스모크(1 MiB LF·CRLF, 1 MiB 초과 뒤 다음 줄)를 먼저 작성해 CRLF 실패를 재현한 뒤, 줄 끝 2바이트 여유를 두고 줄 끝을 뺀 길이로 판정 | [review](https://github.com/EunhakChoi/chemifree/pull/29#pullrequestreview-5690737819) |
| 2 (요청) | 23:01:57 | ebee17f | 23:04:29 | 통과: 이 head의 코드 리뷰 완료, 지적 없음(review state `completed`, `has_any_findings: false`, 영수 댓글 "Didn't find any major issues") | [receipt](https://github.com/EunhakChoi/chemifree/pull/29#issuecomment-6482981592) |

사전 검토 반영(같은 수정 커밋):

1. CHEM-02가 화학 검증 없이 `partial`로 표시됨 → CHEM-02를 `not_implemented`로 되돌리고, 실행하지 않는 연산이 결과 없이 `unsupported`로 답한다는 보장은 API-01.4로 옮김(사례 `operation-unsupported`의 요구사항도 API-01만)
2. 요청의 중복 키가 마지막 값으로 조용히 처리됨 → 적합성 로더의 엄격한 JSON 파서를 application으로 옮겨 연산 요청에도 쓰고 `malformed_request`로 거절. 실패 테스트 먼저 작성
3. `"contract": 1.0`이 `unsupported_contract`로 답함 → 음이 아닌 정수가 아니면 `malformed_request`. 실패 테스트와 변이 확인
4. 연산 경로만 `application/*+json`을 415로 거절 → v1 경로처럼 허용. 본문을 읽지 못한 경우를 모두 413으로 답함 → 길이 초과만 413, 나머지는 400 `malformed_request`. 단위 테스트(변이 확인)와 계약 스모크의 413·`+json` 단계 추가
5. 문서: `document_revision`이 null인 경우, CLI의 빈 줄 처리, CLI는 복구 초안을 쓰지 않아 세션의 초안 필드가 API와 다를 수 있음, legacy-observed 문장의 모호함을 CONTRACT.md에 바로잡음

CI(ebee17f): check·browser 성공(run 40784373555). 병합 85d4788, 병합 후 main CI 성공([run 40784845386](https://github.com/EunhakChoi/chemifree/actions/runs/40784845386), check·browser).

## #30 P09 — 화학 그래프와 장면 배치 분리, 파일 형식 2와 마이그레이션

main(85d4788) 위의 브랜치 `feat/graph-scene-split`. 구현은 새 컨텍스트의 에이전트가 명세(spec/ 아래만)와 설계 지시로 작성했다. PR 전에 독립 사전 검토(P1 없음, P2 2건·P3 5건: `positions` 중복 키의 실제 오류 코드, 다른 배치에서 새 결합이 비틀리는 UI 배치 규칙, 마이그레이션 뒤 이력 한도, 명령이 아닌 카탈로그 행의 executable 표시, 복제 배치 겹침, 같은 ID의 다른 v1 파일 보존, HTTP 본문 한도)를 반영했다(87966e6).

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-29 00:17:50 | 87966e6 | 00:23:12 | 지적 1건(P2): 배치 이동·삭제 버튼을 빠르게 두 번 누르면 두 번째 요청이 화면의 같은 revision으로 나가 `revision_conflict`로 거절되고 그 이동이 사라짐 → 수용: 첫 요청을 붙잡아 두는 브라우저 smoke 단계를 먼저 작성해 409를 재현한 뒤, 이동·삭제 명령을 적용될 때의 최신 문서로 만들도록 바꿈(삭제된 배치·마지막 배치는 보내지 않음). 단위 테스트 추가 | [review](https://github.com/EunhakChoi/chemifree/pull/30#pullrequestreview-5691389656) |
| 2 (요청) | 00:31:12 | 438f766 | 00:46:33 시간 초과 | 통과 아님: 15분 안에 시작 반응(👀)만 있었고 review state는 `running` | [PR](https://github.com/EunhakChoi/chemifree/pull/30) |
| 3 (요청) | 00:46:53 | 438f766 | 00:48:58 | 통과: 이 head의 코드 리뷰 완료, 지적 없음(review state `completed`, `has_any_findings: false`, 영수 댓글 "Didn't find any major issues") | [receipt](https://github.com/EunhakChoi/chemifree/pull/30#issuecomment-6484742580) |

CI(438f766): check·browser 성공(run 40794594222). 병합 1a518cd, 병합 후 main CI 성공([run 40796524510](https://github.com/EunhakChoi/chemifree/actions/runs/40796524510), check·browser).

## #31 P10 — 수소·라디칼·질의·결합 종류 상태 기록(닫음)

main(1a518cd) 위의 브랜치 `feat/atom-bond-states`. 구현은 새 컨텍스트의 에이전트가 명세(spec/ 아래만)와 설계 지시로 작성했다. PR 전에 독립 사전 검토(P1·P2 없음, P3 3건: 원자 상태 필드가 일부 원자에만 있는 응답을 웹이 받아들임, 원자 종류·전하·동위원소를 빠르게 연달아 누르면 거절될 조합을 보냄, CLI 텍스트 출력에 상태가 빠짐)를 반영했다(f0e4320).

| 회차 | 요청 | 대상 head | 종료 | 결과 | 링크 |
|---|---|---|---|---|---|
| 1 (자동) | 2026-10-29 02:03:15 | f0e4320 | 02:06:18 | 지적 1건(P2): 부착점을 저장하지만 캔버스에 그리지 않아, 결합된 탄소에 부착점을 붙여도 꼭짓점만 보임 → 수용: 웹 단위 테스트와 브라우저 smoke 단계를 먼저 작성해 실패를 확인한 뒤 부착점 표시를 그림. 같은 수정에서 smoke 정리 단계가 API 종료(종료 시 초안 쓰기)를 기다리지 않고 임시 디렉터리를 지워 한 번 ENOTEMPTY로 실패한 경쟁을 고침 | [review](https://github.com/EunhakChoi/chemifree/pull/31#pullrequestreview-5692017219) |
| 2 (요청) | 02:16:19 | 02efcb2 | 02:19:27 | 지적 1건(P2): 결합선이 라벨 유무만 보고 7 pt 고정으로 잘려 `CH16`·`H?`·`![N,O]` 같은 넓은 라벨 위를 지나감 → 수용: "결합선 끝이 렌더된 라벨 상자 안에 있지 않다"는 브라우저 검사를 먼저 넣어 실패를 확인한 뒤 라벨 상자 기준으로 자름 | [review](https://github.com/EunhakChoi/chemifree/pull/31#pullrequestreview-5692097642) |
| 3 (요청) | 02:28:58 | 65ade75 | 02:32:55 | 지적 2건(P2): ① 라디칼 점·부착점 표시가 결합을 자르는 상자에 들어가지 않아 그쪽으로 나가는 결합선과 겹침 ② 캔버스 view box가 원자 중심만 보고 라벨 폭을 무시해 캔버스 가장자리의 넓은 질의 라벨이 잘림. 3회차까지 통과하지 못함 | [review](https://github.com/EunhakChoi/chemifree/pull/31#pullrequestreview-5692176139) |

CI(65ade75): check·browser 성공(run 40806609015). 그러나 3회 안에 통과하지 못해 규칙대로 PR을 닫았다(병합 없음, [닫은 이유](https://github.com/EunhakChoi/chemifree/pull/31#issuecomment-6486292409)). 세 회차의 지적이 모두 "캔버스가 라벨·상태 표시·결합 끝·view box를 따로, 일부는 추정한 기하로 배치한다"는 같은 약점이었다. 최신 main(1a518cd)에서 새 브랜치 `feat/recorded-atom-states`로 다시 구현한다(#31 코드 재사용·이름 변경·cherry-pick 없음). 다시 구현할 때는 원자마다 라벨 배치(글자와 상태 표시를 함께) 하나를 두고 결합 자르기와 view box가 모두 그것을 쓰게 한다. 후속 PR은 아래에 기록한다.

## #32 P10 재구현 — 원자·결합 상태 기록(병합)

시간은 UTC다. [#32](https://github.com/EunhakChoi/chemifree/pull/32)는 2026-10-29 04:51:00에 열렸고, 자동 Codex review가 04:54:24에 head `b1f94d5e57ae1b4111411648fc9300fe2fbf3c1c`에 대해 완료됐다. GitHub의 [리뷰 상태](https://github.com/EunhakChoi/chemifree/pull/32#issuecomment-6488262099)는 `completed`, 지적 및 미해결 지적 없음이다. 별도의 제출된 review 객체는 없으며 이 상태를 완료 근거로 기록한다.

PR [CI](https://github.com/EunhakChoi/chemifree/actions/runs/40820040161)는 check·browser 모두 성공했다. 04:54:53에 `b1ccdfb222043cf5d6e69aaa6d5720f2ba4b5654`로 병합했고, 병합 후 [main CI](https://github.com/EunhakChoi/chemifree/actions/runs/40820386893)도 check·browser 모두 성공했다.

## #33 P11 — 장면·그룹·선택 이력(병합하지 않고 종료)

시간은 UTC다. [#33](https://github.com/EunhakChoi/chemifree/pull/33)의 자동 리뷰를 포함한 세 회차는 다음과 같다. 마지막 회차의 시작 반응과 `running` 상태는 완료나 통과가 아니다.

| 회차 | 요청 시각 | 대상 head | 완료 또는 기한 | 결과와 처리 | 링크 |
|---|---|---|---|---|---|
| 1 자동 | 2026-10-29 07:08:11 | `e2232ed2ce363cb1f9dda8e07429c6d30ad4e9b2` | 07:12:17 | P2 두 건: 그룹 요청 중 바뀐 무관한 객체 선택 보존, 저장된 선택과 이력 경계 검증. 수용하고 수정했으며 정상적인 undo·undo·redo 및 이력 잘라내기 상태도 허용하도록 경계를 보완했다. | [리뷰](https://github.com/EunhakChoi/chemifree/pull/33#pullrequestreview-5694073377), [처리 설명](https://github.com/EunhakChoi/chemifree/pull/33#issuecomment-6490732408) |
| 2 요청 | 07:30:32 | `2d7791a6049a9f353d23483728521a1a0488eb7d` | 07:34:41 | 새 P2 한 건: Delete가 선택을 지운 후 큐가 선택을 읽어 Undo에서 선택을 잃음. 명령 시점의 선택을 캡처하는 요구사항을 수용했다. | [요청](https://github.com/EunhakChoi/chemifree/pull/33#issuecomment-6490732733), [리뷰](https://github.com/EunhakChoi/chemifree/pull/33#pullrequestreview-5694265656), [처리 설명](https://github.com/EunhakChoi/chemifree/pull/33#issuecomment-6491288476) |
| 3 요청 | 08:02:02 | `0e8a4212ca2fadc1f49b295508e7d58e0d7a3d68` | 08:17:02 기한 초과 | 15분 내 완료 리뷰가 없어 실패. 저장된 이력에서 명령으로 만들 수 없는 객체 종류·도형 종류 변경을 거절하는 요건도 새 구현에서 검증한다. | [요청](https://github.com/EunhakChoi/chemifree/pull/33#issuecomment-6491288912), [리뷰 상태](https://github.com/EunhakChoi/chemifree/pull/33#issuecomment-6490337177) |

세 head의 CI는 성공했지만 리뷰 조건을 만족하지 못해 08:17:51에 병합 없이 [종료](https://github.com/EunhakChoi/chemifree/pull/33#issuecomment-6491585846)했다. 마지막 head의 [CI](https://github.com/EunhakChoi/chemifree/actions/runs/40839283811)도 성공했다. 후속 P11은 최신 main에서 새 브랜치로 독립적으로 작성하며 #33 구현을 가져오지 않는다.

## #34 P11 재구현 — 장면 트리와 선택 이력(병합)

시간은 UTC다. [#34](https://github.com/EunhakChoi/chemifree/pull/34)는 main `b1ccdfb`에서 시작한 `feat/scene-tree`의 중단된 새 구현을 이어받았으며, #33의 구현을 가져오지 않았다.

| 회차 | 요청 시각 | 대상 head | 완료 시각 | 결과와 처리 | 링크 |
|---|---|---|---|---|---|
| 1 자동 | 2026-10-30 14:51:50 | `0f272a8ded93ccc0ec00edae2217fd68cecbbf2e` | 14:54:54 | P2 한 건: 참조 소유 객체와 그 원자·결합 대상을 함께 삭제할 때 대상이 먼저 삭제되어 `reference_in_use`로 거절됨. 브라우저에서 수정 전 422를 재현하고, 선택한 소유 객체를 먼저 삭제하도록 바꾼 뒤 삭제와 Undo 복원을 검사했다. | [리뷰](https://github.com/EunhakChoi/chemifree/pull/34#pullrequestreview-5714113392), [지적](https://github.com/EunhakChoi/chemifree/pull/34#discussion_r4424973120) |
| 2 요청 | 15:01:02 | `d117e335ef2143eaed1ea2c6c415a80bcc64364e` | 15:04:33 | 통과: 이 head의 코드 리뷰가 완료됐고 지적과 미해결 지적이 없다. 첫 회차 스레드는 수정과 검사 후 해결했다. | [요청](https://github.com/EunhakChoi/chemifree/pull/34#issuecomment-6525896757), [완료](https://github.com/EunhakChoi/chemifree/pull/34#issuecomment-6525972986) |

첫 head의 [PR CI](https://github.com/EunhakChoi/chemifree/actions/runs/41075424089)와 최종 head의 [PR CI](https://github.com/EunhakChoi/chemifree/actions/runs/41076751554)는 check·browser 모두 성공했다. 별도 사전 검증에서는 선택 변경 직후 화면 렌더 전 Delete가 들어오면 이전 렌더의 선택으로 일부만 삭제되는 문제를 재현했다. 명령을 내리는 순간의 선택을 읽도록 수정하고 브라우저 회귀 검사에 추가했다. 15:05:07에 `9efa7cbb435144f446716ed28e93e2f051e020a1`로 병합했고, 병합 후 [main CI](https://github.com/EunhakChoi/chemifree/actions/runs/41077425990)도 check·browser 모두 성공했다. 오래된 초안의 원본 바이트 보관은 별도 P11 후속 변경에서 다룬다.

## #35 P11 후속 — 이전 복구 초안 보관과 동시성

시간은 UTC다. [#35](https://github.com/EunhakChoi/chemifree/pull/35)는 #34의 병합 main에서 시작했다.

| 회차 | 요청 시각 | 대상 head | 완료 시각 | 결과와 처리 | 링크 |
|---|---|---|---|---|---|
| 1 자동 | 2026-10-30 15:43:29 | `34ba7370c31e9eb4d925bc5dc8f9e8d7dd12af2e` | 15:43:33 | 실제 코드 리뷰 서비스 오류로 완료되지 않아 실패로 센다. | [상태](https://github.com/EunhakChoi/chemifree/pull/35#issuecomment-6526872497) |
| 2 요청 | 15:43:42 | `34ba7370c31e9eb4d925bc5dc8f9e8d7dd12af2e` | 15:46:09 | P2 한 건: 초안을 가져오거나 보관하는 동안 다른 프로세스가 저장하면 오래된 기준의 복구가 성공할 수 있다. 수용: 두 지점에 제어 가능한 대기를 둔 회귀 검사가 수정 전 실패함을 확인하고, provider가 저장과 같은 잠금 안에서 기준 리비전을 검사하게 했다. 실제 파일 provider 검사와 이 검사가 누락된 변이의 실패도 확인했다. | [요청](https://github.com/EunhakChoi/chemifree/pull/35#issuecomment-6526874577), [지적](https://github.com/EunhakChoi/chemifree/pull/35#discussion_r4425520660) |

첫 head의 [PR CI](https://github.com/EunhakChoi/chemifree/actions/runs/41083338191)는 check·browser 모두 성공했다. 이 기록 시점에 마지막 리뷰는 아직 요청하지 않았다.
