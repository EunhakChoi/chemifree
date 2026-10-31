# Chemifree 새 세션 인계

이 문서는 완료된 작업과 다음 개발 경계를 전달한다. 인계 문서를 읽는 것만으로 다음 기능 구현을 시작하지 않으며, 새 세션에서 사용자가 요청한 범위를 따른다.

## 프로젝트와 현재 기준

- 이름: Chemifree. 사용자가 직접 확정했으며 이름을 다시 제안하지 않는다.
- 공개 저장소: https://github.com/EunhakChoi/chemifree
- 스캐폴딩·설계 기준 커밋: 90f2b99([CI](https://github.com/EunhakChoi/chemifree/actions/runs/40362091650)), 인계 문서 007a185.
- 그 이후 첫 수직 기능(문서 생성 → 원자·결합 편집 → Undo/Redo → 저장 → 다시 열기)을 구현했고, 이어서 저장·복구 보강(프로세스 간 저장 잠금, 복구용 자동 저장, 비정상 종료 후 복구, 사본으로 저장)과 저장 피드백(실패 사유, 대기 상한, 응답을 잃은 요청의 결과 확인)을 더했다. 커밋과 CI 결과는 git log와 GitHub Actions에서 확인한다.
- 로드맵 P13으로 연산 계약 1(CONTRACT.md "Operation contract 1")을 더했다. 연산별 요청·응답(계약 버전, 호환 프로필, 결과·진단·손실 보고·model_id), HTTP `POST /api/v1/operations`와 CLI `chemifree operations`(줄마다 JSON)가 같은 `Application::execute`를 쓰며 v1 오류 코드를 그대로 진단으로 낸다. 카탈로그(capabilities의 `operation_catalog`, 웹 capability 카드의 Operations 표)는 연산과 트랜잭션 하위 연산마다 표현 가능 여부와 실행 여부를 따로 적는다. 실행되는 연산은 문서 생성·조회·트랜잭션·undo/redo·저장·닫기와 카탈로그뿐이고, 나머지 13개(가져오기·내보내기·검증·물성·명명·배치·렌더·생체고분자·예측·통합·스크립트)는 후속 항목과 함께 unsupported로 답한다. model_id는 항상 null이다.
- 로드맵 P09로 화학 그래프와 문서·장면을 분리했다(CONTRACT.md "Documents", "Edits", "Document file format"). 문서는 그래프 하나(분자 `m1`: 위치 없는 원자·결합)와 페이지 하나(`p1`), 그 위의 순서 있는 분자 장면 인스턴스(`s1`, `s2`…, 별도 ID 공간·재사용 없음, 최대 20개)를 가지며 인스턴스마다 모든 원자의 위치를 정확히 하나씩 둔다. 명령은 v1에 추가했다: 인스턴스가 여럿이면 `add_atom`은 인스턴스별 `positions`, `move_atoms`는 `instance`가 필요하고(빠지거나 남으면 명령 번호와 함께 invalid_command, 변경 없음; `positions`에 같은 키가 두 번 오면 본문 자체가 잘못된 것이라 다른 잘못된 본문처럼 명령 번호 없는 invalid_request이며 HTTP·CLI 모두 contract smoke로 확인), 500개 add_atom × 인스턴스 20개처럼 큰 트랜잭션은 HTTP 256 KiB 한도를 넘을 수 있어 나누거나 CLI(1 MiB)로 보내야 한다(CONTRACT.md "Edits"), `add_instance`·`move_instance`·`delete_instance`(마지막 인스턴스 삭제 거절)가 생겼다. 원자 삭제는 모든 인스턴스의 위치를 같은 변경 집합에서 지우고, 이력 항목은 그래프·인스턴스·위치 변경을 따로 기록하되 Undo/Redo는 한 번에 되돌린다. v1 응답은 기존 필드를 그대로 두고 `atoms[].position`을 첫 인스턴스 위치로 정했으며 `molecule`·`scene`을 더했다. 저장 형식은 document·draft 모두 version 2이고, version 1 파일·초안은 읽을 때 메모리에서 `s1` 하나로 옮긴다(ID·revision·제목·undo/redo 유지, 이동만 한 항목은 위치 변경만). v2는 원자 추가·삭제를 원자+위치 두 변경으로 세므로 v1 이력이 v2 한도(100 000 변경)를 넘을 수 있고, 그때는 편집과 같이 오래된 undo부터 잘라낸다(잘린 항목은 되돌릴 수 없고 원래 바이트는 첫 저장 때 migrated/에 남음, providers 테스트로 고정). 읽기는 쓰지 않으며, version 1 파일을 처음 덮어쓰는 저장은 그 바이트를 `migrated/<id>.v1.json`에 먼저 flush해 둔다. 사본은 덮어쓰지 않는다: 같은 ID로 다른 바이트의 version 1 파일이 다시 저장되면 다음 빈 이름(`<id>.v1.2.json`, `.v1.3.json`…)에 두고, 이미 같은 바이트가 남아 있으면 새로 쓰지 않는다. 다른 버전은 이유와 함께 unreadable로 거절하고 건드리지 않는다. 웹은 인스턴스마다 그림을 그리고 Duplicate placement(마지막 인스턴스를 복사해 그만큼 비켜 놓으므로 반복해도 겹치지 않음)·이동 버튼·삭제를 두며, 원자 끌기는 시작한 인스턴스에서만 옮긴다. 인스턴스가 여럿일 때 새 원자의 다른 인스턴스 위치는 결합 상대 기준이다: 기존 원자에 `bond_to`하면 그 원자가 각 인스턴스에서 옮겨진 만큼, 같은 트랜잭션의 앞선 새 원자(`$N`)에 결합하면 그 원자에 고른 위치에서 같은 간격만큼, 결합 없는 첫 원자만 클릭 지점에서 가장 가까운 원자의 차이만큼 옮긴다. 그래서 결합 길이·방향이 모든 인스턴스에서 같다(UI 정책, core 규칙 아님, web-client-check로 확인). 연산 카탈로그의 하위 연산은 명령 행과 기능 행으로 나뉘며, 기능 행은 그것을 수행하는 명령을 `commands`로 적는다(`scene_placement` → add_instance·move_instance·delete_instance; P09 당시 P10 행들은 `[]`였고 P10에서 executable(표현만)이 되어 수행 명령을 적는다; contract 1 안에서 추가된 선택 필드). 화학 계산은 여전히 없고 문서당 그래프는 하나이며, 여러 그래프·그룹·페이지→인스턴스 밖의 포함 트리·다른 장면 객체·외부 참조는 후속(P11 등)이다.
- 로드맵 P10으로 원자·결합의 기록 상태를 더했다(CONTRACT.md "Documents", "Edits", "Errors", "Document file format"). 원자는 원소 또는 query 원자(`*` any, `A` any_heavy, `Q` heteroatom, `X` halogen, 서로 다른 원소 1–10개 목록과 `negated`)이고, 수소 상태(`inferred` 기본값·개수 없음, `unknown`, `specified` 0–16), 라디칼(none·singlet·doublet·triplet·unknown), attachment point(오름차순·중복 없는 1–4)를 가진다. query 원자는 전하 0·동위원소 없음이며 v1 보기의 `element`에는 기호 대신 라벨(`*`, `![N,O]` 등)이 들어가므로 `kind`를 먼저 읽는다. 결합 차수에 aromatic·dative(begin이 공여체)·query(any 또는 single/double/triple/aromatic 중복 없는 목록)·unknown을 더했다. 모두 v1 안에서 추가한 선택 필드이며 add_atom·update_atom·add_bond·update_bond가 설정하고 Undo/Redo가 전부 되돌린다. 편집·불러온 파일·이력에 같은 규칙을 적용하고, 거절되는 조합은 명령 번호와 고정 `reason` 코드(hydrogen_count_out_of_range, query_atom_charge, query_atom_isotope, query_list_invalid, bond_query_invalid, bond_query_missing, bond_query_unexpected, attachment_points_invalid, atom_kind_conflict)를 가진 invalid_command로 v1 envelope와 연산 진단 `details`에 같이 실린다(HTTP·CLI 같은 reason·메시지, contract smoke로 확인). 형식이 틀린 값은 그대로 reason 없는 invalid_request다. 저장 형식은 version 3(압축 JSON: providers 테스트가 이력 100개 항목 × 명령 500개로 원자를 가장 넓은 원소↔query 상태 사이에서 뒤집은 50 000개 변경(트랜잭션으로 도달 가능한 최대)을 써서 문서·초안 모두 약 23 MB로 읽기 한도 64 MiB 안임을 확인, 들여쓰기 JSON은 68 MB로 실패. 100 000개 변경을 그런 상태로 채운 경우는 약 46 MB로 추정할 뿐 검사하지 않음)이고 version 1·2 파일·초안은 기본 상태로 이력과 함께 옮기며, 첫 덮어쓰기 저장 전에 `migrated/<id>.v2.json`(번호 사본, 같은 바이트 중복 없음)에 원래 바이트를 남긴다. 알 수 없는 버전은 거절하고 건드리지 않는다. 연산 카탈로그의 P10 기능 행 다섯 개는 `commands`를 가진 executable(표현만)이 되었고 `chem.validate`는 not_implemented 그대로다. CLI 텍스트 출력도 상태를 보여 준다. 웹은 원자마다 라벨 배치 하나(기호·query 라벨(목록은 캔버스에서 원소 3개 뒤 "…"), 명시·미상 수소와 개수, 동위원소·전하, 위 라디칼 점, 아래 `ap1,3`)를 계산하고 그 상자 하나로 결합 자르기(상자 밖 2pt에서 멈춤, 공간이 3pt보다 작으면 가운데 3pt 획, 뒤집히거나 사라지지 않음)·보기 상자·적중 판정(원자 중심 9pt 안 또는 라벨 상자 안)을 한다. 글자는 늘이거나 줄이지 않고 원래 폭으로 그린다: 기호와 그 앞뒤 글자는 이어 그리는 텍스트 하나이고, 기호의 예약 폭이 원자 중심에 오도록 놓으며(기호는 앞뒤 글자의 예약 폭과 실제 폭 차이의 절반, 1pt 미만만큼 어긋남), 예약 폭은 번들 글꼴(Geist)과 sans-serif 대체 글꼴의 글자별 advance를 Chromium에서 잰 상한이다(다른 글꼴은 더 넓을 수 있음). 같은 상태·결합 여부·쪽의 원자는 라벨 배치 객체 하나를 공유하고, 결합 획은 배치마다 문서가 바뀔 때만 계산하며 호버·선택은 결합·라벨 층을 다시 그리지 않는다. 배치가 여럿이면 테두리와 ID가 보기 상자 안에 들고 접근성 이름에 배치(`… in s2`)가 붙는다. 결합이 오른쪽으로 기울면 명시 수소는 기호 왼쪽(`H3C`)에 둔다. 상태를 기록한 결합 탄소는 라벨을 보이고 inferred 수소는 아무것도 더하지 않는다. 결합 종류는 모양(aromatic 실선+점선(8원자 이하 고리에 든 결합은 그 결합을 지나는 가장 작은 고리의 중심 쪽(융합 결합은 어느 고리든), 그래서 결합을 그린 방향·치환기와 상관없이 고리 안쪽, 고리 밖 결합은 양 끝 원자의 다른 이웃이 더 많은 쪽; 고리 탐색은 문서마다 한 번 하고 배치들이 공유; 맨 꼭짓점에서 줄임), dative 끝 쪽 화살표, query 점선, unknown 지그재그)과 접근성 이름으로 구별한다. 결합 도구는 언제나 끌기 시작점에서 결합을 시작하므로 dative의 공여체는 시작한 원자·점이다(빈 곳에서 원자로 끌면 원자를 추가한 뒤 그 원자에서 결합, 반대 방향의 기존 결합 위로 그리면 같은 트랜잭션에서 지우고 다시 추가해 결합 ID가 바뀜). 인스펙터 편집은 요청이 실행될 때의 최신 문서로 만들어(query 설정 시 전하·동위원소를 함께 지움, query 원자에는 전하·동위원소를 보내지 않음, 입력한 query 목록과 not은 서로의 최신 값을 유지) 빠른 연속 입력이 거절되지 않는다. 웹 클라이언트는 상태가 있는 응답과 이전 응답을 모두 받고(이전 응답은 기본값으로 채움) 일부만 있거나 섞인 응답은 거절한다. 표현만 하며 원자가·추론 수소 개수·방향족성·Kekulé·query 매칭은 계산하지 않는다(MODEL-02.rest, P16 등).
- HTML 설계(architecture.html)는 scaffold 코드 기준 119f477의 스냅숏이며, 상단에 이후 변경 안내를 추가했다. 현재 계약은 CONTRACT.md가 기준이다.
- P11 장면 핵심은 #34로 병합됐고 포함 트리(그룹·선·사각형·화살표), 객체·원자·결합에 대한 독립적인 방향 참조, 선택 snapshot과 이력을 연결한다. 참조 소유자가 살아 있으면 대상 삭제는 거절되며 참조 해제와 삭제를 한 트랜잭션으로 묶을 수 있다. version 4 문서·초안으로 저장하고 version 1–3은 이력과 함께 메모리에서 이전하며, 첫 덮어쓰기 전에 원본 문서 바이트를 `migrated/`에 보관한다. 이전 초안을 복구할 때도 원본 초안 바이트를 별도로 보관하며 명시적인 폐기는 사본을 만들지 않는다. P09·P10 설명의 저장 형식과 미구현 항목은 해당 작업 당시의 기록이며 현재 계약은 CONTRACT.md를 따른다. 이 항목의 PR·리뷰·병합 상태는 GitHub와 review-history-batch2.md를 확인한다. 화학 계산과 반응 의미는 구현하지 않았다.
- P12는 문서별 결합 길이·선폭·문자 스타일·탄소 라벨·표시 단위를 Undo/Redo와 저장에 포함하고, 전역 테마와 새 문서 기본 스타일은 데이터 디렉터리의 별도 `preferences.json`에 둔다. 새 문서는 생성 시 기본값 위에 환경 설정을 복사하며 기존 문서는 변하지 않는다. 초기화·다른 문서 스타일 가져오기는 선택 snapshot을 포함한 원자적 편집이다. 환경 설정으로 현재 스타일을 복사할 때는 앞선 문서 편집과 뒤따른 환경 설정 편집의 순서를 지킨다. 최초 환경 설정 읽기에 실패하면 Preferences의 재시도로 다시 읽고 저장된 테마를 적용한다. NMR·수치 계산 환경 설정은 미구현이며 명시된 값(`null` 포함)을 거절한다. 형식 version 5는 스타일을 문서·이력에 저장하고 version 1–4를 기본 스타일로 메모리 이전한다. version 4의 장면·참조·선택·이력은 보존하고 이전 문서 덮어쓰기 및 이전 초안 복구 전 원본 바이트를 보관한다. 결합 획과 라벨이 겹칠 공간도 없는 경우 획은 생략하지만 결합은 선택할 수 있다.
- P14는 진행 중이다. 공개 ManifestV1의 정규화·원문 해시를 분리하고 신뢰된 개인용 검토·root·파일 식별자를 검사한 뒤 스물두 데이터 프로필을 불변 타입 값으로 읽는다. phase 6의 열한 프로필과 phase 4의 여덟 프로필은 고정된 registry·형식·manifest 파일 바이트와 필수 파일 집합과 제공된 선택 파일을 검사한다. 명명 label의 종류·하위 종류, 원문 바이트와 반복 표기는 독립적으로 보존한다. NMR 데이터에서는 feature schema의 파일 해시와 항목·범위를 검증한다. 질량 입력은 원문 십진수와 binary64 참고 비트, 원소·동위원소·선택·별칭 및 비영 출처 범위를 검증한다. NMR·질량 계산과 feature-schema 모델 바인딩은 미구현이다. 질량의 #41·#42는 리뷰 3회 한도에 따라 미병합 종료했고 이번 구현은 최신 main에서 독립적으로 다시 작성했다. 회차별 근거는 `docs/verification/review-history-batch2.md`에 있다. 명명 query·retained 패턴의 별도 predicate 선언과 XLogP 100행의 binary64 비트·원시 수치를 보존하지만 매칭·예측 바인딩은 미구현이다. 명명 lexicon과 두 치환 표의 바이트·순서·그래프 참조를 보존하지만 명명 문법과 모델 바인딩은 미구현이다. nickname XML의 ID·원자/결합 참조와 alphabet 후보를 보존하지만 전체 CDXML 참조 검증이나 생체고분자 확장은 미구현이다. 원문 XML의 사용자 정의 엔터티 선언은 미지원으로 거절한다. prediction 11개 파일은 유한 비트·회귀 순서·feature 해시와 출처 범위를 검사하되 전체 메타데이터의 테이블 대응 및 예측 엔진은 미구현이다. phase 4 NMR carrier의 19개 정규화 스트림과 선택적 원문을 검증하지만 NMR 엔진은 미구현이다. phase 4 템플릿 프레이밍의 20,467행을 읽지만 활성 템플릿 연결이나 생체고분자 확장은 미구현이다. phase 4 명명 자료의 텍스트·순번·생산자 파싱 보고를 보존하지만 행 사이 대응과 독립 화학 파싱은 검증하지 않는다. CMR 배열은 원시 바이트·0 기반 위치와 정확한 JSON 숫자 표기를 보존하며 미확정 수치 비트 형식이나 CMR 엔진을 추론하지 않는다. phase 4 메타데이터·binary64 literal·Real48 표를 타입으로 보존하지만 Real48 원문과 유리수의 상호 검증이나 수치 연산은 미구현이다. 윤곽 binary64 원시 비트 디코더는 등록 연결 전 단계다. HTTP·CLI·웹은 데이터 상태를 표시하고 데이터가 없어도 편집할 수 있다. 다른 활성 DATA 프로필, 의존 모델과 전체 active registry 연결은 아직 남아 있으며 데이터 로딩은 화학 엔진이 아니다. 정확한 선택 방법과 한계는 [DATA.ko.md](DATA.ko.md)에 있다.
- P14의 manifest에 선택 원문 파일이 포함돼 있어도 그 파일이 없으면 정규화 자료를 읽을 수 있다. 필수 파일의 누락과 존재하는 선택 파일의 손상·접근 오류는 계속 거절한다.
- 이후 문서나 코드가 변경될 수 있으므로 작업 시작 시 git status, git log와 현재 CI를 다시 확인한다. 기존 사용자 변경은 보존한다.

## 먼저 읽을 파일

1. 프로젝트 루트의 AGENTS.md.
2. 이 문서와 ../README.md.
3. CONTRACT.md: 현재 v1 계약(문서 API·명령·오류·쓰기 보호·파일 형식)과 버전 정책.
4. SHADCN_REVIEW.ko.md: UI 도구 선택, 현재 연결 상태와 정책 제안.
5. TRACEABILITY.ko.md: 요구사항·공개 선언 추적, conformance runner와 추적 검사가 확인하는 것과 확인하지 않는 것.
6. roadmap/implementation-roadmap.json(.html): PR 계획과 행별 상태. 후속 계획 ID는 이 행들이다.
7. verification/review-history.md, verification/review-history-batch2.md: PR별 리뷰 회차·CI 이력과 정정. 닫힌 PR의 지적은 이후 설계 요건이다.
8. architecture.html: scaffold 기준의 레이어·소스 링크를 담은 대화형 설계안(스냅숏). 브라우저로 열 수 있다.
9. 같은 PC에서 존재한다면 private/SESSION_CONTEXT.ko.md: 승인된 공개 기능 명세의 로컬 경로와 기존 사용자 결정만 담은 보조 인계. Git에 포함하지 않는다.

## 완료한 기반

| 영역 | 실제 상태 |
| --- | --- |
| apps/web | React/TypeScript/Vite. 런타임·capability 카드와 문서 편집기: 새 문서·목록(제목 검색, 제목·최근 저장 순 정렬, 이름 변경, 휴지통으로 이동 확인, 휴지통 복원)·열기, SVG 구조 캔버스(src/editor), 도구(선택·원자·결합·지우개), 원소·결합 차수, 인스펙터(원소·query·전하·동위원소·수소·라디칼·attachment point·결합 종류, 최신 문서 기준 편집), Undo/Redo·저장·닫기(미저장 확인), 단축키, 충돌·저장 실패 안내, 파일·복구 초안 상태 표시와 자동 저장(재시도), 복구·초안 버리기·사본으로 복구 배너, 사본으로 저장(대화상자·편집 대상 안내) |
| apps/api | Axum loopback HTTP. 문서 생성·조회(목록 query·sort)·트랜잭션·undo/redo·저장·닫기·autosave·save-as·recover·recover-as-copy·discard-draft·rename·delete, 휴지통 목록·restore. Host/Origin/Sec-Fetch-Site 검사, 프로세스별 세션 토큰, JSON·256 KiB 제한, v1 오류 envelope |
| apps/cli | status/capabilities와 documents list(--query, --sort)·create·show·apply·undo·redo·save-as·recover·recover-as-copy·discard-draft·rename·delete·trash·restore. 같은 Application을 HTTP 없이 호출, 편집마다 저장 |
| crates/domain | 원소(1–118), 기록 상태(원소/query 원자, 수소, 라디칼, attachment point, 결합 종류·query 결합)와 조합 규칙, 화학 그래프(분자 m1)와 장면(페이지 p1, 분자 장면 인스턴스 s1…·인스턴스별 원자 위치) 분리, 안정 ID·참조 불변식, 명령, 원자적 트랜잭션, 그래프·장면을 한 단계로 되돌리는 change-set Undo/Redo, 저장본 복원 검증, 독립 사본(copy_as). 외부 의존성 없음 |
| crates/application | 세션(작업 사본), expected_revision 충돌, request_id 재전송, 실행 중인 request_id는 다른 잠금 없이 즉시 request_in_progress, 문서별 차례(최대 3초 대기 후 busy), 저장 실패 사유(read_only/busy/io), 저장 커밋 후 ACK, 자동 저장·복구(번호 일관성)·사본, 목록 검색·정렬, 이름 변경·휴지통 이동·복원(같은 차례·request_in_progress 규칙), 공유 DTO, DocumentRepository(초안 소유 계약 포함)·CapabilityCatalog port |
| crates/conformance | conformance runner(lib)와 `chemifree-conformance`. conformance/cases의 사례를 공유 Application에 HTTP 핸들러와 같은 요청 타입·인자로 호출하고(HTTP 계층과 상태 코드는 다루지 않음, 사례마다 새 임시 데이터 디렉터리, `restart`는 정상 종료 없는 재시작), passed·failed·not_run을 kind별로 따로 기록한다. 기록은 docs/verification/conformance-report.json |
| 추적 | docs/traceability/requirements.json(요구사항 33개의 부분별 상태·구현 경로·근거·후속 계획), declarations.json(공개 선언 2,914개의 경로별 수), scripts/traceability-check.mjs. 규칙과 한계는 TRACEABILITY.ko.md |
| crates/providers | 로컬 JSON 파일 저장소(형식 version 5, 압축 JSON, version 1–4 문서·초안 읽기 시 이전과 문서 덮어쓰기·초안 복구 전 원본 migrated/ 보관, 임시 파일 → fsync → rename(커밋 시점) → 가능한 경우 디렉터리 sync, base revision 검사, 손상 파일 거절), 별도의 환경 설정 저장소와 잠금, 문서별 OS 저장 잠금(locks/), 복구 초안(drafts/, 소유 잠금, 커밋 중 초안 정리), 휴지통(trash/, 저장 잠금·초안 소유 잠금 아래 rename 한 번으로 이동·복원, 권한 거절은 Denied), capability 목록 |
| 개발 환경 | npm/Cargo workspaces, lockfiles, npm run dev/check, test:contracts, test:browser, GitHub Actions(check·browser job) |

의존 방향은 Application → Domain, Providers → Application + Domain이다. API와 CLI가 가장 바깥에서 Application과 concrete provider를 조립한다.

HTTP 계약은 health·capabilities·session과 /api/v1/documents 아래 생성·조회·transactions·undo·redo·save·close·autosave·save-as·recover·recover-as-copy·discard-draft다. CLI는 status·capabilities·documents 하위 명령으로 같은 Application을 호출한다. 모든 JSON에는 api_version: v1이 있다. 세부 내용은 CONTRACT.md에 있다.

runtime_inspection, document_editing, persistence가 available이다. document_editing은 그린 그대로 기록하는 drawing 모델이며 원자가·암시적 수소·방향족성·입체·배치·명명·물성을 계산하지 않는다. persistence는 로컬 Chemifree JSON 파일(저장 잠금·복구 초안·사본 포함)이며 DB·동기화·공유는 없다. structure_analysis, format_conversion, local_model_loading, authentication, windows_bridge는 not_implemented다. 쓰기 보호(세션 토큰·Host/Origin)는 다른 웹사이트를 막는 장치이며 사용자 인증이 아니다.

## UI 결정

- 웹 우선이며 데스크톱 셸은 선택적 후속 확장이다.
- shadcn/ui의 공식 @shadcn registry, Base UI/Nova를 적용했다.
- 설치 컴포넌트: Alert, Alert Dialog, Badge, Button, Card, Empty, Field(Label, Separator), Input, Kbd, Skeleton, Spinner, Toggle Group(Toggle), Tooltip. 기존 button.tsx는 notice 헤더만 달라 덮어쓰지 않았다.
- 과학 렌더러는 apps/web/src/editor에 두고 일반 UI 컨트롤과 분리했다.
- 일반 UI는 semantic token과 variant를 사용한다. 과학 SVG/Canvas renderer는 별도 제품 모듈로 둔다.
- @shadcn/lint는 ESLint plugin으로 등록되어 있지만 기본/CI 디자인 규칙은 0개다. 등록 사실을 디자인 정책 강제 적용으로 설명하지 않는다.
- 임시 stdin probe에서 no-raw-colors와 no-restyle 탐지를 확인했다. 그 probe가 기본 설정을 변경하지는 않았다.
- Base UI의 내비게이션은 일반 anchor + buttonVariants를 사용한다. Button render anchor 패턴은 링크를 button 역할로 바꾸므로 재도입하지 않는다.

## 검증과 실행

Node 22.22.2, npm 11.13.0, Rust 1.95.0을 사용했다. 세부 의존성은 lockfiles를 기준으로 한다.

~~~bash
npm ci
npm run dev
# Web: http://127.0.0.1:5173
# API: http://127.0.0.1:3001

npm run check
cargo run --locked -p chemifree-cli -- capabilities --json
~~~

검증된 내용(첫 수직 기능과 저장·복구 보강):

- npm run check: lint, TypeScript, web client 검사(test:client), production build, rustfmt, clippy, Rust 테스트 251개(domain 42, application 86, providers 42, api 11, conformance 68, cli 2; P10 브랜치에서 cargo test --workspace로 센 값), test:contracts, test:conformance, test:traceability, test:checks(추적 검사 주요 규칙의 node:test 42개).
- npm run test:conformance: conformance 사례를 실행해 target/conformance/report.json을 쓰고 실패가 있으면 종료 1. npm run conformance:record는 커밋할 기록을 다시 쓴다. npm run test:traceability는 추적 파일·로드맵·기록·npm 스크립트·집계 줄을 검사하고, CHEMIFREE_SPEC_DIR를 주면 명세의 해시·ID·연산 이름·선언 수와도 비교한다. 무엇을 확인하고 무엇을 확인하지 않는지는 TRACEABILITY.ko.md에 있다.
- npm run test:client: 웹 클라이언트(document-contract.ts)를 브라우저 없이 Node의 타입 제거 실행으로 불러 각본대로 답하는 서비스에 붙인다. 브라우저로는 순서를 맞출 수 없는 경우(응답을 잃은 생성·사본·저장을 다시 묻는 사이 다른 요청이 서비스 재시작을 발견)를 매번 재현한다. 재시작 뒤에는 옛 request_id를 새 프로세스에 보내지 않고 결과를 알 수 없다고 알리는지, 프록시(Vite)가 연결이 끊겨 돌려준 502를 '응답 없음'으로 다루는지, 세션 토큰 요청이 실패해도 어디로 보냈는지를 잊지 않는지, 다시 물어 받은 생성·사본 응답이 옛 기록일 수 있어 문서를 다시 읽는지, 그 다시 읽기가 실패해도 ID를 유지해 중복 생성하지 않는지, 초안 실패 기록(code·reason)을 읽고 알 수 없는 reason을 거절하는지, 초안 결과 다시 읽기가 읽기 전·응답 중의 서비스 재시작을 알리고 같은 프로세스면 문서를 돌려주는지, 재시작을 알아챈 뒤의 편집이 옛 토큰으로 거절되어 새 프로세스에 그대로 적용되지 않는지, 아직 쓰지 않은 페이지도 문서를 연 뒤의 재시작을 알아채는지, 재시작을 본 뒤 문서를 다시 읽으면 새 프로세스에 묶이는지 확인한다. capabilities 응답의 연산 카탈로그를 검증하고(이전 v1 응답처럼 없으면 null) 알 수 없는 실행 상태·표현·중복 연산을 거절하는지, 문서 응답의 molecule·scene을 받아들이고(없는 이전 응답도 허용) 위치가 빠지거나 남는 인스턴스·중복 인스턴스·다른 페이지/분자·첫 인스턴스와 다른 atoms[].position을 거절하는지, 새 원자의 인스턴스별 위치 정책(결합 상대 기준으로 옮겨 서로 결합한 새 원자와 기존 원자에 결합한 새 원자의 결합 길이·방향이 모든 인스턴스에서 같음, 결합 없는 첫 원자는 가장 가까운 원자의 차이만큼, 원자가 없으면 같은 점, 인스턴스 하나면 position), Duplicate placement가 마지막 인스턴스를 복사하는지, 배치 이동·삭제가 적용될 때의 최신 문서로 만들어지는지(삭제된 배치·마지막 배치는 보내지 않음), 카탈로그 기능 행의 `commands`(없는 이전 응답 허용, 목록이 아니거나 이름이 아니면 거절)와 인스턴스가 바뀐 뒤 대기 중인 제스처가 버려지는지도 본다. 기록 상태는 상태가 있는 응답·이전 응답(기본값 채움)을 받고 일부만 있거나 섞인 응답·범위 밖 값을 거절하는지, invalid_command의 reason을 읽는지, 라벨 배치(inferred 수소는 아무것도 더하지 않음, 상자가 라디칼 점·attachment 표시를 포함, 긴 query 목록 줄임과 전체 이름, 명시 수소의 쪽), 결합 자르기·좁을 때의 가운데 획, 결합 종류별 모양과 이름, 고리를 섞인 방향으로 그려도 aromatic 점선이 안쪽에 있고 꼭짓점에서 교차하지 않음, 치환기 둘 사이의 고리 결합(ortho)과 융합 고리(나프탈렌 꼴)를 양방향으로 그려도 안쪽, 고리를 한 번 찾아 모든 배치에 씀, 기호 앞뒤 글자가 한 텍스트로 이어지고 기호 예약 폭이 원자 중심, 기호 예약 폭과 측정한 자연 폭(I·Cl·W), 같은 상태의 원자가 라벨 배치를 공유함, 라벨 상자와 배치 테두리·ID로 커지는 보기 상자, 반지름 9pt와 라벨 상자로 하는 적중 판정(O·N의 ±6·±8pt), 배치가 붙는 접근성 이름, 끌기 경로마다 결합이 시작점에서 시작함(dative 반대 방향 결합 교체 포함), 최신 문서로 만드는 인스펙터 편집(전하→query, query→전하, 목록→not, not→목록 순서 모두 유지)을 확인한다(node:test 124개). 같은 문서의 초안이 바뀌어도 사본 복구 요청 키는 같으므로, 한 프로세스에서 같은 문서의 다른 초안을 연달아 사본 복구하면 이전 응답이 재생될 수 있다(두 프로세스가 한 데이터 디렉터리를 쓸 때만 가능).
- npm run test:contracts: 실제 API·CLI 프로세스와 임시 데이터 디렉터리. HTTP = CLI JSON, 쓰기 보호, 생성→편집→undo/redo→저장→닫기→다시 열기, request_id 재전송·재사용 거절, revision/save 충돌, 저장 실패 후 재시도, API 강제 종료 후 마지막 커밋 복구. 같은 기준에서 동시에 저장하는 CLI 6개(정확히 하나만 커밋), 잠금을 쥔 채 강제 종료된 프로세스 뒤의 저장, API와 CLI의 저장 경쟁, 자동 저장 → SIGKILL → 재시작 → 복구(내용·Undo/Redo·ID·리비전), 초안 버리기와 재시작 후 미재출현, 오래된 초안의 사본 복구, 손상 초안, 자동 저장 실패·재시도, HTTP·CLI 사본 저장과 독립성, 사본 저장 실패 후 원본 보존·재시도, 원본 초안을 지울 수 없을 때(drafts/ 권한 오류) 사본은 커밋되고 원본 세션·초안이 그대로 남으며 응답이 closed: false로 알리는지. 저장 피드백: API의 저장이 잠금 안에서 멈춘 동안 같은 request_id로 다시 물으면 즉시 request_in_progress, 그 뒤에 줄 선 요청(저장·자동 저장·사본)은 3초 뒤, 같은 문서를 저장하려는 CLI는 5초 뒤 busy로 거절되고 아무것도 바뀌지 않음, 끝난 뒤 다시 물으면 다시 쓰지 않고 커밋 결과를 돌려줌, 읽기 전용 documents/는 reason read_only. 잠금 보유 여부는 sleep이 아니라 flock 비차단 시도로 확인한다. 경쟁 재현에는 debug 빌드 전용 CHEMIFREE_TEST_COMMIT_DELAY_MS(저장 잠금 안 대기)를 쓴다. 잠금을 끈 빌드에서는 6개가 모두 커밋되어 이 검사가 실패함을 확인했다. 데이터 디렉터리에 둔 version 1 파일을 HTTP와 CLI로 열어 같은 응답·s1 위치·이력을 확인하고(읽기는 파일을 바꾸지 않음), HTTP undo·저장과 CLI redo 커밋이 각각 원래 바이트를 migrated/<id>.v1.json에 남기며 다음 저장이 그것을 덮어쓰지 않는지, version 5 파일이 unreadable로 거절되고 그대로인지 본다.
- npm run test:browser(로컬, CI browser job에서도 실행): Chrome으로 편집 흐름 전체, 키보드 단축키, 미저장 닫기 확인, 충돌 Reload, 낡은 화면에서 확인한 변경 버리기 거절, 저장 실패·재시도, 읽기 전용 저장소와 CLI가 잠금을 쥔 문서의 안내('Storage is read-only', 'Document busy'), 페이지 제한(10초)보다 늦게 온 저장 응답을 같은 request_id로 다시 물어 '저장됨'으로 확인(한 번만 커밋), 응답이 두 번 끊긴 저장의 'Save not confirmed'와 Check again, 사본 요청이 진행 중일 때 같은 ID로 다시 물어 사본 하나만 생성, CLI와의 저장 충돌, 요청 처리 중 반복 입력(원자 겹침 방지·전하 클릭 누락 방지), 잘못된 동위원소 입력 거절, 거절된 편집 오류가 뒤이은 성공에 지워지지 않음, 응답을 잃은 문서 생성의 재시도(중복 생성 없음), 그 사이 API가 재시작되면 재전송하지 않고 목록 확인 안내, 자동 저장 상태, API 강제 종료 후 Reload → 복구(그림·Undo 이력), 초안 버리기(확인 대화상자), 오래된 초안의 사본 복구와 원본 열기, 자동 저장 실패·Retry now, 편집이 계속되어도 밀리지 않는 재시도, 서비스의 초안 실패 기록 표시(사용자 조치 포함)와 서비스 혼자 하는 재시도(페이지 초안 요청 0회), 기록 첫 읽기가 실패해도 페이지가 옆에서 다시 쓰지 않음, 결과 읽기가 실패하면 다시 읽음, 다른 문서로 옮기면 실패가 보이지 않고 떠난 문서를 읽지 않음, 실패 중 API 재시작 때(아직 쓰지 않고 문서만 연 새 페이지에서도) 새 프로세스의 저장본 대신 '중지'와 그림 유지(API가 다시 뜰 때까지 문서 읽기를 route로 붙잡아 프록시 502를 피함), 기록 필드가 없는 서비스에서는 페이지가 직접 재시도, 재시작 뒤 새 서비스 기준의 초안 상태(이전 초안이 있으면 '일시정지', 버린 뒤 다시 초안 작성), 사본으로 저장(원본 파일 불변), 다른 문서로 넘어간 뒤 도착한 자동 저장 응답 무시, 문서를 떠나기 전 대기 중인 작업의 초안 작성, 다른 클라이언트가 세션을 앞서 나가도 자동 저장이 1초마다 반복되지 않음, 원본 초안을 지울 수 없어(drafts/ 읽기 전용) 원본이 미저장 변경과 함께 남은 사본 저장의 안내·원본 열기, 사본으로 돌아왔을 때와 서비스 재시작 뒤 지난 안내가 사라지는지, Duplicate placement로 만든 두 번째 배치를 이동 버튼으로 옮겨도 그래프와 첫 배치가 그대로인지·두 배치에 함께 놓이는 새 원자·한 배치에서만 움직이는 원자 끌기와 Undo, 다시 Duplicate하면 마지막 배치(옮긴 두 번째 배치)를 복사해 겹치지 않는지, 인스펙터로 결합 탄소에 명시 수소 3개·doublet 라디칼, 다른 결합 탄소에 attachment point 1, 결합에 aromatic을 주고 고립 질소는 inferred(개수 없이 N)로 둔 그림을 좁고 넓은 기호(I, 전하를 준 Cl)와 명시 수소를 준 긴 query 목록 원자를 더해, 모든 배치에서 렌더된 SVG로 재어 강제 길이(textLength)로 그린 글자가 없고 기호와 앞뒤 글자(H3C, Cl+, query의 H2) 사이가 2pt 이내이며 그려진 글자·표시가 모두 캔버스가 예약한 라벨 상자(data-box) 안이며 그 상자 안에서 끝나는 결합 획이 없고 모든 라벨 상자·배치 테두리·배치 ID가 보기 상자 안인지, 저장·페이지 새로고침·닫고 다시 열기 뒤에도 같은지, 첫 `/transactions` 요청을 붙잡은 채 전하→query, query→전하, 목록 입력→not을 눌러도 거절되는 요청이 없고 둘 다 남는지, 320/390px, dark mode. 결과는 verification/editor-browser-smoke.json.
- 브라우저 검증 중 serde_json 기본 float 파서가 다시 열 때 1 ULP 오차(226.39999999999998 → 226.4)를 내는 버그를 발견했고 float_roundtrip 기능으로 수정, 회귀 테스트를 추가했다.

scaffold 시점 기록은 verification/의 기존 JSON에 남아 있다. 화학적 정확성 검증은 수행한 상태가 아니다.

이번 작업의 무거운 로컬 검사는 자식 프로세스를 포함하는 단일 systemd 사용자 cgroup에서 직렬 실행했다. 최초 `MemoryMax=8G`에서 사용자 요청에 따라 `16G`로 변경했고 `MemorySwapMax=0`, Cargo 작업 2개·Rust 테스트 스레드 2개를 사용했다. 상위 app.slice에도 16GiB 상한이 있으며 이 세션의 cgroup peak는 16,437,227,520바이트, OOM·OOM kill은 0이었다. scope는 세션 종료 후 달라질 수 있으므로 다음 세션은 실제 cgroup 한도를 다시 확인한다.

CI는 실행 중인 전역 npm을 교체하지 않는다. 별도 runner-temporary prefix에 npm을 설치하고 PATH에 추가하는 현재 방식을 유지한다.

## 알려진 한계

보장과 한계의 기준 목록은 CONTRACT.md의 "Guarantees and known limits"다. 리뷰와 변경은 그 목록에 맞춰 판단한다. 아래는 구현 관점의 보충이다.

- 작업 사본은 API 프로세스 메모리에 있다. 프로세스가 끝나면 마지막 커밋과 마지막 복구 초안이 남는다. 마지막 초안 이후의 편집(웹이 열려 있으면 보통 1–5초, 다른 클라이언트만 있으면 2–10초, 초안 쓰기가 실패·차단 중이면 더 길다)은 비정상 종료 시 사라진다.
- API 프로세스가 클라이언트 없이도 초안을 쓴다(DraftWriter, 250ms마다 확인). 마지막 변경 2초 뒤, 초안에 없는 첫 변경 10초 이내(연속 편집으로 미루지 못함). 실패하면 2·5·15초 뒤, 이후 30초마다 재시도하고 편집이 재시도를 미루지 않으며 session.draft_failure로 알린다. 정상 종료(SIGTERM·Ctrl+C) 때는 마지막 요청 응답 뒤 모든 세션을 즉시 초안으로 쓰고, 못 쓴 문서는 로그에 남긴다. 웹 자동 저장과 같은 문서 차례(최대 3초 대기 후 busy, 실패로 기록해 재시도)를 지키므로 한 리비전을 두 번 쓰지 않는다. 주기적 초안 쓰기는 차례가 잡힌 문서(저장 중 등)를 기다리지 않고 다음 확인(250ms 뒤)에 다시 시도하므로, 바쁜 문서가 다른 문서의 초안을 늦추지 않는다(실패로 세지 않음). 초안 쓰기(웹 자동 저장·서버 주기 쓰기·종료 전 쓰기)는 세션 표에서 그 순간의 사본만 떠 두고 표를 놓은 채 디스크에 쓴다. 디스크가 멈춰도 다른 문서의 요청과 같은 문서의 편집·읽기는 진행되며, 같은 문서의 차례를 기다리는 저장·닫기 등만 기다린다(최대 3초 후 busy). 쓰는 동안 들어온 편집은 초안에 들어갔다고 표시하지 않고(draft_revision은 쓴 리비전) 그 첫 편집부터 다시 일정을 센다. 쓰는 중인 깨끗한 세션은 자리 확보를 위해 닫지 않는다. 서버의 주기적 초안 쓰기와 종료 전 쓰기는 문서마다 별도 스레드에서 실행하므로 한 문서의 쓰기가 멈춰도 다른 문서의 초안은 계속 쓰이고, 쓰는 중인 문서는 다음 확인에서 다시 시작하지 않는다(종료는 멈춘 쓰기가 끝날 때까지 기다린다). 쓰는 동안 그 문서의 응답은 쓰기를 시작할 때 확인한 복구 상태를 보여 주고 저장소를 다시 확인하지 않으며, 파일 저장소의 초안 소유권 확인 잠금은 문서별이라 다른 문서의 초안 상태 확인을 막지 않는다. 정상 종료 때는 실행 중인 요청이 없고, 진행 중인 서버 초안 쓰기가 끝나기를 기다린 뒤 문서마다 차례를 최대 3초 기다린다. 웹은 session.draft_failure(누가·언제 재시도하는지, 시도 횟수, code·reason과 사용자 조치)를 그대로 표시하고, 기록하는 서비스에서는 직접 다시 초안을 쓰지 않고 서비스 재시도 시각 1초 뒤 문서를 다시 읽는다(읽기가 실패하면 2·5·15·30초 backoff로 다시 읽음). 문서를 읽을 때 세션 토큰을 먼저 확인해 화면의 view를 그 프로세스에 묶고, 이 읽기는 토큰이 같은 프로세스인지 확인하므로, 실패 중 API가 재시작되면 새 프로세스의 저장본을 받아들이지 않고 '중지'로 바꾼 뒤 그림을 화면에 둔 채 Reload를 요구한다. 필드가 없는 이전 v1 서비스에서는 페이지가 직접 재시도한다(필드·code·reason은 v1 안에서 추가되어 선택 사항). 버려진 초안이나 다른 프로세스 소유 초안은 덮어쓰지 않고(recovery_pending·draft_in_use로 실패·재시도) 초안은 만료되거나 자동 정리되지 않는다(복구·사본 복구·discard-draft로만 제거). CLI는 편집마다 커밋하므로 초안을 쓰지 않는다.
- 저장은 문서의 차례만 잡고 저장소(다른 프로세스의 저장 잠금 대기 포함)를 기다린다. 그동안 다른 문서 요청과 같은 문서의 편집은 진행되고, 저장은 요청한 리비전을 커밋한다. 커밋·초안을 다루는 작업(저장·자동 저장·닫기·복구·초안 버리기·사본)은 문서별 차례로 직렬화되며, 차례는 최대 3초, 다른 프로세스의 저장 잠금은 최대 5초 기다린 뒤 busy로 거절한다(아무것도 바꾸지 않음).
- 대기는 상한이 있지만 저장소 I/O 자체는 그렇지 않다. 생성·닫기·복구·초안 버리기·사본은 세션 표를 잡은 채 I/O를 하므로(초안 쓰기는 예외, 위 항목) 디스크가 멈추면 같은 프로세스의 다른 요청도 그만큼 기다린다. 실행 중인 request_id에 대한 request_in_progress 응답만은 이를 기다리지 않는다(별도의 작은 표로 확인).
- 저장 잠금과 초안 소유 잠금은 OS advisory lock(std File::lock, Unix flock)이다. Chemifree 프로세스끼리만 지켜지며 일부 네트워크 파일 시스템에서는 보장되지 않는다. locks/와 drafts/*.owner 파일은 의도적으로 지우지 않는다.
- 한 프로세스 안의 요청들은 초안 소유 잠금을 한 번에 하나씩 잡는다. OS 파일 잠금은 같은 프로세스의 다른 핸들도 거절하므로, 그러지 않으면 동시에 들어온 요청이 다른 요청의 잠깐 확인을 다른 프로세스의 소유로 오인해 버려진 초안을 Draft in use로 보였다(#8 조사 중 발견).
- 같은 데이터 디렉터리에 API 프로세스가 둘이면 같은 문서의 초안은 한쪽만 소유한다. 다른 쪽 autosave는 draft_in_use로 거절된다. 실시간 병합은 없다.
- 문서당 초안은 하나다. 버려지지 않은 초안이 있으면 새 작업의 자동 저장은 멈추고(화면에 '일시정지'로 표시) 사용자가 복구·버리기를 정해야 한다. 초안은 만료되지 않는다.
- 웹의 초안 상태는 서버 응답(session.draft_revision, recovery)만으로 판단한다. 페이지가 따로 기억하는 것은 대기 시간과 재시도 일정뿐이고, 문서가 바뀌면 처음부터 다시 센다. 문서를 떠나기 전에는 대기 중인 작업의 초안을 먼저 쓴다.
- request_id 재전송 기억은 프로세스 메모리(문서당 최근 64개, 생성·닫기는 프로세스 단위)에만 있다. 응답을 잃은 요청을 다시 물으면 이 재생 창 안에서만 원래 응답을 돌려주고, 그 밖이면 다시 실행된다. 웹은 응답을 받지 못한 저장을 같은 ID로 즉시 한 번, 진행 중이면 1초마다 최대 12번 더 묻고, 그래도 답이 없으면 실패가 아니라 'Save not confirmed'로 표시한다. 생성·사본 요청의 ID도 진행 중인 동안과 응답이 없을 때 유지한다. 그 사이 API가 재시작되면 재전송하지 않고(중복 생성 방지) 문서가 이미 생겼을 수 있다고 안내하며 목록을 새로 고친다. 확인은 사용자 몫이다.
- revision은 2^53 − 1까지다. 편집은 2^53 − 2에서 멈추고 마지막 번호는 버린 뒤 다시 여는 사본용으로 남긴다. 그 뒤 다른 저장을 따라갈 번호가 없으면 그 프로세스에서는 resource_limit으로 거절한다.
- 사본은 내용·ID 카운터만 가져가고 Undo 이력은 비운 채 revision 0에서 시작한다. 사본으로 저장하면 원본 세션은 닫히고(미저장 변경은 사본에만 남음) 원본은 마지막 저장본으로 다시 열린다. 원본 초안을 지울 수 없으면 원본 세션은 미저장 변경·초안과 함께 남고(closed: false), 웹은 사본 안내에서 그 사실과 원본 열기를 보여 준다(#7). 안내는 사본을 만든 순간의 사실이라 다른 문서로 넘어가거나 서비스 재시작을 알게 되면 지운다. 재시작은 쓰기 요청의 세션 토큰이 거절될 때 알게 되므로, 쓰기가 없으면 그때까지는 안내가 남는다.
- 세션 64개가 모두 열린 상태에서 사본이 들어갈 자리가 필요하면 미저장 변경이 없는 세션 중 ID가 가장 작은 것을 닫는다. 초안을 사본으로 복구할 때 사본 생성 자체는 다른 clean 세션이 없을 때만 원본을 퇴거한다. 초안 제거 중 다른 요청이 원본을 퇴거할 수도 있으며, source.closed는 응답을 기록할 때 원본 세션이 닫혔는지를 알린다(#6).
- 읽기 전용 저장소는 reason read_only로 알리지만, 읽기 전용으로 열기나 다른 위치에 저장하기는 없다. Chemifree 외 프로그램에 대한 파일 잠금은 없다.
- 삭제는 휴지통(trash/)으로 옮기는 것이다. 열린 사본에 미저장 변경이 있거나 복구 초안(버려진 것이든 다른 프로세스 소유든)이 있으면 거절하고, 저장 잠금과 초안 소유 잠금 아래에서 rename 한 번으로 옮긴다. 영구 삭제와 휴지통 비우기는 아직 없다. 다른 프로세스의 깨끗한 사본은 다음 접근에서 닫히고, 미저장 사본은 저장·초안으로 문서를 되살리지 못한다(save_conflict, document_not_found). 초안이 아직 없는 다른 프로세스의 미저장 작업은 CLI가 볼 수 없으므로, 그 작업은 사본 저장으로만 지킬 수 있다.
- 목록 검색·정렬은 Application이 한다(웹·CLI는 질의만 전달). 정렬 recent는 저장 파일의 수정 시각(벽시계) 기준이라, 시계 조정(시간 동기화로 시계가 뒤로 가는 경우 등) 사이에 저장된 문서는 순서가 바뀔 수 있다(WSL2에서 실제로 관찰). 이름 변경은 미저장 변경이 없는 문서에서만 즉시 커밋된다.
- 이름 변경·삭제·복원도 저장과 같은 문서 차례(최대 3초 후 busy)를 따르고, 실행 중인 같은 request_id는 즉시 request_in_progress를 받는다. 재전송 기록은 이름 변경이 문서별(세션), 삭제·복원이 프로세스별이다. 웹은 응답을 못 받으면 같은 request_id로 한 번 다시 묻고 진행 중이면 1초마다 묻는다. 그 뒤의 재시도는 새 요청이며, 첫 시도가 이미 반영됐다면 전제 조건(revision, 휴지통 여부)이 거절한다(revision_conflict, document_not_found).
- revision·ID 재사용 방지 기억은 프로세스 메모리에 있다. API가 재시작되면 웹은 문서를 다시 읽어 화면과 같은 내용일 때만 편집을 재전송하고(Undo/Redo 제외), 다르면 Reload를 요구한다. CLI는 매번 저장본에서 시작한다.
- request_id 재전송 기록은 원래 응답 전체를 보관한다(세션당 64개). 큰 문서를 많이 열면 메모리 사용이 커질 수 있다.
- 문서 목록은 저장된 각 문서를 전부 복원·검증한다. 문서가 많고 크면 목록 조회가 느릴 수 있다(잠금 밖에서 수행).
- 화면에서 만든 편집이 앞선 요청에 밀려 낡으면 revision_conflict로 거절되고 Reload를 안내한다(전하 ± 같은 상대 편집은 최신 상태 기준으로 계산).
- 문서당 화학 그래프·페이지는 하나다. 장면 인스턴스는 최대 20개이며 그룹과 단순 선·사각형·화살표 및 명시적 참조를 기록한다. 여러 그래프·페이지, 텍스트 등 고급 장면 객체, 반응 의미, 화면 확대·이동은 아직 없다.
- version 1 초안은 복구해도 그 세션의 다음 초안 쓰기나 저장 전까지 디스크에 version 1로 남는다(읽을 때마다 다시 옮김). migrated/의 보관본은 Chemifree가 지우거나 목록에 보이지 않으며, 정리는 사용자 몫이다.
- 캔버스는 포인터 중심이다. 키보드만으로 원자를 배치하는 접근 경로는 없다(인스펙터 편집과 단축키는 있음).
- CI의 browser job은 Ubuntu 24.04에서 잠근 playwright-core가 가리키는 Chromium을 시스템 라이브러리와 함께 설치하고, test:browser:routes와 test:browser를 재시도 없이 한 번씩 실행한다. route 검사가 실패해도 smoke는 실행한다. CHEMIFREE_SMOKE_NO_SKIPS=1이라 root 실행 등으로 건너뛴 단계가 있으면 실패한다. 통과·실패와 관계없이(빌드 실패 포함) results.json, build.log, environment.json, requests.log, api.log, web.log, 스크린샷(실패 시 failure.png)을 artifact로 14일 보관한다. 일부러 깨뜨린 빌드로 이 job이 실패하는지 확인한 기록은 verification/browser-ci.json에 있다. 포트 3001·5173을 쓰므로 한 러너에서 동시에 두 번 실행할 수 없다.
- 브라우저 smoke에서 초안 버리기·사본 저장 직후 API를 죽이는 단계는 그 작업이 뒤따라 보내는 목록 읽기 응답을 기다린 뒤 죽인다. CI(run 40652695710)에서 강제 종료가 이 읽기를 끊어 Vite 프록시가 502로 답해 한 번 실패했다. 제품 결함이 아니라 테스트 동기화 문제이며 같은 순서가 main에도 있었다.
- 브라우저 smoke의 '재시작 모의' 단계가 가끔 실패하던 문제(#8)는 제품 결함이 아니라 테스트 동기화 문제였다. Playwright 1.56.1은 마지막 page route가 사라지면(times: 1 route 소진, unroute) 요청 가로채기를 끈다. 그 순간 시작된 요청은 브라우저에 멈춘 채 서버에 닿지 않고 10초 뒤 클라이언트가 중단한다. smoke는 컨텍스트 전체에 통과용 route를 두어 가로채기를 끄지 않는다(scripts/lib/request-interception.mjs). npm run test:browser:routes가 Chemifree 코드 없이 이 경쟁을 재현한다. 재전송 편집 확인은 스피너 상태에만 기대지 않고 실제 200 응답을 기다린다. 실패하면 failure.png, requests.log, api.log(--log-requests), web.log를 남기고 임시 디렉터리를 지우지 않는다. 기록은 verification/browser-route-race.json.

## 추적 현황

현재 집계: 요구사항 33개(partial 9, not_implemented 24) · 부분 58개(implemented 21, partial 2, unverified 2, not_implemented 33) · conformance 사례 29개(passed 27: accept 15·reject 12, not_run 2, failed 0) · 공개 선언 2914개(구현 0, 단일 계획 할당 0)

이 커밋 기준 partial 요구사항은 MODEL-01, MODEL-02, OPS-01, OPS-02, STYLE-01, RENDER-01, API-01, QUALITY-01, CLEANROOM-01이고 나머지는 not_implemented다. 상태는 기록한 판단이며 검사가 판단의 옳음을 보장하지 않는다. 공개 저장소에 개인·공급자 자료가 없다는 점(CLEANROOM-01.2)과 사례 작성의 독립성(QUALITY-01.2)은 자동 검사가 없어 unverified다.

## 다음 구현 단위 제안

사용자 요청이 범위를 정한다. P11은 #34와 이전 초안 보관을 다룬 #35로, P12는 #36으로 병합됐다. 다음 실행 순서는 P14(외부 DATA registry) → P15(수치 프로필)이다. PR·리뷰·병합 상태는 GitHub와 `docs/verification/review-history-batch2.md`에서 확인한다. 그 밖의 후보:

1. 저장·복구 후속(필요 시): 휴지통 영구 삭제·비우기, 초안 만료 정책(현재는 만료 없음), 읽기 전용 모드.
2. 형식 입출력: MOL/SDF·SMILES 독립 codec과 손실 보고(format_conversion). Chemifree 저장 형식은 손상 입력을 통째로 거절하지만, 외부 형식 가져오기는 공개 명세의 손상 입력 관찰(§Parsing)을 참고해 거절·수정·손실을 항목별로 정하고 보고한다. 조용한 수정은 하지 않는다.
3. 화학 그래프 진단: 원자가·암시적 수소·분자식(structure_analysis 일부)과 공개 명세의 관찰 사례 대조.
4. 편집기 확장: 고리 템플릿, 확대·화면 이동과 추가 장면 객체.
5. 사용자 인증·원격 배포는 별도 접근 제어 설계 후 진행.

명명·NMR·물성·고급 형식·Windows bridge·팀 서비스를 한꺼번에 넣지 않는다. 기능을 실제로 연결하고 검증한 뒤 capability 상태를 변경한다.

## 독립 구현 경계

제품 코드는 새 컨텍스트에서 공개 기능 명세와 독립적인 자료만으로 작성한다. 원본 바이너리·원본 코드/디컴파일·조사 evidence·private vendor payload를 열지 않는다. GitHub 저장소의 현재 독립 소스와 검증·설계 문서는 구현 입력으로 사용할 수 있다.

기존 분석 인계와 원시 데이터가 공개 저장소에 포함되어 있다고 가정하지 않는다. 필요한 명세는 승인된 공개 인계에서 기능별로 읽고, 원본 분석을 다시 시작하거나 전체 자료를 공개 저장소에 복사하지 않는다.

기존 개인용 모델 데이터와 서비스용 데이터셋은 별도 범위다. 사용자 결정은 로컬 보조 인계와 실제 요청을 따른다. 원본 실행 엔진이나 가짜 수치로 미구현 기능을 대체하지 않는다.

## 세션을 시작할 때 붙여 넣을 예시

~~~text
이 Chemifree 프로젝트를 이어서 진행해줘.
AGENTS.md, docs/HANDOFF.ko.md와 존재하면 private/SESSION_CONTEXT.ko.md를 먼저 읽어.
첫 수직 기능(생성 → 편집 → Undo/Redo → 저장 → 다시 열기)은 완료됐으니
현재 코드·CI 상태를 확인하고, 다음 범위로 <원하는 기능>을 구현해줘.
제품 코드는 공개 기능 명세와 독립 자료만 사용하고 원본 분석·바이너리는 열지 마.
~~~

프로젝트 지침을 세션 시작 시 읽는 동작은 [공식 OpenAI 문서](https://learn.chatgpt.com/docs/agent-configuration/agents-md)에 설명되어 있다. 작업 재개에는 파일과 현재 Git 상태를 기준으로 사용하며, 과거 채팅 전체가 새 컨텍스트에 들어왔다고 가정하지 않는다.
