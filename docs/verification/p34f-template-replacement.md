# P34f 말단 atom template replacement 후보

## 범위와 결정

P34e HEAD `7ce1c66`에서 분리한 `feat/p34f-template-replace` 후보다. P34는
in_progress다. 다음 작은 slice를 기존 모델로 가능한 말단 교체로 정했으며, 새 약어 모델을
흉내 내지 않는다. AtomKind는 Element/Query이고 scene과 저장 v7에는 collapsed-label,
member graph, boundary mapping 객체가 없다. nickname DATA는 선언형이며 활성 약어 엔진이
아니다. 이번 작업은 schema·외부 DATA·원본 geometry/DLL을 추가하지 않는다.

- 단일 connected native v1 source·explicit source anchor/target/instance/revision/selection,
  정확한 style·한 destination placement를 받는다. target은 incident single 하나만 가능하다.
  고립 원자·degree≥2·다른 bond/query boundary는 template_target_not_terminal_single이다.
- target ID와 position을 남기고 완전 원자 state를 source anchor로 바꾼다. 나머지 N−1
  atoms와 M internal bonds만 fresh IDs다. 외부 bond ID·양끝·mark·좌표·참조와 neighbor
  center/double/group이 그대로다. 삭제→재삽입보다 boundary identity를 보존하기 위한 결정이다.
- source anchor의 stored center/incident double(unknown 포함)는 P34e처럼 거절한다.
  다른 source 입체는 공유 native planner의 Id/Created binding을 거쳐 기존 Set* 명령으로
  canonical remapping한다. parity·double reference 변경을 보존하며 입체를 추론하지 않는다.
- source anchor는 target의 기존 좌표에 바인딩하고 다른 source 좌표만 동일 offset으로
  평행이동한다. 새 connector·direction·rotation·반사·scale·현재 bond length 정규화는 없다.
  target의 이전 state가 명시적으로 대체되며 H/valence/attachment metadata를 보정하지 않는다.
- document.template_replace_plan은 revision lock에서 clone을 검사하고 실제 source와
  candidate/mapping/boundary를 준다. preview는 live snapshot provenance가 없다.
  document.template_replace는 보통 mutation gate/replay 아래 같은 계획을 재검증한다.
  변경은 한 Undo 단계이며 captured selection을 Undo하고 새 graph를 Redo한다. 동일한
  단일 원자 state는 기존 no-op으로 revision/history/dirty/selection/counters가 그대로다.
- source→target mapping은 bound anchor를 fresh selection 순서에서 추측하지 않는다.
  source raw array 순서와 fresh IDs를 결합하고 anchor는 명시 ID에 매핑한다. 저장 schema와
  transaction command vocabulary는 그대로이며 catalog만 40개/기본 executable30개가 된다.
- 실제 웹 file 검증→빈 source/target 선택→before/after state와 retained boundary preview→
  Replace atom with template이다. source/target 변경 및 document/revision/style/process는
  offer를 무효화한다. 기존 immutable template gate와 닫힌 뒤 recovery를 재사용한다.
  OS clipboard에는 접근하지 않는다.

## 작성한 회귀

- Domain template_attachment.rs: non-first source anchor/old ID binding·독립 좌표/ID 기대값,
  outside mark·target position·counter 증분·history·source 불변, 반복 boundary/source-stereo
  거절, 정확히500/501 command와 late bond-ID 실패의 bound-state rollback. 실제 source
  center의 한 번 slot swap에 따른 turn flip을 hand-stated canonical 결과로 검사한다.
- Application template_attachment.rs: native source array rotate·target의 모든 state overwrite,
  기존 outside hash/incoming atom/bond refs·neighbor stored center/double/group, copied unknown
  double·metadata/mapping/selection·clone preview 불변·replay/reuse refusal·Undo/Redo·save/reopen.
  반복 nonterminal 거절과 identical one-atom no-op도 실제 Application에서 검사한다.
- test:client의 web-template-replacement-client-check.mjs: 독립 hand-stated center/relative-double
  canonical remapping과 old neighbor statements/groups/refs, source/candidate provenance,
  boundary/mark/ref/state/position/statement 변조 거절, immutable body·lost reply·restart/conflict,
  단일 원자 no-op을 검사한다. 기존 P34e client 회귀도 유지한다.
- test:contracts: 실제 HTTP=CLI preview/apply·repeated refusal·mapping·retained bond·history/replay.
  공통 check에 이 세 근거가 있으므로 OPS-01.template-replacement는 partial로 연결한다.
  P34d user-file 부분의 별도 browser-only unverified 판정은 바꾸지 않는다. 집계는 부분109개
  (implemented60/partial13/unverified3/not_implemented33)이며 상위33·conformance109는 같다.
- test:browser:user-template: 기존 실제 다운로드 파일을 replacement dialog에서 선택하고,
  부적격 internal target disable·explicit preview 불변/target 변경 무효화·실제 commit 뒤 두
  transport reply loss와 동일 body 확인·boundary/old double 보존·독립 좌표·history/save/reopen을
  검사한다. harness는 navigation/selection만 제공하며 production panel/hook/client/backend를
  사용한다. 전체 canvas navigation이나 외부 앱 compatibility 검증으로 확대하지 않는다.

**부모 지시로 테스트·빌드·브라우저·mutation·push는 실행하지 않았다.** rustfmt와
git diff --check만 수행하며 작성한 테스트·gate의 실행/반증·full/CI는 pending이다.
Rust patterns/testing·TypeScript patterns/testing·API design·shadcn 지침을 적용했다.
기존 모듈의 Id/Created mapping을 공유해 새 복제 stereo 알고리즘을 만들지 않았다.

## 리뷰

- Codex PR 리뷰: PR 미생성으로 pending이며 부모가 PR 단계에서 1회 수행한다.
- fresh-context 독립 subagent `/root/p34d_decision/p34f_review`: 2026-11-06
  07:15:35–07:17:17 UTC, base `7ce1c66e14ca712b402beb754203e630919757b5`의
  tracked23개/untracked3개 local diff 전체를 읽기 전용으로 검토했다. tracked diff SHA-256은
  `4ae41769a87ce4e15fcf7ad56013024eeb7148e501d89ffedee0c271892c6669`다.
  요구사항 임의 보충·state/stereo/ID 보존·no-op/거절·스킬 품질에 수정할 P0–P3 finding은
  없었다. source/schema/약어 범위 확장도 찾지 못했다. 실행 검증을 하지 않았으며 새 main
  통합·full/CI가 필요하다는 한계를 기록했다. 수정 뒤 추가 리뷰는 요청하지 않는다.
- 검토 후 private helper 이름을 `created_atom`에서 `mapped_atom`으로 바꿔 existing/Created
  두 binding을 나타내고 replacement preview의 복사된 doc comment를 바로잡았다. 동작 변경은
  없으며 review finding 처분을 위해 별도 재리뷰를 요청하지 않는다.

## 선행 main 통합

구현을 `821e669`로 커밋한 뒤 origin/main `1618988`(#180 P34e squash)을 fetch/merge했다.
공통 Git ancestor가 P34d여서 P34e 추가 파일·줄이 다수 충돌했지만, 승인 base `7ce1c66`과
최신 main의 차이는 P34e 검증 기록20줄과 contract fixture request_id fix뿐임을 대조했다.
P34f source/검사/40개 catalog와 main의 P34e 거절 reason·기존38개 catalog fix를 보존했다.
또 main의 request_id 형식 단정 및 plan/attach의64자 이내 fixture IDs를 적용하고 P34f의
추가 replacement HTTP=CLI flow를 유지했다. 양쪽 문서와 browser/공통 검사 연결도 보존했다.

통합 담당자는 rustfmt와 git diff --check만 실행했다. 테스트·빌드·브라우저·mutation·push,
skill gates 및 통합 full/CI는 실행하지 않았으며 부모 검증 대기다. 과학 기능/배포 완료나
P34 완료로 기록하지 않는다. PR Codex 리뷰도 부모 단계에서 pending이다.

## 부모 첫 targeted의 compile 실패와 조회 API 정정

- 부모가 통합 `f93b59b`의 첫 격리 targeted 검사를 시작했으나 Domain library compile에서
  E0599로 중단됐다. `clipboard.rs:296`의 `destination.atom(target)`은 실제 Document에
  없는 메서드다. 로그는 `/tmp/chemifree-p29d-verify-j4KjPa/p34f-targeted.log`다.
  최초 정적 리뷰와 rustfmt는 이 타입 오류를 찾지 못했으며 컴파일 통과 근거가 아니다.
- 실제 공개 API `Document::atoms()`는 `(AtomId, &Atom)` iterator를 준다. 존재 검사는
  `atoms().any(...)`로 정정했다. 새 Rust source/tests의 단건 조회를 대조하면서 같은
  잘못된 호출을 test assertion 한 곳에서도 찾아 `atoms().find(...).map(...)`으로 고쳤다.
  assertions의 atom-state 기대값과 거절 경계는 그대로다. 새 단건 조회 API를 추가하지 않았다.
- 새 코드에서 쓴 bond/position/numbering/first_instance 및 transaction/history 조회/수정
  호출도 실제 Document 선언과 대조했다. 남은 `.atom(...)` 호출은 repository Rust 범위
  검색으로 다시 확인한다. 이 수정 담당자는 rustfmt와 diff whitespace만 확인하며,
  테스트·빌드·브라우저·push는 실행하지 않는다. 수정본 compile/targeted/full/CI는 부모
  재검증 대기이며 정적 대조만으로 전체 compile 성공을 주장하지 않는다.

## 두 번째 targeted의 Application fixture request_id 정정

- 부모의 `561f17a` targeted2에서 Domain 검사는 통과했고 Application template 검사6개 중
  5개가 통과했다. 반복 boundary 거절 검사는 expected reason에 실제 Null이 나와 실패했다.
  로그는 `/tmp/chemifree-p29d-verify-j4KjPa/p34f-targeted2.log`이며 전체 성공은 아니다.
- 원인은 fixture가 operation 문자열 `document.template_replace[_plan]`을 request_id에도
  사용한 것이다. request_id의 점은 기존 `[A-Za-z0-9_-]{1,64}` 규칙 밖이므로 execute가
  Domain boundary 검사에 도달하기 전에 protocol_error/invalid_request를 반환한다.
  따라서 details.reason이 없었으며 실제 target a1의 degree3 boundary 계약은 바뀌지 않는다.
- 두 operation은 서로 다른 유효한 fixture ID로 바꾸되 반복 호출에는 동일 ID/body를
  유지했다. 기대값을 약화하지 않고 operation_result/error/invalid_command envelope까지
  먼저 단정하고 실패 시 전체 응답을 출력하도록 했다. 공유 request helper도 fixture ID
  형식을 진입 전에 단정해 같은 실수가 Null reason으로 가려지지 않도록 했다.
- 제품 parser·Domain 거절·no-op 의미는 변경하지 않았다. rustfmt·git diff --check만
  수행하고 테스트·빌드·브라우저·push는 실행하지 않았다. 수정본 Application 검사와 뒤의
  no-op 경로 및 나머지 targeted/full/CI는 부모 재검증 대기다.

## full 검사 catalog 고정 기대값 실패와 전체 대조

- 부모의 `d1e71b1` full은 stereo_group_wire의 catalog 개수 단정에서 실제40/기대38로
  실패했다(18개 중17개 통과). 로그는 `/tmp/chemifree-p29d-verify-j4KjPa/p34f-full.log`다.
  P34f 두 연산을 추가할 때 다른 catalog 단정을 빠뜨린 것으로 전체 성공이 아니다.
- stereo_group_wire는 정확히40개로 고치고 replacement plan/apply가 각각 한 번씩
  supported/executable로 존재함을 단정했다. HTTP=CLI contract catalog에도 정확히40개와
  같은 두 연산의 유일성/표현/실행 단정을 더했다. 기존 group-command/capability 검사는
  그대로 유지한다. 개수를 동적으로 맞추거나 범위 비교로 약화하지 않았다.
- 프로젝트 전체 rg 대조 결과:
  - OperationId::ALL의40개 고정 배열과 string mapping은 두 연산을 이미 포함한다.
  - application/tests/operations.rs의 total40과 기본 executable 고정 목록30개는 이미
    두 연산을 각각 포함하며, catalog 표기와 실제 dispatcher 결과를 비교한다.
  - scripts/contract-smoke는 새 replacement preview/apply/repeated refusal/history flow를
    이미 포함한다. 새 catalog 단정도 더했다. transaction parts의48/37/11은 별도 수이며
    새 operation 두 개는 transaction command가 아니므로 바꾸지 않는다.
  - standard browser summary는 이미 `30 of 40 operations run`, README/current CONTRACT는
    40/30이다. HANDOFF 최상단에도 현재 수를 명시했다. codec/query/CIP catalog 검사는
    개별 기능을 찾아 비교하며 별도 stale 전체 개수/operation 고정 목록은 없다.
  - 웹 catalog는 받은 목록의 길이/실행 수로 표시하고 고정 숫자를 갖지 않는다.
    conformance의 catalog/operation 사례에도 오래된 전체 개수 단정이 없다.
  - P34e 등 과거 검증/당시 scope의38/28 기록은 역사적 근거이므로 현재40으로 덮지 않았다.
- 이 수정 담당자는 rustfmt·git diff --check와 읽기 전용 source/문서 대조만 수행했다.
  테스트·빌드·브라우저·push는 실행하지 않았으며 수정본 full/CI는 부모 재검증 대기다.
