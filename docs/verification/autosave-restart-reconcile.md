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

## 추가 PR #176 Codex P1: old response의 잘못된 세대 귀속

[Codex P1](https://github.com/EunhakChoi/chemifree/pull/176#discussion_r4488910095)은
일반 edit의 old-process 성공 응답을 기다리는 동안 autosave/다른 write가 restart를
먼저 알게 되면 중앙 `setCurrent`가 그 old response에 최신 세대를 부여하는 경계를
지적했다. 그 결과 page-only 이전 edit이 autosave 재개와 background reread를 통해
replacement 내용으로 대체될 수 있었다. 정적으로 경로를 확인했고 지적을 반영했다.

Client는 각 실제 write attempt와 get/reread에서 session await 이전의 process 세대를
캡처한다. HTTP 응답의 document/result/snapshot 경계에 origin을 기록하고 recorded-state
normalization 뒤에도 WeakMap으로 보존한다. origin은 client와 실제 request attempt에
묶이며 JSON·저장 파일에는 새 필드가 없다. 중앙 setCurrent는 같은 client의 현재 세대
응답만 채택한다. 이전 응답이나 출처가 없는 snapshot은 `service_restarted`로 거절해
현재 도면을 보존한다. 내부 create/copy/recover의 안전한 재시도는 실제 새 attempt의
세대를 받으므로 원래 호출 시각에 묶여 잘못 거절되지 않는다.

모든 setCurrent 호출을 대조했다: mutate의 최초/동일-content retry, clipboard/ring,
autosave adopt/refresh, show(Open/Create/import creation), copy, format import,
clipboard reconcile, recover, discardDraft 뒤 get와 Reload가 중앙 guard를 공유한다.
이미 guarded였던 clipboard/auto-save의 외부 세대 검사도 유지했다. null을 채택하는
Close에는 응답 snapshot이 없으므로 request 시작 세대와 완료 세대를 비교한다.
미해결 old snapshot에서 추가 edit/copy/Close를 새 process로 보내는 것도 거절한다.

실제 hook browser 회귀는 일반 edit·Open·Create·copy·recover·Reload·MOL import·Close
각각의 응답을 hold하고 별도 inspection write가 restart를 알린 뒤 old success를
전달한다. 원본 전체 snapshot 보존·restart 오류·새 draft와 자동 mutation/read 부재를
검사하도록 작성했다. Client 회귀는 delayed response가 normalization/get 뒤에도
old origin을 유지하고 다른 client에서 채택되지 않는 경계와 safe-copy retry의 새
attempt origin을 검사한다. transport origin guard를 제거하면 이 단정이 실패해야 한다.

이번 수정·회귀·mutation은 실행하지 않았다. 부모가 실행 중인 이전 `196f4b9` full
snapshot 검사는 이번 수정의 근거가 아니며 current-head full/CI는 pending이다.
이전 독립 리뷰 1회 뒤 추가 리뷰는 부모 지시에 따라 요청하지 않았다.

## 635c6f9 전체 browser 실패와 same-content edit 복구

실행 담당자의 `635c6f9` 전용 targeted는 통과했으나 전체 격리 검사는 실패했다.
`/tmp/chemifree-p29d-verify-j4KjPa/autosave-provenance-full.log`는
`npm run check`의 client 509/509·conformance 109/109·traceability·검사 자체 시험
42/42 성공 뒤 standard browser의 Graph identity 단계에서 종료했다.
최종 `sandbox-run: exit=1`, peak anon 1281 MiB·page cache 283 MiB·cap 6144 MiB·swap 0이다.
이 실행을 전체 검증 성공으로 세지 않는다.

실패한 안내 대기는 identity를 새 세대에서 재검사한 뒤 charge 편집으로 이전 결과가
stale해지는 경계다. Inspection이 먼저 restart를 알리고 새 token을 배웠으므로 뒤의
edit은 401 catch에 들어가지 않는다. `635c6f9`에 추가한 mutate 진입 거절이 기존
same-content 복구도 막아 charge 편집이 없었고, 새 identity 결과는 여전히 current여서
안내가 나타나지 않았다. Snapshot provenance의 old-response 거절은 이 실패의 원인이
아니며 그대로 유지한다.

새 명시 edit은 shown generation이 unresolved이면 read-only get으로 replacement의
현재 snapshot과 완전 content 일치를 확인한다. 일치하는 경우에만 기존 restartedFrom
재구성 규칙으로 edit을 한 번 보내며, 다른 내용·재구성 불가·재조회 중 restart는 원본을
남기고 거절한다. 일반 edit이 직접 401을 만난 경우와 같은 helper를 사용한다.
자동 autosave·늦은 old success는 이 명시 edit의 재확인 권한을 얻지 않는다.

실제 hook 회귀에 inspection이 먼저 다음 restart를 알린 뒤 read-only 검증 1회와
charge 편집 1회·새 snapshot·오류 해소·autosave 재개를 추가했다. 8종 old-response
회귀의 후속 명시 edit도 read-only replacement 확인 뒤 다른 content에는 mutation을
보내지 않고 copy/Close도 거절하는지 검사한다. 기존 full smoke의 Graph identity
단정은 유지하고 누락된 step 이름만 바로잡았다.

이번 수정은 테스트·빌드·브라우저·mutation을 실행하지 않았다. 현재 HEAD의 targeted,
full 검사와 CI는 pending이며 이전 targeted/full의 성공 부분을 이번 수정의 근거로
확대하지 않는다. 독립 리뷰는 기존 1회 기록을 유지하고 추가 요청하지 않았다.

## d944960 standard browser 실패와 명시 read의 token 검증

실행 담당자의 `/tmp/chemifree-p29d-verify-j4KjPa/autosave-same-targeted.log`에서
`d944960`의 전용 `test:browser:autosave-restart`는 통과했으나 뒤의 standard browser는
`aromaticity inspection and restart reconciliation`에서 aromatic cycle 결과를 기다리다
실패했다. 최종 `sandbox-run: exit=1`, peak anon 1482 MiB·page cache 78 MiB·cap
6144 MiB·swap 0이다. 전용 통과를 전체 성공으로 세지 않는다.

첫 inspection이 restart를 알린 뒤 새 token을 배웠지만 화면은 여전히 이전 snapshot을
유지했다. 두 번째 crash 뒤 명시 edit의 read-only preflight와 Reload는 cached token을
그대로 사용하는 get으로 새 process의 drawing을 읽었다. get은 token 변경을 확인하지
않아 그 snapshot을 이전 세대로 표시했고, 다음 Inspect rings는 오래된 token의 401로
다시 세대가 바뀌어 결과가 표시되지 않았다. 기존 일반 edit의 401 경로는 이 token을
갱신했지만 새 preflight의 read는 이 검증을 우회했다.

명시 get은 cached token만 신뢰하지 않고 실제 session을 document 읽기 전과 후에
확인한다. 이전 token과 다르면 replacement를 알리고 token과 세대를 갱신한다.
읽는 동안 token 또는 generation이 바뀌면 해당 snapshot을 거절한다. 이미 reread가
교체를 알린 경우 중복 통지는 피한다. Background reread의 stop/refusal 계약과
명시 edit의 complete-content 비교, 중앙 old-origin 거절은 그대로 유지한다.

실제 editor/autosave hook·Client·AromaticityPanel 회귀는 첫 inspection의 restart 통지와
새 token 학습 뒤 미통지 두 번째 교체를 강제한다. 다른 내용 preflight의 mutation 부재·
원본 보존, 명시 Reload의 새 snapshot 채택, 바로 뒤 첫 Inspect rings의 새 token과
production panel 결과 표시를 검사한다. HTTP만 scripted이며 화학 계산 증거는 아니다.
Client 회귀는 아직 401을 만나지 않은 cached token의 명시 get 갱신과 read 도중 교체의
거절·후속 명시 재조회 성공을 검사하도록 작성했다.

이번 수정·추가 회귀·mutation은 실행하지 않았다. 현재 HEAD의 full·browser·CI는
pending이며 이전 전용 검사 성공을 이번 변경의 실행 근거로 확대하지 않는다.

## 9774d4c substructure navigation 실패와 명시 local Close

`/tmp/chemifree-p29d-verify-j4KjPa/autosave-double-restart-targeted.log`에서
`9774d4c`의 lint/type/client/build·전용 autosave browser는 통과했다. Standard browser는
aromaticity를 통과해 `editor-aromaticity.png`를 남겼으나 substructure search의 restart
뒤 `closeShown()`에서 `No document open`을 기다리다 실패했다. 최종 `exit=1`, peak
anon 1551 MiB·page cache 73 MiB·cap 6144 MiB·swap 0이며 전체 성공이 아니다.

이 경로는 substructure 결과가 restart로 stale해지는 검사를 마친 뒤 사용자가 clean
Close 또는 Discard and close로 이전 drawing을 떠나는 정상 흐름이다. 추가했던 Close
진입 guard는 이미 끝난 process의 화면도 일괄 거절해 이 명시 이동을 막았다.

알려진 이전 process의 clean Close나 명시 discard Close는 page의 snapshot만 해제하고
list를 새로 읽도록 수정했다. Replacement process에는 close/delete/draft-discard 요청을
보내지 않으며, 원래 process가 남긴 recovery draft도 이 local Close로 제거하지 않는다.
Discard=false인 unsaved drawing은 계속 원본을 유지하고 오류를 보인다. Captured
revision/content가 바뀐 Close는 거절하며 queued Open 대상 불변·pending ring/clipboard
gate·이미 전송한 old Close 응답의 generation 거절은 유지했다.

실제 hook 회귀에 restart 뒤 unsaved Close(false)의 원본 보존·HTTP 부재,
Close(true)의 명시 local 해제·replacement mutation 부재를 더했다. 8종 old-response
회귀는 늦은 응답 자체로는 화면을 보존하며, 이후 별도의 clean Close는 서버를 건드리지
않고 local 해제하는지 검사한다. 기존 full smoke의 Close·substructure 단정은 유지했다.

이번 수정·추가 회귀·mutation은 테스트·빌드·브라우저로 실행하지 않았다. 현재 HEAD의
targeted/full/CI는 pending이며 이전 실행의 성공 구간을 이번 수정의 근거로 확대하지
않는다. 독립 리뷰는 기존 1회 기록을 유지하며 추가 요청하지 않았다.

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
- 추가 Codex P1 old-response 귀속 지적은 위 transport origin·중앙 guard·회귀로 처리했다.
- TypeScript patterns/testing skills를 적용했다. 실행 gate와 mutation proof는 부모의
  명시 지시로 미실행이며 pending이다. 현재 수정 HEAD의 full 검증과 CI는 pending이다.
