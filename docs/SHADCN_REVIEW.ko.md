# Chemifree의 shadcn/ui 및 @shadcn/lint 검토

검토일: 2026-10-27. 사용자 요청의 shardcn은 shadcn/ui로 해석했다. 이 문서는 기술 선택과 초기 연결 범위를 설명한다.

## 결론

shadcn/ui는 애플리케이션의 일반적인 인터페이스에 사용한다. 화학 문서의 의미 모델과 SVG/Canvas 렌더러는 독립 모듈로 개발한다. 초기에는 웹 앱 안에 필요한 컴포넌트만 두며 별도 공용 UI 패키지는 만들지 않는다.

@shadcn/lint는 개발 의존성과 ESLint 플러그인 등록을 준비한다. 초기 기본 설정과 CI에는 새로운 디자인 규칙을 켜지 않는다. 따라서 일반 lint 통과는 플러그인 연결과 기존 코드 규칙의 통과를 뜻하며 디자인 일관성 검증을 뜻하지 않는다.

설정은 apps/web/eslint.config.js, 컴포넌트·테마 경로는 apps/web/components.json에 있다. 저장소 루트에서 npm run lint로 실행한다.

## 두 도구의 역할

| 도구 | 역할 | Chemifree에서의 경계 |
| --- | --- | --- |
| shadcn/ui | 프로젝트에 추가해 관리하는 UI 컴포넌트 소스 | 버튼, 상태 표시, 카드, 빈 화면부터 시작하고 필요한 메뉴·필드·대화상자를 추가 |
| @shadcn/lint | Tailwind 디자인 시스템을 위한 ESLint/Oxlint 플러그인 | 컴포넌트의 외형 변경과 테마 토큰 사용을 정적 검사하는 정책 도구 |
| TypeScript / ESLint | 타입, 일반 코드 규칙과 React 사용 점검 | 계속 유지하며 디자인 lint와 함께 사용 |
| 화학 렌더러 | 좌표·라벨·결합·선택·스펙트럼 표시 | 별도의 제품 모듈; shadcn으로 화학 그래프나 계산을 대체하지 않음 |

컴포넌트를 프로젝트의 소스로 소유하는 방식은 편집기용 단축키·밀도·패널 구성에 맞춘 조정을 가능하게 한다. 업데이트 시 변경 내용을 직접 검토해야 한다. [shadcn/ui 공식 문서](https://ui.shadcn.com/docs)

@shadcn/lint는 shadcn CLI의 lint 하위 명령이 아니다. Tailwind v4 프로젝트에서 사용하며 shadcn/ui 자체는 필수가 아니다. 현재 선택한 React 프로젝트에서는 ESLint에 플러그인을 등록한다. [공식 README](https://github.com/shadcn-ui/lint/blob/main/README.md)

## 이번 스캐폴딩의 선택

- React + TypeScript + Vite + Tailwind v4.
- 공식 @shadcn registry와 Base UI 기반 nova 스타일.
- 일반 UI의 색은 semantic token과 컴포넌트 variant를 사용한다.
- 컴포넌트 생성은 공식 CLI를 사용하고 생성 소스를 검토한다.
- 초기 앱에 쓰지 않는 컴포넌트 전체를 설치하지 않는다.
- 과학적 결과, 편집 성공, 저장 성공을 UI 데모로 가장하지 않는다.
- atom 색, 선택 상태, 스펙트럼 색 등은 이후 별도의 의미 토큰으로 정의한다.

확인 시점의 npm 배포 버전은 shadcn 4.22.1, @shadcn/lint 0.2.3이었다. @shadcn/lint의 요구 조건은 Node 20.19 이상, ESLint 9.30 이상, TypeScript ESLint parser 8.40 이상이다. 프로젝트가 실제로 설치한 전체 버전은 package-lock.json을 기준으로 한다. [배포 메타데이터](https://www.npmjs.com/package/@shadcn/lint), [시작 안내](https://github.com/shadcn-ui/lint/blob/main/README.md#get-started)

현재 CLI에서는 Base UI + nova 초기화를 base와 preset 인자로 각각 지정했다. 오래된 예제의 preset 문자열을 그대로 사용하지 않고 실행한 CLI의 도움말과 결과를 확인했다.

브라우저 확인에서 링크의 의미도 점검했다. Base UI Button으로 anchor를 렌더링하면 button 역할이 적용되므로, 내비게이션은 일반 anchor에 buttonVariants 스타일을 적용했다. 현재 공식 문서의 As Link 안내를 따른다. [Button 문서](https://ui.shadcn.com/docs/components/base/button#as-link)

## 디자인 규칙 도입 제안

| 규칙 | 권장 초기 정책 | 주의할 경계 |
| --- | --- | --- |
| no-raw-colors | 일반 UI에서 경고로 시작 | atom·스펙트럼의 의미 색은 theme token을 먼저 정의 |
| no-restyle | UI 사용 지점에서 경고, layout 허용 | components/ui 내부는 자체 스타일 정의를 위해 제외 |
| no-arbitrary-values | 디자인 시스템 확정 후 좁은 범위부터 | 컴포넌트의 구조적 값과 편집기의 계산된 기하는 구분 |
| no-inline-styles | 일반 UI에 필요한 경우만 적용 | 과학 렌더러의 동적 좌표·변환에 전역 금지 적용 금지 |
| no-unknown-classes | theme/import discovery 검증 후 시험 | 커스텀 utility와 외부 stylesheet의 인식 확인 |
| require-static-classes | 도입 시 파일 범위를 제한 | 런타임 데이터 표현을 잘못된 코드로 간주하지 않도록 점검 |

이 표는 앞으로 적용할 정책 제안이다. 현재 활성화된 규칙 목록은 ESLint 설정을 확인한다. 공식 설치 가이드도 플러그인 연결과 프로젝트 정책 선택을 구분한다. [설치 가이드](https://github.com/shadcn-ui/lint/blob/main/SETUP.md), [단계적 도입](https://github.com/shadcn-ui/lint/blob/main/docs/adoption.md)

no-restyle은 component의 variant와 size를 사용하도록 유도한다. className에서 layout을 허용하는 정책으로 시작할 수 있다. [규칙 설명](https://github.com/shadcn-ui/lint/blob/main/docs/rules/no-restyle.md)

no-raw-colors는 raw palette, 미선언 색 토큰, SVG의 literal 색 등을 검사한다. 임의의 hex class는 별도 규칙의 대상이며 동적인 SVG 표현까지 모두 검증하는 것은 아니다. [규칙 설명](https://github.com/shadcn-ui/lint/blob/main/docs/rules/no-raw-colors.md)

## 도입만으로 해결되지 않는 것

이 플러그인은 정적 분석이다. 키보드 조작, 포커스 이동, 색 대비, 브라우저 호환성, 렌더링 성능과 화학적 정확성은 각각 테스트해야 한다.

컴포넌트 import와 components.json의 경로, Tailwind theme discovery가 올바르게 연결되어야 한다. CSS 본문, 일부 parent selector, 다른 파일에서 가져온 class 값과 동적인 표현 등에는 분석 한계가 있다. lint 결과가 깨끗하다는 사실만으로 모든 스타일 경로가 검사되었다고 판단하지 않는다. [분석 범위와 한계](https://github.com/shadcn-ui/lint/blob/main/docs/how-it-works.md#what-it-cannot-see)

프로젝트가 정하는 새로운 token, variant, 예외와 규칙 비활성화도 코드 리뷰 대상이다. 초기 설정에서는 일반 ESLint·타입·빌드·브라우저 확인을 유지하고, 디자인 정책은 측정 가능한 작은 범위부터 추가하는 것이 적합하다.

## 실제 프로젝트에서 확인한 결과

[stdin probe 기록](verification/shadcn-lint-probe.json)에서 프로젝트의 ESLint 설정과 생성된 Button, theme을 그대로 사용했다. 임시 입력에 CLI 인자로 두 규칙만 경고 수준으로 켰으며 프로젝트 설정과 소스는 변경하지 않았다.

| 검증 | 결과 |
| --- | --- |
| 기본 설정의 shadcn plugin 등록 | 확인 |
| 기본 설정에서 활성화된 shadcn 디자인 규칙 | 0개 |
| Button에 raw 색과 padding을 준 임시 입력 | 색 토큰 위반 1개, 외형 변경 위반 2개 탐지 |
| Button variant·size와 layout만 사용한 임시 입력 | 진단 0개 |
| 컴포넌트·theme 인식 | 실제 Button의 variant·size와 테마 대안이 진단에 표시됨 |

실제 설치 버전은 ESLint 10.13.0, typescript-eslint 8.72.0, @shadcn/lint 0.2.3이다. 이 버전 조합으로 플러그인 로딩과 위의 제한된 probe가 통과했다. 이 결과를 모든 규칙이나 전체 디자인 시스템의 검증으로 확대하지 않는다.
