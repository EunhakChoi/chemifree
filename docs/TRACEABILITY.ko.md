# 요구사항·공개 선언 추적과 conformance 실행

이 문서는 P08이 더한 추적 파일, conformance runner와 추적 검사가 무엇을 기록하고 무엇을 확인하는지 정확히 적는다. 여기 적힌 확인 범위를 넘는 성질(화학적 정확성, 다른 프로그램과의 동등성, 기능 완성도)은 이 장치들로 확인되지 않는다.

P38a는 bounded SVG drawing export를 추적 부분 `RENDER-01.svg-export`(구현)로 연결한다. `document.export`의 `format: "svg"`가 원자·결합·라벨, 장면 그래픽과 PNG 이미지를 그리고, 그리지 않거나 바꾼 특성은 손실로 보고한다. 보이는 특성과 입체 설정은 `accepted_losses` 동의를 요구하며, 제목·선택·참조·장면 그룹은 동의 없이 손실로만 보고한다. 구현 경로는 `crates/application/src/svg.rs`, `svg/*.rs`, `operations/svg.rs`이고, 근거는 적합성 사례 `svg-export-drawing`·`svg-export-needs-consent`·`svg-import-refused`와 `check:check:rust`·`check:test:client`·`check:test:contracts`다. 이 부분은 그 bounded 내보내기 범위만 확립한다. 일반 SVG 출력, `render.scene`, 글자 셰이핑, 클리핑, 레이어와 웹 공유 렌더러는 확립하지 않는다. RENDER-01 요구사항은 partial로 남고 P38은 `in_progress`다. 범위와 그리지 않는 목록은 [HANDOFF의 P38b1·P38a 섹션](HANDOFF.ko.md)과 [계약의 SVG 절](CONTRACT.md#svg-drawing-export-p38a)을 따른다.

P34f는 단일 외부 single 결합의 말단 target ID에 source anchor를 바인딩하는 교체다.
기존 boundary/mark/좌표/참조·이웃 stereo/group을 보존하고 N−1 atom/M internal bond만
fresh IDs로 추가한다. 새 부분 OPS-01.template-replacement는 공통 Rust/client/HTTP=CLI
검사에 연결한 partial이며, 작성한 검사는 이번 구현 세션에서 실행하지 않았다. 별도
file/backend browser와 리뷰·검증 대기는 [P34f 기록](verification/p34f-template-replacement.md)을 따른다.

P34e 후보는 명시 native template anchor의 translation-only single-bond 연결이다.
source-anchor 저장 입체 손실·단절·style/배치/ID/한도를 거절하고 기존 paste remapping·
AddBond destination notes/group·atomic history/replay를 재사용한다. inspect/clone plan은
읽기 전용이며 웹은 명시 source/target/vector→preview→apply와 immutable recovery다.
Domain/Application/HTTP=CLI/client/real-backend browser 회귀는 작성만 했으며 실행하지
않았다. 적격 evidence나 상위 상태/집계를 승격하지 않고 [P34e 기록](verification/p34e-template-attachment.md)을 따른다.

P34d의 `OPS-01.user-template-files`는 별도 browser 검사로 실제 파일 다운로드→재선택→
backend 삽입을 확인했다(`5c9e148` 부모 격리 targeted 통과). 이 전용 script는
`npm run check`에서 호출되지 않아 현재 traceability 정책의 적격 evidence가 아니다.
따라서 구현 경로와 P34 후속은 유지하되 evidence는 비우고 해당 부분을 `unverified`로
기록한다. 별도 browser 통과를 공통 검사 근거로 치환하거나 기존 native 검사에 새 UI의
검증 범위를 덧씌우지 않는다. `a19062d` 통합 full은 이 evidence 자격과 집계 불일치로
실패했으며, 수정 후 full/CI 재검증은 pending이다. [P34d 기록](verification/p34d-user-template.md)을 따른다.

P34c 후보는 기존 원자에서 방향을 지정한 5/6 탄소 고리 연결을 더한다. shared attach_ring은
기존 insert_bond의 stereo clearing·group prune·notes, transaction 원자성/history와 웹의
immutable recovery를 재사용한다. Domain/Application(일반 bond-edit oracle)/HTTP=CLI/client/
production UI 회귀를 작성했으나 실행하지 않았다. 상위 상태/기존 conformance 근거는
승격하지 않으며 [P34c 기록](verification/p34c-attached-ring.md)을 따른다.

P34b 후보는 기존 고리 명령에 size5(생략/explicit6은 기존6)를 더하고 실제 웹 5/6 선택과
captured size의 immutable recovery를 연결한다. 독립 5-cycle·기하·rollback·저장/history·
HTTP=CLI·client·production UI 회귀를 작성했으나 실행하지 않았다. 상위 상태나 기존
conformance 개수/통과 근거는 승격하지 않으며 [P34b 기록](verification/p34b-five-ring.md)을 따른다.

P34a 후보는 shared `insert_ring` 명령과 웹 중심 클릭으로 독립 탄소 6원 단일결합 고리를
추가한다. 현재 길이·단일 배치·기존 선택 유지·한 Undo 및 미확정 결과의 같은 attempt
확인으로 제한하며, 전체 MODEL-02/OPS-02/MODEL-04의 상태는 승격하지 않는다.
회귀는 작성했으나 이 작업에서 실행하지 않아 새 conformance 통과 근거를 만들지 않았다.
attachment·fusion·사용자 template·약어·복수 배치·layout/conformation은 후속이다.
[P34a 기록](verification/p34a-independent-ring.md)에 검증 범위와 실행 대기를 구분한다.

P15 수치 기반 위에서 P19는 한정된 분자식·질량·동위원소 계산과 별도의 질량 표시 자릿수 환경 설정을 연결한다. 일반 수치 정밀도, 배치·예측·NMR 등은 후속 작업이다.

P20은 한정된 V2000 MOL/SDF 입출력과 레코드별 손실 보고를 연결한다. 고급 CTfile 기능과 다른 형식은 후속 범위다.

P21은 한정된 OpenSMILES 읽기와 기록 순서·canonical SMILES 쓰기를 연결한다. P29c는 사면체와 CX 그룹 읽기를, P29d는 기록형 사면체와 지원 그룹 쓰기를 더한다. Canonical 입체와 지원 밖 입체는 손실 동의가 필요하다. `/`·`\` 방향 결합, query/반응 SMILES와 InChI import는 후속 범위다. P30의 `chem.inchi`는 명시적으로 등록한 공개 InChI 1.07.5 Linux provider의 제한된 Standard InChI/InChIKey 생성이며 버전·SHA·옵션·경고·손실을 보고한다. 미설정/로딩 오류는 unavailable이며 공식 배포본의 별도 실행 파일과 native 6개·Application 4개 정확 비교는 통과했으며 독립 화학 엔진 비교가 아니다. 최종 로컬 검사·브라우저 smoke와 [PR #149의 두 리뷰 및 수정](https://github.com/EunhakChoi/chemifree/pull/149)은 완료했다. 공식 비교는 로컬 opt-in 실행 근거이며 일반 CI의 자동 비교가 아니다. [병합 후 main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42081222618)의 check·browser가 통과해 P30의 제한된 Linux 생성 프로필을 완료로 기록한다. CODEC-05.inchi만 implemented로 바꾸고 CODEC-05·CHEM-02 및 미구현 catch-all은 partial/미구현 상태를 유지한다. InChI import·웹 패널 등 전체 UX는 열린 P48 범위에 남으며 더 넓은 화학 지원·다른 플랫폼·강제 중단·공식 비교의 CI 자동화는 납품 범위 밖이다.

P25a-2b는 입체 표시와 명시된 입체 배치의 저장·설정·해제(MODEL-03.1, 구현)를, P25a-3a는 요청이 있을 때만 도면에서 배치를 읽어 저장하는 `set_configuration_from_drawing`(MODEL-03.2, 구현)을, P25a-3b는 저장 배치와 도면을 비교하는 읽기 전용 `chem.stereo`(MODEL-03.3, 구현)를 연결한다. P25b-1은 웹 편집기에서 쐐기·해시 쐐기·물결 결합 표시를 그리고 같은 Undo 단계에 도면에서 읽은 배치를 저장하는 제스처(MODEL-03.4, 구현)를, P25b-2는 웹 편집기가 입체가 있는 도면에서 `chem.stereo`를 스스로 불러 요약 줄·Stereo 패널·캔버스 링으로 저장 배치와 도면의 일치를 보이고 한 번의 클릭으로 갱신·저장·제거하는 표시(MODEL-03.5, 구현; 증거는 `check:test:client`이며 브라우저 스모크는 본문에서만 언급한다)를 연결한다. P26b는 저장된 배치에서 R/S/E/Z를 읽기 전용으로 계산하는 `chem.cip`(`chemifree-cip-1`, 규칙 1a·1b·2·3만, MODEL-03.6, 부분; 증거는 적합성 사례 14개와 `check:check:rust`·`check:test:contracts`)을 연결한다. 규칙 4a–5·의사비대칭 r/s·헤테로방향족·축/평면, 동위원소·대칭·분할·고급 그룹, 거울상·뒤집기 명령, crossed 캔버스 도구는 MODEL-03.rest(미구현)로 남고, 형식별 입체 입출력(CODEC-03, CODEC-05)은 근거 사례로만 연결되며 구현 표시가 아니다. P26c는 CIP의 웹 표시(원자·결합 레이블 스위치 둘, 기본 꺼짐, 저장 배치의 레이블만, MODEL-03.7, 구현; 증거는 `check:test:client`이고 브라우저 스모크는 증거 검사가 아니다)를 연결한다. P25와 P26은 진행 중이다.

P28a는 MOL V3000 코덱 핵심(CODEC-03.v3000-core, 구현)을 제공자 계층에서만 연결한다: 논리 줄 읽기·이어쓰기·블록 파서와 핵심 부분집합 쓰기이며 그 시점에는 연산·와이어·웹이 쓰지 않아 `document.import`·`document.export`가 V3000을 거절했고(P28b1이 연결했다, 아래), 근거는 `cargo nextest run --workspace`가 도는 `check:check:rust`다(RDKit 대조는 손으로 돌리는 스크립트라 근거로 세지 않는다). SGroup·query·입체·그룹 매핑·V2000 하향은 CODEC-03.rest에 남는다.
P28b1은 V3000 핵심 부분집합과 결합 표시를 `document.import`·`document.export`에 연결한다(CODEC-03.v3000-wire, 구현): 레코드의 V2000/V3000 판별, 방향족 결합, 쐐기·해시·물결·교차 표시의 입출력과 가져올 때의 배치 유도, `ctab` 요청 키, V2000 결합 입체 열 쓰기·읽기, 표시가 보여 주지 않는 입체에 대한 `accepted_losses` 동의다. 근거는 적합성 사례 `molfile-v3000-wedge-round-trip`·`stereo-molfile-export-consent`·`molfile-v3000-refusals-leave-document-unchanged`와 `cargo nextest run --workspace`·계약 스모크이며, RDKit 대조는 여전히 손으로 돌리는 스크립트라 근거로 세지 않는다. query 원자·결합, dative, 입체 collection↔그룹 매핑, SGroup, 웹 선택은 CODEC-03.rest에 남는다. 이전 사례 `stereo-molfile-export-refused`는 `stereo-molfile-export-consent`로 바뀌었다.
P28b2는 V3000 `COLLECTION` 블록과 향상된 입체 그룹(absolute·and·or)의 양방향 매핑, chiral 플래그 쓰기 규칙, `mol-v2000`·`mol-v3000` 형식 id 별칭을 더한다(CODEC-03.v3000-groups, 구현): 번호를 적힌 그대로(0·음수·int32 아무 값) 쓰고 읽으며, 저장 배치가 있으나 그룹에 속하지 않은 중심은 동의가 필요한 손실이다(Chemifree가 지어내지 않는 의도적 제품 선택이며 다른 프로그램의 해석에 대한 주장이 아니다). 근거는 적합성 사례 `molfile-v3000-stereo-groups-round-trip`·`molfile-v3000-collection-refusals-leave-document-unchanged`·`molfile-v3000-group-append-renumbers-colliding-number`와 `cargo nextest run --workspace`·계약 스모크이며 RDKit 대조는 손으로 돌리는 partition 비교라 근거로 세지 않는다. V2000·CDX·CXSMILES의 그룹 매핑(P29c가 CXSMILES 읽기를 더했다), query, dative, SGroup은 남는다(웹 선택은 아래 P28c).
P28c는 MOL·SDF 웹 패널을 V3000 연산에 연결한다(CODEC-03.v3000-web, 구현; Rust·계약 의미 변경 없음): 버전 선택(V2000 기본·V3000·Auto)을 `ctab` 키로 보내며 이는 페이지의 선택일 뿐 문서에 저장되지 않고, 기록된 방언을 `result.ctab`로 보여 주며, 내보내기 손실 동의(평이한 문장·`accepted_losses`·바뀐 다이제스트 재요청)와 V3000 거절 이유별 평이한 문장(계약의 이유 표와 대조)을 제공한다. 근거는 `check:test:client`뿐이고(순수 클라이언트 검사와 변이 확인), 브라우저 스모크 시나리오는 보조이며 근거로 세지 않는다. query·SGroup·R-group 지원은 주장하지 않는다(거절된다).

P27b는 향상된 입체 그룹(absolute·and·or)의 와이어 노출을 연결한다: `document.transaction`의 명령 8개와 스냅샷 `stereo.groups`, 의미가 바뀔 때의 통지, SMILES 내보내기의 동의 손실 `stereo_group`(MODEL-03.8과 STEREO-01.1, 구현; 증거는 적합성 사례 12개와 `check:check:rust`·`check:test:contracts`·`check:test:client`)이다. 웹 표시·편집(P27c)은 STEREO-01.2(구현, 증거 `check:test:client`)로 구현했고, V3000·CDXML·CDX 등 형식 입출력(P28·P29)은 STEREO-01.rest(미구현)로 남는다. 번호 0·음수·생략은 진술한 대로 저장되고 그 밖의 뜻을 주장하지 않으며, AND의 비율과 어떤 형식과의 동등성도 주장하지 않는다. 적합성 사례는 정해진 입력에 대한 기대값만 확립하고 그룹의 화학적 의미를 판단하지 않는다.

P24는 한정된 부분구조 검색(`chem.substructure`)과 원자 대응을 연결한다(MODEL-02.7). 고리·차수·수소 개수 조건, 입체와 질의 SMILES는 기록할 수 없으므로 후속 범위다. 입체가 있는 도면은 일치 대응을 입체 없이 계산하므로 응답이 `partial`과 `stereo` 손실을 함께 보고한다(`chem.identity`도 같다). RDKit 비교(`scripts/verify-query-rdkit.py`)는 수동 실행이며 `npm run check` 밖이고 fixture 범위를 넘는 동등성을 주장하지 않는다.

P22는 한정된 CDXML rich 문서와 손실 동의 기반 단일 그래프 투영을 연결한다(P29b는 그 투영에 공개 근거가 확인하는 입체 읽기를 더했다: CODEC-01.cdxml-stereo-read, CODEC-02.cdx-stereo-atoms(원자 태그와 결합 Display, partial), STEREO-01.cdxml-groups-read; 바이너리 결합 Display(0x0601)는 읽고 Display2·DoublePosition은 아직 없다; P29e가 아래 제한된 도면 내보내기를 더했다). P23은 같은 공통 모델에 한정된 CDX 바이너리 코덱을 연결한다(CODEC-02는 partial). 고급 객체와 일반 장면 렌더링은 후속 범위다.

P29c는 SMILES·CXSMILES 입체 읽기만 더한다: CODEC-05.smiles-stereo-read(`@`·`@@`를 표 없는 저장 배치로, 거절은 코드·계열·span과 함께), STEREO-01.cxsmiles-groups-read(`a:`·`o<n>:`·`&<n>:`를 향상된 입체 그룹으로, 문법 근거는 공개 RDKit Book 예시뿐이고 기능 명세에는 CXSMILES가 없다). 증거는 적합성 사례 두 개와 `cargo nextest run --workspace`이며 RDKit 대조(`scripts/verify-smiles-stereo-rdkit.py`)는 수동이라 근거로 세지 않는다. 이중결합 방향 `/`·`\` 읽기와 쓰기 및 남은 입체 형식은 CODEC-05.rest·STEREO-01.rest에 남는다.
P29c2a는 방향 결합 `/`·`\` 읽기만 더한다: CODEC-05.smiles-directional-read(저장 이중결합 배치, 표시 없음; 놓을 수 없는 표시는 코드·계열·span과 함께 거절). 증거는 적합성 사례 두 개와 `cargo nextest run --workspace`이며 RDKit 2026.09.1 대조는 수동이라 근거로 세지 않는다. `/`·`\` 쓰기(P29c2b)는 CODEC-05.rest에 남는다.

P29c2b는 기록형 SMILES의 방향 결합 `/`·`\` 쓰기를 더한다: CODEC-05.smiles-directional-write(not_implemented에서 implemented로, P21 후속 제거). 증거는 적합성 사례 두 개(`smiles-directional-bond-write-recorded`, `smiles-directional-bond-write-omissions`)와 `npm run check`가 실행하는 `cargo nextest run --workspace`이며, RDKit 2026.09.1 대조(`verify-smiles-stereo-rdkit.py`)는 수동이라 근거에 넣지 않았다. 쓸 수 없는 배치는 동의가 필요한 `stereo_configuration` 손실이고 canonical은 여전히 입체를 쓰지 않는다. 입체성 판정과 다른 프로그램과의 동등성은 주장하지 않는다.

P29e는 기존 CODEC-01·CODEC-02·STEREO-01 부분의 범위에 제한된 CDXML/CDX 도면 내보내기를 더한다. 확인된 typed carrier 네 개와 엄격한 opaque 보존, 첫 배치의 표시·실제 출력 위치에서 유도하는 그룹, 1–127 번호 정규화와 손실 동의를 검사한다. 도면이 향상 입체 표시 가시성을 저장하지 않으므로 출력 그룹 구성원의 `0445=true`는 보존이 아니라 명시적 선택이며, 그룹을 쓸 때 한 번의 `enhanced_stereo_display`(`normalized`·`rendering`, 동의 불필요)로 보고한다. 배치·그룹 왕복 검사는 이 가시성 설정을 포함하지 않는다. 검증은 export 연산 7개, import 연산 34개, carrier 제공자 5개, application 단위 25개다. 추적 근거는 기존 `check:check:rust`이며 이 개수 자체를 새 적합성 사례로 세지 않는다. P29는 `in_progress`이고 방향 결합 SMILES와 전체 형식 호환은 미완료다.

P29f는 기존 CODEC-01·CODEC-02의 제한된 도면 export에 웹 패널을 연결한다. 명시적 손실 동의 전 다운로드 없음, 도면·형식·service generation 변경 시 미리보기·제안 무효화, 내려받기 직전 snapshot·바이트·손실 재검증과 CDX 바이너리 저장을 검사 대상으로 둔다. 구현 경로와 `check:test:client`를 기존 부분에 더하며 부분 상태·개수와 공개 선언 수는 바꾸지 않는다. P29f npm run check·브라우저 smoke와 Codex·독립 subagent 리뷰 두 건은 완료했다. P29g는 아래의 제한된 프로필 근거를 확인한다.

P29g는 [형식별 표](CONTRACT.md#stereo-interchange-evidence-p29g)와 [codec corpus 검사](../crates/application/tests/stereo_codec_corpus.rs), 새 적합성 사례 세 개를 연결한다. suite 25는 99/99 통과(accept 74·reject 25)이며 corpus 두 테스트는 도면 경로 92개와 별도 선언한 그룹 경로 16개를 통과했다. 기존 23개 corpus는 그대로이고 그래프·배치·partition·표시를 따로 비교한다. 출력 `0445=true`는 도면이 저장하지 않는 가시성의 명시적 정규화(`enhanced_stereo_display`)이며 보존된 배치·그룹의 비교 축에 섞지 않는다. RDKit 2022.03.2 별도 대조는 도면 23개 0 problems·SMILES 24개 0 failures·지원 V3000 core/marks/partition 일치였으나 `STERAC-128` 번호는 RDKit이 건너뛰며 CDXML/CDX 출력은 독립 판독하지 않았다. 수동 RDKit 대조를 새 적합성 사례나 자동 추적 근거로 세지 않는다. P29는 이 문서화된 bounded 프로필을 `done`으로 기록한다. 방향 결합은 열린 P21에 연결한 미구현 후속 **P29c2**이며 SMILES의 lone-pair 넷째 자리 표는 계속 거절이다. CODEC/STEREO의 미구현 catch-all 상태는 유지하고 완료한 P29 대신 해당 열린 행에 후속을 연결한다.

P32a–f의 bounded 선택·기하 납품을 완료했다. 공유 scope/move union·outermost whole-object align6종/distribute2종·명시 pivot transform과 Scene 제어·포함 marquee/Shift 선택·active-placement mixed 이동/변환을 제공한다. 범위·중복·references·비선택 geometry·stored stereo/marks/groups·revision/원자성·Undo/Redo는 domain/application 및 여섯 P32 conformance 사례, client/browser 검사로 비교한다. 최종 구현 eeb6f7a의 격리 npm run check(373 client·105/105 conformance·traceability)와 browser가 통과했다. strict routes는0/1000 stalled, no-skips browser는 skipped_steps=[]와 두 mixed selection/transform flags=true다. shared lock/private copy·MemoryMax6G·swap0, peak anon1462/cache123MiB이며 guard 제거 mutation은 회귀 실패를 확인했다.

[#161](https://github.com/EunhakChoi/chemifree/pull/161)의 병합2c7a51847cd316b73a2324cb8a8152f95ee3ecc3과 [최종 PR CI](https://github.com/EunhakChoi/chemifree/actions/runs/42115749139)·[병합 main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42117006940)의 성공을 근거로 bounded P32를 done으로 기록한다. 이 완료 문서 PR 자체의 CI는 아직 대기다. #154 late one-ULP center suppression과 #159 midpoint tie는 개선 여부의 exact 비교로 수정했고, post-union-center drift는 #161의 arrangement_center_unstable 사전 거절로 반복 변화를 막는다. #152 late handoff와 #161 refusal 문서 지적도 반영했다. 각 PR의 두 리뷰와 수정 근거는 [P32 완료 기록](verification/p32-selection-completion.md)에 있다.

세 웹 부분은 check:test:client 실행 근거로 implemented로 승격했다. 정렬/분배는 outermost whole-object, marquee는 active-placement atom 중심·graphic 두 점 포함, 명시 graph 대상은 단일 active placement다. 웹 변환은 geometry-center quarter turn/축 반사이며 rectangle은 정확한 quarter turn만 받는다. lasso/교차 선택·여러 배치 raw graph 선택·partial-atom arrangement·custom-angle/pivot 웹 UI·대각선 반사·snapping·cleanup은 범위 밖이다. 전체 OPS-01/OPS-02는 partial을 유지한다.

P33은 로드맵의 제한된 clipboard 범위와 명시 인수 기준에 한해 `done`이다. [완료 기록](verification/p33-clipboard-completion.md)은 #172의 isolated check·standard browser·8개 clipboard browser와 PR CI 성공을 기록하고, merge `1082293`의 [post-merge main CI](https://github.com/EunhakChoi/chemifree/actions/runs/42149543576) 성공도 별도로 기록한다. 완료 문서 자체의 final-head CI는 별도 확인 대상이다. 상위 OPS-01/OPS-02/CODEC 요구사항과 미구현 catch-all은 그대로 partial/미구현이다.

P33a/b/c는 native graph version1의 Copy/Cut/Paste다. 명시 source placement, induced 내부 결합·crossing 보고, 기록 속성/mark·완전한 0D stereo, fresh ID·선택·Undo/Redo·same-request replay를 제공한다. destination style은 정확히 같아야 하며 graph paste는 한 placement만 받는다. Cut은 한 source placement·crossing 없음·incoming graph refs 없음과 successful browser write 뒤 exact source/process를 요구한다. clipboard write와 삭제는 별개이며 수동 copy는 삭제를 허가하지 않는다.

P33e/f/j는 별도 native graphics version2 forest Copy/Cut/Paste다. paint/containment 순서·내부 self/cycle refs를 fresh IDs로 보존하고 여러 placement 목적지에서도 chemistry/placements를 유지한다. instance·mixed graph target·outgoing/graph refs·child 두 개 미만 group을 거절하고 Cut은 outside incoming도 거절한다. P33j는 frozen roots를 subtree 삭제하며 외부 부모를 자동 prune하지 않고 history와 refs를 복구한다. Scene Cut이 없다는 과거 기록은 P33j 이후 현재 상태가 아니다.

P33d/g는 기존 MOL·SMILES preview/loss-consent를 사용하는 명시 text bridge다. 전체 graph export와 단일-placement 기존 문서 append를 제공하고 기존 좌표/grammar·생성 grid·live selection을 유지한다. P33h는 CDXML whole-drawing export 동의 뒤 명시 clipboard copy, P33i는 정확한 UTF-8 source·page/molecule 선택·preview/loss 동의를 거쳐 새 drawing 생성을 제공한다. CDX는 binary download이며 clipboard text나 기존 문서 append가 아니다. Browser 전송은 명시 read/write·수동 대안을 제공하며 변경을 만드는 import는 immutable uncertain request/process 추적과 restart reconcile 규칙을 따른다.

P33k의 `test:browser:clipboard-integration`은 production panel/hook/client→실제 HTTP/Application/Domain에서 graph 및 nested forest 문서 간 Copy/Paste와 source 불변·fresh IDs/속성/참조·style refusal·history/selection을 확인했다. Clipboard transport만 대체하며 permission/error/restart 검사는 별도 scripted HTTP/clipboard 경계 검사다. 전체 editor canvas/navigation, 실제 OS clipboard 권한 성공과 다른 앱 호환·과학적 정확성은 입증하지 않는다.

Instance 복제·enhanced-stereo-group graph 복사·global shortcut·custom MIME·external Cut·Windows/OLE는 bounded P33 밖이다. 이 완료는 형식 전체/모든 내부 객체의 clipboard 지원을 뜻하지 않는다. 기존 conformance 보고서 집계는 재실행 없이 유지한다. CODEC-06.rest·OPS-01.rest·OPS-02.rest의 open follow-up에서는 완료된 P33을 제거한다. 고급 형식/통합 파일·text UX는 기존 codec 후속과 P48, 전문 객체의 포함/선택은 P72, 정리·키보드 UI는 P47/P85, 전문 scene/reaction 변경 집합은 해당 기존 후속에 남긴다. 선언된 native 복제/shortcut 제한에 새 구현 약속을 임의로 부여하지 않는다.

현재 집계: 요구사항 33개(partial 18, not_implemented 15) · 부분 113개(implemented 64, partial 13, unverified 3, not_implemented 33) · conformance 사례 118개(passed 118: accept 86·reject 32, not_run 0, failed 0) · 공개 선언 2914개(구현 0, 단일 계획 할당 0)

위 줄은 추적 파일과 기록된 실행에서 계산한 값이며, `npm run test:traceability`가 이 문서와 HANDOFF.ko.md에 정확히 같은 줄이 한 번씩 있는지 확인한다.

P31은 웹 캔버스의 로컬 중심·배율, pointer 고정 zoom·pan·fit·100% reset, 역 SVG 좌표 변환과 CSS px 기준 hit/drag 범위를 더한다. 뷰 상태는 문서·명령·revision·Undo·저장에 들어가지 않고 편집·resize에서 유지되며 문서 ID 변경·새로고침은 초기 fit이다. Pan view(H)의 한 포인터 터치와 툴바는 있지만 pinch·다중 터치는 없다. 로드맵은 이 제한된 뷰포트 범위를 `done`으로 기록하고 `OPS-01.viewport`·`RENDER-01.viewport`를 구현된 부분으로 연결한다. `scripts/web-viewport-check.mjs`, 브라우저 smoke, 리뷰 및 병합 후 CI의 실제 근거와 제한은 [P31 완료 기록](verification/p31-viewport-completion.md)에 있다. 상위 요구사항은 partial이며, P31 도입 당시 후속이던 bounded P32 선택·변환은 위 완료 기록의 검증·리뷰·병합 근거로 완료됐다. 이 완료 문서 PR 자체의 리뷰/CI는 대기다.

## 파일

| 파일 | 내용 |
| --- | --- |
| `docs/traceability/requirements.json` | 공개 준비 요구사항 33개. 요구사항마다 자체 요약, 관련 연산 이름(operations.json의 이름, 자체 대응), 부분(part) 목록과 부분에서 도출한 상태. 출처 두 개(`requirements`, `operations`)를 `spec/` 아래 상대 경로와 SHA-256으로만 적는다 |
| `docs/traceability/declarations.json` | 공개 선언 2,914개를 제안 경로(route) 8개별로 센 수, 출처별 수, 경로별 상태·구현 수·단일 계획 할당 수·후속 로드맵 행. 선언 수는 기능 수가 아니다(collection·메뉴·상태 getter 포함) |
| `conformance/suite.json`, `conformance/cases/*.json` | 사례 묶음(`suite_version`)과 사례(실행 가능 또는 대기(pending)). 개수는 위 집계 줄에 있다 |
| `crates/conformance` | runner(lib)와 실행 파일 `chemifree-conformance` |
| `docs/verification/conformance-report.json` | 커밋된 실행 기록. 시각을 담지 않으며, 실패한 사례가 없는 실행은 같은 바이트를 쓴다 |
| `scripts/traceability-check.mjs`, `scripts/traceability-check.test.mjs` | 추적 검사와 주요 규칙의 테스트(모든 규칙에 테스트가 있지는 않다) |
| `docs/roadmap/implementation-roadmap.json` | 현재 로드맵. 후속 계획은 이 파일의 행 ID다 |

기능 명세 전체는 저장소 밖(로컬)에 둔다. 추적 파일에는 경로·SHA-256과 명세에서 도출한 ID·개수·자체 요약만 있다. DATA 로더가 정확한 해시를 확인하는 공개 personal-local context 한 파일은 코드에 포함한다.

## 상태와 도출 규칙

부분 상태는 `implemented`, `partial`, `unverified`, `not_implemented`, `intentionally_rejected` 중 하나다. 이 상태는 기록한 판단이며, 검사는 판단이 옳은지가 아니라 아래 형식·연결·일관성만 확인한다.

요구사항 상태는 부분 상태에서 도출하며 적힌 값과 같아야 한다.

1. 모든 부분이 `intentionally_rejected` → `intentionally_rejected`
2. 모든 부분이 `implemented` 또는 `intentionally_rejected` → `implemented`
3. `implemented`나 `partial` 부분이 하나라도 있으면 → `partial`
4. 그 밖에 `unverified` 부분이 있으면 → `unverified`
5. 그 밖 → `not_implemented`

부분이 없는 요구사항은 거절한다. 요구사항마다 다루지 않은 나머지를 담는 catch-all 부분(`catch_all: true`)이 정확히 하나 있고, 그 상태는 `not_implemented`나 `unverified`만 될 수 있다. `intentionally_rejected` 부분은 비지 않은 `reason`이 필요하다(다른 상태는 `reason`을 가질 수 없다). `implemented`·`partial` 부분은 존재하는 구현 경로 하나 이상과 실행되는 근거 하나 이상이 필요하다. `not_implemented`·`unverified`·`partial` 부분은 후속 계획이 필요하고 `implemented` 부분은 후속 계획이 없다. 모든 후속 계획은 완료(done)가 아닌 로드맵 행이다.

이 버전에서는 모든 요구사항이 catch-all을 하나 가져야 하고 catch-all은 `not_implemented`나 `unverified`이므로, 위 규칙에서 요구사항 상태는 `implemented`나 `intentionally_rejected`가 될 수 없고 최대 `partial`이다. 요구사항이 catch-all 없이 끝날 수 있게 하는 변경은 검사도 함께 바꿔야 한다.

공개 선언 경로의 상태는 구현 수로 정해진다. 구현 0이면 `not_implemented`나 `unverified`, 전부면 `implemented`, 그 사이면 `partial`. 현재 모든 경로가 구현 0, 단일 계획 할당 0이다.

## 근거는 두 종류뿐이다

| 근거 | 실행되는 근거로 인정하는 조건 | 확립하는 것 |
| --- | --- | --- |
| `case:<사례 ID>` | 사례가 묶음에 있고, 그 사례의 `requirements`가 이 부분의 요구사항을 포함하고, 기록된 실행에서 `passed`. 검사는 새 실행(`target/conformance/report.json`)이 기록과 바이트 단위로 같은지도 확인한다 | 그 사례의 단계가 비교하는 값들이 이 제품에서 그 실행 때 기대대로였다는 것. 사례가 비교하지 않는 성질은 확립하지 않는다 |
| `check:<npm 스크립트 이름>` | 아래 도달 규칙으로 `check`에서 도달한 스크립트 | `npm run check`가 그 스크립트를 호출한다는 것(앞선 단계가 성공했을 때). 그 스크립트가 무엇을 검사하는지, 이 부분과 관련된 것을 검사하는지는 확립하지 않는다 |

구현 경로는 근거가 아니며 작업 트리에 존재하는지만 확인한다(Git 추적 여부나 관련성은 확인하지 않는다). 대기 사례(`not_run`)는 근거가 될 수 없다. 이 밖의 근거 형식은 거절한다.

`check:` 도달 규칙: `&&`로 나눈 모든 조각이 정확히 `npm run <이름>`(낱말 셋, 이름은 `[A-Za-z0-9:_.@/-]`)인 스크립트만 "순수"하다. 순수한 스크립트가 가리키는 이름은 호출된 것으로 세고, 그 이름도 순수하면 다시 따라간다. 순수하지 않은 스크립트는 잎(leaf)이다: 호출된 것으로 세지만 그 안에서 무엇을 실행하는지는 읽지 않는다(`exit 0`, `cd`, `exec`, `npm run x -- --flag`, `npm run x --workspace w` 등을 해석하지 않는다). `check` 자체가 순수하지 않으면 아무 스크립트도 도달한 것으로 세지 않는다. npm이 암묵적으로 실행하는 `pre<이름>`·`post<이름>` 스크립트가 `check`나 도달한 스크립트에 있으면 오류다.

## conformance runner

사례 하나는 `id`, `title`, `requirements`, `axis`, `kind`(`accept`|`reject`)와 `steps` 또는 `pending {followup, reason}` 중 하나를 가진다. 선택적인 `data_fixture`는 conformance 전용 합성 자료의 이름과 SHA-256을 고정하며 runner가 실제 내장 fixture의 바이트를 대조한다. 이 경로는 제품 API나 CLI의 DATA 주입 기능이 아니고 결과에도 합성 자료라고 표시된다. 단계는 공유 Application을 HTTP 핸들러와 같은 요청 타입(같은 DTO로 본문을 역직렬화)과 같은 인자로 호출하고, 모든 쓰기에 `request_id`를 보낸다. HTTP 계층(상태 코드, Host·Origin·토큰 검사, JSON 거절 응답)은 거치지 않으며 상태 코드는 비교하지 않는다. 호출: `create`, `get`, `transact`, `undo`, `redo`, `save`, `autosave`, `recover`, `close`, `capabilities`, `restart`.

- 모든 쓰기는 `request_id`를 보낸다. 단계에 없으면 `step-<단계 번호>`를 만든다(`[A-Za-z0-9_-]`만 사용). `get`·`capabilities`·`restart`에 `request_id`가 있으면 거절한다.
- 문서는 `create`의 `bind` 이름으로 가리킨다. 바인딩 전 사용, 두 번 바인딩, 오류를 기대하는 `create`의 바인딩은 거절한다.
- `restart`는 Application을 정상 종료 절차(`draft_all`) 없이 버리고 같은 임시 데이터 디렉터리로 API 시작 때처럼 다시 연다. 프로세스 충돌처럼 저장·초안에 없는 편집은 남지 않고 열린 파일 핸들(잠금)은 풀린다. 같은 프로세스 안의 재시작이며 실제 프로세스 강제 종료는 `npm run test:contracts`가 따로 다룬다. `restart`에는 기대값을 둘 수 없다.
- API 프로세스가 돌리는 백그라운드 초안 작성기(DraftWriter)는 runner에서 실행하지 않는다. 초안은 `autosave` 단계로만 쓰인다. 따라서 "저장·초안 없는 편집은 충돌 뒤 사라진다"는 사례는 그 편집이 실제 서비스에서 초안 주기(최대 약 10초)보다 먼저 충돌했을 때에 해당한다.
- 사례마다 새 임시 데이터 디렉터리를 쓴다.
- 불러올 때 거절: 호출별 알 수 없는 필드(명시적으로 확인), 중복 JSON 키, 사례 ID 중복, 잘못된 사례·요구사항·로드맵 ID, 빈 제목, 단계가 있는 pending, 단계 없는 실행 사례, `expect`와 `expect_error`를 함께 가진 단계, 알 수 없는 축, 요청 본문 DTO가 거절하는 본문.
- 기대값: `expect`는 JSON 객체, `expect_error`는 문자열 `code`를 가진 객체. 어느 깊이에서든 빈 객체, 비었거나 목록이 아닌 `$contains`, 알 수 없는 `$` 연산자, 옆에 다른 키가 있는 `$contains`를 거절한다. `api_version` 말고 비교하는 값이 하나 이상 있어야 한다(`null`과 `[]`도 값). 크기 2^53 이상의 실수는 정수가 반올림됐을 수 있으므로 거절한다.
- 일치 규칙: 객체는 부분집합, 배열은 같은 길이로 원소별, `$contains`는 서로 다른 원소에 대응(최대 이분 매칭, 원소 재사용 없음), 정수는 반올림 없이 정확히, 실수와 정수는 정확히 같은 값일 때만 같다. 잘못된 기대값은 실행 중에도 일치하지 않는다.
- `operation` 단계는 연산 계약 1의 요청(`request`, JSON 객체 또는 바이트 그대로 보낼 문자열)을 HTTP·CLI와 같은 `Application::execute`에 넘긴다. `document`가 있으면 묶인 문서 ID를 `parameters.document_id`에 넣고(요청에 `parameters` 객체가 있어야 함), `bind`가 있으면 결과 문서를 그 이름에 묶는다. 요청이 자기 `request_id`를 가지므로 단계의 `request_id`는 거절한다. 연산은 결과가 무엇이든 연산 응답으로 답하므로 `expect_error`는 거절하고, `expect`에 문자열 `outcome`이 있어야 한다.
- `operation`의 `capture`는 응답 JSON 포인터의 값을 이름으로 보관하고, 다음 `substitute`는 기존 `parameters` JSON 포인터에 그 값을 넣는다. 이름 중복·선언 전 사용·없는 입력 위치는 로드 때, 없는 응답 위치는 실행 때 실패한다. 파일 바이트와 동의 digest를 실제 응답에서 다음 요청으로 넘기는 데 쓴다.
- kind 규칙: 실행 가능한 `reject` 사례는 `expect_error` 단계가, `accept` 사례는 (`restart`가 아닌 호출의) `expect` 단계가 하나 이상 있어야 한다. `operation` 단계는 기대하는 `outcome`으로 센다: `ok`·`partial`은 결과, `error`·`unsupported`는 거절. 결과는 kind별로 센다.
- 사례 경로는 묶음 파일 기준 상대 경로이며, 원래 문자열을 `/`와 `\`로 나눠 빈 조각·`.`·`..`가 있거나, 절대 경로이거나, 드라이브 접두(`C:`)가 있으면 거절한다. Rust와 JS가 같은 규칙을 쓴다.
- 실행 파일: `chemifree-conformance [--out PATH] SUITE`. 값 없는(또는 `-`로 시작하는 값의) `--out`, 두 번째 `--out`, 알 수 없는 옵션, 묶음 없음·두 번째 묶음은 실행 없이 사용법 오류(종료 2). 실패한 사례가 있으면 종료 1(기록은 씀), 묶음을 불러오지 못하면 종료 1(기록 없음). `--out`이 없으면 기록을 표준 출력에 쓴다.
- 기록: 결과마다 `id`, `title`, `requirements`, `axis`, `kind`, `outcome`(`passed`|`failed`|`not_run`), `pending`, `failure`, 사례 파일 바이트의 SHA-256, 그리고 kind별·전체 개수. 시각은 기록하지 않는다. 실패한 사례가 없는 실행은 같은 바이트의 기록을 쓴다. `failure` 문구에는 응답 JSON(`saved_at_ms`, 생성된 문서 ID)과 저장소 오류 메시지가 그대로 들어가므로 실행마다 다른 값과 데이터 디렉터리 경로가 포함될 수 있다(커밋하는 기록에는 실패가 없어야 한다).
- 애플리케이션이 panic하면 runner 프로세스가 중단된다(종료 101, 기록 없음). 모든 결과가 기록된다는 보장은 runner가 끝까지 실행됐을 때만 성립한다.

## 검사가 확인하는 것

`npm run test:traceability`(`scripts/traceability-check.mjs`)는 입력을 한 번 읽고 아래 규칙만 확인한다.

- 위의 상태 도출·catch-all·reason·구현 경로·근거·후속 계획 규칙.
- 로드맵의 완료 행마다 `delivered {prs, merge_commit, main_ci}`.
- 로드맵의 `requirement_mapping`이 추적한 요구사항마다 항목을 정확히 하나 가지고, 그 `current_status`가 requirements.json의 상태와 같음(두 파일이 어긋나지 않게).
- 모든 사례의 요구사항 ID가 requirements.json에 있고, 대기 사례의 후속 계획이 열린 로드맵 행.
- 출처: requirements.json에 `requirements`·`operations` 출처가 정확히 하나씩, declarations.json에 `declarations` 출처 하나. 각각 `spec/` 아래 상대 경로(역슬래시·빈 조각·`.`·`..` 없음)와 소문자 64자리 SHA-256. 명세 디렉터리 없이도 확인한다.
- declarations.json: 정수, 경로 합 = 전체, 출처별 합 일치, 경로마다 `0 ≤ implemented ≤ assigned_to_single_plan_entry ≤ declarations`, `about`이 선언 수가 기능 수가 아님을 말함.
- 기록: 결과가 묶음의 사례 순서대로이고 id·title·requirements·axis·kind·pending이 사례 파일과 같으며 현재 사례 파일의 SHA-256을 가짐, pending일 때만 `not_run`, `failed` 없음, 개수 = 재계산. `target/conformance/report.json`이 있어야 하고(없으면 `npm run test:conformance` 안내) 기록과 바이트 단위로 같아야 한다.
- `npm run check`가 `test:conformance`, `test:traceability`, `test:checks`를 직접 실행하고, 앞의 것을 뒤의 둘보다 먼저 실행.
- `현재 집계:` 줄이 이 문서와 HANDOFF.ko.md에 정확히 한 번씩 있고 계산값과 같음. 다르면 붙여 넣을 줄을 출력한다.
- HANDOFF.ko.md의 `이 커밋 기준 partial 요구사항은 …이고` 문장이 정확히 한 번 있고, 거기 적힌 ID가 requirements.json에서 상태가 `partial`인 요구사항의 ID와 등록 순서까지 같음. 다르면 빠진 ID, 더 적힌 ID와 기대하는 목록을 출력한다.
- `CHEMIFREE_SPEC_DIR`(그 아래에 `spec/`이 있는 디렉터리)가 있으면 추가로: 출처 파일의 SHA-256, 요구사항 ID 33개의 집합, 적힌 연산 이름이 operations.json에 있는지, 전체·출처별·경로별(출처별 포함) 선언 수.

## 검사가 확인하지 않는 것

- 상태 판단이 옳은지. 예를 들어 `implemented` 부분의 요약이 사례가 실제로 비교하는 것보다 넓을 수 있다. 근거 연결은 실행 여부만 보장한다.
- `check:` 스크립트가 올바른 것을 검사하는지, 이 부분과 관련된 테스트를 포함하는지.
- 사례가 다른 프로그램의 코드나 자료와 독립적으로 작성됐는지(QUALITY-01.2는 `unverified`).
- 공개 저장소에 개인 자료, 공급자 자료, 실행 엔진, 명세 사본이 없는지. 정책(AGENTS.md)과 .gitignore가 있을 뿐 이를 증명하는 자동 검사는 없다(CLEANROOM-01.2는 `unverified`, 후속 P88·P89).
- `CHEMIFREE_SPEC_DIR` 없이는(CI와 `npm run check` 기본값) 출처 해시·요구사항 ID·연산 이름·선언 수가 명세와 같은지. 그래서 CLEANROOM-01.1은 `partial`이다.
- 단독으로 실행한 `test:traceability`는 runner를 실행하지 않는다. 있는 새 실행 파일을 비교할 뿐이며, `npm run check`가 그 직전에 `test:conformance`를 실행한다.
- 화학, 형식 변환, 다른 프로그램과의 동등성, 완성도.

## 실행

~~~bash
npm run test:conformance        # 새 실행 → target/conformance/report.json, 실패가 있으면 종료 1
npm run conformance:record      # 커밋할 기록 docs/verification/conformance-report.json을 다시 씀
npm run test:traceability       # 추적 검사
npm run test:checks             # 추적 검사 규칙별 테스트(node:test)
CHEMIFREE_SPEC_DIR=<spec/을 가진 로컬 디렉터리> npm run test:traceability   # 명세와도 비교
cargo test -p chemifree-conformance   # runner 단위·통합 테스트
~~~

`npm run check`는 `test:contracts` 다음에 `test:conformance`, `test:traceability`, `test:checks`를 차례로 실행한다.

## 갱신 규칙

bounded P35 완료 후 `MODEL-01.rest`·`RENDER-01.rest`·`STYLE-01.rest`는 P35를 열린 후속에서 제외한다. 남은 advanced scene/graphic style은 P36, geometric references는 P37, general shaping/bounds/clip/SVG는 P38, pages/settings는 P40, reaction-role/mapping references는 P49에 연결하며 기존 prediction/NMR/carrier 후속은 유지한다. P38을 임의 atom-text parser나 미정 label editor의 구현 약속으로 확대하지 않는다. 정확한 대상·범위는 [P35 감사의 후속 대조](verification/p35-completion-audit.md#documentation-integration-checks-still-required)에 기록했다. 이는 추적 경로 수정이며 미구현 요구사항을 완료 처리하거나 적합성 결과를 생성한 것이 아니다.

- 사례를 더하거나 바꾸면 `npm run conformance:record`로 기록을 다시 쓰고, 바뀐 결과를 검토해 함께 커밋한다. 기록은 LF로 고정한다(.gitattributes).
- 기능을 구현하면 해당 부분의 상태·구현 경로·근거를 바꾸고 후속 계획을 지운다. 구현되지 않은 나머지는 catch-all에 남긴다. 근거는 `case:`나 `check:`만 쓴다.
- 상태를 올리기 전에 근거가 그 요약을 실제로 비교하는지 직접 확인한다. 검사는 이를 대신하지 않는다.
- 로드맵 행이 완료되면 그 행을 가리키는 후속 계획을 열린 행으로 바꾸거나 부분을 완료 처리한다(완료 행은 후속 계획이 될 수 없다).
- 명세가 바뀌면 `CHEMIFREE_SPEC_DIR`를 주고 검사를 실행해 해시·ID·개수를 다시 맞춘다. 명세 파일이나 긴 발췌는 저장소에 넣지 않는다.
- 집계가 바뀌면 검사가 출력하는 `현재 집계:` 줄을 이 문서와 HANDOFF.ko.md에 붙여 넣는다.
