# 외부 DATA 등록과 현재 지원 범위

외부 데이터는 저장소와 문서 저장 디렉터리에 포함하지 않는다. 사용자가 별도로 선택한 로컬 DATA 디렉터리와 신뢰하는 등록 기술서를 이용하며, 서버는 시작할 때 한 번 읽은 불변 세대를 사용한다. 데이터가 없거나 거절되어도 문서 편집과 저장은 계속된다. `GET /api/v1/data`, `chemifree data status --json`, 웹의 External data 영역은 데이터 상태와 미구현 엔진을 따로 표시한다. 등록을 바꾸면 서비스를 다시 시작해야 한다.

등록 가능한 개인용 정규화 프로필 중 `private-naming-ring-element-ranks-v1`, `private-naming-valence-reference-v1`, `private-naming-element-endpoint-v1`은 지정 경로와 정확한 레코드 수, 정수 표기·범위·인덱스 집합을 검사한다. `private-caw-element-symbols-v1`, `private-caw-type-elements-v1`, `private-naming-element-ranks-v1`은 공개 phase 6 목록에 고정된 데이터셋 ID·버전과 manifest·원본 registry·형식 프로필의 **실제 파일 바이트 해시**를 대조하고, 지정된 파일 전체를 검증한 뒤 타입 값을 보존한다. 두 명명 순위 파일의 맥락 A와 B는 구분하며 A의 2–109 인덱스가 완전하고 중복되지 않아야 한다. 정수 `-0`은 정수 0으로 읽고 소수·지수 표기를 정수로 받아들이지는 않는다.

`personal-local-nmr-coupling-parameters-v1`도 같은 고정 catalog 식별자와 권리 검토를 사용한다. lookup, amplitude, width, feature schema, source bounds의 다섯 파일을 모두 검증하며, feature schema의 원문 바이트 해시가 manifest의 파일 해시 및 `feature_schema_sha256`과 같아야 한다. 각 binary64 원시 비트와 단위, 배열 순서, 별도 항목 종류를 보존하고 수·키·참조·원본 범위가 모순되면 전체 등록을 거절한다. source bounds는 불활성 출처 정보이며 원본 파일을 열거나 실행하지 않는다. 실제 개인 DATA를 열어 검증하지 않았으며, 공개 명세와 독립 합성 fixture로 reader를 검사했다. 이 데이터가 유효해도 NMR 엔진, feature 의미의 실행 검증, 비어 있지 않은 feature-schema를 요구하는 모델 바인딩은 아직 구현되지 않았다.

`private-naming-enum-labels-v1`은 종류와 하위 종류의 두 파일을 함께 검증하고 서로 다른 맥락, 원래 순서, 반복 표기, ASCII 원문 바이트와 코드·출처 ID를 보존한다. catalog의 고정된 데이터셋 ID·버전과 실제 metadata 파일의 원문 해시가 모두 맞아야 하며 한 파일이 손상되면 해당 데이터셋 전체를 거절한다. 표기는 불활성 데이터이며 명명 엔진이나 실행 경로를 제공하지 않는다. 이 reader도 실제 개인 DATA 대신 독립 합성 fixture로 검증했다.

`private-naming-lexicon-typed-v1`은 사전 항목과 두 텍스트 치환 표를 하나의 불변 세대로 검사한다. 바이트와 Latin-1 텍스트, 반복 표기·순서·구획을 보존하고 두 원자 참조 목록, 다섯 주석 열과 결합 참조가 각 그래프의 범위 안에 있는지 검사한다. 다섯 번째 열의 낮은 바이트는 공개 정정에 따라 수소 수가 아니라 명시적 valence에 1을 더한 값이다. 선택 원문이 있으면 manifest 해시와 사전 행의 원문 범위를 검사하지만 원래 문법을 해독하지 않는다. 텍스트 치환의 출처 범위는 원문과 연결된다는 별도 계약이 없어 정수 범위만 보존한다. 사전 문법·그래프 매칭과 명명 모델 바인딩은 미구현이다.

`private-sequence-nickname-graphs-v1`은 nickname 그래프와 순서 있는 alphabet 후보를 함께 검증한다. 각 문서 안의 ID, `n`·`b` 개수와 결합의 B/E 원자 참조, alphabet 내 키·순번의 유일성 및 nickname 참조를 확인하며 XML 태그·속성·text·tail과 반복 후보의 순서를 보존한다. 선택 원문 CDXML은 일부만 있어도 되며 제공된 각 파일은 길이·해시와 정규화 XML 트리를 대조한다. 공개 catalog는 원문 경로의 glob을 정할 뿐 파일명과 nickname 키의 일치를 규정하지 않으므로 그 관계를 추측하지 않는다. 파일 경로별 바이트 식별자는 신뢰된 manifest와 별도 권리 검토 기록으로 검증한다. 원문은 UTF-8 XML만 지원한다. 외부 DTD를 읽지 않으며 사용자 정의 엔터티 선언은 확장하지 않고 `profile_unsupported`로 거절한다. 일반 XML 속성은 보존하지만 B/E 이외 모든 CDXML 참조의 완전성이나 전체 화학 XML 스키마를 검증했다고 주장하지 않는다. 시퀀스 결합·생체고분자 확장과 모델 바인딩은 미구현이다.

`private-independent-aqsoldb-v1.0-original`은 공개 catalog가 고정한 단일 필수 UTF-8 CSV의 manifest·출처 registry·형식 파일과 파일 ID·분류·해시를 검사한다. 9,982개 행의 지정된 26개 열과 추가 열을 읽으며 쉼표·인용부호·줄바꿈 및 각 셀의 문자열 값을 보존한다. 추가 열의 이름과 값은 개인 입력 안에만 불투명 메타데이터로 남고, 상태 응답에는 추가 열의 개수만 보고한다. 단위 추론과 용해도 예측은 구현하지 않는다. 공개 검사는 독립 fixture를 사용한다.

`private-independent-nist-comp-4.1-snapshot-2026-10-24`은 공개 catalog가 고정한 manifest·출처 registry·형식 파일과 단일 필수 snapshot을 검증하고 출처 텍스트, 선택·null 필드, 원소와 동위원소 항목 및 불투명한 합계 JSON을 보존한다. 정수 필드는 정수값인 소수·지수 표기도 정확히 읽으며 출처·원소·동위원소 필드는 부호 있는 64비트, 개수는 부호 없는 64비트 범위를 벗어나면 거절한다. 출처 경로·URL과 출처가 보고한 바이트 개수는 읽기 명령이나 검증된 실제 파일 크기가 아닌 기술 메타데이터이며 원자량·동위원소 계산은 구현하지 않는다. 개인 입력은 사용자가 별도로 권한을 확인해야 하며 공개 검사는 독립 fixture를 사용한다.

`private-data-clogp-carriers-v1`은 공개 catalog의 19개 파일 ID·역할·분류와 정확한 metadata 해시를 확인하고 열 필수 정규화 표의 11,987행을 함께 읽는다. 문자열·원시 바이트·F32 비트와 십진 입력, 1 기반 위치를 보존한다. 정수 필드는 정수값인 소수·지수 표기도 정확히 읽고 비정수와 범위 밖의 값을 거절한다. 명세가 규정하지 않은 원시 바이트와 표시 문자열의 대응이나 원문 크기 관계를 추측하지 않는다. 선택 원문은 manifest가 허용하고 실제로 없을 때만 생략한다. fragment 매칭, 단위, 예측 엔진과 모델 바인딩은 미구현이며 개인 DATA 대신 독립 fixture로 검증했다.

`private-data-chemprop-empirical-v1`은 공개 catalog의 17개 파일 ID·역할·분류와 manifest·registry·형식의 바이트 해시를 검사한다. 세 텍스트 표와 경험값·그래프의 다섯 필수 정규화 스트림 44,202행을 원자적으로 검증하고 원문 십진수 표기, F32 little-endian 원시 비트, 플래그·출처 정보와 1 기반 그래프 참조를 보존한다. 정수 필드는 [JSON Schema](https://json-schema.org/understanding-json-schema/reference/numeric)에 따라 소수·지수 표기도 정확한 값이 정수이고 해당 타입 범위 안이면 허용하며, 정수 `-0`은 0으로 읽는다. 비정수 값이나 범위를 벗어난 값과 가수 또는 지수 숫자가 4096자를 초과한 표기는 근사하지 않고 거절한다. F32의 음수 0과 불투명 JSON 원문은 그대로 보존한다. F32 값은 F64를 경유하지 않고 직접 반올림해 비트와 비교하며 비유한 값은 거절한다. 선택 원문 파일은 manifest가 허용하고 실제로 없는 경우에만 생략한다. 단위·경험값 선택·그래프 매칭과 예측 엔진은 구현되지 않았고 개인 DATA 대신 독립 fixture를 사용한다.

HELM, 템플릿 프레이밍, phase 4 명명 및 CMR 입력의 정수 필드는 정수인 소수·지수 표기를 정확히 처리한다. CMR 값의 원문 십진수와 부호 있는 0은 인덱스 처리와 별도로 보존한다. 정수값이 아니거나 타입 범위를 벗어나거나 숫자 성분이 4096자를 초과하면 근사하지 않고 거절한다.

`private-independent-helm-core-fecc972`은 공개 catalog에 고정된 HELM JSON의 705개 레코드와 닫힌 스키마를 검사한다. 필수 `createDate: null`, `capGroupSmiles`와 `capGroupSMILES`의 독립적인 선택 필드, `Terminal` 등의 원문 문자열을 보존한다. 이 자료의 그래프·입체화학·attachment 및 말단 결합 규칙은 검증하지 않았고 생체고분자 확장과 모델 바인딩은 미구현이다.

`private-data-biopolymer-template-candidates-v1`은 공개 catalog의 다섯 정규화 스트림 20,467행과 닫힌 필드 스키마를 검사하고 객체·속성 프레이밍을 타입으로 보존한다. 필수인 nullable 부모와 생략 가능한 필드를 구분하고 제공된 payload hex를 검사한다. 출처 위치와 payload 해시 표기는 불투명 정보로 보존하며 행 사이의 트리·속성 참조나 원문과의 대응을 독립 검증했다고 주장하지 않는다. 선택 원문은 manifest가 허용하고 실제 파일이 없는 경우에만 생략하며, 제공된 원문은 파일 해시를 검사해 불투명 바이트로 보존한다. 활성 템플릿 연결, 생체고분자 확장과 모델 바인딩은 미구현이다.

`private-data-naming-declarative-v1`은 공개 catalog에 고정된 세 정규화 스트림의 127·159·159행과 닫힌 스키마를 검사하고 텍스트·생산자 순번·속성·생산자의 파싱 보고를 타입으로 보존한다. 순번의 시작점과 중복을 추측하지 않으며 보고 행과 원본 행 사이의 대응은 별도로 검증하지 않는다. 원문 두 파일은 manifest가 선택으로 정하고 실제 파일이 없을 때만 생략할 수 있고, 제공된 원문은 해시를 검사한 불투명 바이트로 보존한다. 보고된 구조나 파싱 성공을 Chemifree가 독립 검증했다고 주장하지 않으며 명명 엔진과 모델 바인딩도 미구현이다.

`private-data-biobyte-cmr-static-data-v1`은 catalog에 고정된 여덟 필수 파일을 네 원시 배열과 정규화 스트림으로 짝지어 검증한다. 행 수는 100·100·62·62개이며 각 0 기반 저장 위치가 유일하고 정규화 원시 바이트가 원시 배열의 해당 위치와 같아야 한다. 숫자 행의 JSON 숫자 표기는 기계 부동소수로 변환하지 않는 별도 타입으로 보존하며 음수 0과 긴 가수·지수도 유지한다. 키는 원시 5바이트와 Latin-1로 표현 가능한 텍스트를 각각 보존하고 패딩·트리밍 관계를 추측하지 않는다. 공개된 자료는 이 배열의 수치 비트 형식·단위와 분자 feature의 대응을 완전히 정의하지 않으므로 원시 바이트와 숫자 값의 수치적 일치, CMR 계산 또는 모델 바인딩을 주장하지 않는다.

`private-data-nmr-carriers-v1`은 공개 catalog의 57개 파일 ID·단계·역할·분류와 고정된 registry·형식·manifest 해시를 확인한다. 19개 정규화 스트림의 행 수 340,992개와 각 파일에 지정된 다섯 가지 닫힌 스키마, 1 기반 줄 번호·토큰·순서를 검증해 타입 값으로 보존한다. 파일별 줄 번호는 원문 위치이며 고유 ID로 추론하지 않는다. 원문과 디코드 중간 바이트는 불투명 자료로만 보존하고 실행하지 않는다. manifest가 선택으로 지정한 원문·중간 파일은 없는 경우 생략할 수 있으며, 존재하지만 손상됐거나 접근 오류가 난 파일과 필수 정규화 파일의 누락은 거절한다. 정규화 자료는 모두 검증된 후 한 세대로만 공개한다. NMR 분석·예측과 모델 바인딩은 미구현이며 실제 개인 DATA 대신 독립 fixture로 검사했다.

`private-data-molnet-metadata-v1`, `private-data-molnet-supporting-numeric-literals-v1`, `private-data-chemprop-typed-parameter-tables-v1`은 공개 phase 4 catalog와 registry 및 manifest의 정확한 바이트 해시를 확인하고 파일 ID·역할·분류와 모든 필수 파일을 함께 검증한다. 메타데이터 파일은 실행하지 않는 바이트로 보존한다. 수치 literal은 유한 binary64 원시 비트와 JSON 값·의미 역할을 별도로 보존한다. 세 ChemProp 표는 243·304·87행과 Latin-1 키 바이트, 여섯 바이트 Real48 원문 및 임의 정밀도의 정확한 유리수 분자·분모를 보존하며 분모 0과 4096자보다 긴 정수를 거절한다. Real48 원문과 유리수의 일치를 계산하거나 Real48 연산을 수행하지 않으며, 이 데이터는 실행 가능한 모델이 아니다.

`personal-local-prediction-numeric-profile-v1`은 catalog에 고정된 manifest·원본 registry·형식 파일과 11개 필수 JSON 파일을 함께 검사한다. 정수 키, 유한 binary64 원시 비트와 값, 계수·단위, 순서 있는 회귀 행과 텐서를 보존한다. pKa 절편 행과 LogS의 별도 영 절편은 구분한다. 다섯 pKa 계열의 총 29행과 각 계열의 크기, LogS의 11행, XLogP의 100행, 거리 1–5 분모와 텐서의 존재를 확인한다. 회귀 feature 순서·선언된 행 수와 텐서 크기, 사용되는 거리 분모, 출처 범위와 선택적 literal 행 인덱스의 일관성을 확인한다. 출처 literal 목록은 같은 행에 서로 다른 범위가 여러 개 있으면 순서대로 보존하며 하나의 범위로 합치지 않는다. 원본 출처 파일은 열지 않으므로 출처 바이트 자체를 다시 해시하지 않는다. feature 메타데이터에 열거된 `table_ids`는 읽은 테이블 중 존재하는지 확인한다. 전체 `row_count`가 집계하는 범위와 XLogP 축의 행 대응은 공개 계약만으로 확인할 수 없어 값을 보존하되 전수 대응을 검증했다고 주장하지 않는다. feature 추출·예측 엔진 및 모델 바인딩은 미구현이다.

`personal-local-atomic-isotope-mass-profile-v1`은 원소, 동위원소, 정적 선택, 별칭, 상수, feature schema, 출처의 일곱 파일을 함께 검사한다. 양수 원자량·동위원소 질량과 개별 0..100% 풍부도를 지원하고 합계를 정규화하지 않는다. 원문 십진수와 별도 binary64 비트, 단위, 토큰 유무와 순서를 보존하며 원소별 동위원소 번호, 선택 결과, 기호 및 별칭의 참조 일관성을 검사한다. 동위원소의 선택적 nickname과 별도 alias 행은 각각 보존하며, 공개 스키마는 두 이름의 일치나 nickname마다 alias가 있어야 한다고 요구하지 않는다. 이름 해석 정책과 명명 엔진은 미구현이다. 질량 번호의 반정수 경계는 원문 십진수로 정확히 판단하며, 독립 binary64 참고값은 별도 증거로만 다룬다. 이 경계 정책은 binary64 입력으로 먼저 반올림한 뒤 판단할 때와 경계에서 다를 수 있으며 원본 실행의 반올림 결과를 캡처했다는 뜻이 아니다. 빈 기호, 잘못된 키·순서 선언, 비영 보조값과 빈 단위를 거절한다. 십진수 가수 또는 지수의 숫자 부분이 4096자보다 길면 근사하지 않고 거절한다. 출처 행은 `element`와 `isotope` 종류만 지원하며, 줄과 계수 범위 모두 길이가 양수이고 원본 크기 안이어야 한다. manifest에서 선택적 원문 텍스트가 없으면 정규화 파일만 사용한다. 원문이 있으면 파일과 각 줄의 해시를 확인하며, 접근 오류나 손상은 무시하지 않고 거절한다. 줄의 의미와 정규화 행이 일치하는지는 별도 파서 없이 주장하지 않는다. 계수 출처는 길이·위치·해시 표기만 검사하고 그 출처 파일은 열지 않으므로 계수 바이트의 실제 해시를 검증했다고 주장하지 않는다. 실제 개인 DATA는 읽지 않았고 질량 계산·feature-schema 모델 바인딩은 미구현이다.

`private-naming-query-patterns-v1`과 `private-naming-retained-patterns-v1`은 각자의 파일과 고정 catalog를 검증한다. 출처 순번은 파일 안에서 유일해야 하며 ASCII 원문 바이트, null과 빈 문자열, 배열 순서·flag 및 명시된 predicate ID를 각각 보존한다. annotation의 속성 ID와 별도 predicate 목록 사이에 명세에 없는 동일성 규칙을 만들지 않는다. 필요한 독립 predicate와 그래프 matcher는 구현되지 않았으므로 이 두 프로필을 포함하는 모델 바인딩은 `model_unavailable`이다.

`private-data-molnet-xlogp-100-f64-data-v1`은 manifest의 명시적인 두 file ID로 원시 800바이트와 정규화 100행을 짝지어 검증한다. 각 저장 인덱스와 원시 binary64 비트, 별도로 파싱한 JSON 숫자를 보존한다. phase 4의 빈 물리적 줄·CRLF·대문자 hex 허용 규칙을 따른다. 파일 배열 순서로 화학 feature를 추론하지 않으며 독립 분류기와 XLogP 모델은 미구현이다. 세 프로필 모두 개인 DATA 대신 독립 fixture로 검증했다.

고정 윤곽 158행과 대형 고리 seed 2행은 호스트가 신뢰하는 등록에서 `layout_reader`를 각각 `fixed_contours` 또는 `large_twenty_seed`로 명시해야 한다. 두 역할 모두 원본 registry와 공개 schema의 고정 바이트 해시를 검증하며 대형 고리는 공개된 데이터셋 ID·버전과 두 operand ID, 20개 원소를 확인한다. binary64 원시 비트와 부호 있는 0 및 역할은 타입 값으로 보존한다. registry 해시 검증만으로 불투명한 aggregate registry에서 manifest의 소속을 증명하지 않으며, 그 연결과 권리는 별도 신뢰된 호스트 등록과 검토에 의존한다. 빈 행·중복 키·손상된 레코드는 거절하고 JSON Lines 마지막 개행은 선택이다. layout schema의 정수는 값이 정확히 정수인 소수·지수 표기도 허용한다. 레이아웃 엔진과 이 자료를 사용하는 모델 바인딩은 미구현이다. 다른 프로필의 정수 입력 경계와 의존 모델 바인딩은 추가 검증이 필요하다. 공통 scalar 디코더를 제공하는 것만으로 과학 계산이 구현된 것은 아니다.

데이터 검증과 모델 실행은 별개다. 타입 값은 후속 application 계층에서 `DataRegistry::datasets`와 각 세대의 `records`로 읽을 수 있지만, 이를 사용할 화학 엔진은 없다. `prediction.evaluate`는 `model_unavailable`을 반환하며 결과를 만들지 않는다. 현재 코드에는 원본 실행 파일·벤더 엔진·개인 데이터·화학 모델 계수가 들어 있지 않다.

## 신뢰 경계와 선택

API와 CLI에 `--model-data-root <DATA 디렉터리>` 및 `--model-trusted-registry <신뢰하는 기술서>`를 함께 주거나 `CHEMIFREE_MODEL_DATA_ROOT`, `CHEMIFREE_MODEL_TRUSTED_REGISTRY`를 설정한다. 둘 중 하나만 주면 상태는 `registry_configuration_incomplete`다. 기술서는 DATA 디렉터리와 별도로 관리하며 사용자가 신뢰하는 로컬 등록 정보여야 한다. 데이터셋이 자기 manifest에 적은 권리 설명이나 임의의 URL은 신뢰된 검토가 아니다. 공개 저장소, CI, 테스트 로그에 개인 데이터나 개인 검토 파일을 넣지 않는다.

기술서는 독립적으로 정의한 로컬 선택 wrapper다. 공개 명세의 active-contract 원문이나 aggregate registry와 같은 파일 형식이라고 주장하지 않는다. `registry_version: 1`, `root_binding_id`, `registrations` 배열이 필요하다. 각 항목은 `manifest_path`, `review_path`(DATA 디렉터리 아래 상대 경로)와 `trust` 객체를 가진다. `trust`에는 `registration_id`, 정수 `generation`, `dataset_id`, `dataset_version`, `family_id`, `profile_id`, `engine_contract_id`, 정규화 `manifest_sha256`, 원문 `manifest_file_sha256`, 원문 `review_sha256`, `source_registry_binding_id`, `private_root_binding_id`가 필요하다. 모든 해시는 소문자 SHA-256 64자리이고 마지막 root ID는 wrapper의 root ID와 같아야 한다. 등록 기술서와 검토 기록의 신뢰성은 기술서를 선택한 호스트가 제공한다. 검증기는 데이터셋이 제공한 자기 주장으로 신뢰를 만들지 않는다.

서로 다른 데이터셋에서 같은 상대 파일명을 쓸 수 있으므로 각 등록에 선택적으로 `data_root`를 지정한다. manifest의 payload 경로만 이 하위 디렉터리 기준으로 읽으며, `manifest_path`와 `review_path`는 선택한 DATA 디렉터리 기준이다. 하위 디렉터리도 파일 기술자로 열어 해당 등록의 모든 파일에 고정하고, 그 밖으로 나가는 심볼릭 링크를 거절한다. 이 보장을 제공하지 못하는 provider는 하위 디렉터리 등록을 거절한다. phase 6의 열한 프로필과 phase 4의 열세 프로필에는 `source_registry_path`와 `format_profile_path`도 함께 지정한다. 두 경로는 선택한 DATA 디렉터리 기준이며, 이 파일들의 바이트를 공개 형식 목록에 고정된 해시와 대조한다. 둘 중 하나만 있거나 안전하지 않은 상대 경로는 거절한다. 처음 세 phase 7 명명 프로필은 이 두 필드를 사용하지 않는다. 레이아웃 역할은 두 경로와 `layout_reader`를 모두 지정해야 한다.

별도의 일반 JSON/JSONL 로더는 독립 fixture와 범용 타입 검증에 사용하며 과학 데이터 프로필을 활성화하지 않는다. 이 경로도 별도로 신뢰한 공개 manifest 원문 해시가 필요하다. manifest 자체의 `public` 표시나 자유 형식 권리 설명으로 허용 범위를 판단하지 않고, 개인 데이터를 이 일반 경로로 승인하지 않는다.

예를 들어 빈 값이 아닌 신뢰된 ID와 각각 독립적으로 확인한 해시를 채운 뒤 다음 모양을 사용한다. 예시는 payload가 아니며 실제 승인 기록이 아니다.

```json
{
  "registry_version": 1,
  "root_binding_id": "selected-local-root",
  "registrations": [{
    "manifest_path": "metadata/manifest.json",
    "review_path": "metadata/review.json",
    "trust": {
      "registration_id": "local-registration",
      "generation": 1,
      "dataset_id": "legacy-naming-element-priority-endpoint-operands",
      "dataset_version": "sha256-set-e2083d6c9d2f0e780e171b83caf9d839688c53e25f470248544206bea79bb7bc",
      "family_id": "naming-element-rank-endpoint-operands",
      "profile_id": "private-naming-element-endpoint-v1",
      "engine_contract_id": "independent-contract-id",
      "manifest_sha256": "<canonical-manifest-sha256>",
      "manifest_file_sha256": "<exact-manifest-file-sha256>",
      "review_sha256": "<exact-review-file-sha256>",
      "source_registry_binding_id": "trusted-source-registry-id",
      "private_root_binding_id": "selected-local-root"
    }
  }]
}
```

manifest는 공개 ManifestV1 필드와 추가 필드 거절 규칙을 사용한다. 정규화 해시는 중복 키·BOM·부동소수 표기를 거절하고 객체 키 정렬, UTF-8, 공백 없는 정수 JSON으로 계산하며, 공백을 포함하는 원문 바이트 해시와 별개다. 리뷰는 고정된 공개 personal-local context의 원문 해시, 승인 상태, 출처·root ID, manifest와 정확히 같은 파일 ID·바이트 수·해시·내용 분류, `local_load`와 `decode` 허용 범위를 확인한다. `fixture_only` 리뷰는 실제 등록에서 거절한다. 권리 확인은 해당 개인 로컬 사용의 사용자 확인을 받아들이는 계약이며 법률·문서 검증을 주장하지 않는다. 더 넓은 배포·원격 처리·원본 모듈 실행 권한은 추론하지 않는다.

`unknown_field_policy`는 데이터 payload의 알 수 없는 필드를 해당 프로필이 비핵심 metadata로 명시한 경우에만 보존할 수 있다는 정책이다. 공개 `spec/phase4-external-data-manifest.schema.json`은 정책과 무관하게 manifest의 추가 필드를 거절하며, `spec/phase4-external-data-loader-contracts.ko.md`는 그 규칙과 payload 정책의 차이를 명시한다. 현재 등록한 프로필은 선언되지 않은 JSON 속성을 거절한다. nickname XML 속성처럼 스키마가 명시적으로 허용한 자유 형식 맵은 그대로 보존하며, 새 스키마의 알 수 없는 JSON 필드를 opaque하게 보존하는 기능은 아직 없다.

각 파일은 같은 바이트를 한 번 읽어 길이·해시·형식을 검사한 뒤에만 공개한다. 한 파일이 실패한 데이터셋은 다른 파일의 성공 여부에 관계없이 부분 공개하지 않는다. 첫 읽기에서 선택한 디렉터리를 열어 이후 읽기에도 같은 디렉터리를 사용하므로 그 경로가 교체되어도 다른 원본과 섞이지 않는다. Linux에서는 열린 디렉터리 기준 `openat2`의 `RESOLVE_BENEATH`와 `NO_MAGICLINKS`로 경로를 원자적으로 제한한다. Linux에서 해당 시스템 호출을 지원하지 않거나 다른 운영체제에서는 외부 DATA 읽기를 거절한다. 편집기 자체의 플랫폼 지원과는 별개다.

모델 바인딩 검증은 신뢰된 기술서 원문 해시와 등록 ID·세대·정규화 manifest 해시·엔진 계약을 대조한다. feature-schema가 필요한 모델 바인딩은 의미를 검증한 독립 엔진 계약이 아직 없어 `model_unavailable`로 남는다. 결과는 데이터 바인딩 검증일 뿐 실행 가능한 모델을 뜻하지 않는다. 데이터 접근 후속 사용에는 해당 시점의 권리 범위·철회·만료 재검사가 필요하며, 아직 엔진 호출이나 cache/export는 연결하지 않았다.
