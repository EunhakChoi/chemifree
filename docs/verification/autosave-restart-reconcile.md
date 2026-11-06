# Autosave restart reconciliation 후보

## 원인과 범위

main `71321a9` 기반 별도 `fix/autosave-restart-reconcile` 후보다. P34b의 코드와
검증 문서 변경은 포함하지 않는다. 새로운 과학 기능이나 저장 형식을 추가하지 않는다.

[PR #175 CI 42157390516](https://github.com/EunhakChoi/chemifree/actions/runs/42157390516)의
browser smoke는 `Reload`가 pointer interception 뒤 detach되어 실패했다. 이전
[main CI 42137823643 attempt 1](https://github.com/EunhakChoi/chemifree/actions/runs/42137823643/attempts/1)도
같은 locator가 invisible/detached되어 실패했다. 로그의 `SMILES panel`은 앞선 step
이름이 남은 것이며 실제 해당 locator는 다음 aromaticity restart block에 있다.
이번 변경은 그 block의 step 이름을 명시하며 sleep·timeout 증대·force click을 넣지 않는다.

제품 경로는 두 가지다. Autosave host refresh가 일반 editor queue의 성공 경로를 써서
기존 error를 지웠다. refresh 내부 catch는 외부 promise만 reject하고 queue task를
fulfilled로 남겨 실패한 background read도 오류를 지울 수 있었다. 또 failed edit의
restart 확인용 `client.get`은 새 token을 배웠으나 다른 내용의 화면 snapshot을 채택하지
않았다. 여전히 동작하는 autosave timer는 새 token으로 draft를 요청하고, 다른 내용의
reply를 받은 뒤 background read를 통해 화면을 바꾸고 경고를 지울 수 있었다.
CI의 charge 401 → get → autosave 200 → get/session 200 순서는 이 경로와 부합한다.
정적 코드와 요청 로그의 분석이며 이번 후보를 실행해 재현한 결과는 아니다.

## 변경과 계약

- Background queue task는 기존 page error를 success/failure 모두 보존한다. refresh
  실패는 queue의 정식 ownFailure 경로에 전달하며 내부 catch로 성공처럼 바꾸지 않는다.
- 화면 snapshot의 process generation은 실제 snapshot 채택 때만 갱신한다. Client가
  새 token을 배우는 일은 snapshot 채택이 아니다. Restart 통지가 autosave timer를
  즉시 멈추며, 이전 세대 snapshot의 manual retry도 전송하지 않는다.
- 이미 보낸 draft는 취소했다고 주장하지 않는다. 그 reply의 세대가 달라지면 화면
  채택·자동 reread·오류 상태 갱신에 쓰지 않는다. Background refresh는 queue 실행
  전과 read 뒤 process generation을 확인한다.
- 명시 Reload/reconcile과 기존 same-content edit recovery는 실제 snapshot을 채택해
  새 세대에서 autosave를 재개한다. ID/revision 일치만으로 다른 내용은 채택하지 않는다.

## 작성 회귀와 검증 대기

`scripts/web-autosave-restart-check.mjs`는 production editor/autosave hooks와
DocumentClient를 실제 Chromium에 올리고 HTTP 경계·브라우저 시간만 제어한다.
미전송 timer의 restart 정지, 이미 전송한 draft의 늦은 다른-content 응답 무시,
전체 unsaved document 보존·Reload 유지, 명시 Reload 뒤 새 process에서 drafting 재개,
background read success/failure의 기존 error 보존을 검사하도록 작성했다.
진행 중인 background read가 다른 요청으로 restart를 알게 된 뒤 다른-content 응답을
받아도 원본 snapshot을 유지하는 경계도 검사한다.
또한 background refresh를 held edit 뒤에 큐잉하고 same-content restart retry가 성공한
뒤 오래된 refresh가 새 오류/중단을 만들지 않으며 자동 drafting이 재개되는지 검사한다.
`npm run test:browser:autosave-restart`와 CI browser job에 연결했다.

부모의 실행 격리 지시로 테스트·빌드·브라우저·push를 실행하지 않았다.
lint/typecheck와 위 browser 회귀, 전체 `npm run check`, standard browser,
current PR CI 모두 pending이다. 기존 P34a/P34b의 통과는 이 변경의 증거가 아니다.
정적 `git diff --check`만 수행하며 컴파일·런타임 성공으로 세지 않는다.

실패 증명 계획: preserveError guard 제거는 background success의 오류 보존 단정,
generation/timer guard 제거는 replacement-process draft 부재와 원본 snapshot 보존,
late reply guard 제거는 held draft 응답 뒤 추가 read 부재 단정이 실패해야 한다.
이 mutation 계획은 미실행이며 통과로 주장하지 않는다.

## PR #176 Codex P1: flush의 false success

[Codex P1](https://github.com/EunhakChoi/chemifree/pull/176#discussion_r4488873718)은
새 process와 아직 reconcile하지 않은 unsaved snapshot에서 `save.current()`가 전송
없이 끝나 `flush()`가 null을 반환하는 경로를 지적했다. 이전 show는 Open/Create로
화면을 바꾸고 `draft_not_written`도 보고하지 않아 page-only 변경을 잃을 수 있었다.

flush는 draft가 덮지 못한 미저장 작업에 대해 전송 전과 완료 뒤 process generation을
확인하고 unresolved이면 non-null restart 사유를 반환하도록 수정했다. 이 경우 show는
destination HTTP 전에 거절해 원본 snapshot을 화면에 남긴다. `service_restarted`를
유지하므로 실제 UI의 Reload 버튼이 남으며, 교체 process에 원본이 남아 있다고 말하지
않는다. 정상 process의 draft 실패는 기존 전환+`draft_not_written` 경로를 유지하고,
clean/already-drafted navigation과 명시 Reload/reconcile은 그대로다.

실제 hook browser 회귀에 blocked flush의 non-null·Open/Create 요청 부재·원본 전체
보존·Reload 유지, 명시 Reload와 완료 draft 뒤 Open 허용, clean 이전 세대 Open 허용,
정상 process의 draft 실패 뒤 destination 전환+`draft_not_written`을 추가했다.
flush의 새 guard나 show의 전송 전 거절을 제거하면 이 단정이 실패해야 한다.
이번 수정과 추가 회귀는 테스트·빌드·브라우저·mutation을 실행하지 않았다.
부모가 실행 중인 이전 `1931415` snapshot의 full 검사는 이 추가 수정의 근거가 아니다.

## 리뷰

- Codex 작성자 정적 검토: restart notification·token 재학습·snapshot 채택·autosave
  timer와 queued refresh의 순서를 대조했다. background read의 ownFailure 전달,
  old-generation 응답·explicit reload와 same-content retry 경계를 점검했다.
- fresh-context independent subagent `/root/p34b_integrate/autosave_restart_review`:
  obsolete queued refresh의 synthetic `service_restarted`가 먼저 성공한 same-content
  edit recovery의 현재 snapshot을 다시 stopped로 만드는 P2를 지적했다. 오래된 refresh는
  읽기 전/후에 폐기하며 이미 채택한 새 세대의 autosave bookkeeping은 초기화하도록
  수정했다. obsolete read 실패도 새 세대 snapshot을 stop하지 않도록 제한했고,
  held edit 뒤의 queued-refresh·same-content recovery·drafting 재개 회귀를 추가했다.
  이 지적은 정적 수정으로 처리했으며 추가 테스트/빌드/브라우저는 실행하지 않았다.
- PR #176 Codex review의 P1은 위 false-success 수정과 추가 회귀로 처리했다.
- TypeScript patterns/testing skills를 적용했다. 실행 gate와 mutation proof는 부모의
  명시 지시로 미실행이며 pending이다. 현재 수정 HEAD의 full 검증과 CI는 pending이다.
