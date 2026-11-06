# P34h 기존 원자에 명시 Kekulé 6탄소 고리 연결 후보

## 범위와 결정

P34g #182 main `80e32043f0d28538bdfd236d158f8758ee43333d` 기반 별도
`feat/p34h-attach-kekule-ring` 후보다. P34g 병합은 전체 P34 완료가 아니며 P34는
**in_progress**다. 약어 표시 모델의 사용자 결정과 독립적으로, 이미 명시된 고리 패턴을
기존 single connector 연결에 제공한다. 이 기록은 이전 후보의 당시 범위·실행 기록을
덮어쓰지 않는다.

- `attach_ring`에 optional `pattern:"single"|"kekule"`을 추가한다. 생략과 명시 single은
  기존 size5/6 all-single graph·좌표 산술·notes를 유지한다. size 생략은6이다.
- 명시 kekule은 size6만 지원한다. 원래 recipe vertex순서의 cycle edge0/2/4는 double,
  edge1/3/5는 single이며 외부 connector는 항상 single이다. 6 fresh C/7 fresh bonds를
  만들고 기존 target ID/state·marks·positions는 보존한다. inferred H를 기록하지만
  H/valence를 보정하거나 방향족성·공명·CIP·입체를 추론하지 않는다.
- null/unknown/non-string pattern은 invalid_request다. size5+kekule은
  invalid_command/ring_pattern_invalid와 command index로 전체 거절한다. Domain 검증과
  P34g의 typed ring refusal mapping을 재사용한다. 기존 명시 target/유일 placement·
  finite nonzero direction·현재 bond length·좌표/entity/ID/command 한도를 유지한다.
- 마지막 일반 insert_bond가 target center/incident double configuration을 지우고 group을
  prune/report하는 기존 notes와 partial/ok 규칙을 유지한다. 다른 stereo/state/scene/
  references/selection/settings는 그대로다. 늦은 실패도 counters/redo/history까지 rollback하며
  Undo/Redo/save/reopen/replay는 기존 transaction 경로다. catalog/명령 수는 변하지 않는다.
- Attach size6에도 기존 pattern ToggleGroup을 보인다. 기존 Attach 진입 single reset과
  size5 전환 single reset을 유지한다. 사용자가 Attach 후 Kekulé를 명시 선택한다.
  mode/size/pattern은 busy/drag 중 잠근다. canvas drag state가 press의 size/pattern을
  보관하고 release 때 그 값을 callback에 준다. prop rerender도 이를 바꾸지 못한다.
- hook은 captured pattern을 큐와 immutable ring attempt에 넘긴다. client는 정확한 cycle
  phase·single connector·기존 내용·stereo/group notes를 검증한다. 응답 분실은 같은
  body/request/process만 재확인하고 restart/conflict는 명시 reconciliation으로 멈춘다.
  OS clipboard·새 DATA/provider/DLL·schema·약어·fusion/spiro/layout는 추가하지 않는다.

## 작성한 회귀

- Domain attach_ring: hard-coded edge/ID/order와 connector oracle, 두 독립 위치 기대값,
  target state·한 단계 history·Undo 후 fresh ID, size5/zero/nonfinite direction 거절,
  늦은 geometry와 마지막 connector ID 실패의 prior command/counters/redo 보존이다.
  기존 single의 방향·현재 length·missing target/placement 경계 검사는 유지한다.
- Application attach_ring: 기존5/6/default/explicit single과6 Kekulé를 singleton/non-singleton
  stereo group에 적용한다. 일반 add_atom/add_bond를 독립 oracle로 사용해 exact bonds와
  notes·무관한 stereo/group 보존, 늦은 transaction 실패 rollback, request replay/reuse,
  Undo/Redo/save/reopen을 검사한다. malformed/null/size5 조합의 완전 snapshot/redo 보존과
  omitted/explicit single DTO 일치를 추가한다.
- P34g의 과거 Attach pattern 자체 거절 fixture는 이제 unknown aromatic pattern 거절로
  바꾼다. production 독립 ring browser의 Attach 진입은 새 지원에 맞춰 control 숨김 대신
  기존 single reset을 확인한다. 새 지원을 이유로 기존 기본 body 단정은 완화하지 않는다.
- Client: default/explicit single/6 Kekulé lost reply의 exact body, unrelated state와
  stereo notes, runtime-invalid의 HTTP 비전송, phase/connector/notes 변조 거절,
  restart/conflict 이후 미전송을 검사한다. 기존 test:client 연결을 사용한다.
- 실제 HTTP=CLI contract-smoke: attached6 Kekulé의 정확한7개 결합 order·기존 그래프 보존·
  replay·malformed/null/size5 reason/index·Undo/Redo를 추가한다.
- production browser attach-ring: 기존5 single과6 Kekulé의 실제 선택·atom drag·busy/drag
  mode/size/pattern 잠금·응답2회 분실·same-body recovery·stereo notes·Undo/Redo다.
  별도 실제 canvas harness는 pointer down6/kekule 뒤 caller props를5/single로 바꾸고
  release callback이 여전히6/kekule인지 확인한다. HTTP boundary fixture이며 real backend
  browser라고 주장하지 않는다. 기존 test:browser:attach-ring과 CI 연결을 사용한다.

## 실행과 리뷰 상태

부모 지시로 **테스트·빌드·브라우저·mutation·push는 실행하지 않았다**. rustfmt는 소스
서식 편집으로만 실행했다. 결과 통과·test-failure proof·새 통합 검증은 없다. 후속 담당자는
`npm run check`, `npm run test:browser:attach-ring`, `npm run test:browser:ring`과 표준
browser/routes를 통합 snapshot에서 실행하고 실제 결과를 기록해야 한다.

Rust/TypeScript patterns·testing 지침을 적용했고 금지된 gate는 실행하지 않았다. shadcn
skill로 기존 ToggleGroup을 재사용하고 CLI docs 및
[공식 Base UI 문서](https://ui.shadcn.com/docs/components/base/toggle-group)를 확인했다.
dependency/registry component 설치는 없다.

- 구현자 자체 Codex 정적 검토: Domain recipe/connector와 DTO→client→canvas press/release→
  hook queue/frozen recovery→회귀→문서를 대조했다. 기존 single body/geometry·일반
  AddBond stereo/group notes와 기존 graph 보존, 새로운6 Kekulé의 phase와 single connector,
  pattern/size capture를 읽기 전용 검토했다. 결함을 찾지 못했으며 실행 검증의 대체가 아니다.
- fresh-context 독립 subagent `/root/p34g_decision/p34h_review`: 2026-11-06 08:20:59 UTC,
  base full SHA 위의 local uncommitted code/tests/docs diff를 읽기 전용 검토했다. **finding
  없음**. 기존 default·single-connector stereo/group·atomic transaction·gesture/recovery
  choices 보존을 확인했고 미정 약어 모델의 임의 결정이나 다른 scope 확장을 찾지 못했다.
  지적 처분은 없으며 테스트/빌드/브라우저/mutation/push는 하지 않았다. 이후 자기 검토에서
  client 변조 회귀 이름만 실제 단일 edge 변조에 맞춰 명확히 했다.
- `git diff --check`는 공백 오류 없이 끝났으며 실행 검증이 아니다. PR Codex 리뷰와
  실제 실행 검증은 부모 후속 단계에서 **pending**이다. 후속 main 통합/CI까지 완료로
  보고하지 않는다.
