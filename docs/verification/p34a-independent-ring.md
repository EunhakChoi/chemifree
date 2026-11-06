# P34a 독립 탄소 6원 고리 검증 계획과 구현 기록

## 결정과 제한

`insert_ring`은 독립적으로 정의한 정육각형 탄소 고리를 생성하는 bounded transaction
command다. 별도 범용 template registry나 새 operation result를 만들지 않고 기존
Domain/Application transaction·replay·history·persistence를 공유한다. HTTP/CLI의
추가 command와 웹 R 도구는 같은 기능을 호출한다. provider/외부 DATA·기존 framing
자료를 활성화하지 않는다. 기존 stage나 전체 P34 기능 완료를 주장하지 않는다.

현재 `bond_length_pt`, 명시 중심, 고정 방향, 6개 중성 C/6 single, 단일 배치,
기존 선택 유지가 의도적인 첫 범위다. attach/fuse/replace, 방향족, 다른 크기,
사용자 저장 template, 약어, 복수 배치, collision avoidance, conformation은 후속이다.
한 recipe의 기하를 위해 새로운 provider interface나 framework를 만들지 않는다.
기존 500-command 제한에서 하나로 세고 domain entity/coordinate/ID 제한을 적용한다.
이 command 자체에는 `$N`이 지칭할 단일 생성 entity가 없다.

## 작성한 검사

- `crates/domain/tests/insert_ring.rs`: 6-cycle adjacency·default atom state·현재 길이·
  독립 위치 기대값, 한 history step과 동일 ID Redo·Undo 뒤 fresh ID, late coordinate
  실패의 앞선 rename/ID/history/redo 전체 rollback, 배치 거절, atom limit·ID exhaustion,
  근거 없는 `$N` 거절을 검사한다.
- `crates/application/tests/insert_ring.rs`: 실제 file repository에서 기존 atom/bond·
  crossed mark·stored configuration·graphic/reference·선택 보존, immutable replay,
  history, save/reopen과 malformed/unsupported fields·unknown instance·좌표·revision
  거절을 검사한다. 기존 operation catalog 검사도 새 command를 포함하도록 갱신했다.
- `scripts/web-ring-client-check.mjs` (`npm run test:client`에 포함): frozen input,
  새 graph/geometry 및 기존 내용 보존, 잘못된 성공 응답의 미확정 처리, 같은 body의
  lost-response 확인과 restart 뒤 미재전송, unsupported destination의 전송 전 거절.
- `scripts/contract-smoke.mjs` (`npm run test:contracts`): 실제 HTTP와 장기 CLI
  operations process 간 결과·거절·replay·Undo/Redo 동등성.
- `scripts/web-ring-check.mjs` (`npm run test:browser:ring`, CI 연결): 실제 canvas·hook·
  client를 사용하고 HTTP 경계만 대체한다. zoom/pan 뒤 중심 클릭, lost committed reply
  이후 변경 잠금·같은 attempt 확인, 기존 선택·한 Undo/Redo를 검사한다. 실제 backend의
  canvas 연계 검증이나 화학/외부 앱 호환 증거는 아니다.
  후속 회귀는 실제 `DocumentWorkspace`의 clean Close·복구 UI·정상 discard dialog,
  dispatch 전 두 전송 실패, 미확정이 되기 전에 이미 큐에 들어간 direct close/open/create,
  graph/scene clipboard 미확정의 navigation gate, Open 뒤 stale Close를 다룬다.

## 실행 상태

구현 담당자는 공유 격리 실행 지시에 따라 Rust formatting과 diff의 정적 검토만
수행했으며 테스트·빌드·브라우저·mutation 검사를 직접 실행하지 않았다.

이후 부모가 구현 커밋 `59c8478d96ba27f332ce9a6033be88b16be7f649`를 공유 격리
환경에서 검증했다. `cargo test --locked -p chemifree-domain`, `npm run lint`,
`npm run typecheck`, `npm run test:client`(500 passed, 0 failed), `npm run build`
(웹 production build와 Rust workspace build), `npm run test:browser:ring`이 통과했다.
로그는 `/tmp/chemifree-p29d-verify-j4KjPa/p34a-targeted.log`이며 `sandbox-run: exit=0`,
peak anon 1217 MiB·page cache 3110 MiB·cap 6144 MiB·swap 0을 기록한다.
전용 browser는 scripted HTTP 경계의 canvas/hook/client 검사이며 실제 backend의
canvas 통합이나 전체 browser smoke 통과를 뜻하지 않는다.

전체 `npm run check`, Application ring 전용 테스트, HTTP=CLI 계약 smoke,
전체 browser smoke·mutation 검증·PR/병합 CI는 아직 이 기록의 통과 근거가 없다.
다음 남은 명령은 격리 실행 담당자가 확인하고 실제 결과를 추가해야 한다.

```sh
cargo test --locked -p chemifree-application --test insert_ring
npm run check
npm run test:browser
```

실패 증명 계획: closing edge 제거는 adjacency/client graph 검사, 좌표 guard 제거는
late failure 검사, snapshot 보존 검사 제거는 client 응답 tamper 검사,
request-ID 재생성·process guard 제거는 lost-response/restart 검사로 탐지해야 한다.

## 리뷰

- 작성자의 Codex 자체 검토: Domain draft/ID rollback, DTO/catalog, 웹 frozen attempt와
  성공 응답 검증, graph/scene/selection 보존 및 문서의 bounded 범위를 정적으로 검토했다.
  초기 작성 중 발견한 잘못된 closing-edge tamper와 tuple형 settings command 시험 코드는
  수정했다. `git diff --check`는 통과했으며 실행 근거는 없음.
- fresh-context 독립 subagent `/root/p34_decision/p34a_review`: base HEAD
  `10822930dc7c6114944156eb15f0a9c5a424aa6a`의 tracked diff와 새 파일 6개를
  2026-11-06T02:36:16Z에 읽기 전용으로 검토, actionable finding 없음. Domain 원자성·
  IDs/history·DTO/catalog·중심 클릭·immutable process gate·작성 검사·범위를 확인했다.
  테스트/빌드/브라우저/mutation/skill gate는 실행하지 않았다. 이후 보강한 시험은
  drag 비삽입·OS clipboard 접근 금지·terminal revision conflict의 명시 reconcile다.
- [PR #174 Codex review](https://github.com/EunhakChoi/chemifree/pull/174#pullrequestreview-5795053934)는
  `59c8478d96`의 선택 배열 순서 비교를 P2로 지적했다
  ([지적](https://github.com/EunhakChoi/chemifree/pull/174#discussion_r4488469866)).
  scene reorder 뒤 capture는 paint/DFS 순서, Rust 응답은 ID 순서여서 정상 삽입도
  미확정으로 잠겼다. objects/atoms/bonds를 기존 distinct-ID 집합 비교로 검사해
  멤버 보존·개수를 검증하고 순서 차이만 허용하도록 수정했다. 재정렬된 두 root의
  실제 `selectionSnapshot` 입력과 잃은 응답 재확인 성공·같은 request 유지 회귀,
  선택 누락/추가/교체/중복 거절 회귀를 더했다. 이 수정과 추가 회귀는 공유 격리
  검증 중인 이전 snapshot과 별개이며 아직 실행하지 않았다.
  전체 검증과 PR/병합 CI는 pending이며 위 targeted 통과로 완료 처리하지 않는다.
- [추가 Codex 지적](https://github.com/EunhakChoi/chemifree/pull/174#discussion_r4488519609)은
  dispatch 전 두 ring 전송이 실패한 clean 문서에서 Close가 복구 UI를 없애는 경로를
  지적했다. 정적으로 확인한 기존 코드에서는 `close()`도 `run()`을 거치며,
  `run()`은 큐 실행 시 `clipboardAttempt.current`를 확인한다. 따라서 먼저 큐에 넣은
  close/open도 앞선 ring이 미확정으로 남으면 HTTP를 보내지 않는 방어가 이미 있다.
  이 방어가 없었다거나 기존 코드에서 Close 성공을 재현했다고 주장하지 않는다.
  반면 실제 Close trigger에는 busy 조건이 없어 미확정 상태에도 누를 수 있었다.
  trigger와 discard action을 `editor.busy` 동안 disable해 복구 UI와 일치시켰다.
  hook의 기존 gate를 유지하고 Close의 문서 ID/리비전을 함께 캡처해, 앞서 큐에 든
  Open이 성공해도 다른 문서를 닫지 않고 lapse하게 했다.
  전용 browser 회귀는 실제 production Close의 disabled 상태·복구 후 정상 discard,
  전송 중 대기시킨 ring 뒤 direct clean/discard Close·Open·Create의 HTTP 부재,
  graph/scene 미확정의 같은 방어와 명시 결과 확인 뒤 정상 clean Close,
  성공 Open 뒤 stale Close의 대상 불변을 단정하도록 작성했다. UI disable을 제거하면
  production Close 단정, run gate를 제거하면 navigation HTTP 부재 단정,
  캡처한 ID 확인을 제거하면 새 문서 유지 단정이 실패해야 한다.
  이번 수정에서는 테스트·빌드·브라우저·mutation을 실행하지 않았다. 부모가 보고한
  이전 `78b5124` 전체 검사와 `bda7835` targeted 통과는 이 추가 수정의 실행 증거가 아니다.

### Close 보강 후 fixture 수정

부모의 `bdee128` 격리 targeted 실행은 lint/typecheck/client/build를 통과한 뒤
`scripts/web-ring-check.mjs:266`에서 실패했다. 로그는
`/tmp/chemifree-p29d-verify-j4KjPa/p34a-close-targeted.log`이며, fixture가 전송 유실 뒤
`invalid_command` 응답이면 곧바로 시도가 해소된다고 잘못 기대했다. 실제 shared
attempt는 이미 유실된 결과를 이 오류만으로 확정하지 않고 `_unconfirmed`를 유지한다.
제품의 보수적인 복구 동작을 유지하고 fixture를 수정했다. graph/scene 양쪽에서 이
응답 뒤 navigation이 계속 차단됨을 단정하고, 같은 request의 다음 확인에서 terminal
`revision_conflict`를 받아 `_reconcile`로 전환한 뒤에만 명시 authoritative reread로
잠금을 해소하고 clean Close한다. 네 번의 전송 모두 같은 body이며, 재조회한 문서의
새 revision도 단정한다. fixture 수정 이후 테스트·빌드·브라우저를 재실행하지 않았다.

Rust/TypeScript patterns/testing skills를 적용했다. shadcn의 기존 Button/Alert/ToggleGroup
구성을 재사용하며 새로운 UI primitive나 dependency는 추가하지 않았다.
