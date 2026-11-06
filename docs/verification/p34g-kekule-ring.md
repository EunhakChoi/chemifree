# P34g 독립 Kekulé 6탄소 고리 후보

## 범위와 결정

P34f HEAD `3b3b2a7cdc6187ee47c6f7736c6cf504bdfb9b98` 기반 별도
`feat/p34g-kekule-ring` 후보다. 약어 표현은 사용자 의미 결정 대기이며 이 후보는
방향 변경 없는 명시 고리 recipe만 추가한다. P34 전체는 **in_progress**다.

- 기존 `insert_ring`에 optional `pattern:"single"|"kekule"`을 추가한다. 생략과 명시
  single은 기존 size5/6 all-single 동작·geometry를 유지한다. kekule은 독립 size6에만
  가능하며 오른쪽 vertex0부터 시계방향 edge0/2/4는 double, edge1/3/5는 single이다.
  고리 선택·결합 순서는 명시 recipe이며 방향족성·입체·공명·최적 geometry를 추론하지 않는다.
- null/unknown/non-string pattern은 invalid_request다. size5+kekule은 Domain의
  invalid_command/ring_pattern_invalid로 transaction 전체를 거절한다. AttachRing은
  pattern을 받지 않으며 기존 all-single recipe다. DTO·Domain·client가 같은 경계를 가진다.
- 단일 기존 placement·현재 bond length·neutral natural-abundance C6·inferred H·fresh
  atom/bond IDs·한 Undo 단계·atomic rollback·replay/save/reopen을 재사용한다. `$N` 생성,
  새 schema/provider/DATA/DLL, 약어·fusion/spiro·layout·conformation·입체 유도는 없다.
- 실제 Ring UI는 independent size6에서만 All single bonds / Kekulé alternating bonds를
  보인다. 기본은 single이며 size5 또는 Attach 전환 시 single로 되돌린다. busy/drag 중
  control을 잠그고 클릭 시 captured pattern을 queue와 immutable attempt에 고정한다.
  client는 정확한 alternating phase까지 확인하며 lost reply는 같은 body/request/process만
  재확인한다. 새 process로 재전송하지 않으며 OS clipboard를 쓰지 않는다.
- catalog 연산 수와 transaction 명령 수는 변하지 않는다. insert_ring의 지원 설명만 고친다.

## 작성한 회귀와 실행 대기

- `crates/domain/tests/insert_ring.rs`: 독립 edge/ID/order oracle, 현재 기본 geometry,
  기존 default constructor의 명시 single, 한 Undo/Redo 단계, size5 거절, late coordinate/ID
  실패의 prior command/counters/redo 보존, atom budget at/over 및 없는/복수 placement 거절.
- `crates/application/tests/insert_ring.rs`: 기존 omitted/5/6에 explicit single과
  omitted/explicit6 Kekulé를 더해 기존 stereo/scene/selection 보존·replay·요청 ID 재사용
  거절·Undo/Redo·save/reopen을 검사한다. null/unknown/non-string과 size5/Attach 거절의
  revision/history/selection/redo 무변경을 검사한다.
- `scripts/web-ring-client-check.mjs`: exact request body/phase, single↔Kekulé 오응답,
  역 phase 거절, runtime-invalid 입력의 HTTP 비전송, lost reply 동일 body 확인과
  restart 이후 미전송이다. 기존 `npm run test:client` 경로를 사용한다.
- `scripts/contract-smoke.mjs`: 실제 HTTP=CLI에 Kekulé 삽입·정확한 orders·기존 결합 보존·
  replay·null/unsupported/size5 거절·Undo/Redo를 추가한다.
- `scripts/web-ring-check.mjs`: 실제 production workspace의 기본 선택·명시 Kekulé·
  size5/Attach 전환 reset·응답 전송 실패2회·pending 중 controls/Close 잠금·동일 request
  복구·Undo/Redo를 작성했다. HTTP는 boundary fixture이며 real backend browser라고
  주장하지 않는다. 기존 `npm run test:browser:ring` 및 CI 연결을 사용한다.

부모의 명시 지시로 **테스트·빌드·브라우저·mutation·push는 실행하지 않았다.**
rustfmt는 소스 서식 편집으로만 실행했다. 기능 검증 통과나 test-failure proof는 없다.
`git diff --check`는 공백 오류 없이 끝났으며 실행 검증이 아니다.
후속 실행 담당자는 `npm run check`, `npm run test:browser:ring`, 표준 browser/routes 및
기존 attach-ring/template 회귀를 적절한 통합 snapshot에서 실행하고 실제 결과를 기록해야 한다.
새 checks가 통과하기 전 납품 완료나 P34 전체 완료로 기록하지 않는다.

## 리뷰와 disposition

- 구현자 자체 Codex 정적 검토: 로컬 diff를 Domain recipe→DTO→client response validation→
  queue/frozen recovery→production control→회귀→계약 순서로 읽었다. 기존 default request
  body/6원 좌표 산술·Attach single 유지, 정확한 교대 phase, unsupported 조합 거절과
  pending 동안 요청 의미 고정을 대조했다. 새 domain 회귀의 StereoTable과 persistable Vec
  간 타입 불일치를 정적 검토에서 찾아 persistable stereo끼리 비교하도록 고쳤다. 실행 검증과 PR Codex
  리뷰는 대체하지 않는다. PR Codex 리뷰는 부모의 후속 단계에서 **pending**이다.
- fresh-context 독립 subagent `/root/p34g_decision/p34g_review`: 2026-11-06 07:43:49 UTC,
  base full SHA 위의 로컬 uncommitted code/tests/README/CONTRACT/handoff/roadmap diff를
  읽기 전용 검토했다. 실질 finding 없음. 미정 요구의 임의 확장이나 기본 호환성·recipe·
  거절·atomic history·replay·frozen recovery의 문제를 찾지 못했다.
- 문법 `The tool and sends`와 새 Rust test 서식을 경미 정리로 지적했다. 문장을 고치고
  rustfmt로 서식을 편집했다. 자체 마무리에서 pattern 변경의 request_id_reused 단정과
  atom-budget/placement 경계 회귀를 추가했다. 이 기록은 리뷰 후 작성했으므로 독립 리뷰
  대상에 포함되지 않았다. 수정 후 테스트·빌드·브라우저는 미실행이다.

Rust/TypeScript patterns·testing 지침을 적용했으며 금지된 실행 gate는 돌리지 않았다.
shadcn skill로 기존 ToggleGroup을 재사용했고 CLI docs 결과와
[공식 Base UI ToggleGroup 문서](https://ui.shadcn.com/docs/components/base/toggle-group)를
확인했다. 컴포넌트 설치나 dependency 변경은 없다.
