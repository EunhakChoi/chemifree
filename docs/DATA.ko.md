# 외부 DATA 등록과 현재 지원 범위

외부 데이터는 저장소와 문서 저장 디렉터리에 포함하지 않는다. 사용자가 별도로 선택한 로컬 DATA 디렉터리와 신뢰하는 등록 기술서를 이용하며, 서버는 시작할 때 한 번 읽은 불변 세대를 사용한다. 데이터가 없거나 거절되어도 문서 편집과 저장은 계속된다. `GET /api/v1/data`, `chemifree data status --json`, 웹의 External data 영역은 데이터 상태와 미구현 엔진을 따로 표시한다. 등록을 바꾸면 서비스를 다시 시작해야 한다.

현재 등록 가능한 개인용 정규화 프로필은 `private-naming-ring-element-ranks-v1`, `private-naming-valence-reference-v1`, `private-naming-element-endpoint-v1`, `private-caw-element-symbols-v1`, `private-caw-type-elements-v1`, `private-naming-element-ranks-v1`이다. 처음 세 프로필은 지정 경로와 정확한 레코드 수, 정수 표기·범위·인덱스 집합을 검사한다. 나중 세 프로필은 공개 phase 6 목록에 고정된 데이터셋 ID·버전과 manifest·원본 registry·형식 프로필의 **실제 파일 바이트 해시**를 대조하고, 지정된 파일 전체를 검증한 뒤 타입 값을 보존한다. 두 명명 순위 파일의 맥락 A와 B는 구분하며 A의 2–109 인덱스가 완전하고 중복되지 않아야 한다. 정수 `-0`은 정수 0으로 읽고 소수·지수 표기를 정수로 받아들이지는 않는다.

`personal-local-nmr-coupling-parameters-v1`도 같은 고정 catalog 식별자와 권리 검토를 사용한다. lookup, amplitude, width, feature schema, source bounds의 다섯 파일을 모두 검증하며, feature schema의 원문 바이트 해시가 manifest의 파일 해시 및 `feature_schema_sha256`과 같아야 한다. 각 binary64 원시 비트와 단위, 배열 순서, 별도 항목 종류를 보존하고 수·키·참조·원본 범위가 모순되면 전체 등록을 거절한다. source bounds는 불활성 출처 정보이며 원본 파일을 열거나 실행하지 않는다. 실제 개인 DATA를 열어 검증하지 않았으며, 공개 명세와 독립 합성 fixture로 reader를 검사했다. 이 데이터가 유효해도 NMR 엔진, feature 의미의 실행 검증, 비어 있지 않은 feature-schema를 요구하는 모델 바인딩은 아직 구현되지 않았다.

`private-naming-enum-labels-v1`은 종류와 하위 종류의 두 파일을 함께 검증하고 서로 다른 맥락, 원래 순서, 반복 표기, ASCII 원문 바이트와 코드·출처 ID를 보존한다. catalog의 고정된 데이터셋 ID·버전과 실제 metadata 파일의 원문 해시가 모두 맞아야 하며 한 파일이 손상되면 해당 데이터셋 전체를 거절한다. 표기는 불활성 데이터이며 명명 엔진이나 실행 경로를 제공하지 않는다. 이 reader도 실제 개인 DATA 대신 독립 합성 fixture로 검증했다.

`personal-local-atomic-isotope-mass-profile-v1`은 원소·동위원소·정적 선택·별칭·상수·feature schema·출처의 일곱 파일을 함께 검사한다. 원문 십진수, 독립 binary64 참고 비트, 단위, 순서와 풍부도 토큰의 유무를 보존한다. 이 reader의 지원 범위는 양수 자연 원자량·동위원소 질량과 개별 0..100 퍼센트 풍부도다. 개별 풍부도의 합은 제한하거나 정규화하지 않는다. 원문 십진수의 정확한 순서를 이용하므로 binary64에서 0이나 100으로 반올림되는 범위 밖 값도 거절한다. 십진수 지수 부분이 4096자보다 길면 근사하지 않고 거절한다. 질량 번호는 독립 binary64 참고값의 반올림 경계와 대조하며 원본 실행값을 캡처했다는 뜻은 아니다. 공개 스키마가 출처 종류 문자열을 열거하지 않아 `element`와 `isotope`만 지원하며, 각 출처는 존재하는 행을 유일하게 가리켜야 한다. 선택적 원문이 manifest에 있으면 권리·파일 해시와 줄 범위·해시도 확인한다. 줄 바이트와 정규화 행의 의미상 일치는 원문 파서 없이 주장하지 않으며, 원문이 없을 때 그 출처 해시는 검증되지 않은 metadata다. 실제 개인 DATA는 읽지 않았다. 질량 계산과 feature-schema 모델 바인딩은 미구현이다.

윤곽의 고정 158행·대형 고리 2행에 대한 독립 디코더는 binary64 원시 비트와 부호 있는 0을 보존하지만 아직 등록 프로필과 연결하지 않았다. 나머지 활성 DATA 프로필과 feature-schema·의존 모델이 있는 바인딩은 지원하지 않고 거절한다. 현재 활성 계약의 29개 데이터셋 전체를 읽는 로더가 완성된 것은 아니다. 공통 scalar 디코더는 U64, binary64 비트·단위, 원문 십진수와 ASCII·Latin-1 바이트 쌍을 보존하지만, 이를 제공하는 것만으로 다른 프로필의 등록이나 과학 계산이 구현된 것은 아니다.

데이터 검증과 모델 실행은 별개다. 타입 값은 후속 application 계층에서 `DataRegistry::datasets`와 각 세대의 `records`로 읽을 수 있지만, 이를 사용할 화학 엔진은 없다. `prediction.evaluate`는 `model_unavailable`을 반환하며 결과를 만들지 않는다. 현재 코드에는 원본 실행 파일·벤더 엔진·개인 데이터·화학 모델 계수가 들어 있지 않다.

## 신뢰 경계와 선택

API와 CLI에 `--model-data-root <DATA 디렉터리>` 및 `--model-trusted-registry <신뢰하는 기술서>`를 함께 주거나 `CHEMIFREE_MODEL_DATA_ROOT`, `CHEMIFREE_MODEL_TRUSTED_REGISTRY`를 설정한다. 둘 중 하나만 주면 상태는 `registry_configuration_incomplete`다. 기술서는 DATA 디렉터리와 별도로 관리하며 사용자가 신뢰하는 로컬 등록 정보여야 한다. 데이터셋이 자기 manifest에 적은 권리 설명이나 임의의 URL은 신뢰된 검토가 아니다. 공개 저장소, CI, 테스트 로그에 개인 데이터나 개인 검토 파일을 넣지 않는다.

기술서는 독립적으로 정의한 로컬 선택 wrapper다. 공개 명세의 active-contract 원문이나 aggregate registry와 같은 파일 형식이라고 주장하지 않는다. `registry_version: 1`, `root_binding_id`, `registrations` 배열이 필요하다. 각 항목은 `manifest_path`, `review_path`(DATA 디렉터리 아래 상대 경로)와 `trust` 객체를 가진다. `trust`에는 `registration_id`, 정수 `generation`, `dataset_id`, `dataset_version`, `family_id`, `profile_id`, `engine_contract_id`, 정규화 `manifest_sha256`, 원문 `manifest_file_sha256`, 원문 `review_sha256`, `source_registry_binding_id`, `private_root_binding_id`가 필요하다. 모든 해시는 소문자 SHA-256 64자리이고 마지막 root ID는 wrapper의 root ID와 같아야 한다. 등록 기술서와 검토 기록의 신뢰성은 기술서를 선택한 호스트가 제공한다. 검증기는 데이터셋이 제공한 자기 주장으로 신뢰를 만들지 않는다.

서로 다른 데이터셋에서 같은 상대 파일명을 쓸 수 있으므로 각 등록에 선택적으로 `data_root`를 지정한다. manifest의 payload 경로만 이 하위 디렉터리 기준으로 읽으며, `manifest_path`와 `review_path`는 선택한 DATA 디렉터리 기준이다. 하위 디렉터리도 파일 기술자로 열어 해당 등록의 모든 파일에 고정하고, 그 밖으로 나가는 심볼릭 링크를 거절한다. 이 보장을 제공하지 못하는 provider는 하위 디렉터리 등록을 거절한다. phase 6의 여섯 프로필에는 `source_registry_path`와 `format_profile_path`도 함께 지정한다. 두 경로는 선택한 DATA 디렉터리 기준이며, 이 파일들의 바이트를 공개 형식 목록에 고정된 해시와 대조한다. 둘 중 하나만 있거나 안전하지 않은 상대 경로는 거절한다. 처음 세 phase 7 프로필은 이 두 필드를 사용하지 않는다.

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
      "dataset_id": "dataset-id-from-trusted-registry",
      "dataset_version": "dataset-version-from-trusted-registry",
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

`unknown_field_policy`는 데이터 payload의 알 수 없는 필드를 해당 프로필이 비핵심 metadata로 명시한 경우에만 보존할 수 있다는 정책이다. 공개 `spec/phase4-external-data-manifest.schema.json`은 정책과 무관하게 manifest의 추가 필드를 거절하며, `spec/phase4-external-data-loader-contracts.ko.md`는 그 규칙과 payload 정책의 차이를 명시한다. 현재 등록한 아홉 프로필의 공개 스키마는 추가 payload 필드를 허용하지 않으므로 `retain_opaque_and_report`를 선택한 manifest도 정상 입력은 처리하되 추가 payload 필드는 거절한다. opaque 필드 보존을 지원하는 프로필은 아직 없다.

각 파일은 같은 바이트를 한 번 읽어 길이·해시·형식을 검사한 뒤에만 공개한다. 한 파일이 실패한 데이터셋은 다른 파일의 성공 여부에 관계없이 부분 공개하지 않는다. 첫 읽기에서 선택한 디렉터리를 열어 이후 읽기에도 같은 디렉터리를 사용하므로 그 경로가 교체되어도 다른 원본과 섞이지 않는다. Linux에서는 열린 디렉터리 기준 `openat2`의 `RESOLVE_BENEATH`와 `NO_MAGICLINKS`로 경로를 원자적으로 제한한다. Linux에서 해당 시스템 호출을 지원하지 않거나 다른 운영체제에서는 외부 DATA 읽기를 거절한다. 편집기 자체의 플랫폼 지원과는 별개다.

모델 바인딩 검증은 신뢰된 기술서 원문 해시와 등록 ID·세대·정규화 manifest 해시·엔진 계약을 대조한다. feature-schema가 필요한 모델 바인딩은 의미를 검증한 독립 엔진 계약이 아직 없어 `model_unavailable`로 남는다. 결과는 데이터 바인딩 검증일 뿐 실행 가능한 모델을 뜻하지 않는다. 데이터 접근 후속 사용에는 해당 시점의 권리 범위·철회·만료 재검사가 필요하며, 아직 엔진 호출이나 cache/export는 연결하지 않았다.
