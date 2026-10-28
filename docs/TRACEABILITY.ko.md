# 요구사항·공개 선언 추적과 적합성 검사

이 문서는 공개 기능 명세의 요구사항 33개와 공개 선언 inventory를 Chemifree의 실제 구현·검사·로드맵에 잇는 방법을 설명한다. 추적표는 현재 제품(드로잉 편집·저장 slice)의 상태만 기록한다. 화학 계산·파일 형식·배치·명명·예측은 구현되지 않았으며, 어떤 표기도 다른 프로그램과의 동등성을 뜻하지 않는다.

## 파일

| 파일 | 내용 |
| --- | --- |
| `docs/traceability/requirements.json` | 요구사항 33개. 각 요구사항은 부분(part)으로 나뉘고, 부분마다 상태·구현 경로·증거·후속 로드맵 항목이 있다. 요구사항마다 아직 다루지 않은 나머지를 담는 catch-all 부분(`catch_all: true`)이 정확히 하나 있다. `operations`는 명세의 연산 이름 중 이 요구사항이 닿는 것을 이 프로젝트가 분류한 것이다. |
| `docs/traceability/declarations.json` | 공개 선언 2914개를 제안 경로(route) 8개별로 센 수, 출처별·증거 상태별 수, 경로별 상태와 후속 항목. |
| `conformance/suite.json`, `conformance/cases/*.json` | 버전 있는 적합성 suite와 독립 작성 사례. |
| `crates/conformance` | 적합성 runner(라이브러리 + `chemifree-conformance` 바이너리). |
| `docs/verification/conformance-report.json` | 커밋된 실행 기록. |
| `scripts/traceability-check.mjs` | 추적 검사(`npm run test:traceability`). |
| `scripts/clean-room-check.mjs` | 공개 저장소 검사(`npm run test:clean-room`). |
| `scripts/repository-checks.test.mjs` | 두 검사의 테스트(`npm run test:checks`). |
| `docs/roadmap/implementation-roadmap.json` | 후속 항목이 가리키는 로드맵. 완료 행은 `delivered {prs, merge_commit, main_ci}`를 가진다. |
| `docs/verification/review-history.md`, `review-history-batch2.md` | GitHub 기록(PR·리뷰·CI 실행)으로 다시 정리한 리뷰 회차와 CI 범위, 이전 완료 보고의 정정. 제품 동작을 검증한 기록이 아니다. |

명세 파일은 저장소에 복사하지 않는다. `sources`(declarations.json은 `source`)에 명세 작업 공간 기준 경로와 SHA-256만 남기고, 거기서 얻은 ID·개수·직접 쓴 짧은 요약만 기록한다. requirements.json은 `role`이 `requirements`·`operations`인 출처를, declarations.json은 `role`이 `declarations`인 출처를 정확히 하나씩 가져야 하며, 각각 `spec/` 아래 상대 경로와 64자리 SHA-256이 있어야 한다. 이 형식 검사는 명세가 없는 CI에서도 실행된다.

## 상태와 도출 규칙

부분의 상태:

- `implemented`: 그 부분이 말하는 동작이 제품에 있고, 증거가 그 동작을 실제로 실행한다.
- `partial`: 일부만 있다. 후속 항목이 필요하다.
- `not_implemented`: 없다. 후속 항목이 필요하다.
- `unverified`: 있을 수 있으나 그 동작을 실행하는 증거가 없다.
- `intentionally_rejected`: 제품이 하지 않기로 결정한 것이다. 그 부분은 비어 있지 않은 `reason`을 가져야 한다.

요구사항의 상태는 저장하지만 부분에서 다음 규칙 하나로 도출한 값과 같아야 한다.

1. 모든 부분이 `intentionally_rejected`이면 `intentionally_rejected`.
2. 모든 부분이 `implemented` 또는 `intentionally_rejected`이면 `implemented`.
3. 그 밖에 `implemented`나 `partial` 부분이 하나라도 있으면 `partial`.
4. 그 밖에 `unverified` 부분이 하나라도 있으면 `unverified`.
5. 그 밖에는 `not_implemented`.

catch-all 부분은 `not_implemented`·`unverified`만 가질 수 있다(`implemented`·`partial`·`intentionally_rejected`는 거절). 다루게 된 부분이나 하지 않기로 한 부분은 catch-all에서 떼어 자기 부분으로 만든다. catch-all은 늘 `implemented`·`intentionally_rejected`가 아니므로, catch-all이 있는 한 요구사항은 `implemented`나 `intentionally_rejected`로 도출되지 않는다. 부분이 하나도 없는 요구사항은 검사가 거절하며, 도출 규칙에서도 `intentionally_rejected`가 아니라 `not_implemented`로 센다. 요구사항이 `partial`이라는 것은 일부 부분이 구현됐다는 뜻이지 요구사항 대부분이 구현됐다는 뜻이 아니다.

증거는 `종류:대상` 형식이다. `implemented`·`partial` 부분은 아래에서 "실행"인 증거가 하나 이상 있어야 한다. `report:`·`doc:`나 이름 없는 `test:`만으로는 안 된다.

| 종류 | 확인 내용 | 실행 |
| --- | --- | --- |
| `case:<id>` | suite에 있고 `requirements`에 그 부분의 요구사항을 담은 사례. `implemented`·`partial` 부분의 증거라면 커밋된 실행 기록에서 `passed`여야 한다(pending·미실행 사례는 증거가 되지 않는다). | 통과한 경우 |
| `test:<파일>.rs#<이름>` | `npm run check`가 실행하는 script 중 하나에 `&&`로 나뉜 단계 `cargo test --workspace …`가 있어야 한다. 검사는 `cargo test --workspace --locked --no-run --message-format=json`이 빌드하는 test profile 실행 파일(대상의 `src_path`로 찾음: 라이브러리는 `src/lib.rs`, 통합 테스트는 `tests/x.rs`)만 쓰고, 그 실행 파일의 `--list --format terse`에 있고 `--ignored`에는 없는 테스트만 센다. 그런 실행 파일이 없는 대상(예: `test = false`)은 "cargo test --workspace가 실행하지 않는 대상"으로 거절한다. 라이브러리 파일이면 그 파일의 모듈이나 그 아래 inline 모듈의 테스트여야 하고, 통합 테스트 파일이면 파일 자신의 테스트여야 한다(같은 디렉터리의 `y.rs`·`y/mod.rs`로 선언된 `mod y;`의 테스트는 세지 않음). 주석 처리·cfg로 빠진·`#[ignore]` 테스트는 세지 않는다. Node 테스트는 이름으로 인용하지 않고 그 파일을 실행하는 스크립트를 `check:`로 인용한다. | 예 |
| `test:<파일>` | 파일이 있다. | 아니오 |
| `smoke:<스크립트>` | 파일이 있고 `npm run check`가 (중첩된 `npm run`을 따라) 실행하는 script가 그 파일을 실행한다. CI workflow는 읽지 않으므로 CI job에서만 도는 스크립트(예: 브라우저 smoke)는 증거가 되지 않는다. | 예 |
| `check:<npm script>` | `npm run check`가 (중첩된 `npm run`을 따라) 실제로 실행하는 script다. | 예 |
| `report:<파일>` | 검사가 아는 커밋된 실행 기록이다. 지금은 `npm run conformance:record`가 쓰고 이 검사가 사례와 대조하는 conformance-report.json 하나뿐이다. 무엇을 실행한 증거로도 세지 않는다. | 아니오 |
| `doc:<파일>` | 문서가 있다. | 아니오 |

## 적합성 runner

- 사례는 공유 `Application`을 HTTP adapter가 넘기는 인자로 호출한다(`create`, `get`, `transact`, `undo`, `redo`, `save`, `autosave`, `recover`, `close`, `capabilities`, `restart`). HTTP는 모든 쓰기에 `request_id`를 보내므로, `request_id`가 없는 쓰기 단계는 단계 번호로 만든 ID(`conformance-step-<번호>`)를 보낸다. 재전송 사례처럼 단계가 ID를 주면 그것을 쓴다. CLI는 `request_id` 없이 호출하므로 그 경로는 이 suite가 아니라 test:contracts가 다룬다.
- 사례마다 새 임시 데이터 디렉터리를 쓴다. `restart`는 정상 종료 없이 Application을 버리고(프로세스를 강제 종료한 것과 같다) 같은 디렉터리에서 새로 만든다. 저장소에 있는 것만 남는다. HTTP 서비스의 정상 종료는 미저장 편집을 먼저 초안으로 쓰지만 `restart`는 그러지 않는다.
- 사례는 요구사항 ID, 축(`graph`, `scene`, `stereo`, `numeric`, `encoding`, `host`, `history`, `revision`, `idempotency`, `persistence`, `validation`, `capability`, `contract`), 종류를 가진다. `accept`는 결과를 보여야 하고 `reject`는 의도된 거절을 보여야 한다. 결과는 passed·failed·not_run으로 종류별로 센다.
- 아직 실행할 수 없는 사례는 `pending: {followup, reason}`만 가지며 단계가 없고, 그 후속 항목과 함께 not_run으로 보고된다.
- 기대값은 v1 JSON 응답의 부분집합이다. 객체는 적은 키만, 배열은 같은 길이로 원소별로 비교한다. `{"$contains": [...]}`는 순서와 무관하게 서로 다른 원소가 각 항목과 맞으면 통과한다. 이 연산자 객체는 다른 키를 가질 수 없고 값은 목록이어야 한다(읽을 때 거절하고, 실행 중 만나도 실패). 정수는 정확히(2^53 위에서도 반올림 없이), 실수는 정수와 값으로 비교한다.
- 읽기는 다음을 거절한다: 호출이 받지 않는 단계 필드, `request_id`를 받지 않는 호출(get·capabilities·restart)의 `request_id`, `create`가 묶기 전에 쓰인 문서, 중복 사례 ID, 형식이 틀린 요구사항 ID, 단계가 있는 pending 사례, 단계 없는 실행 사례, 오류를 기대하는 단계가 없는 `reject` 사례, 성공 응답을 기대하는(`expect`) 단계가 없는 `accept` 사례, 객체가 아닌 `expect`, `code`(문자열)가 없는 `expect_error`, 어느 깊이에서든 빈 객체(`{"document": {}}` 등)나 빈 `$contains`, `api_version` 말고는 비교하는 값(문자열·숫자·불리언·`null`·`[]`)이 하나도 없는 기대값, `restart`의 기대값, 쓰인 문자열 그대로 보아 절대 경로이거나 드라이브 접두어가 있거나 빈·`.`·`..` 구간(`/`·`\` 기준)이 있는 사례 경로. 그래서 종류 규칙은 아무것도 확인하지 않는 기대값으로 채울 수 없다. `null`과 `[]`는 값이 없음·비어 있음을 확인하므로 허용한다.
- 결과마다 사례 파일 바이트의 SHA-256을 기록한다(`report_version` 2). 줄바꿈 변환으로 바이트가 달라지지 않도록 `.gitattributes`가 `conformance/**/*.json`을 LF로 고정한다.
- 바이너리: `chemifree-conformance [SUITE] [--out PATH]`. `--out`이 있으면 보고서를 그 파일에만 쓰고, 없으면 표준 출력에 쓴다. 값 없는 `--out`, 알 수 없는 옵션, 두 번째 suite 경로는 사용 오류(종료 코드 2)이며, 실패한 사례나 읽을 수 없는 suite는 종료 코드 1이다. 보고서에는 시각·경로가 없어 같은 suite와 제품이면 같은 내용이 나온다.

## 실행

~~~bash
npm run test:conformance     # target/conformance/report.json에 새로 실행, 실패가 있으면 종료 코드 1
npm run conformance:record   # docs/verification/conformance-report.json을 다시 씀
npm run test:checks          # 두 검사 스크립트의 테스트
npm run test:traceability
npm run test:clean-room
npm run check                # 위 검사를 모두 포함

# 로컬 명세 작업 공간이 있을 때(그 루트, spec/을 담은 디렉터리)
CHEMIFREE_SPEC_DIR=<명세 작업 공간> npm run test:traceability
CHEMIFREE_SPEC_DIR=<명세 작업 공간> npm run test:clean-room
~~~

추적 검사는 다음을 확인한다.

- 요구사항 상태 = 부분에서 도출한 상태, 요구사항마다 catch-all 부분 하나.
- 모든 구현 경로가 있고, 모든 증거가 위 표대로 해석된다.
- `not_implemented`·`partial` 부분은 후속 항목이 있고 `implemented` 부분은 없다. 후속 항목은 모두 로드맵에 있고 완료가 아니다. 완료된 로드맵 행은 `delivered {prs, merge_commit, main_ci}`를 가진다.
- 커밋된 실행 기록이 현재 사례와 정확히 같다(ID·제목·요구사항·축·종류·pending 사유·순서, 사례 파일 SHA-256, pending ⇔ not_run). 기록된 수가 결과를 다시 센 수와 같고 실패가 없다.
- `npm run test:traceability`는 `target/conformance/report.json`(새 실행 결과)을 요구한다. 없으면 실패하고 `npm run test:conformance`를 먼저 실행하라고 알린다. `npm run check`는 바로 앞에서 test:conformance를 실행한다. 새 실행 결과의 사례 SHA-256이 현재 사례 파일과 같아야 하고(사례를 고친 뒤의 오래된 실행이면 실패), 커밋된 기록과 같아야 한다. 검사 함수를 테스트에서 직접 부를 때는 이 요구를 끈다.
- 사례 경로는 Rust와 같은 규칙(쓰인 문자열의 구간 검사)으로 거절하고 suite 파일의 디렉터리 기준으로 해석한다.
- 사례의 요구사항 ID가 추적표에 있고, pending 후속 항목이 열려 있다.
- 선언 수의 합이 맞고 경로 상태가 구현 수와 맞는다. 경로마다 `0 ≤ implemented ≤ assigned_to_single_plan_entry ≤ declarations`인 정수다. 선언은 계획 항목 하나에 배정된 뒤에만 구현으로 셀 수 있다.
- 아래 현재 집계 문장과 HANDOFF.ko.md의 같은 문장이 도출값과 정확히 같다. 다르면 붙여 넣을 문장을 출력한다.
- `CHEMIFREE_SPEC_DIR`가 있으면 명세 파일의 SHA-256, 요구사항 ID 33개와 영역, 연산 이름, 경로·출처·증거 상태별 선언 수를 명세에서 다시 센 값과 비교한다. CI처럼 없으면 이 비교만 건너뛴다.

저장소 루트는 스크립트 위치에서 `fileURLToPath`로 구하므로 공백이 있는 경로에서도 동작한다.

## 갱신 규칙

- 기능을 구현하는 PR은 해당 부분을 나누거나 상태를 바꾸고, 그 동작을 실행하는 증거(통과한 사례·테스트·검사)를 함께 넣는다. 증거 없이 `implemented`로 바꾸지 않는다.
- 사례를 추가·변경하면 `npm run conformance:record`로 실행 기록을 다시 쓰고 함께 커밋한다(사례 파일의 SHA-256이 기록과 다르면 검사가 실패한다). 사례 형식을 바꾸면 `suite_version`을, 보고서 형식을 바꾸면 `report_version`을 올린다.
- pending 사례는 그 후속 항목이 구현될 때 단계를 가진 실행 사례로 바꾼다. 실행할 수 없는 사례를 통과로 세지 않는다.
- 로드맵 행이 완료되면 그 행을 후속 항목으로 쓰는 부분을 정리한다(완료된 행은 후속 항목이 될 수 없다).
- 상태가 바뀌어 집계가 달라지면 검사가 출력하는 문장을 이 문서와 HANDOFF.ko.md에 그대로 붙여 넣는다.
- 명세 파일이 바뀌면 `CHEMIFREE_SPEC_DIR`로 검사를 돌려 해시·수를 확인하고, 이 프로젝트의 판단(상태·후속 항목)을 다시 검토한다.

## 현재 집계

현재 집계: 요구사항 33개 — implemented 0, partial 7, not_implemented 26, unverified 0, intentionally_rejected 0 (partial: MODEL-01, MODEL-02, OPS-01, OPS-02, API-01, QUALITY-01, CLEANROOM-01). 부분 항목 47개 — implemented 13, partial 1, not_implemented 33, unverified 0, intentionally_rejected 0. 공개 선언 2914개 중 구현 0개, 단일 계획 항목 배정 0개. 적합성 사례 17개 — accept 통과 8·실패 0·미실행 4, reject 통과 4·실패 0·미실행 1.

partial인 7개는 드로잉 slice가 실제로 다루는 부분 때문이다: 안정 ID와 참조 무결성(MODEL-01), 그린 그대로의 원소·전하·동위원소 기록과 범위 거절(MODEL-02), 원자 삭제 시 연결 결합 제거(OPS-01), 원자적 트랜잭션·revision·Undo/Redo·저장/복구(OPS-02), v1 응답·오류 envelope·request_id 재전송·capability 보고(API-01), 적합성 runner와 회귀 검사(QUALITY-01), 공개 저장소 검사와 명세 참조 방식(CLEANROOM-01). 각 요구사항의 나머지(그래프/장면 분리, 수소·라디칼·질의, 그룹·포함, 연산별 버전 계약, 차등 비교, DATA reader 등)는 catch-all 부분에 후속 항목과 함께 남아 있다.

## 한계

- 선언 수는 기능 수가 아니다. 2914개에는 collection·메뉴·availability·상태 getter가 포함되며, 경로는 설계 분류일 뿐이다. 아직 단일 계획 항목에 배정된 선언은 0개이고 구현된 선언도 0개다. 경로의 후속 항목은 그 경로를 다룰 것으로 보는 로드맵 항목이며, 각 항목이 어떤 선언을 구현하는지는 그 PR에서 정한다.
- 통과한 사례는 사례가 확인하는 것만 보여 준다. 드로잉 편집·저장 동작의 검사이며 화학적 정확성이나 다른 프로그램과의 호환성 증거가 아니다.
- 상태와 후속 항목 배정은 이 프로젝트의 판단이다. 검사는 형식·참조·일관성을 강제하지만 판단이 옳은지는 리뷰가 확인해야 한다.
- clean-room 검사는 Git이 추적하는 파일의 경로·바이트·단어를 본다. 작업 트리가 아니라 index의 이름(바이트 그대로, 탭 포함)과 blob 내용을 읽으므로 작업 트리에서 바꾸거나 지운 파일도 커밋될 내용으로 검사하고, 올바른 UTF-8이 아닌 이름은 거절한다. 복사본과 산출물(로컬 전용 경로, 실행 파일, 큰 파일, 명세가 기술하는 프로그램의 이름, 명세 파일과 바이트가 같은 파일)은 잡지만, 바꿔 쓰거나 다시 인코딩하거나 나눈 비공개 자료는 잡지 못한다. 빈 파일(0바이트)은 명세 파일과 비교하지 않는다. 이름은 소문자 단어의 SHA-256으로만 저장되어 있어 검사 파일에도 이름이 없으며, 영문자 단어 단위로(제품 이름은 더 긴 단어 안에서도) 비교한다. 띄어 쓰거나 다른 문자로 바꾼 표기는 잡지 못한다.
- 명세 비교는 `CHEMIFREE_SPEC_DIR`가 있을 때만 실행된다. CI에는 명세가 없으므로 CI 통과는 해시·ID·수가 명세와 같다는 증거가 아니다.
