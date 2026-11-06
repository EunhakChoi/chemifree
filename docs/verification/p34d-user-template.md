# P34d 사용자 그래프 템플릿 파일 후보

## 범위와 결정

P34c `36893cc` 기반 `feat/p34d-user-template`의 bounded 후속이다. P34는 in_progress다.
사용자 승인 범위는 선택 그래프 파일 저장·독립 삽입이며 더 많은 고리 recipe 대신
재사용할 사용자 도형을 직접 저장할 수 있게 한다. 새로운 화학·약어 의미를 만들지 않는다.

- 전체 연결 성분만 저장한다. 기존 copy의 atoms/bond endpoints/induced bonds를 쓰고,
  `boundary_bonds`가 있으면 저장을 거절한다. 파일에 담기지 않는 잘린 결합을 attachment로
  오해하지 않기 위한 bounded 제한이다. 원본은 변경하지 않는다.
- 기존 `chemifree.clipboard` v1 본문을 그대로 파일 저장한다. 이름은 filename뿐이며
  새 metadata/schema/registration/persistence를 만들지 않는다. browser File/Download는
  사용자 파일 I/O adapter다. inert `phase4_templates`를 활성화하지 않는다.
- 명시 선택한 파일은 64KiB·strict UTF-8로 읽기만 한다. 파일명/확장자를 신뢰하지 않으며
  실제 구조/version/style 검증은 기존 decoder가 Insert template에서 수행한다.
- 재선택·닫기·document/process 전환 뒤 늦은 File 응답을 버린다. 원본 snapshot이 바뀐
  준비 offer는 다운로드하지 않는다. 미감지 service restart를 다운로드 클릭이 새로
  조회하지는 않는다. 준비된 파일은 일반 data이며 명령이나 실행 파일이 아니다.
- 기존 native paste의 정확한 style·한 destination placement·명시 pt translation,
  fresh IDs·state/mark/self-contained stereo·한 Undo 단계·save/reopen을 재사용한다.
  insertion click의 generation을 큐에서 재확인하며 immutable body/request/process로
  lost reply를 확인한다. 닫아도 recovery controls를 남긴다. OS clipboard 접근은 없다.
- scene/template library/attachment/replacement/rotation/scale/layout/collision/약어와
  외부 DATA/원본 geometry/DLL은 범위 밖이다. 참조는 템플릿 본문이 아니며 destination
  참조는 보존한다. attachment_points는 보존만 하고 소비하지 않는다.

## 작성한 검증과 한계

`npm run test:browser:user-template` (`scripts/web-user-template-check.mjs`)를 CI에
연결했다. 테스트 파일은 production panel·hook·client와 실제 Rust backend를 사용하며,
harness는 navigation/selection controls만 제공한다. 실제 editor canvas 전체 navigation이나
외부 앱 파일 호환을 이 harness로 입증하지 않는다.

- crossing/mixed 선택 저장 거절과 source snapshot/history 불변.
- 명시 Download의 실제 파일 bytes/이름/선택 source placement 좌표, 별도 destination의
  File 선택과 Insert 클릭 전 mutation 부재.
- 파일 재선택/닫기/문서 전환·실제 API 재시작 중 지연 File 읽기의 stale 응답 폐기.
- 64KiB 초과·invalid UTF-8의 client 거절, malformed JSON/unknown version/unknown field/
  style mismatch의 실제 decoder 거절, 복수 destination placement 거절과 무변경.
- 실제 backend commit 뒤 두 응답만 transport에서 버리고 같은 body/request/offset으로
  confirmation하는 경로. 늦은 확정 뒤 중복 삽입 없이 한 revision/history step.
- 독립 기대 좌표·fresh IDs·원본 isotope/charge/H/radical/attachment state·bond mark,
  선택·destination settings/references·source 불변, Undo/Redo와 save/page-reload/reopen.

**부모 지시로 테스트·빌드·브라우저·mutation·push는 실행하지 않았다.** skill gate,
`npm run check`, standard browser smoke, 새 browser script의 실행 결과와 반증 실행은
pending이며 이 기록은 성공 근거가 아니다. 새 Application/Domain operation이나 과학
알고리즘은 없으므로 동일한 기존 native paste unit tests를 새 이름으로 복제하지 않았다.
TypeScript patterns/testing과 shadcn 스킬을 적용하고 공식 Base UI component 문서를 읽었다.

## 리뷰

- Codex PR review: PR 미생성으로 pending. 부모의 PR 단계에서 한 번 수행하고 결과를 기록한다.
- fresh-context 독립 subagent `/root/p34d_decision/p34d_review`: 2026-11-06
  05:31:15–05:32:52 UTC, HEAD `9d855fbfe5766f0d7dc91db15cf4d28a0b7763e1`의
  tracked 변경 10개·untracked 3개 local diff 전체와 P34c 대비를 읽기 전용 검토했다.
  추상 요구사항의 임의 보충·async/file/recovery·코드 품질에 수정할 finding은 없었다.
  새 fixture에 stored configuration과 비어 있지 않은 destination references가 없다는
  검증 한계를 알렸다. 리뷰 후 source unknown double configuration과 destination incoming
  atom reference를 fixture에 더하고 보존의 독립 기대값을 작성했다. 이 보강도 미실행이다.

최신 main `af6c724` (P34c #177 squash)을 `9d855fb`에서 충돌 없이 합쳤다.
동일 선행 source라 merge 자체의 추가 source 변경은 없다. `git diff --check`는 통과했으며
이는 테스트·빌드·브라우저 성공을 뜻하지 않는다. PR Codex 리뷰·실행 검증은 부모가 이어간다.

## 부모 격리 검증의 첫 실패와 fixture 수정

- 부모가 `851937e`에서 lint/typecheck/client/web build를 통과시켰다. 로그는 로컬
  `/tmp/chemifree-p29d-verify-j4KjPa/p34d-targeted.log`다. 새 real-backend browser는
  첫 crossing-selection alert 대기(`scripts/web-user-template-check.mjs:202`)에서 실패했다.
  전체 검사·browser 성공이 아니며 실패한 원래 기대값은 유지한다.
- 정적 원인 대조: 새 Vite fixture는 임의 port를 썼지만 실제 API 기본 허용 web origin은
  `127.0.0.1:5173`/`:4173`이다. 읽기/open 뒤 copy POST가 Origin guard에 거절되어
  template-specific crossing 문구에 도달하지 못하는 fixture 구성이다. 기존 real-backend
  native integration과 같이 `5173`/strictPort를 쓰도록 수정했다. API Origin 보호는 바꾸지 않는다.
- source의 wedge는 fragment에 포함되지 않은 b1에서 제외되고 unknown configuration도
  copied_bonds에 없는 b2여서 제외된다. copy는 a1 한 원자·boundary b1을 반환해야 한다.
  이제 UI 문구 대기 전에 실제 HTTP200/outcome ok·copied selection·boundary list를
  독립 기대값으로 검사한다. mixed 선택도 정확한 backend refusal reason을 먼저 검사한다.
  따라서 Origin 403이나 더 이른 stereo 거절은 단순 alert timeout으로 감춰지지 않는다.
- 이 수정 담당자는 테스트·빌드·브라우저·push를 실행하지 않았다. 부모의 재검증 대기다.

## 부모 격리 재검증의 두 번째 실패와 form 수명 수정

- 부모의 `8113f0f` real-backend browser 재검증은 crossing/mixed 선택과 실제 다운로드를
  통과해 stale-source 단계까지 진행했으나, modal이 열린 동안 `Reload drawing`을 찾는
  role locator(`scripts/web-user-template-check.mjs:252`)에서 실패했다. 로그는 로컬
  `/tmp/chemifree-p29d-verify-j4KjPa/p34d-browser2.log`다. 실행 성공으로 기록하지 않는다.
- Base UI modal은 배경 toolbar를 접근성 트리에서 숨긴다. fixture는 외부 snapshot 갱신을
  일으키려고 background harness button에 DOM event를 dispatch했지만, 그 전에 기본
  `getByRole`이 숨겨진 button을 제외했다. harness-only 외부 갱신/navigation dispatch는
  `includeHidden: true` helper로 명시했다. 실제 사용자 저장/선택/삽입/recovery는 계속
  일반 role locator와 실제 click을 쓴다. 배경 toolbar를 사용자가 modal 너머 누를 수
  있다고 주장하지 않으며 전체 editor navigation 검증도 아니다.
- 같은 실행의 controlled→uncontrolled warning은 제품 form 수명 문제다. `close()`가
  mode를 null로 바꿨을 때 닫히는 Dialog의 남아 있는 body가 insert 분기로 바뀌어,
  filename의 controlled text Input과 uncontrolled file Input을 재사용할 수 있었다.
  null mode에서는 form을 렌더하지 않고 save/insert FieldGroup에 서로 다른 key를 줘
  입력기의 수명을 분리했다. native file Input을 controlled로 바꾸지는 않는다.
- browser console의 controlled/uncontrolled 경고를 수집해 최종 기대값 `[]`를 추가했다.
  실패 시 dialog/alert/current-document text도 실행 로그에 남겨 다음 실패를 상태와
  함께 확인하게 했다. 이 수정은 읽기 전용 검토와 `git diff --check`만 수행했으며,
  테스트·빌드·브라우저·push는 부모 재검증 대기다.

## 부모 격리 재검증의 세 번째 실패와 portal 종료 동기화

- `409c0f6`의 부모 재검증은 실제 파일 삽입의 lost-reply 단계까지 진행했지만 Close 직후
  전역 Check locator가 두 버튼(닫히는 dialog와 본문 panel)에 동시에 일치해 실패했다.
  로그는 `/tmp/chemifree-p29d-verify-j4KjPa/p34d-browser3.log`이며 lint/type/client535개와
  build는 통과했다. 새 browser 성공은 아니다.
- mode null일 때 닫히는 portal 내부 recovery도 렌더하지 않게 하여 persistent panel과
  종료 중 dialog가 같은 recovery action을 중복 노출하지 않도록 했다. 미확정 attempt는
  기존대로 유지하고, 닫힌 뒤 본문 recovery만 남긴다.
- fixture는 dialog 내부 Check를 명시 확인한 뒤 Close하고, portal의 실제 detached 상태를
  기다린 다음 **User graph templates** region의 Check 한 개를 눌러 확인한다. 성공 뒤 그
  persistent Check의 제거까지 기다린다. `first()`/임의 sleep 없이 이 테스트의 의도인
  “닫은 뒤에도 남는 recovery”와 실제 사용자 영역을 검사한다.
- 수정 담당자는 `git diff --check`만 실행했다. 테스트·빌드·브라우저·push와 부모의
  재검증은 pending이다.
