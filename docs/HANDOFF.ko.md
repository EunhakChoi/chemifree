# Chemifree 새 세션 인계

이 문서는 완료된 작업과 다음 개발 경계를 전달한다. 인계 문서를 읽는 것만으로 다음 기능 구현을 시작하지 않으며, 새 세션에서 사용자가 요청한 범위를 따른다.

## 프로젝트와 현재 기준

- 이름: Chemifree. 사용자가 직접 확정했으며 이름을 다시 제안하지 않는다.
- 공개 저장소: https://github.com/EunhakChoi/chemifree
- 스캐폴딩·설계 기준 커밋: 90f2b99([CI](https://github.com/EunhakChoi/chemifree/actions/runs/40362091650)), 인계 문서 007a185.
- 그 이후 첫 수직 기능(문서 생성 → 원자·결합 편집 → Undo/Redo → 저장 → 다시 열기)을 구현했다. 커밋과 CI 결과는 git log와 GitHub Actions에서 확인한다.
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
| apps/web | React/TypeScript/Vite. 런타임·capability 카드와 문서 편집기: 새 문서·목록·열기, SVG 구조 캔버스(src/editor), 도구(선택·원자·결합·지우개), 원소·결합 차수, 인스펙터(원소·전하·동위원소·차수), Undo/Redo·저장·닫기(미저장 확인), 단축키, 충돌·저장 실패 안내 |
| apps/api | Axum loopback HTTP. 문서 생성·조회·트랜잭션·undo/redo·저장·닫기. Host/Origin/Sec-Fetch-Site 검사, 프로세스별 세션 토큰, JSON·256 KiB 제한, v1 오류 envelope |
| apps/cli | status/capabilities와 documents list·create·show·apply·undo·redo. 같은 Application을 HTTP 없이 호출, 편집마다 저장 |
| crates/domain | 원소(1–118), 문서 그래프·안정 ID·참조 불변식, 명령, 원자적 트랜잭션, change-set Undo/Redo, 저장본 복원 검증. 외부 의존성 없음 |
| crates/application | 세션(작업 사본), expected_revision 충돌, request_id 재전송, 저장 커밋 후 ACK, 공유 DTO, DocumentRepository·CapabilityCatalog port |
| crates/providers | 로컬 JSON 파일 저장소(임시 파일 → fsync → rename → 디렉터리 sync, base revision 검사, 손상 파일 거절), capability 목록 |
| 개발 환경 | npm/Cargo workspaces, lockfiles, npm run dev/check, test:contracts, 수동 test:browser, GitHub Actions |

의존 방향은 Application → Domain, Providers → Application + Domain이다. API와 CLI가 가장 바깥에서 Application과 concrete provider를 조립한다.

HTTP 계약은 health·capabilities·session과 /api/v1/documents 아래 생성·조회·transactions·undo·redo·save·close다. CLI는 status·capabilities·documents 하위 명령으로 같은 Application을 호출한다. 모든 JSON에는 api_version: v1이 있다. 세부 내용은 CONTRACT.md에 있다.

runtime_inspection, document_editing, persistence가 available이다. document_editing은 그린 그대로 기록하는 drawing 모델이며 원자가·암시적 수소·방향족성·입체·배치·명명·물성을 계산하지 않는다. persistence는 로컬 Chemifree JSON 파일이며 DB·동기화·공유는 없다. structure_analysis, format_conversion, local_model_loading, authentication, windows_bridge는 not_implemented다. 쓰기 보호(세션 토큰·Host/Origin)는 다른 웹사이트를 막는 장치이며 사용자 인증이 아니다.

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

검증된 내용(첫 수직 기능):

- npm run check: lint, TypeScript, production build, rustfmt, clippy, Rust 테스트 39개(domain 16, application 10, providers 5, api 8).
- npm run test:contracts: 실제 API·CLI 프로세스와 임시 데이터 디렉터리. HTTP = CLI JSON, 쓰기 보호, 생성→편집→undo/redo→저장→닫기→다시 열기, request_id 재전송·재사용 거절, revision/save 충돌, 저장 실패 후 재시도, API 강제 종료 후 마지막 커밋 복구.
- npm run test:browser(수동, CI 제외): Chrome으로 편집 흐름 전체, 키보드 단축키, 미저장 닫기 확인, 충돌 Reload, 저장 실패·재시도, CLI와의 저장 충돌, 요청 처리 중 반복 입력(원자 겹침 방지·전하 클릭 누락 방지), 잘못된 동위원소 입력 거절, 응답을 잃은 문서 생성의 재시도(중복 생성 없음), 320/390px, dark mode. 결과는 verification/editor-browser-smoke.json.
- 브라우저 검증 중 serde_json 기본 float 파서가 다시 열 때 1 ULP 오차(226.39999999999998 → 226.4)를 내는 버그를 발견했고 float_roundtrip 기능으로 수정, 회귀 테스트를 추가했다.

scaffold 시점 기록은 verification/의 기존 JSON에 남아 있다. 화학적 정확성 검증은 수행한 상태가 아니다.

CI는 실행 중인 전역 npm을 교체하지 않는다. 별도 runner-temporary prefix에 npm을 설치하고 PATH에 추가하는 현재 방식을 유지한다.

## 알려진 한계

- 작업 사본은 API 프로세스 메모리에 있다. 저장하지 않은 편집은 프로세스 종료 시 사라지고 마지막 커밋이 남는다(의도된 정책, 테스트됨).
- 저장의 base revision 검사와 rename은 프로세스 간 단일 원자 연산이 아니다. 두 프로세스가 같은 순간 저장하면 경쟁할 수 있다.
- request_id 재전송 기억은 프로세스 메모리(문서당 최근 64개)에만 있다. 웹은 응답을 받지 못한 생성 요청의 ID를 재시도에 다시 쓰지만, 그 사이 API가 재시작되면 재시도가 새 문서를 만든다.
- 문서 삭제 기능은 아직 없다.
- 장면은 문서당 위치 한 벌이다. 공개 설계의 그래프/장면 인스턴스 분리, 선택의 이력 포함, 확대·이동, 텍스트·화살표 객체는 아직 없다.
- 캔버스는 포인터 중심이다. 키보드만으로 원자를 배치하는 접근 경로는 없다(인스펙터 편집과 단축키는 있음).
- 브라우저 smoke는 CI에서 실행하지 않는다.

## 다음 구현 단위 제안

사용자 요청이 범위를 정한다. 후보:

1. 형식 입출력: MOL/SDF·SMILES 독립 codec과 손실 보고(format_conversion). Chemifree 저장 형식은 손상 입력을 통째로 거절하지만, 외부 형식 가져오기는 공개 명세의 손상 입력 관찰(§Parsing)을 참고해 거절·수정·손실을 항목별로 정하고 보고한다. 조용한 수정은 하지 않는다.
2. 화학 그래프 진단: 원자가·암시적 수소·분자식(structure_analysis 일부)과 공개 명세의 관찰 사례 대조.
3. 편집기 확장: 고리 템플릿, 확대·이동, 그래프/장면 분리, 선택 이력.
4. 사용자 인증·원격 배포는 별도 접근 제어 설계 후 진행.

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
