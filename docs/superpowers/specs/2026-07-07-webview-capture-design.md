# 웹뷰 네트워크 캡처 → 시나리오 생성 (v2) 설계

날짜: 2026-07-07
상태: 승인됨
관련: v1 설계 `2026-07-06-contrabass-e2e-test-tool-design.md`

## 개요

Tauri 웹뷰에 contrabass 사이트를 띄우고, 브라우저가 보내는 contrabass 자체 백엔드
API 호출을 fetch/XHR 후킹으로 캡처한다. 사용자가 실시간 목록에서 선택한 호출만
`http_call` 스텝으로 변환해 시나리오를 생성한다.

요구사항 확정 사항:
- 캡처 대상: contrabass 자체 백엔드 API (raw OpenStack API 아님)
- 캡처 전달: 로컬 수집 서버(127.0.0.1) — 원격 사이트에 Tauri IPC를 열지 않음
- 선택 UX: 실시간 목록 + 체크박스
- 변환: 토큰 헤더만 `{{auth_token}}` 치환, URL·경로 id·바디는 리터럴 그대로
- 비동기: 캡처는 `http_call`만 생성. `wait_event`는 사용자가 빌더에서 수동 추가

## 1. 아키텍처

```
┌───────────────────────── Tauri 앱 ─────────────────────────┐
│  메인 창 (기존 4탭 + 새 "캡처" 탭)                            │
│   CaptureView ── capture-recorded 이벤트 수신 → 실시간 목록   │
│        │ 체크한 항목 → http_call 변환 → 시나리오 빌더로 전달   │
│        │ "캡처 세션 시작(URL 입력)" command                   │
│  ┌─────┴──────────────── Rust ────────────────────────────┐ │
│  │ capture_server : axum, 127.0.0.1:<포트>, POST /capture   │ │
│  │        → 받은 캡처를 capture-recorded 이벤트로 메인창 emit │ │
│  │ capture_session: 별도 웹뷰 창 생성 + 후킹 스크립트 주입    │ │
│  └──────────────────────────────────────────────────────┘ │
│  캡처 웹뷰 창 (대상 contrabass 사이트)                        │
│   initialization_script: fetch/XHR 후킹 → POST 로컬서버      │
└────────────────────────────────────────────────────────────┘
```

Rust는 수집 서버 + 창 관리만, 프론트는 목록/선택/변환만 담당. 대상 사이트 웹뷰에는
IPC를 열지 않고 로컬 HTTP POST만 나간다.

## 2. 캡처 데이터 모델과 후킹

주입 스크립트가 fetch/XHR를 감싸 각 요청마다 아래를 수집해 로컬 서버로 POST:

```
CapturedCall {
  id: string          // 세션 내 순번 (프론트 목록 key)
  method: string
  url: string
  request_headers: { [k]: string }
  request_body: string | null
  status: number      // 응답 상태코드
  response_body: string | null   // 변수 캡처 제안용, 상한 truncate
  timestamp: number
}
```

후킹:
- fetch: `window.fetch` 래핑, 인자에서 method·url·headers·body 읽기, 응답은 `clone()`
  후 본문 읽어 페이지 동작에 영향 없이 캡처
- XHR: `open`/`setRequestHeader`/`send` 오버라이드, `loadend`에서 상태·응답 확보
- 전송: `sendBeacon` 또는 `fetch(keepalive)`로 비동기 (페이지 블로킹 없음)

한계:
- fetch/XHR만 잡힘 — 전체 페이지 이동·WebSocket은 대상 아님. SPA면 의미 있는 API 커버
- 대상 CSP가 엄격하면 주입 차단 가능 → 캡처 0건 시 UI 안내

## 3. 로컬 수집 서버와 이벤트 브리지

- Rust가 axum 서버를 `127.0.0.1:0`(OS 할당 빈 포트)에 바인딩, 실제 포트를 주입
  스크립트에 심어 전달 (고정 포트 충돌 없음)
- 엔드포인트 `POST /capture` 하나. 바디는 CapturedCall JSON. 받는 즉시 메인 창으로
  `capture-recorded` emit 후 200 반환
- 보안 경계: 루프백 바인딩 + 세션마다 무작위 토큰 생성해 주입 스크립트/서버가 공유.
  `/capture`는 토큰(헤더) 없으면 거부 — 다른 프로세스의 큐 오염 방지
- 세션 종료 시 서버 종료 + 토큰 폐기
- CORS: 127.0.0.1은 신뢰 컨텍스트라 HTTPS 대상에서도 mixed-content 차단 없음.
  서버가 `Access-Control-Allow-Origin` 응답 헤더 부착

## 4. 캡처 화면과 스텝 변환

세션 흐름:
1. 대상 URL 입력 + "세션 시작" → 수집 서버 기동 + 캡처 웹뷰 창 생성(스크립트·포트·토큰 주입)
2. 사이트 조작 → `capture-recorded` 이벤트가 실시간 목록에 행 추가(메서드·URL·상태, 최신순)
3. 체크박스로 선택
4. "선택 항목을 시나리오에 추가" → 각 캡처를 `http_call` 스텝으로 변환

변환 규칙:
- method, url, request_body → 그대로
- request_headers → 그대로 복사하되 인증 토큰 헤더만 값을 `{{auth_token}}`으로 치환.
  판별은 헤더명이 `X-Auth-Token`(대소문자 무시). 다른 이름이면 세션 설정에서 지정 가능
- expect_status → 캡처된 응답 상태코드로 자동 설정
- captures/assert → 비움 (빌더에서 추가)
- 스텝 이름 → `{METHOD} {url 경로}` 자동 생성

빌더 연결: 변환된 스텝을 새 시나리오 초안으로 로드하거나, 편집 중 시나리오가 있으면
끝에 append (기존 dirty 가드와 연결).

변환은 순수 함수 `capturesToSteps(selected, tokenHeaderName)`로 분리해 단위 테스트.

## 5. 에러 처리와 엣지 케이스

- CSP 후킹 차단: 세션 시작 후 캡처 0건이면 안내 (치명적 아님)
- 캡처 창 직접 닫힘: Rust가 감지해 세션 정리 + `capture-session-ended` emit.
  CaptureView는 "세션 종료됨" 표시하되 쌓인 목록·선택 유지(변환 계속 가능)
- 세션 중복 시작: 한 번에 한 세션. 진행 중이면 "세션 시작" 비활성
- 잘못된 대상 URL: 창 생성 실패 시 에러 표시 + 서버 정리(좀비 방지)
- 대용량 응답: `response_body` 주입 스크립트에서 상한(8KB) truncate 후 전송
- 비-JSON/바이너리 응답: 문자열로 못 읽으면 생략, 캡처는 유지
- 앱 종료 시 세션 활성: 수집 서버·캡처 창 정리

## 6. 테스트 전략

- vitest 순수 함수: `capturesToSteps` — 토큰 치환, expect_status, 이름 생성, 다건 변환 (핵심, 두텁게)
- Rust 단위: `/capture` 토큰 인증(유효/무효), CapturedCall 역직렬화, 이벤트 emit은 trait fake
- 주입 스크립트: 실브라우저 의존이라 로컬 mock 페이지로 수동 검증
- 통합: 실제 contrabass 사이트로 세션 → 조작 → 목록 → 변환 → 실행 수동 E2E

## 7. 파일 구조 (신규)

```
src-tauri/src/
├── capture_server.rs   # axum 수집 서버 + 토큰 인증
└── capture_session.rs  # 캡처 웹뷰 창 생성/정리, 후킹 스크립트 템플릿

src/
├── capture.ts          # CapturedCall 타입, capturesToSteps 순수 함수
├── capture.test.ts     # 변환 로직 vitest
└── views/CaptureView.tsx  # 세션 시작 + 실시간 목록 + 선택/변환
```

기존 `commands.rs`에 캡처 command 3개(세션 시작/종료/상태), `App.tsx`에 "캡처" 탭 추가.

## 향후 (v2 범위 밖, 백로그)

- 생성/삭제 호출 감지 시 wait_event 자동 제안
- base_url 파라미터화, 응답 id → 변수 캡처 자동 제안
- 자동 필터(정적 리소스·반복 폴링 제외)
