# Browser smoke 요청 기록 종료 경합 후보

main `af6c724` 기반 `fix/browser-smoke-teardown`이다. 제품 동작이나 과학 기능을
변경하지 않으며 smoke의 network 진단 listener 수명만 보강한다.

## 실패 근거와 수정

[main CI 42168367473 attempt 1](https://github.com/EunhakChoi/chemifree/actions/runs/42168367473/attempts/1)의
browser 로그는 전체 results와 `uncaught_browser_errors: []`를 출력한 뒤 step `done`에서
`request.response: Target page, context or browser has been closed`로 실패했다.
이 통과 항목 출력은 smoke 프로세스의 최종 성공을 의미하지 않는다. 조사 시점에는
같은 main SHA의 attempt 2가 in_progress였으며 이 수정의 실행 근거로 세지 않는다.

기존 requestfinished listener는 EventEmitter가 기다리지 않는 async callback에서
`await request.response()`를 호출했다. success records를 쓴 뒤 cleanup이 browser를
닫으면 그 promise가 뒤늦게 reject되어 uncaughtException 경로를 실행할 수 있었다.
닫힌 target 오류를 무시하는 filter나 timeout 증대 없이 이 비동기 작업을 없앴다.

새 request-log observer는 response 이벤트에서 request별 status를 동기 보관하고,
requestfinished 또는 requestfailed에서 완료 시각·status/실패 원인을 동기 기록한다.
[Playwright Request 문서](https://playwright.dev/docs/api/class-request)는 response status/
headers 이벤트 뒤 body 다운로드가 끝나면 requestfinished가 발생하고, HTTP 4xx/5xx도
finished로 기록된다고 설명한다. 이 순서를 사용하며 locked Playwright 1.56.1의 API만
쓴다. 새 버전 전용 existingResponse나 의존성 변경은 없다.

완료 중 response event가 없으면 instrumentation 실패를 명시적으로 발생시킨다.
Cleanup과 failure screenshot 전에 이 observer가 등록한 네 listener만 해제한다.
Console/pageerror listener·기존 provoked HTTP error 분류·모든 smoke 단정·no-skips
검사·request의 완료 기준과 monotonic duration은 유지한다.

## 작성 회귀와 검증 상태

`scripts/lib/request-log.test.mjs`의 node EventEmitter 경계 회귀는 다음을 검사한다.

- HTTP 503도 body 완료 뒤 원래 method/path/query·status·duration으로 기록한다.
- Response가 없는 network 실패와 headers 뒤 body 중단의 실패 이유를 기록한다.
- `request.response()`가 closed-target 오류를 내는 boundary에서도 그 함수를 호출하지
  않고 동기 완료 기록을 남긴다. Stop은 idempotent이며 teardown 뒤 callback이 없고
  별도의 pageerror observer는 유지된다.
- 관찰 중 response event가 누락되면 오류를 감추지 않는다.

`npm run test:checks`에 연결해 기존 npm run check/CI check 경로에서 실행하게 했다.
부모의 실행 격리 지시로 테스트·빌드·브라우저·push는 실행하지 않았다. Static
`git diff --check`만 통과했으며 unit·full check·standard browser·current PR CI는
pending이다. 이 기록은 runtime 통과나 완료를 주장하지 않는다.

실패 증명 계획: completion listener를 async request.response로 되돌리면 closed-target
stub/동기 완료 단정이 실패하고, stop의 listener 제거를 빼면 teardown 뒤 기록·listener
count 단정이 실패해야 한다. Mutation은 미실행이다.

## 리뷰

- Codex 작성자 정적 검토: 유일한 async requestfinished callback과 cleanup의 순서,
  response/finish/failure event 계약, failure record 수명과 기존 오류 observer 유지를
  대조했다. Status를 headers 시점에 기록하지 않고 body 완료 시점까지 보관해 기존
  로그 duration 의미를 유지했다.
- fresh-context 독립 subagent `/root/p34b_integrate/smoke_teardown_review`: actionable
  finding 없음. Synchronous status capture·owned-listener cleanup·HTTP/network/body
  failure·missing-status 회귀·진단 범위를 확인했다. 테스트/빌드/브라우저/수정/push는
  실행하지 않았고 정적 diff 검사만 수행했다.
- TypeScript testing skill의 boundary regression·실패를 숨기지 않는 기준을 적용했다.
  PR Codex review와 current-head 실행 gate는 아직 pending이다.
