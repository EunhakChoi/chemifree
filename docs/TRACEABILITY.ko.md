# 요구사항·공개 선언 추적과 conformance 실행

이 문서는 P08이 더한 추적 파일, conformance runner와 추적 검사가 무엇을 기록하고 무엇을 확인하는지 정확히 적는다. 여기 적힌 확인 범위를 넘는 성질(화학적 정확성, 다른 프로그램과의 동등성, 기능 완성도)은 이 장치들로 확인되지 않는다.

현재 집계: 요구사항 33개(partial 7, not_implemented 26) · 부분 50개(implemented 13, partial 1, unverified 2, not_implemented 34) · conformance 사례 22개(passed 19: accept 10·reject 9, not_run 3, failed 0) · 공개 선언 2914개(구현 0, 단일 계획 할당 0)

위 줄은 추적 파일과 기록된 실행에서 계산한 값이며, `npm run test:traceability`가 이 문서와 HANDOFF.ko.md에 정확히 같은 줄이 한 번씩 있는지 확인한다.

## 파일

| 파일 | 내용 |
| --- | --- |
| `docs/traceability/requirements.json` | 공개 준비 요구사항 33개. 요구사항마다 자체 요약, 관련 연산 이름(operations.json의 이름, 자체 대응), 부분(part) 목록과 부분에서 도출한 상태. 출처 두 개(`requirements`, `operations`)를 `spec/` 아래 상대 경로와 SHA-256으로만 적는다 |
| `docs/traceability/declarations.json` | 공개 선언 2,914개를 제안 경로(route) 8개별로 센 수, 출처별 수, 경로별 상태·구현 수·단일 계획 할당 수·후속 로드맵 행. 선언 수는 기능 수가 아니다(collection·메뉴·상태 getter 포함) |
| `conformance/suite.json`, `conformance/cases/*.json` | 사례 묶음(`suite_version`)과 사례(실행 가능 또는 대기(pending)). 개수는 위 집계 줄에 있다 |
| `crates/conformance` | runner(lib)와 실행 파일 `chemifree-conformance` |
| `docs/verification/conformance-report.json` | 커밋된 실행 기록. 시각을 담지 않으며, 실패한 사례가 없는 실행은 같은 바이트를 쓴다 |
| `scripts/traceability-check.mjs`, `scripts/traceability-check.test.mjs` | 추적 검사와 주요 규칙의 테스트(모든 규칙에 테스트가 있지는 않다) |
| `docs/roadmap/implementation-roadmap.json` | 로드맵(입력, 이 작업에서 수정하지 않음). 후속 계획은 이 파일의 행 ID다 |

명세 파일은 저장소 밖(로컬)에 있고 복사하지 않는다. 추적 파일에는 경로·SHA-256과, 명세에서 도출한 ID·개수·자체 요약만 있다.

## 상태와 도출 규칙

부분 상태는 `implemented`, `partial`, `unverified`, `not_implemented`, `intentionally_rejected` 중 하나다. 이 상태는 기록한 판단이며, 검사는 판단이 옳은지가 아니라 아래 형식·연결·일관성만 확인한다.

요구사항 상태는 부분 상태에서 도출하며 적힌 값과 같아야 한다.

1. 모든 부분이 `intentionally_rejected` → `intentionally_rejected`
2. 모든 부분이 `implemented` 또는 `intentionally_rejected` → `implemented`
3. `implemented`나 `partial` 부분이 하나라도 있으면 → `partial`
4. 그 밖에 `unverified` 부분이 있으면 → `unverified`
5. 그 밖 → `not_implemented`

부분이 없는 요구사항은 거절한다. 요구사항마다 다루지 않은 나머지를 담는 catch-all 부분(`catch_all: true`)이 정확히 하나 있고, 그 상태는 `not_implemented`나 `unverified`만 될 수 있다. `intentionally_rejected` 부분은 비지 않은 `reason`이 필요하다(다른 상태는 `reason`을 가질 수 없다). `implemented`·`partial` 부분은 존재하는 구현 경로 하나 이상과 실행되는 근거 하나 이상이 필요하다. `not_implemented`·`unverified`·`partial` 부분은 후속 계획이 필요하고 `implemented` 부분은 후속 계획이 없다. 모든 후속 계획은 완료(done)가 아닌 로드맵 행이다.

이 버전에서는 모든 요구사항이 catch-all을 하나 가져야 하고 catch-all은 `not_implemented`나 `unverified`이므로, 위 규칙에서 요구사항 상태는 `implemented`나 `intentionally_rejected`가 될 수 없고 최대 `partial`이다. 요구사항이 catch-all 없이 끝날 수 있게 하는 변경은 검사도 함께 바꿔야 한다.

공개 선언 경로의 상태는 구현 수로 정해진다. 구현 0이면 `not_implemented`나 `unverified`, 전부면 `implemented`, 그 사이면 `partial`. 현재 모든 경로가 구현 0, 단일 계획 할당 0이다.

## 근거는 두 종류뿐이다

| 근거 | 실행되는 근거로 인정하는 조건 | 확립하는 것 |
| --- | --- | --- |
| `case:<사례 ID>` | 사례가 묶음에 있고, 그 사례의 `requirements`가 이 부분의 요구사항을 포함하고, 기록된 실행에서 `passed`. 검사는 새 실행(`target/conformance/report.json`)이 기록과 바이트 단위로 같은지도 확인한다 | 그 사례의 단계가 비교하는 값들이 이 제품에서 그 실행 때 기대대로였다는 것. 사례가 비교하지 않는 성질은 확립하지 않는다 |
| `check:<npm 스크립트 이름>` | 아래 도달 규칙으로 `check`에서 도달한 스크립트 | `npm run check`가 그 스크립트를 호출한다는 것(앞선 단계가 성공했을 때). 그 스크립트가 무엇을 검사하는지, 이 부분과 관련된 것을 검사하는지는 확립하지 않는다 |

구현 경로는 근거가 아니며 작업 트리에 존재하는지만 확인한다(Git 추적 여부나 관련성은 확인하지 않는다). 대기 사례(`not_run`)는 근거가 될 수 없다. 이 밖의 근거 형식은 거절한다.

`check:` 도달 규칙: `&&`로 나눈 모든 조각이 정확히 `npm run <이름>`(낱말 셋, 이름은 `[A-Za-z0-9:_.@/-]`)인 스크립트만 "순수"하다. 순수한 스크립트가 가리키는 이름은 호출된 것으로 세고, 그 이름도 순수하면 다시 따라간다. 순수하지 않은 스크립트는 잎(leaf)이다: 호출된 것으로 세지만 그 안에서 무엇을 실행하는지는 읽지 않는다(`exit 0`, `cd`, `exec`, `npm run x -- --flag`, `npm run x --workspace w` 등을 해석하지 않는다). `check` 자체가 순수하지 않으면 아무 스크립트도 도달한 것으로 세지 않는다. npm이 암묵적으로 실행하는 `pre<이름>`·`post<이름>` 스크립트가 `check`나 도달한 스크립트에 있으면 오류다.

## conformance runner

사례 하나는 `id`, `title`, `requirements`, `axis`, `kind`(`accept`|`reject`)와 `steps` 또는 `pending {followup, reason}` 중 하나를 가진다. 단계는 공유 Application을 HTTP 핸들러와 같은 요청 타입(같은 DTO로 본문을 역직렬화)과 같은 인자로 호출하고, 모든 쓰기에 `request_id`를 보낸다. HTTP 계층(상태 코드, Host·Origin·토큰 검사, JSON 거절 응답)은 거치지 않으며 상태 코드는 비교하지 않는다. 호출: `create`, `get`, `transact`, `undo`, `redo`, `save`, `autosave`, `recover`, `close`, `capabilities`, `restart`.

- 모든 쓰기는 `request_id`를 보낸다. 단계에 없으면 `step-<단계 번호>`를 만든다(`[A-Za-z0-9_-]`만 사용). `get`·`capabilities`·`restart`에 `request_id`가 있으면 거절한다.
- 문서는 `create`의 `bind` 이름으로 가리킨다. 바인딩 전 사용, 두 번 바인딩, 오류를 기대하는 `create`의 바인딩은 거절한다.
- `restart`는 Application을 정상 종료 절차(`draft_all`) 없이 버리고 같은 임시 데이터 디렉터리로 API 시작 때처럼 다시 연다. 프로세스 충돌처럼 저장·초안에 없는 편집은 남지 않고 열린 파일 핸들(잠금)은 풀린다. 같은 프로세스 안의 재시작이며 실제 프로세스 강제 종료는 `npm run test:contracts`가 따로 다룬다. `restart`에는 기대값을 둘 수 없다.
- API 프로세스가 돌리는 백그라운드 초안 작성기(DraftWriter)는 runner에서 실행하지 않는다. 초안은 `autosave` 단계로만 쓰인다. 따라서 "저장·초안 없는 편집은 충돌 뒤 사라진다"는 사례는 그 편집이 실제 서비스에서 초안 주기(최대 약 10초)보다 먼저 충돌했을 때에 해당한다.
- 사례마다 새 임시 데이터 디렉터리를 쓴다.
- 불러올 때 거절: 호출별 알 수 없는 필드(명시적으로 확인), 중복 JSON 키, 사례 ID 중복, 잘못된 사례·요구사항·로드맵 ID, 빈 제목, 단계가 있는 pending, 단계 없는 실행 사례, `expect`와 `expect_error`를 함께 가진 단계, 알 수 없는 축, 요청 본문 DTO가 거절하는 본문.
- 기대값: `expect`는 JSON 객체, `expect_error`는 문자열 `code`를 가진 객체. 어느 깊이에서든 빈 객체, 비었거나 목록이 아닌 `$contains`, 알 수 없는 `$` 연산자, 옆에 다른 키가 있는 `$contains`를 거절한다. `api_version` 말고 비교하는 값이 하나 이상 있어야 한다(`null`과 `[]`도 값). 크기 2^53 이상의 실수는 정수가 반올림됐을 수 있으므로 거절한다.
- 일치 규칙: 객체는 부분집합, 배열은 같은 길이로 원소별, `$contains`는 서로 다른 원소에 대응(최대 이분 매칭, 원소 재사용 없음), 정수는 반올림 없이 정확히, 실수와 정수는 정확히 같은 값일 때만 같다. 잘못된 기대값은 실행 중에도 일치하지 않는다.
- `operation` 단계는 연산 계약 1의 요청(`request`, JSON 객체 또는 바이트 그대로 보낼 문자열)을 HTTP·CLI와 같은 `Application::execute`에 넘긴다. `document`가 있으면 묶인 문서 ID를 `parameters.document_id`에 넣고(요청에 `parameters` 객체가 있어야 함), `bind`가 있으면 결과 문서를 그 이름에 묶는다. 요청이 자기 `request_id`를 가지므로 단계의 `request_id`는 거절한다. 연산은 결과가 무엇이든 연산 응답으로 답하므로 `expect_error`는 거절하고, `expect`에 문자열 `outcome`이 있어야 한다.
- kind 규칙: 실행 가능한 `reject` 사례는 `expect_error` 단계가, `accept` 사례는 (`restart`가 아닌 호출의) `expect` 단계가 하나 이상 있어야 한다. `operation` 단계는 기대하는 `outcome`으로 센다: `ok`·`partial`은 결과, `error`·`unsupported`는 거절. 결과는 kind별로 센다.
- 사례 경로는 묶음 파일 기준 상대 경로이며, 원래 문자열을 `/`와 `\`로 나눠 빈 조각·`.`·`..`가 있거나, 절대 경로이거나, 드라이브 접두(`C:`)가 있으면 거절한다. Rust와 JS가 같은 규칙을 쓴다.
- 실행 파일: `chemifree-conformance [--out PATH] SUITE`. 값 없는(또는 `-`로 시작하는 값의) `--out`, 두 번째 `--out`, 알 수 없는 옵션, 묶음 없음·두 번째 묶음은 실행 없이 사용법 오류(종료 2). 실패한 사례가 있으면 종료 1(기록은 씀), 묶음을 불러오지 못하면 종료 1(기록 없음). `--out`이 없으면 기록을 표준 출력에 쓴다.
- 기록: 결과마다 `id`, `title`, `requirements`, `axis`, `kind`, `outcome`(`passed`|`failed`|`not_run`), `pending`, `failure`, 사례 파일 바이트의 SHA-256, 그리고 kind별·전체 개수. 시각은 기록하지 않는다. 실패한 사례가 없는 실행은 같은 바이트의 기록을 쓴다. `failure` 문구에는 응답 JSON(`saved_at_ms`, 생성된 문서 ID)과 저장소 오류 메시지가 그대로 들어가므로 실행마다 다른 값과 데이터 디렉터리 경로가 포함될 수 있다(커밋하는 기록에는 실패가 없어야 한다).
- 애플리케이션이 panic하면 runner 프로세스가 중단된다(종료 101, 기록 없음). 모든 결과가 기록된다는 보장은 runner가 끝까지 실행됐을 때만 성립한다.

## 검사가 확인하는 것

`npm run test:traceability`(`scripts/traceability-check.mjs`)는 입력을 한 번 읽고 아래 규칙만 확인한다.

- 위의 상태 도출·catch-all·reason·구현 경로·근거·후속 계획 규칙.
- 로드맵의 완료 행마다 `delivered {prs, merge_commit, main_ci}`.
- 로드맵의 `requirement_mapping`이 추적한 요구사항마다 항목을 정확히 하나 가지고, 그 `current_status`가 requirements.json의 상태와 같음(두 파일이 어긋나지 않게).
- 모든 사례의 요구사항 ID가 requirements.json에 있고, 대기 사례의 후속 계획이 열린 로드맵 행.
- 출처: requirements.json에 `requirements`·`operations` 출처가 정확히 하나씩, declarations.json에 `declarations` 출처 하나. 각각 `spec/` 아래 상대 경로(역슬래시·빈 조각·`.`·`..` 없음)와 소문자 64자리 SHA-256. 명세 디렉터리 없이도 확인한다.
- declarations.json: 정수, 경로 합 = 전체, 출처별 합 일치, 경로마다 `0 ≤ implemented ≤ assigned_to_single_plan_entry ≤ declarations`, `about`이 선언 수가 기능 수가 아님을 말함.
- 기록: 결과가 묶음의 사례 순서대로이고 id·title·requirements·axis·kind·pending이 사례 파일과 같으며 현재 사례 파일의 SHA-256을 가짐, pending일 때만 `not_run`, `failed` 없음, 개수 = 재계산. `target/conformance/report.json`이 있어야 하고(없으면 `npm run test:conformance` 안내) 기록과 바이트 단위로 같아야 한다.
- `npm run check`가 `test:conformance`, `test:traceability`, `test:checks`를 직접 실행하고, 앞의 것을 뒤의 둘보다 먼저 실행.
- `현재 집계:` 줄이 이 문서와 HANDOFF.ko.md에 정확히 한 번씩 있고 계산값과 같음. 다르면 붙여 넣을 줄을 출력한다.
- `CHEMIFREE_SPEC_DIR`(그 아래에 `spec/`이 있는 디렉터리)가 있으면 추가로: 출처 파일의 SHA-256, 요구사항 ID 33개의 집합, 적힌 연산 이름이 operations.json에 있는지, 전체·출처별·경로별(출처별 포함) 선언 수.

## 검사가 확인하지 않는 것

- 상태 판단이 옳은지. 예를 들어 `implemented` 부분의 요약이 사례가 실제로 비교하는 것보다 넓을 수 있다. 근거 연결은 실행 여부만 보장한다.
- `check:` 스크립트가 올바른 것을 검사하는지, 이 부분과 관련된 테스트를 포함하는지.
- 사례가 다른 프로그램의 코드나 자료와 독립적으로 작성됐는지(QUALITY-01.2는 `unverified`).
- 공개 저장소에 개인 자료, 공급자 자료, 실행 엔진, 명세 사본이 없는지. 정책(AGENTS.md)과 .gitignore가 있을 뿐 이를 증명하는 자동 검사는 없다(CLEANROOM-01.2는 `unverified`, 후속 P14·P88·P89).
- `CHEMIFREE_SPEC_DIR` 없이는(CI와 `npm run check` 기본값) 출처 해시·요구사항 ID·연산 이름·선언 수가 명세와 같은지. 그래서 CLEANROOM-01.1은 `partial`이다.
- 단독으로 실행한 `test:traceability`는 runner를 실행하지 않는다. 있는 새 실행 파일을 비교할 뿐이며, `npm run check`가 그 직전에 `test:conformance`를 실행한다.
- 화학, 형식 변환, 다른 프로그램과의 동등성, 완성도.

## 실행

~~~bash
npm run test:conformance        # 새 실행 → target/conformance/report.json, 실패가 있으면 종료 1
npm run conformance:record      # 커밋할 기록 docs/verification/conformance-report.json을 다시 씀
npm run test:traceability       # 추적 검사
npm run test:checks             # 추적 검사 규칙별 테스트(node:test)
CHEMIFREE_SPEC_DIR=<spec/을 가진 로컬 디렉터리> npm run test:traceability   # 명세와도 비교
cargo test -p chemifree-conformance   # runner 단위·통합 테스트
~~~

`npm run check`는 `test:contracts` 다음에 `test:conformance`, `test:traceability`, `test:checks`를 차례로 실행한다.

## 갱신 규칙

- 사례를 더하거나 바꾸면 `npm run conformance:record`로 기록을 다시 쓰고, 바뀐 결과를 검토해 함께 커밋한다. 기록은 LF로 고정한다(.gitattributes).
- 기능을 구현하면 해당 부분의 상태·구현 경로·근거를 바꾸고 후속 계획을 지운다. 구현되지 않은 나머지는 catch-all에 남긴다. 근거는 `case:`나 `check:`만 쓴다.
- 상태를 올리기 전에 근거가 그 요약을 실제로 비교하는지 직접 확인한다. 검사는 이를 대신하지 않는다.
- 로드맵 행이 완료되면 그 행을 가리키는 후속 계획을 열린 행으로 바꾸거나 부분을 완료 처리한다(완료 행은 후속 계획이 될 수 없다).
- 명세가 바뀌면 `CHEMIFREE_SPEC_DIR`를 주고 검사를 실행해 해시·ID·개수를 다시 맞춘다. 명세 파일이나 긴 발췌는 저장소에 넣지 않는다.
- 집계가 바뀌면 검사가 출력하는 `현재 집계:` 줄을 이 문서와 HANDOFF.ko.md에 붙여 넣는다.
