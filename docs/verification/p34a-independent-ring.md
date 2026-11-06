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

## 실행 상태

이 구현 작업에서는 사용자/부모가 공유 격리 실행을 맡도록 지정해 **테스트·빌드·
브라우저 및 mutation 검사를 실행하지 않았다**. 검사 작성은 통과 증거가 아니다.
Rust formatting과 diff의 정적 검토만 수행한다. 실행 담당자는 다음을 격리 환경에서
확인하고 실제 결과/변이 근거를 이 기록에 추가해야 한다.

```sh
cargo test --locked -p chemifree-domain --test insert_ring
cargo test --locked -p chemifree-application --test insert_ring
npm run check
npm run test:browser:ring
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
- PR Codex review와 최종 검증·CI: 아직 요청/실행하지 않았다.

Rust/TypeScript patterns/testing skills를 적용했다. shadcn의 기존 Button/Alert/ToggleGroup
구성을 재사용하며 새로운 UI primitive나 dependency는 추가하지 않았다.
