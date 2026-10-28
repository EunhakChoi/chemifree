# Chemifree 새 세션 인계

이 문서는 완료된 작업과 다음 개발 경계를 전달한다. 인계 문서를 읽는 것만으로 다음 기능 구현을 시작하지 않으며, 새 세션에서 사용자가 요청한 범위를 따른다.

## 프로젝트와 현재 기준

- 이름: Chemifree. 사용자가 직접 확정했으며 이름을 다시 제안하지 않는다.
- 공개 저장소: https://github.com/EunhakChoi/chemifree
- 스캐폴딩·설계 기준 커밋: 90f2b99([CI](https://github.com/EunhakChoi/chemifree/actions/runs/40362091650)), 인계 문서 007a185.
- 그 이후 첫 수직 기능(문서 생성 → 원자·결합 편집 → Undo/Redo → 저장 → 다시 열기)을 구현했고, 이어서 저장·복구 보강(프로세스 간 저장 잠금, 복구용 자동 저장, 비정상 종료 후 복구, 사본으로 저장)을 더했다. 커밋과 CI 결과는 git log와 GitHub Actions에서 확인한다.
- HTML 설계(architecture.html)는 scaffold 코드 기준 119f477의 스냅숏이며, 상단에 이후 변경 안내를 추가했다. 현재 계약은 CONTRACT.md가 기준이다.
- 이후 문서나 코드가 변경될 수 있으므로 작업 시작 시 git status, git log와 현재 CI를 다시 확인한다. 기존 사용자 변경은 보존한다.

## 먼저 읽을 파일

1. 프로젝트 루트의 AGENTS.md.
2. 이 문서와 ../README.md.
3. CONTRACT.md: 현재 v1 계약(문서 API·명령·오류·쓰기 보호·파일 형식)과 버전 정책.
4. SHADCN_REVIEW.ko.md: UI 도구 선택, 현재 연결 상태와 정책 제안.
5. architecture.html: scaffold 기준의 레이어·소스 링크를 담은 대화형 설계안(스냅숏). 브라우저로 열 수 있다.
6. 같은 PC에서 존재한다면 private/SESSION_CONTEXT.ko.md: 승인된 공개 기능 명세의 로컬 경로와 기존 사용자 결정만 담은 보조 인계. Git에 포함하지 않는다.

## 완료한 기반

| 영역 | 실제 상태 |
| --- | --- |
| apps/web | React/TypeScript/Vite. 런타임·capability 카드와 문서 편집기: 새 문서·목록·열기, SVG 구조 캔버스(src/editor), 도구(선택·원자·결합·지우개), 원소·결합 차수, 인스펙터(원소·전하·동위원소·차수), Undo/Redo·저장·닫기(미저장 확인), 단축키, 충돌·저장 실패 안내, 파일·복구 초안 상태 표시와 자동 저장(재시도), 복구·초안 버리기·사본으로 복구 배너, 사본으로 저장(대화상자·편집 대상 안내) |
| apps/api | Axum loopback HTTP. 문서 생성·조회·트랜잭션·undo/redo·저장·닫기·autosave·save-as·recover·recover-as-copy·discard-draft. Host/Origin/Sec-Fetch-Site 검사, 프로세스별 세션 토큰, JSON·256 KiB 제한, v1 오류 envelope |
| apps/cli | status/capabilities와 documents list·create·show·apply·undo·redo·save-as·recover·recover-as-copy·discard-draft. 같은 Application을 HTTP 없이 호출, 편집마다 저장 |
| crates/domain | 원소(1–118), 문서 그래프·안정 ID·참조 불변식, 명령, 원자적 트랜잭션, change-set Undo/Redo, 저장본 복원 검증, 독립 사본(copy_as). 외부 의존성 없음 |
| crates/application | 세션(작업 사본), expected_revision 충돌, request_id 재전송, 저장 커밋 후 ACK, 자동 저장·복구(번호 일관성)·사본, 공유 DTO, DocumentRepository(초안 소유 계약 포함)·CapabilityCatalog port |
| crates/providers | 로컬 JSON 파일 저장소(임시 파일 → fsync → rename(커밋 시점) → 가능한 경우 디렉터리 sync, base revision 검사, 손상 파일 거절), 문서별 OS 저장 잠금(locks/), 복구 초안(drafts/, 소유 잠금, 커밋 중 초안 정리), capability 목록 |
| 개발 환경 | npm/Cargo workspaces, lockfiles, npm run dev/check, test:contracts, 수동 test:browser, GitHub Actions |

의존 방향은 Application → Domain, Providers → Application + Domain이다. API와 CLI가 가장 바깥에서 Application과 concrete provider를 조립한다.

HTTP 계약은 health·capabilities·session과 /api/v1/documents 아래 생성·조회·transactions·undo·redo·save·close·autosave·save-as·recover·recover-as-copy·discard-draft다. CLI는 status·capabilities·documents 하위 명령으로 같은 Application을 호출한다. 모든 JSON에는 api_version: v1이 있다. 세부 내용은 CONTRACT.md에 있다.

runtime_inspection, document_editing, persistence가 available이다. document_editing은 그린 그대로 기록하는 drawing 모델이며 원자가·암시적 수소·방향족성·입체·배치·명명·물성을 계산하지 않는다. persistence는 로컬 Chemifree JSON 파일(저장 잠금·복구 초안·사본 포함)이며 DB·동기화·공유는 없다. structure_analysis, format_conversion, local_model_loading, authentication, windows_bridge는 not_implemented다. 쓰기 보호(세션 토큰·Host/Origin)는 다른 웹사이트를 막는 장치이며 사용자 인증이 아니다.

## UI 결정

- 웹 우선이며 데스크톱 셸은 선택적 후속 확장이다.
- shadcn/ui의 공식 @shadcn registry, Base UI/Nova를 적용했다.
- 설치 컴포넌트: Alert, Alert Dialog, Badge, Button, Card, Empty, Field(Label, Separator), Input, Kbd, Skeleton, Spinner, Toggle Group(Toggle), Tooltip. 기존 button.tsx는 notice 헤더만 달라 덮어쓰지 않았다.
- 과학 렌더러는 apps/web/src/editor에 두고 일반 UI 컨트롤과 분리했다.
- 일반 UI는 semantic token과 variant를 사용한다. 과학 SVG/Canvas renderer는 별도 제품 모듈로 둔다.
- @shadcn/lint는 ESLint plugin으로 등록되어 있지만 기본/CI 디자인 규칙은 0개다. 등록 사실을 디자인 정책 강제 적용으로 설명하지 않는다.
- 임시 stdin probe에서 no-raw-colors와 no-restyle 탐지를 확인했다. 그 probe가 기본 설정을 변경하지는 않았다.
- Base UI의 내비게이션은 일반 anchor + buttonVariants를 사용한다. Button render anchor 패턴은 링크를 button 역할로 바꾸므로 재도입하지 않는다.

## 검증과 실행

Node 22.22.2, npm 11.13.0, Rust 1.95.0을 사용했다. 세부 의존성은 lockfiles를 기준으로 한다.

~~~bash
npm ci
npm run dev
# Web: http://127.0.0.1:5173
# API: http://127.0.0.1:3001

npm run check
cargo run --locked -p chemifree-cli -- capabilities --json
~~~

검증된 내용(첫 수직 기능과 저장·복구 보강):

- npm run check: lint, TypeScript, production build, rustfmt, clippy, Rust 테스트 69개(domain 20, application 25, providers 15, api 9).
- npm run test:contracts: 실제 API·CLI 프로세스와 임시 데이터 디렉터리. HTTP = CLI JSON, 쓰기 보호, 생성→편집→undo/redo→저장→닫기→다시 열기, request_id 재전송·재사용 거절, revision/save 충돌, 저장 실패 후 재시도, API 강제 종료 후 마지막 커밋 복구. 같은 기준에서 동시에 저장하는 CLI 6개(정확히 하나만 커밋), 잠금을 쥔 채 강제 종료된 프로세스 뒤의 저장, API와 CLI의 저장 경쟁, 자동 저장 → SIGKILL → 재시작 → 복구(내용·Undo/Redo·ID·리비전), 초안 버리기와 재시작 후 미재출현, 오래된 초안의 사본 복구, 손상 초안, 자동 저장 실패·재시도, HTTP·CLI 사본 저장과 독립성, 사본 저장 실패 후 원본 보존·재시도. 경쟁 재현에는 debug 빌드 전용 CHEMIFREE_TEST_COMMIT_DELAY_MS(저장 잠금 안 대기)를 쓴다. 잠금을 끈 빌드에서는 6개가 모두 커밋되어 이 검사가 실패함을 확인했다.
- npm run test:browser(수동, CI 제외): Chrome으로 편집 흐름 전체, 키보드 단축키, 미저장 닫기 확인, 충돌 Reload, 낡은 화면에서 확인한 변경 버리기 거절, 저장 실패·재시도, CLI와의 저장 충돌, 요청 처리 중 반복 입력(원자 겹침 방지·전하 클릭 누락 방지), 잘못된 동위원소 입력 거절, 거절된 편집 오류가 뒤이은 성공에 지워지지 않음, 응답을 잃은 문서 생성의 재시도(중복 생성 없음), 그 사이 API가 재시작되면 재전송하지 않고 목록 확인 안내, 자동 저장 상태, API 강제 종료 후 Reload → 복구(그림·Undo 이력), 초안 버리기(확인 대화상자), 오래된 초안의 사본 복구와 원본 열기, 자동 저장 실패·Retry now, 편집이 계속되어도 밀리지 않는 재시도, 재시작 뒤 새 서비스 기준의 초안 상태(이전 초안이 있으면 '일시정지', 버린 뒤 다시 초안 작성), 사본으로 저장(원본 파일 불변), 다른 문서로 넘어간 뒤 도착한 자동 저장 응답 무시, 문서를 떠나기 전 대기 중인 작업의 초안 작성, 다른 클라이언트가 세션을 앞서 나가도 자동 저장이 1초마다 반복되지 않음, 320/390px, dark mode. 결과는 verification/editor-browser-smoke.json.
- 브라우저 검증 중 serde_json 기본 float 파서가 다시 열 때 1 ULP 오차(226.39999999999998 → 226.4)를 내는 버그를 발견했고 float_roundtrip 기능으로 수정, 회귀 테스트를 추가했다.

scaffold 시점 기록은 verification/의 기존 JSON에 남아 있다. 화학적 정확성 검증은 수행한 상태가 아니다.

CI는 실행 중인 전역 npm을 교체하지 않는다. 별도 runner-temporary prefix에 npm을 설치하고 PATH에 추가하는 현재 방식을 유지한다.

## 알려진 한계

보장과 한계의 기준 목록은 CONTRACT.md의 "Guarantees and known limits"다. 리뷰와 변경은 그 목록에 맞춰 판단한다. 아래는 구현 관점의 보충이다.

- 작업 사본은 API 프로세스 메모리에 있다. 프로세스가 끝나면 마지막 커밋과 마지막 복구 초안이 남는다. 마지막 자동 저장 이후의 편집(보통 1–5초, 자동 저장이 실패·일시정지 중이면 더 길다)은 사라진다.
- 자동 저장은 웹 클라이언트가 호출한다. 페이지가 닫혀 있거나 autosave를 부르지 않는 HTTP 클라이언트의 편집은 초안이 없다. CLI는 편집마다 커밋하므로 초안을 쓰지 않는다.
- 저장은 문서별 게이트만 잡고 저장소(다른 프로세스의 저장 잠금 대기 포함)를 기다린다. 그동안 다른 문서 요청과 같은 문서의 편집은 진행되고, 저장은 요청한 리비전을 커밋한다. 커밋·초안을 다루는 작업(저장·자동 저장·닫기·복구·초안 버리기·사본)은 같은 문서 게이트로 직렬화된다.
- 저장 잠금과 초안 소유 잠금은 OS advisory lock(std File::lock, Unix flock)이다. Chemifree 프로세스끼리만 지켜지며 일부 네트워크 파일 시스템에서는 보장되지 않는다. locks/와 drafts/*.owner 파일은 의도적으로 지우지 않는다.
- 한 프로세스 안의 요청들은 초안 소유 잠금을 한 번에 하나씩 잡는다. OS 파일 잠금은 같은 프로세스의 다른 핸들도 거절하므로, 그러지 않으면 동시에 들어온 요청이 다른 요청의 잠깐 확인을 다른 프로세스의 소유로 오인해 버려진 초안을 Draft in use로 보였다(#8 조사 중 발견).
- 같은 데이터 디렉터리에 API 프로세스가 둘이면 같은 문서의 초안은 한쪽만 소유한다. 다른 쪽 autosave는 draft_in_use로 거절된다. 실시간 병합은 없다.
- 문서당 초안은 하나다. 버려지지 않은 초안이 있으면 새 작업의 자동 저장은 멈추고(화면에 '일시정지'로 표시) 사용자가 복구·버리기를 정해야 한다. 초안은 만료되지 않는다.
- 웹의 초안 상태는 서버 응답(session.draft_revision, recovery)만으로 판단한다. 페이지가 따로 기억하는 것은 대기 시간과 재시도 일정뿐이고, 문서가 바뀌면 처음부터 다시 센다. 문서를 떠나기 전에는 대기 중인 작업의 초안을 먼저 쓴다.
- 한 API 프로세스 안에서 같은 문서의 저장 여러 개가 한꺼번에 줄을 서면(예: 탭 두 개) 각 저장의 잠금 대기(최대 5초)가 쌓여 웹의 10초 제한을 넘길 수 있다. 그때 웹은 결과를 알 수 없다고 표시한다.
- request_id 재전송 기억은 프로세스 메모리(문서당 최근 64개)에만 있다. 웹은 응답을 받지 못한 생성·사본 요청의 ID를 재시도에 다시 쓴다. 그 사이 API가 재시작되면 재전송하지 않고(중복 생성 방지) 문서가 이미 생겼을 수 있다고 안내하며 목록을 새로 고친다. 확인은 사용자 몫이다.
- revision은 2^53 − 1까지다. 편집은 2^53 − 2에서 멈추고 마지막 번호는 버린 뒤 다시 여는 사본용으로 남긴다. 그 뒤 다른 저장을 따라갈 번호가 없으면 그 프로세스에서는 resource_limit으로 거절한다.
- 사본은 내용·ID 카운터만 가져가고 Undo 이력은 비운 채 revision 0에서 시작한다. 사본으로 저장하면 원본 세션은 닫히고(미저장 변경은 사본에만 남음) 원본은 마지막 저장본으로 다시 열린다.
- 세션 64개가 모두 열린 상태에서 사본이 들어갈 자리가 필요하면 미저장 변경이 없는 세션 중 ID가 가장 작은 것을 닫는다. 초안을 사본으로 복구할 때는 다른 clean 세션이 없을 때만 원본 세션을 닫고, 그 사실을 source.closed로 알린다(#6).
- 읽기 전용 파일 처리, Chemifree 외 프로그램에 대한 파일 잠금, 문서 삭제는 없다.
- revision·ID 재사용 방지 기억은 프로세스 메모리에 있다. API가 재시작되면 웹은 문서를 다시 읽어 화면과 같은 내용일 때만 편집을 재전송하고(Undo/Redo 제외), 다르면 Reload를 요구한다. CLI는 매번 저장본에서 시작한다.
- request_id 재전송 기록은 원래 응답 전체를 보관한다(세션당 64개). 큰 문서를 많이 열면 메모리 사용이 커질 수 있다.
- 문서 목록은 저장된 각 문서를 전부 복원·검증한다. 문서가 많고 크면 목록 조회가 느릴 수 있다(잠금 밖에서 수행).
- 화면에서 만든 편집이 앞선 요청에 밀려 낡으면 revision_conflict로 거절되고 Reload를 안내한다(전하 ± 같은 상대 편집은 최신 상태 기준으로 계산).
- 장면은 문서당 위치 한 벌이다. 공개 설계의 그래프/장면 인스턴스 분리, 선택의 이력 포함, 확대·이동, 텍스트·화살표 객체는 아직 없다.
- 캔버스는 포인터 중심이다. 키보드만으로 원자를 배치하는 접근 경로는 없다(인스펙터 편집과 단축키는 있음).
- 브라우저 smoke는 CI에서 실행하지 않는다.
- 브라우저 smoke의 '재시작 모의' 단계가 가끔 실패하던 문제(#8)는 제품 결함이 아니라 테스트 동기화 문제였다. Playwright 1.56.1은 마지막 page route가 사라지면(times: 1 route 소진, unroute) 요청 가로채기를 끈다. 그 순간 시작된 요청은 브라우저에 멈춘 채 서버에 닿지 않고 10초 뒤 클라이언트가 중단한다. smoke는 컨텍스트 전체에 통과용 route를 두어 가로채기를 끄지 않는다(scripts/lib/request-interception.mjs). npm run test:browser:routes가 Chemifree 코드 없이 이 경쟁을 재현한다. 재전송 편집 확인은 스피너 상태에만 기대지 않고 실제 200 응답을 기다린다. 실패하면 failure.png, requests.log, api.log(--log-requests), web.log를 남기고 임시 디렉터리를 지우지 않는다. 기록은 verification/browser-route-race.json.

## 다음 구현 단위 제안

사용자 요청이 범위를 정한다. 후보:

1. 저장·복구 후속(필요 시): 문서 삭제, 초안 만료 정책, 읽기 전용 파일 안내, 서버 측 자동 저장(페이지 없이도 초안 유지).
2. 형식 입출력: MOL/SDF·SMILES 독립 codec과 손실 보고(format_conversion). Chemifree 저장 형식은 손상 입력을 통째로 거절하지만, 외부 형식 가져오기는 공개 명세의 손상 입력 관찰(§Parsing)을 참고해 거절·수정·손실을 항목별로 정하고 보고한다. 조용한 수정은 하지 않는다.
3. 화학 그래프 진단: 원자가·암시적 수소·분자식(structure_analysis 일부)과 공개 명세의 관찰 사례 대조.
4. 편집기 확장: 고리 템플릿, 확대·이동, 그래프/장면 분리, 선택 이력.
5. 사용자 인증·원격 배포는 별도 접근 제어 설계 후 진행.

명명·NMR·물성·고급 형식·Windows bridge·팀 서비스를 한꺼번에 넣지 않는다. 기능을 실제로 연결하고 검증한 뒤 capability 상태를 변경한다.

## 독립 구현 경계

제품 코드는 새 컨텍스트에서 공개 기능 명세와 독립적인 자료만으로 작성한다. 원본 바이너리·원본 코드/디컴파일·조사 evidence·private vendor payload를 열지 않는다. GitHub 저장소의 현재 독립 소스와 검증·설계 문서는 구현 입력으로 사용할 수 있다.

기존 분석 인계와 원시 데이터가 공개 저장소에 포함되어 있다고 가정하지 않는다. 필요한 명세는 승인된 공개 인계에서 기능별로 읽고, 원본 분석을 다시 시작하거나 전체 자료를 공개 저장소에 복사하지 않는다.

기존 개인용 모델 데이터와 서비스용 데이터셋은 별도 범위다. 사용자 결정은 로컬 보조 인계와 실제 요청을 따른다. 원본 실행 엔진이나 가짜 수치로 미구현 기능을 대체하지 않는다.

## 세션을 시작할 때 붙여 넣을 예시

~~~text
이 Chemifree 프로젝트를 이어서 진행해줘.
AGENTS.md, docs/HANDOFF.ko.md와 존재하면 private/SESSION_CONTEXT.ko.md를 먼저 읽어.
첫 수직 기능(생성 → 편집 → Undo/Redo → 저장 → 다시 열기)은 완료됐으니
현재 코드·CI 상태를 확인하고, 다음 범위로 <원하는 기능>을 구현해줘.
제품 코드는 공개 기능 명세와 독립 자료만 사용하고 원본 분석·바이너리는 열지 마.
~~~

프로젝트 지침을 세션 시작 시 읽는 동작은 [공식 OpenAI 문서](https://learn.chatgpt.com/docs/agent-configuration/agents-md)에 설명되어 있다. 작업 재개에는 파일과 현재 Git 상태를 기준으로 사용하며, 과거 채팅 전체가 새 컨텍스트에 들어왔다고 가정하지 않는다.
