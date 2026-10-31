# 수치 저장 프로필과 구현 범위

`chemifree-numerics`는 후속 독립 엔진이 사용할 수치 경계를 제공한다. 저장 표현과 정확한 유리수 사이의 변환 및 유한 값의 기본 사칙연산을 다룬다. 호출자가 연산마다 저장 형식을 지정해 중간 반올림을 제어한다. 선택된 각도 표현식·불규칙도 반올림, 독립 물리 단위 변환과 표시 정책을 명시한다. 이 라이브러리를 추가해도 화학 계산이나 모델 실행이 가능해지는 것은 아니다.

## 값과 원시 비트

`Exact`는 부호와 기약된 비음수 분자·양수 분모를 따로 가진다. 0에도 부호를 보존한다. 입력 분자와 분모는 각각 최대 65,536비트이며 분모 0과 이 제한을 넘는 입력은 오류다. 타입은 단위나 표시 문자열을 추론하지 않는다.

`Binary32`와 `Binary64`는 원시 비트를 그대로 보유하며 0·서브노멀·정상·무한대·quiet NaN·signaling NaN을 구분한다. 비유한 비트도 분류와 보관은 가능하지만 `Exact`로 변환하려고 하면 `nonfinite_value`다. 유한 값을 정확한 유리수에서 저장할 때 각각 24비트·53비트 유효숫자로 가장 가까운 값을 고르고 정확한 중간값은 짝수 쪽으로 반올림한다. 서브노멀과 부호 있는 0을 지원한다. 이 유한 저장 정책에서 오버플로는 `numeric_overflow`이며 무한대를 조용히 만들지 않는다. 이는 IEEE 예외 플래그·하드웨어 트랩이나 모든 비유한 연산을 구현한다는 뜻이 아니다. 원시 비트의 구조는 [IEEE 저장 형식 설명](https://docs.oracle.com/cd/E19957-01/806-3568/ncg_math.html)과 별도로 검사한다.

`Real48`은 여섯 바이트 원문을 보존한다. 첫 바이트는 bias 129의 지수이고, 나머지 다섯 바이트의 낮은 39비트는 가수이며 마지막 바이트의 높은 비트는 부호다. 지수 바이트 0은 값이 양의 0이며, 비정규 0의 원래 바이트는 따로 읽을 수 있다. 새 저장 값의 0은 여섯 바이트가 모두 0이다. 정확한 유리수에서 40비트 유효숫자로 반올림할 때 중간값은 크기가 커지는 쪽을 고른다. 반올림 후 지수가 -128보다 작으면 양의 0으로 내리고, 최대 지수 126을 넘으면 오류다. 변환 오류는 기존 대상 바이트를 바꾸지 않는다. 바이트 배치는 [Real48 저장 형식](https://docwiki.embarcadero.com/RADStudio/Florence/en/Internal_Data_Formats_%28Delphi%29)과 대조하며, 반올림·경계 정책은 승인된 공개 기능 명세의 별도 계약이다.

`Extended80`은 가수의 명시적 정수 비트를 포함한 10바이트 little-endian 표현을 보존한다. 부호 있는 0, 서브노멀, 정상값, 무한대, quiet·signaling NaN, pseudo-denormal 및 지원하지 않는 부호화를 구분한다. pseudo-denormal은 원문 바이트를 보존하면서 대응하는 정상값으로 정확히 읽으며 새 저장에는 정규 표현을 쓴다. 유한 값을 저장할 때 64비트 유효숫자로 최근접·동률 짝수 반올림하고 서브노멀을 지원한다. 유한 binary64와의 변환도 명시적으로 제공한다. 무한대·NaN 연산은 `nonfinite_value`, 지원하지 않는 부호화는 `unsupported_encoding`이다. 이 유한 정책은 x87 예외 플래그, 하드웨어 트랩이나 모든 비유한 연산을 구현하지 않는다. 형식은 [Intel 부동소수점 참고 자료](https://www.intel.com/content/www/us/en/developer/articles/technical/floating-point-reference-sheet-for-intel-architecture.html)와 대조한다.

서로 다른 저장 형식 사이의 변환은 `Exact`를 거쳐 한 번의 명시적 저장 반올림으로 수행한다. 네 저장 형식의 `checked_add`, `checked_sub`, `checked_mul`, `checked_div`는 입력을 정확한 유리수로 계산한 다음 해당 형식으로 한 번 저장한다. 나눗셈의 0 제수, 비유한 입력, 지원하지 않는 부호화, 오버플로와 자원 한도는 오류다. 최근접 반올림의 정확한 상쇄는 양의 0이고 곱셈·나눗셈의 0은 부호 규칙을 따른다(Real48은 새 0을 양수로 저장). 호출자는 각 중간 결과의 형식과 연산 순서를 명시해야 한다.

## 명시적 계산 순서

`next_heading`은 저장된 binary64 이전 각도와 회전량을 받아 확장 형식으로 넓힌다. 회전량에 저장된 binary64 π(`0x400921fb54442d18`)를 곱하고 저장된 binary64 180으로 나눈 다음 이전 각도에 더하거나 뺀다. 세 연산은 각각 확장 형식의 64비트 유효숫자로 반올림하고 완료된 각도만 binary64로 저장한다. 다음 단계는 그 반환 비트를 이전 각도로 다시 받아야 한다. 초기 각도는 그대로 첫 결과이며 명세의 첫 회전량은 이 생성 단계에서 읽지 않는다. 이 함수는 후보 선택이나 고리 배치 엔진이 아니다.

`round_deviation`은 유한한 비음수 binary64 각도를 정확히 읽어 정수 반쪽을 위로 반올림한다. 반올림한 정수가 10보다 클 때만 불규칙 플래그가 참이다. 음의 0은 0으로 받고 음의 비영 값과 비유한 값은 거절하며, 결과 정수는 큰 값에서도 잘리지 않는다. 이 규칙은 입력 각도가 이미 주어진 뒤의 경계이며 라디안 변환이나 도형 탐색을 구현하지 않는다.

## 명시적인 단위와 저장

`LengthValue`는 유한한 binary64 원시 비트와 `ComputerPoint`, `Millimetre`, `Centimetre`, `Inch` 단위를 함께 갖는다. 컴퓨터용 point는 1/72 inch이고 국제 inch는 정확히 25.4 mm다. 이는 전통 인쇄용 point와 구별한다. [NIST의 컴퓨터 point 표](https://www.nist.gov/pml/special-publication-811/nist-guide-si-appendix-b-conversion-factors/nist-guide-si-appendix-b9)와 [국제 inch 정의](https://www.nist.gov/pml/owm/si-units-length)를 참조한다. 서로 다른 단위의 변환은 정확한 유리수 비율을 적용한 뒤 binary64로 한 번 반올림한다. 같은 단위는 원래 비트와 부호 있는 0을 그대로 유지한다. 오버플로와 비유한 입력은 오류이며 원래 값은 바뀌지 않는다. 이는 독립 물리 변환 정책이고 문서 스타일 UI의 표시 반올림을 바꾸지 않는다.

`ConcentrationValue`는 Real48 원시 바이트와 mol/L 또는 g/L을 함께 갖는다. 두 단위 사이의 독립 물리 관계에는 호출자가 양수 `MolarMassGramsPerMole`을 명시해야 한다. mol/L에서 g/L로 갈 때 곱하고 반대 방향은 나눈 다음 매번 Real48로 저장한다. 보조값이 없거나 0 이하이면 각각 `missing_auxiliary` 또는 `nonpositive_auxiliary`다. 같은 단위는 보조값 없이 원래 바이트를 보존하지만 새 변환 저장은 Real48의 0 정규화와 반올림을 따른다. 부호 있는 입력의 과학적 타당성은 이 수치 계층이 판단하지 않는다. 이 정책은 ChemProp의 특정 레거시 상수·압력·Henry·로그 변환을 대체하거나 구현했다고 주장하지 않는다.

## 표시 문자열과 F32 저장

`format_default_six`는 유한 binary64의 정확한 값을 6자리 유효숫자로 표시하고 십진 중간값은 짝수로 반올림한다. 반올림한 지수가 -4 미만이거나 6 이상이면 소문자 `e` 과학 표기를 쓰고 그 밖에는 고정 표기를 쓴다. 불필요한 뒤쪽 0은 제거하며 지수 부호와 최소 두 자리, 음의 0을 유지한다. 이는 소수점이 `.`인 C-locale 방식의 명시적 정책으로, [default-float 형식의 일반 규칙](https://learn.microsoft.com/en-us/cpp/c-runtime-library/format-specification-syntax-printf-and-wprintf-functions?view=msvc-170)과 독립적으로 대조했다.

`format_fixed`는 유한 binary64를 0–8자리 고정 소수로 표시하고 뒤쪽 0을 유지한다. 십진 중간값은 짝수로 반올림하고 표시 자릿수 밖은 `precision_out_of_range`다. 질량 결과는 저장된 환경 설정의 0–8자리 표시값을 사용하며 웹 선택지는 0–5자리다. 이 설정은 산술 정밀도가 아니라 표시 문자열에만 적용되고 기존 문서나 이전 결과를 바꾸지 않는다.

`round_three_half_away_to_binary32`는 `Exact` 값을 십진 소수 셋째 자리에서 크기가 커지는 쪽으로 중간값을 반올림한 뒤, 그 십진 유리수를 binary32로 한 번 저장한다. Real48·binary64·확장 형식에서 정확히 읽은 값을 직접 전달할 수 있으며 보통 binary64 중간값을 강제하지 않는다. 저장의 binary32 반올림은 최근접·동률 짝수다. 오류는 비유한 값의 `Exact` 변환 실패, F32 오버플로 또는 자원 한도로 구분한다. 이는 명시된 유한 십진 경계이며 특정 원본 환경의 모든 부동소수점 코너나 로캘 호환성을 주장하지 않는다.

단위, 값, 비트와 문자열은 별개의 정보다. 예를 들어 72 computer point를 mm로 변환하면 결과 단위는 mm이고 binary64 원시 비트는 `4039666666666666`이다. 이 저장 값을 두 자리로 표시한 `25.40`은 별도 문자열이며 원시 비트를 바꾸지 않는다. 후속 결과는 선택한 단위·프로필·정밀도와 데이터 출처를 함께 기록해야 한다.

## 검증

`crates/numerics/tests/storage.rs`는 정확한 반정수·최소 서브노멀·최대 유한값·부호 있는 0·비유한 비트·Real48의 반올림 후 언더플로와 목적지 보존을 검사한다. `tests/extended.rs`는 확장 형식의 원시 부호화와 유한 연산, 명시적 중간 저장과 재결합의 차이를 확인한다. `tests/data/generate_storage_vectors.py`는 Python `Fraction`과 별도의 이분 탐색으로 기존 저장 형식 850개와 확장 형식 328개의 합성 기준을 생성하며 `--check`로 재현성을 확인한다. `tests/ordered.rs`는 확장 정밀도 C 실행과 유리수 이분 탐색을 별도로 대조해 준비한 양 방향 18개 순서 기준과 반쪽 경계를 확인한다. `tests/units.rs`는 독립 유리수 기준 384개와 Real48 중간 저장의 손실 가능성·보조값 오류를 확인한다. `tests/display.rs`는 C와 Python 출력이 일치하는 표시 2,240개 및 독립 유리수 탐색으로 만든 F32 저장 224개를 대조한다. C 실행은 테스트마다 자동으로 실행하지 않는다. 이 기준은 화학 데이터가 아니다. 개인 DATA·모델 계수·원본 실행은 사용하지 않는다.

기준은 저장소 루트에서 다음 명령으로 다시 대조할 수 있다. C의 각도 기준은 유효숫자 64비트의 `long double`과 최근접 반올림을 확인한 환경에서만 실행한다. 표시 C 기준은 C 로캘과 `double`의 2진 53비트 가수·8바이트 크기를 확인한다. 두 C 파일은 `crates/numerics/tests/data/`에 있으며 제품 코드나 모델 데이터가 아니다.

```sh
python3 -B crates/numerics/tests/data/generate_storage_vectors.py --check
python3 -B crates/numerics/tests/data/generate_heading_vectors.py --check
python3 -B crates/numerics/tests/data/generate_unit_vectors.py --check
python3 -B crates/numerics/tests/data/generate_display_vectors.py --check
```
