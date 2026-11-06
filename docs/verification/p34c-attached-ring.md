# P34c 기존 원자에 탄소 고리 연결 후보

## 범위와 정책

P34b `86c584c` 기반 `feat/p34c-attach-ring` 후보이며 선행 PR 병합 후 최신 main을
합쳐 검증한다. P34는 in_progress다. 기존 independent insert_ring 의미는 유지한다.

`attach_ring`은 `{instance,atom,direction:{dx,dy},size?:5|6}`을 받는다. 기존 원자와
새 고리를 single 하나로 연결한다. N개의 기본 중성 C와 N+1개 single이 fresh IDs로
추가된다. 공유 원자/기존 결합 교체는 없고 numbered attachment_points를 소비하지 않는다.
단일 명시 배치만 지원한다. 크기 생략은6이고 null/다른 크기/비정수는 malformed다.
대상은 명시 atom ID이며 `$N`은 지원하지 않는다. vector magnitude는 무시하고 유한
비영벡터의 방향만 쓴다. 정규화 전에 max-component로 scale해 huge/subnormal도 받는다.

L은 큐 실행 당시 document bond_length_pt다. target 최신 위치를 A, direction 단위벡터를
u, 독립 recipe의 offsets를 (x,y), 첫 offset x를 R이라 하면 새 vertex는
`A + (L+(R-x))*u + y*(u_y,-u_x)`다. 첫 꼭짓점은 A+L*u이며 cycle은 그 바깥에 놓인다.
이는 독립 정의한 regular 2D geometry이며 collision avoidance·conformation이 아니다.

새 ring cycle 뒤 마지막 연결을 기존 insert_bond로 만들어 AddBond의 의미를 재사용한다.
연결 target의 저장 center와 target을 끝점으로 가진 double bond의 저장 record를 폐기하고
기존 notes를 그대로 보고한다. group prune/의미 변경 notes도 기존 transaction 경로다.
기존 marks는 유지하고 unrelated configuration/group은 유지한다. 새 mark/configuration/CIP를
만들지 않으며 Undo는 폐기한 원래 statement/group을 복구한다. result/operation outcome의
ok/partial 규칙은 기존 notes 계약 그대로다.

기존 AddBond처럼 valence를 새 edit gate로 삼지 않는다. query/charge/isotope/H/radical/
attachment-point state는 보존하며 원자가 변화는 기존 chem.validate가 진단한다. full
chemistry나 약어/monomer attachment semantics를 제공한다고 주장하지 않는다.

웹 R의 explicit Independent/Attach to atom 모드에서 Attach는 기존 atom에서 방향을
드래그한다. click/empty-space drag는 대상이나 방향을 추측하지 않는다. pointer start에
atom/instance/size를 캡처하고 vector는 inverse SVG의 target center→release다. mode/size는
gesture/busy 동안 disable한다. 방향 preview는 unmarked single이며 15° snap을 쓰지 않는다.
기존 gate의 ring attempt가 captured target/vector/size/body/process를 고정하고 replay를
확인한다. 뒤의 선택·도구 변경은 retarget하지 않고 restart/conflict에는 명시 reconcile다.
OS clipboard는 접근하지 않는다.

client는 fresh cycle/link·방향 기하·기존 상태 보존, target과 incident double의 정확한
record 폐기, group prune, note의 feature/path/disposition 집합을 검증한다. 누락/중복/
추가 notes나 unrelated stereo 손실은 성공으로 확정하지 않는다. notes의 reason 문장은
기존 runtime validator로 읽고 UI의 기존 StereoNotes에 보여 주며 새 문장을 조합하지 않는다.

fusion/spiro/replace, 방향족, 추가 치환 패턴, 다른 크기, 사용자 template, 약어, 복수 배치,
자동 layout/collision/conformation, 외부 DATA/template geometry/DLL은 범위 밖이다.

## 작성한 회귀

- Domain attach_ring: 5/6 cycle 및 N+1 topology, cardinal/3:4 방향과 현재40pt length를
  독립 trig 기대값으로 확인, 기본 atom state·원본 좌표·stable Undo/Redo/fresh IDs,
  finite huge/subnormal vector, zero/NaN/Infinity 거절, late final-link ID 실패와 앞선
  rename·redo rollback, late coordinate 실패, target/placement 거절을 작성했다.
- Application attach_ring: target의 center와 incident double record·crossed/wedge mark,
  unrelated center, AND group의 member 잔존/전체 제거, scene/selection 보존과 notes를
  검사한다. 실제 일반 add_atom/add_bond transaction을 oracle로 사용해 stereo와 notes를
  정확 비교한다(이 비교의 좌표 재사용은 topology 의미만 분리하고 기하 증거는 아니다).
  연결·stereo/group clearing 뒤 다음 move의 좌표 실패가 전체 snapshot을 rollback하는 검사도 있다.
  replay·Undo/Redo·save/reopen과 malformed/unknown target·size·direction rollback도 있다.
- HTTP=CLI contract smoke: omitted6 연결·fresh counts·원본 보존·notes/replay·zero direction
  거절·Undo/Redo를 실제 장기 CLI/HTTP process 간 비교하도록 작성했다.
- client: 5/6 frozen size/target/vector/source/selection, exact notes/group 정리, wrong
  target/closing edge/direction/old mark·notes 누락/중복/추가 거절, other double-bond end의
  center/group 보존, no-send 거절, lost reply·restart/conflict no-resend를 작성했다.
- `npm run test:browser:attach-ring`: production DocumentWorkspace/canvas/hook/client의 explicit
  mode와 size5 선택, zoom/pan atom gesture, click/empty-space drag no-op, gesture/pending lock,
  두 reply loss 뒤 같은 body 확인, stereo notes UI와 Undo/Redo를 검사하도록 작성했다.
  HTTP는 scripted이며 실제 backend canvas 통합·과학·외부 앱 호환 증거는 아니다.
  test:client와 기존 CI browser job에 연결했다.

## 선행 main 통합과 확인된 targeted 실행

부모의 격리 실행 담당자가 구현 HEAD `798ce9deb85067f41f17f135569bcb797702ca67`를
검증했다. `/tmp/chemifree-p29d-verify-j4KjPa/p34c-targeted.log`의 결과는 다음과 같다.
이 작성자는 로그를 읽기만 했으며 명령을 재실행하지 않았다.

- Domain attach_ring 4/4, Application attach_ring 2/2 통과.
- npm run lint, npm run typecheck, npm run test:client 통과(client 532 passed,
  0 failed, 0 skipped).
- npm run build(웹 production build 및 Rust workspace build),
  npm run test:browser:attach-ring 통과.
- sandbox-run exit=0, peak anon 1238 MiB·page cache 2590 MiB,
  cap 6144 MiB·swap 0. 전용 browser는 scripted HTTP 경계의 production
  workspace/canvas/hook/client 검사이며 실제 backend 통합/과학/외부 앱 호환 근거가 아니다.

이후 P34b #175가 main에 병합되어 `origin/main`의 `561be7b`를 P34c에 합쳤다.
선행 P34a #174와 autosave/restart #176(`5be98a8`)도 포함한다. squash 병합 때문에
발생한 겹침은 P34b 작성 HEAD `86c584c`를 공통 내용 기준으로 대조했다. 고리 연결의
Domain/DTO/catalog/웹/회귀와 main의 process-generation·snapshot provenance·autosave
오류/재시작 조정·브라우저 회귀를 모두 보존했다. package scripts와 CI의 충돌은
attach-ring/autosave-restart 두 검사 모두 남겼고, P34a/P34b 실행 기록은 main의 최신
버전을 유지했다. 새 실행 없이 정적으로 병합과 whitespace만 확인했다.

위 targeted 통과는 병합 전 `798ce9d`에 대한 근거다. **현재 병합 결과의 final full
검사·전체 browser·PR/병합 CI는 pending**이며 위 결과를 이 HEAD의 통과로 세지 않는다.

## 실행 대기

작성자는 부모의 공유 격리 지시에 따라 테스트·빌드·브라우저·lint/typecheck/clippy·mutation·push를
실행하지 않았다. 초기 구현에서 `cargo fmt --all` 및 `git diff --check`만 실행했고, 이번
통합에서는 fetch/merge·정적 대조·whitespace 검사만 수행한다. 부모의 이전 targeted 통과는
위에 분리 기록했다. 선행 slice의 통과/CI를 이 변경의 근거로 삼지 않는다. 현재 병합 HEAD의
남은 검증 명령:

```sh
cargo test --locked -p chemifree-domain --test attach_ring
cargo test --locked -p chemifree-application --test attach_ring
npm run check
npm run test:browser:ring
npm run test:browser:attach-ring
npm run test:browser
```

실패 증명은 closing/link 제거→topology/client, direction/size 변경→기하/body,
insert_bond clearing 우회→oracle/notes/group, validation 생략→response tamper,
request/process 재생성→immutable/restart 회귀로 수행해야 한다. 아직 실행하지 않았다.

## 리뷰

- Codex 작성자 정적 검토: 기존6 arithmetic, finite direction scaling, fresh graph/link,
  Domain draft/history/ID rollback, ordinary insert_bond/stereo/group 경로, DTO/catalog,
  client의 정확한 허용 diff/notes와 immutable body/process, hook queue, 실제 UI gesture와
  독립 기하·일반 편집 oracle·문서의 bounded 범위를 대조했다. 정적 검토 중 private/public
  API·identifier 거절 유형·preview의 이전 wedge/snap 상속과 unlabeled-C browser selector를
  바로잡았다. 마지막 정적 API 대조에서 DocumentParts.atoms가 Vec임을 확인해 새 검사의
  AtomId map-index를 tuple Vec index로 수정했다. 실행 근거는 없으며 PR Codex review도 아직 대기다.
- 독립 fresh-context `/root/p34b_decision/p34c_review`: base `86c584c`의 latest tracked diff와
  새 파일을 읽기 전용으로 검토했다. Domain/Application 기하·fresh topology·atomic
  rollback/history·기존 insert_bond clearing/group notes, client validation/recovery,
  explicit web gestures·작성 회귀·최신 공개 문서를 확인했고 actionable finding 없음,
  arbitrary scope expansion 없음으로 보고했다. tests/build/browser/mutation/mechanical
  gate는 실행하지 않았다. 이후의 late-command rollback 회귀 보강도 미실행이다.

software-design-philosophy의 책임/인터페이스 진단은 10/10이며 실패 행은 없다.
Rust patterns/testing·TypeScript patterns/testing·shadcn·software-design-philosophy skills를
적용했다. 기존 ToggleGroup을 재사용하며 provider/framework/새 의존성은 추가하지 않는다.
실제 skill gate/mutation proof는 위 지시로 미실행이다.
