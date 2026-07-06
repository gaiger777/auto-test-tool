# contrabass E2E 자동화 테스트 툴 설계

날짜: 2026-07-06
상태: 승인됨

## 개요

contrabass(OpenStack 기반 플랫폼)를 대상으로 하는 E2E 자동화 테스트 데스크톱 앱.
사용자가 UI에서 스텝을 조합해 시나리오를 만들고, OpenStack API를 HTTP로 호출한 뒤
RabbitMQ notification으로 비동기 완료를 감지하여 다음 단계로 진행한다.

- 스택: Tauri 2 (Rust) + React (TypeScript, Vite)
- 시나리오 정의: UI 시나리오 빌더 (저장/재사용)
- 완료 감지: OpenStack 내부 notification (RabbitMQ, `compute.instance.create.end` 등)
- 인증: 환경 프로필 저장 + Keystone 토큰 자동 발급/갱신
- 저장: SQLite + 시나리오 JSON 내보내기/가져오기

## 아키텍처

```
┌─────────────────────────── Tauri 앱 ───────────────────────────┐
│  React 프론트엔드                                                │
│  ├─ 환경 프로필 관리 화면                                         │
│  ├─ 시나리오 빌더 (스텝 편집 + OpenStack 프리셋)                    │
│  ├─ 실행 화면 (스텝별 실시간 상태)                                  │
│  └─ 히스토리 화면                                                │
│         │ Tauri command (실행 요청)  ↑ Tauri event (진행 상황)     │
│  Rust 백엔드                                                    │
│  ├─ engine    : 시나리오 실행기 (스텝 순차 실행, 변수 해석)           │
│  ├─ steps     : http_call / wait_event / assert / sleep         │
│  ├─ openstack : Keystone 토큰 발급·캐시·갱신                       │
│  ├─ mq        : RabbitMQ notification 소비자 (lapin)             │
│  └─ store     : SQLite (시나리오, 실행 결과, 환경 프로필)            │
└─────────────────────────────────────────────────────────────────┘
          │ HTTP (reqwest)              │ AMQP
          ▼                             ▼
   OpenStack API (nova 등)      RabbitMQ notifications exchange
```

핵심 원칙: Rust 엔진은 범용 스텝 4종만 이해한다. "인스턴스 생성" 같은 OpenStack
스텝은 프론트엔드 프리셋이 범용 스텝 조합으로 펼쳐서 저장한다. 실행은 전부 Rust에서
일어나므로 UI 조작이 실행에 영향을 주지 않는다.

## 스텝 모델과 변수

시나리오 = 스텝의 순차 리스트(JSON). 스텝 4종:

- **`http_call`** — 메서드, URL, 헤더, 바디. 응답에서 JSONPath로 변수 캡처
  (예: `$.server.id` → `server_id`). 기대 상태코드 지정 가능.
- **`wait_event`** — RabbitMQ notification 중 `event_type`
  (예: `compute.instance.create.end`)과 payload 조건
  (예: `instance_id == {{server_id}}`)이 일치하는 메시지를 타임아웃 내에 대기.
- **`assert`** — 캡처된 변수에 대한 검증 (같음/포함/정규식).
- **`sleep`** — 고정 시간 대기.

모든 문자열 필드에서 `{{변수명}}` 치환 지원. 내장 변수: `{{auth_token}}`(Keystone
토큰), `{{base_url.nova}}` 등 서비스 엔드포인트 — 프리셋이 환경에 독립적이게 한다.

**레이스 방지**: MQ 소비자는 실행 시작 시점에 구독을 열고 실행 중 모든 notification을
버퍼링한다. `wait_event`는 버퍼 + 실시간 스트림 양쪽에서 매칭하므로, 스텝 시작 전에
도착한 이벤트도 놓치지 않는다.

## 데이터 모델 (SQLite)

- `environments` — 이름, Keystone URL, 프로젝트/도메인/사용자, RabbitMQ 접속정보,
  서비스 엔드포인트. 비밀번호는 OS 키체인(keyring crate)에 저장하고 DB에는 참조만 둔다.
- `scenarios` — 이름, 설명, 스텝 JSON. JSON 내보내기/가져오기는 이 JSON을 파일로.
- `runs` — 시나리오 ID, 환경 ID, 상태(running/passed/failed/cancelled), 시작/종료 시각.
- `step_results` — 실행 ID, 스텝 순번, 상태, 요청/응답 스냅샷, 캡처된 변수,
  에러 메시지, 소요 시간.

## 실행 흐름

1. 실행 클릭 → Tauri command `run_scenario(scenario_id, env_id)`
2. 엔진이 Keystone 토큰 발급(캐시 재사용), MQ 소비자 구독 시작
3. 스텝 순차 실행, 스텝마다 `step-started` / `step-finished` 이벤트를 프론트로 발행
4. 스텝 실패 시 실행 중단, 실패로 기록
5. cleanup으로 표시된 스텝(예: 인스턴스 삭제)은 앞 스텝이 실패해도 항상 실행
6. 실행 중 취소 → 현재 스텝 중단 후 cleanup 스텝 실행

## 에러 처리

- `wait_event` 타임아웃 필수(기본값 제공), HTTP 요청 타임아웃 존재
- Keystone 401 시 토큰 1회 재발급 후 재시도
- MQ 접속 실패 시 실행 시작을 실패 처리, 명확한 에러 표시.
  v1에 폴링 폴백 없음 — `wait_event`를 전략 인터페이스로 추상화해 추후 추가 가능.

## OpenStack 프리셋 (v1)

프론트엔드 정의 템플릿:

- 인스턴스 생성 (POST /servers + `compute.instance.create.end` 대기)
- 인스턴스 삭제 (DELETE + `compute.instance.delete.end` 대기, cleanup 기본)
- 네트워크 생성/삭제 (neutron)
- 볼륨 생성/삭제 (cinder 이벤트 대기)

프리셋 선택 시 전용 입력폼(이미지, 플레이버 등)이 뜨고 범용 스텝으로 펼쳐져 저장된다.
펼쳐진 뒤에도 일반 스텝처럼 수정 가능.

## 기술 스택

- Tauri 2, React + TypeScript + Vite
- Rust: `reqwest`(HTTP), `lapin`(AMQP), `rusqlite`(SQLite), `serde_json`,
  `jsonpath-rust`, `keyring`, `tokio`
- 테스트: 엔진 단위 테스트(변수 치환, 이벤트 매칭, assert 로직), `wiremock`으로
  HTTP 스텝 통합 테스트, MQ는 trait 추상화 + 인메모리 fake로 테스트
