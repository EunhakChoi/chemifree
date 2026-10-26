# Chemifree 새 세션 인계

이 문서는 완료된 작업과 다음 개발 경계를 전달한다. 인계 문서를 읽는 것만으로 다음 기능 구현을 시작하지 않으며, 새 세션에서 사용자가 요청한 범위를 따른다.

## 프로젝트와 현재 기준

- 이름: Chemifree. 사용자가 직접 확정했으며 이름을 다시 제안하지 않는다.
- 공개 저장소: https://github.com/EunhakChoi/chemifree
- 마지막 검증된 스캐폴딩·설계 커밋: 90f2b9941919c4ba80ad0dd17cdbb85cf6666e9a.
- 해당 커밋의 [GitHub CI 통과 기록](https://github.com/EunhakChoi/chemifree/actions/runs/40362091650).
- HTML 설계의 코드 기준은 119f47765152fd564e5c97bb0c1ad6fa5b5b20c3이다. 그다음 커밋은 HTML과 문서 추가다.
- 이후 문서나 코드가 변경될 수 있으므로 작업 시작 시 git status, git log와 현재 CI를 다시 확인한다. 기존 사용자 변경은 보존한다.

## 먼저 읽을 파일

1. 프로젝트 루트의 AGENTS.md.
2. 이 문서와 ../README.md.
3. CONTRACT.md: 현재 v1 계약과 버전 정책.
4. SHADCN_REVIEW.ko.md: UI 도구 선택, 현재 연결 상태와 정책 제안.
5. architecture.html: 실제 레이어·소스 링크·다음 기능을 담은 대화형 설계안. 브라우저로 열 수 있다.
6. 같은 PC에서 존재한다면 private/SESSION_CONTEXT.ko.md: 승인된 공개 기능 명세의 로컬 경로와 기존 사용자 결정만 담은 보조 인계. Git에 포함하지 않는다.

## 완료한 기반

| 영역 | 실제 상태 |
| --- | --- |
| apps/web | React/TypeScript/Vite 웹 셸, API 조회·timeout·refresh/retry·empty canvas·반응형·시스템 dark |
| apps/api | Axum read-only HTTP, IPv4 loopback 바인딩, JSON 404/405 |
| apps/cli | clap 기반 status/capabilities, --json, HTTP 없이 공통 Application 호출 |
| crates/domain | 외부 의존성 없는 CapabilityId·ImplementationStatus 용어 |
| crates/application | 공용 유스케이스·v1 DTO·Application-owned CapabilityCatalog port |
| crates/providers | 위 port의 ScaffoldCapabilities 구현; 실제 데이터 IO 없음 |
| 개발 환경 | npm/Cargo workspaces, lockfiles, 도구 버전 고정, npm run dev/check, GitHub Actions |

의존 방향은 Application → Domain, Providers → Application + Domain이다. API와 CLI가 가장 바깥에서 Application과 concrete provider를 조립한다.

현재 동작하는 HTTP 계약은 GET /api/v1/health와 GET /api/v1/capabilities다. CLI 대응은 status --json과 capabilities --json이다. 모든 JSON에는 api_version: v1이 있다.

runtime_inspection만 available이다. document_editing, structure_analysis, format_conversion, local_model_loading, persistence, authentication, windows_bridge는 모두 not_implemented다. health의 ok는 화학 기능 준비 완료를 뜻하지 않는다.

## UI 결정

- 웹 우선이며 데스크톱 셸은 선택적 후속 확장이다.
- shadcn/ui의 공식 @shadcn registry, Base UI/Nova를 적용했다.
- Button, Card, Badge, Empty, Alert, Skeleton만 설치했다.
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

검증된 내용:

- clean npm ci, lint, TypeScript, production build, rustfmt, clippy, Rust 테스트 3개.
- 실제 HTTP와 CLI의 JSON 동일성, read-only·오류·버전 경계.
- API 미실행 오류 → 실제 API 시작 → Retry 회복, Refresh 재조회.
- 잘못된 버전·중복 ID·잘못된 capability status 거절.
- 모바일 320/390px, 시스템 dark, 링크 의미와 브라우저 예외 확인.
- HTML 설계의 두 관점·구성요소·소스 탐색·실행 흐름·다운로드·인쇄.

기록은 verification/에 있다. 브라우저 결과는 수동 smoke 기록이며 CI에 브라우저 runner는 아직 없다. 화학적 정확성 검증은 수행한 상태가 아니다.

CI는 실행 중인 전역 npm을 교체하지 않는다. 별도 runner-temporary prefix에 npm을 설치하고 PATH에 추가하는 현재 방식을 유지한다.

## 다음 구현 단위 제안

사용자가 다음 기능 구현을 요청하면 문서 한 장을 끝까지 다루는 수직 기능부터 진행한다.

1. 문서·분자 그래프·장면 모델과 안정적인 ID·참조 불변식.
2. 문서 생성과 원자·결합 추가/편집 명령.
3. 브라우저의 SVG 표시·선택·pointer preview.
4. 원자적인 명령 적용과 Undo/Redo.
5. 로컬 저장·다시 열기와 리비전.
6. API와 CLI가 같은 Application 유스케이스를 사용.

쓰기 경로를 만들 때 로컬 세션·Host/Origin 검사·입력 한도를 함께 구현한다. request_id 멱등성, expected_revision 충돌, 저장 커밋 후 ACK, 실패·재시작 복구를 수용 기준에 포함한다. 구체적인 wire schema와 v1 호환 변경은 CONTRACT.md와 함께 검토한다.

명명·NMR·물성·고급 형식·Windows bridge·팀 서비스는 이 첫 기능에 한꺼번에 넣지 않는다. 필요한 기능만 실제로 연결하고 검증한 뒤 capability 상태를 변경한다.

## 독립 구현 경계

제품 코드는 새 컨텍스트에서 공개 기능 명세와 독립적인 자료만으로 작성한다. 원본 바이너리·원본 코드/디컴파일·조사 evidence·private vendor payload를 열지 않는다. GitHub 저장소의 현재 독립 소스와 검증·설계 문서는 구현 입력으로 사용할 수 있다.

기존 분석 인계와 원시 데이터가 공개 저장소에 포함되어 있다고 가정하지 않는다. 필요한 명세는 승인된 공개 인계에서 기능별로 읽고, 원본 분석을 다시 시작하거나 전체 자료를 공개 저장소에 복사하지 않는다.

기존 개인용 모델 데이터와 서비스용 데이터셋은 별도 범위다. 사용자 결정은 로컬 보조 인계와 실제 요청을 따른다. 원본 실행 엔진이나 가짜 수치로 미구현 기능을 대체하지 않는다.

## 세션을 시작할 때 붙여 넣을 예시

~~~text
이 Chemifree 프로젝트를 이어서 진행해줘.
AGENTS.md, docs/HANDOFF.ko.md와 존재하면 private/SESSION_CONTEXT.ko.md를 먼저 읽어.
스캐폴딩은 완료됐으니 현재 코드·CI 상태를 확인하고,
문서 생성 → 원자·결합 편집 → Undo/Redo → 저장 → 다시 열기를
첫 수직 기능으로 구현해줘.
제품 코드는 공개 기능 명세와 독립 자료만 사용하고 원본 분석·바이너리는 열지 마.
~~~

프로젝트 지침을 세션 시작 시 읽는 동작은 [공식 OpenAI 문서](https://learn.chatgpt.com/docs/agent-configuration/agents-md)에 설명되어 있다. 작업 재개에는 파일과 현재 Git 상태를 기준으로 사용하며, 과거 채팅 전체가 새 컨텍스트에 들어왔다고 가정하지 않는다.
