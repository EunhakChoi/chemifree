# P34e 명시 native template anchor 연결 후보 기록

## 결정과 경계

P34d HEAD `8113f0f`를 기반으로 분리한 `feat/p34e-template-attach` 후보다. 구현 커밋은
`4ba2c8b58c9dcf583647062473d97d078c955f5d`이며 이후 main `91938d2`를 통합했다. P34는 in_progress이며 납품·과학 기능 완료를
주장하지 않는다. `.chemifree-fragment.json`의 native v1 본문과 storage schema는 그대로다.

명시 source anchor 하나와 destination atom/instance, finite nonzero direction으로 native
fragment를 평행이동하고 single 하나를 연결한다. N fresh atom/M fresh internal bond와
fresh connector 하나이며 기존 원자/결합을 공유·대체하지 않는다. source는 단일 연결성분
(원자 하나 허용), destination은 단일 명시 배치와 정확히 같은 style이어야 한다.
source-anchor stored center 또는 incident stored double record는 unknown도 포함해 거절한다.
기존 `Applied.cleared`가 transaction 이전에 존재한 record만 notes로 보고하므로, 복사한
record를 같은 transaction에서 잃는 경로를 열지 않기 위한 bounded 정책이다.

그 외 source 상태/marks/stereo는 기존 native paste remapping이다. destination clearing,
group prune와 notes는 마지막 ordinary AddBond 그대로이며 valence gate/H 보정/CIP/입체 유도는
추가하지 않는다. attachment_points는 보존만 하고 읽어 연결 지시로 소비하지 않는다.

direction은 max-component scale 뒤 hypot로 정규화한다. target 최신 위치+현재 bond_length
방향의 점에 source anchor를 놓도록 모든 source 좌표에 같은 offset을 더한다. 회전·반사·
scale·collision/layout/conformation이나 numbered polymer semantics는 없다.

`document.template_inspect`는 source decoder/연결성/anchor 적격성만 읽는다.
`document.template_plan`은 exact destination revision 아래 clone에 같은 commands/selection을
적용하고 원본 source snapshot과 candidate snapshot, mapping/connector/예상 notes를 반환한다.
어떤 ID/history/selection도 예약하거나 변경하지 않는다. `document.template_attach`는
revision/mutation gate에서 다시 계획·검증하고 한 Undo 단계로 실행한다. replay는 payload,
두 anchor/instance/vector/revision/selection을 fingerprint로 묶으며 이전 응답은 Undo 뒤에도
불변이다. fresh fragment atoms/internal bonds만 선택하고 connector/target은 자동 선택하지 않는다.

웹은 file 검증→빈 source/destination 선택과 명시 dx/dy 입력→Preview template connection→
Attach template이다. 방향/anchor를 추측하지 않도록 numeric vector를 명시 입력하는 bounded
UI를 선택했다. 입력/file 변화는 offer를 지우고 document/style/revision/process가 바뀌면
adoption을 거절한다. candidate preview에는 live snapshot provenance를 주지 않으며 실제
apply만 editor가 채택한다. 기존 immutable template/paste gate를 재사용하고 닫은 dialog
밖의 recovery controls를 유지한다. OS clipboard에는 접근하지 않는다.

client는 기존 destination state/geometry, source atom/bond 속성·marks·translation과 완전한
copied stereo remapping, destination clearing/group·예상 notes 집합을 비교한다. apply는
frozen preview/document/notes/mapping/connector와 같아야 한다. 유실/비호환 성공은 미확정으로
유지하고 같은 request/body/process만 확인한다. restart/conflict에는 명시 reconcile이며
새 process 재전송을 하지 않는다.

fusion/spiro/replace·복수 anchor·단절 fragment 연결·약어·template library·자동 회전/layout·
외부 DATA/원본 geometry/DLL은 범위 밖이다. 기존 P34d 독립 파일 삽입은 유지한다.

## 작성한 검사와 근거의 한계

- Domain `template_attachment.rs`: 3:4 방향의 독립 예상 좌표(28,44)/(58,44)/(88,44),
  fresh graph/mapping과 비선택 connector, source/기존 target 불변·history를 비교한다.
  source incident double 및 center anchor 거절, neighbor anchor에서 unrelated center 보존,
  단절/대상/배치/zero·NaN direction, huge/subnormal 정규화·좌표 한도, connector를 포함한
  500/501 command 경계와 final-link ID exhaustion을 검사하도록 작성했다.
- Application `template_attachment.rs`: 실제 native copy payload의 isotope/charge/H/radical/
  attachment_points/mark·stored-double 보존, read-only inspect/clone plan의 snapshot 불변과
  apply 동일성, destination center/double/group clearing 및 기존 scene/reference 보존,
  mapping/selection·immutable replay/reuse refusal·Undo/Redo·save/reopen을 작성했다.
  일반 native paste→AddBond를 별도 oracle로 실행해 graph/stereo/notes를 비교한다. 이 oracle의
  두 history step을 atomic attachment의 한 단계 증거로 혼동하지 않는다.
- 실제 HTTP=CLI contract smoke에 inspect/plan/apply·preview 불변·mapping/connector·zero 방향
  거절·replay/Undo/Redo 동등성을 작성했다. catalog는 additive 38 operations이며 기본 host의
  표준 browser summary 기대값도 28/38로 맞췄다. transaction sub-operation 수는 그대로다.
- `web-template-attachment-client-check.mjs`를 test:client에 연결했다. candidate/live provenance
  분리, frozen source/vector/body/request·late source mutation, lost reply/restart/conflict,
  wrong connector/state/translation/old mark/missing notes·snapshot 차이 거절을 작성했다.
  비정렬 source IDs의 tetrahedral odd parity와 relative-double 단일 reference 변경의
  hand-stated canonical 결과를 수용하고 turn/relation/known→unknown 변조는 거절한다.
- 기존 `test:browser:user-template`의 real backend File/Download→production panel/hook/client
  script에 explicit source/target/vector·unsupported-anchor option·readonly preview·input
  invalidation, actual attach 두 reply loss 뒤 same-body 확인·selection/coordinates·Undo/Redo·
  source 불변·save/reload/reopen을 더했다. 기존 CI 명령에서 실행되도록 작성했을 뿐이며,
  이번 작업에서 브라우저나 HTTP/CLI 검사를 실행하지 않았다.

## 선행 main 통합

부모 지시에 따라 구현/리뷰 처분 커밋 이후 origin/main `91938d2`(P34d #178)를 fetch/merge했다.
P34c #177·smoke teardown #179(`02def38`)도 포함한다. squash 겹침은 P34d 작성 HEAD
`8113f0f`를 기준으로 양쪽 변경을 대조했다. main의 null-mode form 비표시·단일 recovery,
save/insert FieldGroup key와 controlled/uncontrolled 회귀·modal 외부-event harness·diagnostics,
request-log teardown 및 lifecycle 검사·package 명령을 모두 보존했다. 새 attach form에도
독립 key를 주고 닫힌 뒤 단일 recovery를 region 안에서 확인하도록 맞췄다. P34e의 Domain/
Application/client/HTTP=CLI/실제 template 연결 browser 단계와 공개 문서를 보존했다.

P34d 전용 browser 근거를 적격 공통 evidence로 승격하지 않는 main의 unverified 부분·집계·
실패/성공 기록도 그대로 유지한다. 이 통합에서 테스트·빌드·브라우저·mutation은 실행하지
않았다. P34e current integrated head의 full/PR CI/병합 CI는 pending이다.

## 첫 통합 HEAD의 targeted 실패와 fixture 수정

부모 실행 담당자가 `810729e70d0bef70d58f3f4f3d40f4d874311dc8`를 격리 실행했다.
`/tmp/chemifree-p29d-verify-j4KjPa/p34e-targeted.log`에서 Domain template_attachment는
5/5 통과했고 Application template_attachment는 0/2 통과·2개 실패했다. 실패 원인은
source-seed의 public add_atom에 지원하지 않는 charge/isotope 필드를 넣어 decoder가
`invalid_request`(unknown field charge)를 반환한 것이다. 전체 명령은 exit101로 끝났으며
뒤의 lint/typecheck/client/build/browser 통과 근거는 없다.

fixture는 O 원자를 public add_atom의 지원 필드로 만든 뒤 같은 transaction의
update_atom a2에서 charge -1/isotope18을 설정하도록 고쳤다. 원자/결합 IDs와 단일 seed
history, H/attachment_points·mark·stored-double 및 이후 보존 검사는 그대로다. 원본 O의
charge/isotope가 실제 의도한 값인지 별도 기대값도 추가했다. 새 Application/HTTP=CLI/
file-browser seed의 나머지 add_atom 필드와 update_atom DTO를 정적으로 대조했으며,
native payload에 필요한 charge/isotope 완전 상태 필드는 제거하지 않았다.

작성자는 로그를 읽고 fixture/문서만 수정했으며 테스트·빌드·브라우저·push를 재실행하지
않았다. 수정 HEAD의 Application 이후 검사·current full/CI는 부모 재실행 대기다. 이
fixture 실패 이전에 기능 검사가 통과했다거나 수정 후 통과했다고 주장하지 않는다.

## 두 번째 targeted 결과와 TypeScript narrowing 수정

부모가 fixture 수정 HEAD `695be66056a858f35da4edf78d1941f9a7e6866f`를 격리 실행했다.
`/tmp/chemifree-p29d-verify-j4KjPa/p34e-targeted2.log`에서 Domain 5/5, Application 2/2,
npm run lint는 통과했다. typecheck는 document-contract.ts의 두 copied atom/bond lookup에서
callback 안의 mutable record.id가 unknown으로 돌아가 Map.get(string)에 전달될 수 없다는
TS2345로 실패했다. sandbox exit2이며 이후 client/build/browser 통과 근거는 없다.

기존 runtime isRecord/id-string/좌표/kind 검사를 모두 유지하고, 검증 직후 Map lookup을
동기 실행해 const copiedId에 담은 뒤 find callback에서는 그 결과만 캡처하도록 수정했다.
unknown을 assertion/any로 우회하거나 잘못된 record를 허용하지 않는다. 같은 검증 함수의
다른 callback 경계도 정적으로 대조했다. 작성자는 typecheck·테스트·빌드·브라우저·push를
실행하지 않았다. 수정 HEAD의 재검증과 full/CI는 부모 실행 대기다.

## 실행 대기

부모의 공유 격리 실행 지시에 따라 테스트·빌드·브라우저·lint/typecheck/clippy·mutation·
push를 실행하지 않았다. `cargo fmt --all`과 `git diff --check`만 수행했다. 선행 slice의
통과를 이 변경의 근거로 삼지 않고 적격 evidence/상위 상태/집계를 승격하지 않는다.
현재 후보의 full/PR CI/병합 CI는 모두 pending이다. 남은 명령은 실행 담당자가 확인한다.

```sh
cargo test --locked -p chemifree-domain --test template_attachment
cargo test --locked -p chemifree-application --test template_attachment
npm run check
npm run test:browser:user-template
npm run test:browser
```

실패 증명 계획: connected/anchor-stereo guard 제거→Domain 거절 검사, connector 누락→graph/
mapping·HTTP parity, source remapping/translation 변경→Domain/client/state 검사, destination
clearing 우회→ordinary AddBond oracle/notes, copied stereo ID-only 비교→turn/relation tamper,
request/process 변경→immutable/restart 검사다. mutation은 실행하지 않았으며 이 계획을
검사 통과로 세지 않는다.

## 리뷰와 처분

- Codex 작성자 정적 검토: Domain fragment/style/connectedness/anchor/stereo/limits와 clone
  plan·atomic mutation·fingerprint/replay, API/catalog, preview/live provenance와 동일 response
  검증, UI blank choices/input invalidation/closed dialog recovery를 대조했다. 작성 중
  dispatcher의 잘못된 변수명을 known.operation으로 정정하고 finite 좌표·operation summary
  기대값·source/destination status를 보강했다. read-only plan의 envelope revision은 candidate가
  아닌 current source를 보고하도록 정정하고 Application/HTTP=CLI/client 검사를 맞췄다.
  실행 근거는 없다. PR Codex review는 대기다.
- 독립 fresh-context `/root/p34b_decision/p34e_review` 1회: base8113f0f tracked diff와 새
  Domain/Application/UI/client·작성 회귀를 읽기 전용 검토했다. backend는 기존 의미와
  맞고 arbitrary scope expansion 추가 지적은 없었다. P2 하나는 copied-stereo response
  검사가 ID 집합만 비교하여 turn/relation/known→unknown 변조를 수용할 수 있다는 점이다.
  이를 수용해 source→fresh mapping의 완전한 canonical slot parity/reference/relation
  비교를 추가했고, 비정렬 IDs의 유효 canonical 변화 수용 및 변조 거절 회귀를 작성했다.
  이 수정과 이후 HTTP/CLI·공개 문서·oracle 보강은 미실행이며 추가 리뷰 라운드는 하지 않았다.
  reviewer도 tests/build/browser/mutation/mechanical gate를 실행하지 않았다.
- Rust patterns/testing·TypeScript patterns/testing·API design·shadcn·software-design-philosophy
  skills를 적용했다. 기존 Select/Field/Dialog/Button을 재사용하며 dependency/provider/framework를
  추가하지 않았다. 설계 진단은 10/10, 실패 행 없음이다. 실제 skill gate/mutation proof는
  위 지시로 미실행이다.
