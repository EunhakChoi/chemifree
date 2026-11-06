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

선행 P34e PR 완료 후 최신 main 통합과 실제 검증은 부모가 이어간다.
