# 기능·공개 선언·검증 사례 추적

이 문서는 공개 기능 명세의 요구사항과 공개 선언이 Chemifree의 어떤 구현·검증·후속 계획에 연결되는지 설명한다. 대상은 이 저장소이며 다른 프로그램과의 동등성을 판정하지 않는다. 사례·부분·선언의 개수는 화학 기능의 비율이 아니다.

## 파일

| 파일 | 내용 |
| --- | --- |
| [traceability/requirements.json](traceability/requirements.json) | 33개 요구사항. 요구사항마다 기능군(group), 명세의 연산 이름(operations), 부분(parts)별 상태·구현 위치·검증 근거·후속 계획 |
| [traceability/declarations.json](traceability/declarations.json) | 공개 선언 2,914개를 제안 경로(route) 8개로 묶은 개수·근거 상태·후속 계획 |
| [../conformance/suite.json](../conformance/suite.json), [../conformance/cases/](../conformance/cases/) | 독립적으로 작성한 conformance 사례와 그 묶음(버전 2026.10.1) |
| [verification/conformance-report.json](verification/conformance-report.json) | 위 묶음을 실행한 기록 |
| [roadmap/implementation-roadmap.json](roadmap/implementation-roadmap.json), [.html](roadmap/implementation-roadmap.html) | PR 계획. 완료 항목에는 병합 PR·커밋·병합 후 main CI가 있다 |
| [verification/review-history.md](verification/review-history.md), [review-history-batch2.md](verification/review-history-batch2.md) | GitHub 기록으로 다시 정리한 리뷰 회차·CI 범위와 이전 보고의 정정 |

명세 파일 자체는 저장소에 넣지 않는다. 추적 파일은 명세 파일을 경로와 SHA-256으로만 가리킨다.

## 상태

부분(part)마다 다음 중 하나다. 요구사항 전체의 상태는 부분들로 정해진다(모두 구현·의도적 거절이면 implemented, 구현·일부 구현이 섞이면 partial, 그 밖에는 not_implemented).

| 상태 | 뜻 |
| --- | --- |
| implemented | 구현했고, 적힌 근거(사례·테스트·smoke)가 그것을 확인한다 |
| partial | 일부만 구현·확인했다 |
| not_implemented | 제품에 없다. 반드시 후속 계획(followups)을 적는다 |
| unverified | 코드는 있지만 확인하는 검사가 없다(현재 해당 없음) |
| intentionally_rejected | 제품이 의도적으로 하지 않는다(예: 원본 실행 파일을 대신 실행하기, 없는 서비스의 성공 응답 흉내) |

현재(2026-10-29): 33개 중 partial 9개(MODEL-01, MODEL-02, OPS-01, OPS-02, CHEM-02, RENDER-01, API-01, QUALITY-01, CLEANROOM-01), not_implemented 24개. 화학 계산·형식 입출력·배치·명명·예측은 전부 not_implemented다. OPS-01의 구현 부분은 문서 목록 관리이며 이 요구사항의 선택 모델은 아니다.

공개 선언은 모두 제안 경로에 배정됐지만 개별 PR에 배정되지 않았고(to_single_plan_entry 0), 구현된 선언도 없다. 웹·HTTP·CLI의 문서 편집은 선언된 automation 멤버를 구현한 것이 아니다.

## conformance 실행 기반

`crates/conformance`가 사례 파일(`chemifree-conformance/1`)을 읽어 공유 Application을 HTTP·CLI 어댑터와 같은 호출로 실행한다. 사례마다 새 임시 데이터 디렉터리를 쓰고, `restart`는 같은 디렉터리로 Application을 새로 만들어 프로세스 재시작처럼 저장소에 남은 것만 보게 한다. 기대값은 v1 JSON 응답·오류 envelope의 부분집합이다(배열은 길이까지 같아야 하고, `$contains`는 순서 없이 포함만 확인, 숫자는 값으로 비교하되 정수끼리는 정확히 비교).

- 결과는 passed·failed·not_run이고, 사례 종류(accept: 결과를 내야 함, reject: 의도적으로 거절해야 함)와 함께 센다. reject 사례는 오류를 기대하는 단계가 없으면 묶음을 불러올 때 거절된다.
- 아직 실행할 수 없는 사례는 `pending`에 후속 계획을 적고 not_run으로 남는다. 구현되면 단계를 채워 실행 사례로 바꾼다.
- 알 수 없는 단계 필드·묶이지 않은 문서 이름·중복 ID·잘못된 요구사항 ID는 실행 전에 거절한다.

현재 기록: accept 6개·reject 4개 통과, 실행 못 함 5개(P09·P10·P13·P19·P20 대기), 실패 0개.

~~~bash
npm run test:conformance      # 실행(실패가 있으면 종료 코드 1)
npm run conformance:record    # docs/verification/conformance-report.json 갱신
npm run test:traceability     # 추적표·사례·기록·로드맵의 일관성
CHEMIFREE_SPEC_DIR=<명세 작업 공간> npm run test:traceability   # 명세 파일 해시·요구사항 ID·경로별 선언 수까지 대조
~~~

`npm run check`(CI 포함)는 test:conformance와 test:traceability를 실행한다. 명세 작업 공간이 없는 CI에서는 명세 대조를 하지 않는다.

## 갱신 규칙

- 기능을 구현하는 PR은 해당 부분의 상태·구현 위치·근거를 바꾸고, 가능하면 pending 사례를 실행 사례로 바꾼다. 근거 없이 implemented로 바꾸지 않는다.
- 사례가 실패하면 기록을 갱신하지 않고 원인을 고친다. 실패를 숨기려고 기대값을 느슨하게 만들지 않는다.
- 로드맵을 바꾸면 HTML에 같은 JSON을 다시 넣는다(test:traceability가 확인).
