# P34b 독립 탄소 5원 고리 후보 기록

## 결정과 지원 경계

P34a PR HEAD `5a8c20e`에서 분리한 `feat/p34b-five-ring` 후보다. P34a squash
병합 `71321a9`를 이 브랜치에 합쳤다. 최종 통합본의 전체 격리 검증과 현재 PR CI는
아직 pending이다. P34는 in_progress이며 이 문서는 납품 완료를 주장하지 않는다.

기존 `insert_ring`에 optional `size:5|6`을 더한다. 생략과 explicit6은 같은 Domain
RingSize::Six로 매핑하고 기존6 좌표 산술을 유지한다. explicit null·0/4/7/음수·
비정수·문자열·bool은 invalid_request다. Domain은 닫힌 RingSize만 받는다.

size5는 독립 중성 기본 C5와 single5 cycle이다. 현재 bond_length_pt=L을 변 길이로
하는 정오각형이며, 오른쪽 꼭짓점부터 y-down 시계 방향이다. 외접반경은
L/(2 sin(π/5))이고 Domain은 golden-ratio radicals로 고정 좌표를 계산한다.
이는 Chemifree가 독립 정의한 2D drawing recipe이며 conformation/화학 layout이 아니다.
외부 DATA·원본 monomer/template geometry·원본 DLL을 사용하지 않는다.

HTTP·CLI·operations는 같은 Application transaction을 사용한다. 단일 명시 배치,
fresh IDs·기존 상태/선택 보존·단일 Undo/Redo·save/reopen·immutable replay와 늦은
좌표/ID/entity 한도 실패의 전체 rollback을 기존 경로로 유지한다. schema 변경은 없다.

실제 웹 R 도구의 Ring size ToggleGroup은 5/6(초기6, 문서 비저장)이다. 클릭에서 size를
캡처하고 현재 bond length는 큐 실행 시 해석한다. busy 동안 size control을 disable한다.
기존 immutable attempt에 size가 포함되어 같은 request/body/process만 확인하며,
restart/terminal conflict는 명시 reconcile이다. 뒤의 tool/size 변경은 원래 시도를
retarget하지 않는다. OS clipboard에 접근하지 않는다.

attachment/fusion/replace, 방향족 패턴, 5/6 밖 크기, 치환기, 사용자 template,
약어 축약/펼침, 복수 배치, collision avoidance, conformation은 후속이다.
provider/범용 registry/framework는 추가하지 않는다.

## 작성한 회귀와 실패 증명 계획

- Domain `crates/domain/tests/insert_ring.rs`: 기존6 검사 보존, 독립 decimal 기대값의
  60pt pentagon 좌표·5-cycle adjacency·기본 상태·각 변 길이·한 history step·Undo/Redo의
  동일 IDs와 Undo 뒤 fresh IDs를 검사한다. size5의 late coordinate/atom-ID/bond-ID
  실패·앞선 rename rollback, 2000 atom 경계, size5/6 좌표 실패의 redo 보존을 검사한다.
- Application `crates/application/tests/insert_ring.rs`: 기존 실제 file repository
  persistence 시나리오를 omitted/5/6 모두 실행하도록 작성했다. 기존 atom/bond·stereo·
  graphic/reference·선택 보존, replay·Undo/Redo·save/reopen과 malformed 크기의 revision/
  history/redo 불변을 검사한다. omitted/explicit6은 동일 CommandDto인지 비교한다.
- `scripts/contract-smoke.mjs`: 실제 HTTP=CLI operations의 기존6 이후 독립5 append,
  기존 내용 보존·replay·잘못된 크기 거절·Undo/Redo 동등성을 작성했다.
- `scripts/web-ring-client-check.mjs` (`npm run test:client`에 포함): size5 frozen body,
  source/center/selection 변경·유실된 결과의 같은 request 확인, 잘못된 크기의 성공 응답,
  open cycle/기하 tamper·잘못된 size 전송 전 거절·restart 뒤 미재전송을 검사한다.
  fixture는 공개 경계의 독립 decimal 기대값이며 Rust의 radicals를 복제하지 않는다.
- `scripts/web-ring-check.mjs` (`npm run test:browser:ring`, 기존 CI 연결): 기존6
  canvas/hook/client 검사와 production Close/clipboard navigation 회귀를 유지한다.
  production DocumentWorkspace의 초기6→5 선택, 좌표 클릭, 전송 전 두 유실, pending
  동안 5/6 disable, 같은 body의 size5 확인과 실제5개 추가를 검사하도록 작성했다.
  HTTP만 scripted이며 실제 backend canvas 연계나 외부 앱 호환 증거는 아니다.

실패 증명은 검증 담당자가 수행해야 한다. size5→6 강제 변경은 Domain adjacency·
client count·production size 요청 검사, closing edge 제거는 adjacency/client cycle,
DTO null 기본값 허용은 malformed 검사, frozen size를 현재 선택으로 교체하면
lost-response same-body 검사, 좌표 guard 제거는 late rollback 검사가 실패해야 한다.
이 문서 작성자는 mutation 검사를 실행하지 않았다.

## 실행 상태와 남은 검증

작성 담당자는 부모의 공유 격리 실행 지시에 따라 테스트·빌드·브라우저·mutation·
lint/typecheck/clippy gate를 직접 실행하지 않았다. 초기 작성에서는 Rust source
formatting `cargo fmt --all`과 정적 whitespace 검사 `git diff --check`만 실행했다.
전자의 성공을 컴파일/테스트 성공으로 세지 않는다.

이후 실행 담당자가 구현 HEAD `86c584c`의 격리 targeted 검증을 통과시켰다. 로그는
`/tmp/chemifree-p29d-verify-j4KjPa/p34b-targeted.log`이며 `sandbox-run: exit=0`,
peak anon 1124 MiB·page cache 2929 MiB·cap 6144 MiB·swap 0이다.

- `cargo test --locked -p chemifree-domain --test insert_ring`: 9 passed/0 failed.
- `cargo test --locked -p chemifree-application --test insert_ring`: 4 passed/0 failed.
- `npm run lint`, `npm run typecheck`, `npm run test:client`(515 passed/0 failed),
  `npm run build`, `npm run test:browser:ring` 통과.

이 targeted 검증은 전체 `npm run check`, HTTP=CLI 계약 smoke, standard browser,
mutation 검증이나 현재 PR CI의 통과 근거가 아니다. 전용 ring browser는 scripted
HTTP 경계의 production canvas/hook/client 검사이며 실제 backend canvas 통합을
입증하지 않는다. 최종 main 통합본의 full 검증과 PR CI는 pending이다.

P34a squash `71321a9`와 기존 분기점 `5a8c20e`의 tree 차이는 P34a 검증 문서 한
개뿐이었다. squash로 생긴 19개 중복 이력 충돌은 P34b의 5원 변경을 유지하고
`docs/verification/p34a-independent-ring.md`에는 최신 main의 전체 격리 검증 기록을
채택해 해결했다. 코드와 CI는 P34a 분기점 이후 main에 추가 차이가 없었으며 기존6
구현과 CI 연결을 보존했다. 통합 담당자는 테스트·빌드·브라우저·push를 실행하지 않았다.

실행 담당자의 남은 명령:

```sh
cargo test --locked -p chemifree-domain --test insert_ring
cargo test --locked -p chemifree-application --test insert_ring
npm run check
npm run test:browser:ring
npm run test:browser
```

## 리뷰와 skill 적용

- Codex 작성자 정적 검토: Domain recipe/transaction 원자성, DTO omitted/null 차이와
  닫힌 크기, 기존6 산술, client size별 success 검증, hook captured 값과 production
  ToggleGroup 전달·disable, HTTP=CLI와 독립 기대값·문서 경계를 대조했다. 정적 검토 중
  기본30pt settings의 no-op를 새 history step으로 기대한 검사를 발견해 60pt로 수정했고,
  private contract module 경유 import를 public CommandDto re-export로 수정했다.
- 독립 fresh-context subagent `/root/p34b_decision/p34b_review`: base HEAD
  `5a8c20e92f416f5f2309d7c270e4b2e2f9900c71`의 local diff와 이 새 기록을 읽기 전용으로
  검토했다. actionable finding 없음. DTO omission/null, 5원 기하·기존6 산술,
  transaction/history, size별 응답 검증, captured size·recovery, production UI와
  작성 회귀·scope를 확인했다. 테스트/빌드/브라우저/mutation/mechanical gate는
  실행하지 않았고 compilation/runtime은 미검증이다. 초기 missing-link 우려는 이
  기록 작성 뒤 최종 검토에서 해소됐다. 추가 finding 처분은 없다.
- Rust patterns/testing·TypeScript patterns/testing을 적용했다. shadcn skill의 기존
  ToggleGroup/ToggleGroupItem을 재사용했고 upstream generated source나 의존성은 바꾸지
  않았다. software-design-philosophy의 인터페이스/책임 진단은 10/10이며 실패 행은 없다.
  실제 실행 gate와 mutation proof는 위 지시로 미실행이다. PR Codex review는 아직 없다.
