# autoTestTool — VS Code 톤앤매너 이식 + 시나리오 인라인 실행

작성일: 2026-07-08
브랜치: `feature/vscode-theme-inline-run`

## 배경 / 목표

autoTestTool(Tauri + React)의 UI를 사내 앱 **wFlowApp(dripper)** 의 **VS Code 다크 톤앤매너**로 통일하고, 시나리오 목록에서 **탭 이동 없이 바로 테스트를 실행**할 수 있게 한다.

성공 기준:
- 앱이 VS Code 다크 테마(좌측 아이콘 사이드바 + 다크 팔레트)로 보인다.
- 시나리오 목록 각 행의 **▶실행** 버튼으로 선택 환경 대상 실행이 시작되고, 그 자리에서 진행상황이 보인다.
- 기존 기능(실행/시나리오/캡처/환경/히스토리) 회귀 없음. `tsc`·기존 vitest 통과.

## 범위

**포함**
- 레이아웃: 상단 탭 → 좌측 activity bar(아이콘 사이드바) 구조.
- 테마: wFlowApp `--vsc-*` 토큰 이식, 전 컴포넌트 VS Code 다크 재스타일.
- 아이콘: `@vscode/codicons` 도입.
- 기능: 시나리오 목록 전역 환경 선택 + 행별 ▶실행 + 인라인 진행상황.
- 리팩터: 실행 이벤트/상태 로직을 `useRun` 훅으로 추출(RunView와 공유).

**제외 (YAGNI)**
- 커스텀 타이틀바(네이티브 창 장식 유지). 트래픽라이트/드래그영역 작업 안 함.
- 동시 다중 실행. 시나리오 뷰에서는 **동시 1건**만.
- 백엔드/엔진/Rust 변경 없음. `run_scenario` 등 기존 커맨드 그대로 사용.

## 설계

### 1. 레이아웃 (`src/App.tsx`)

```
.app (flex column, height 100vh, 다크)
 └ .workspace (flex row, flex:1)
    ├ .activitybar   ← 좌측 세로 아이콘 5개 (실행/시나리오/캡처/환경/히스토리)
    └ .main (flex:1) ← 활성 뷰
```

- activity bar 항목: codicon + 툴팁. 아이콘 매핑 —
  실행 `codicon-play`, 시나리오 `codicon-list-tree`, 캡처 `codicon-record`,
  환경 `codicon-server-environment`, 히스토리 `codicon-history`.
- 활성 항목: 좌측 2px accent 바 + `--vsc-fg-strong`, 나머지 `--vsc-fg-muted`.
- 뷰 마운트 전략은 현행 유지: `run`·`capture`는 `display` 토글(상태 보존), `scenarios`·`envs`·`history`는 조건부 마운트.

### 2. 테마 (`src/App.css` 전면 재작성)

- `:root`에 wFlowApp의 `--vsc-*` 토큰 복사(값 동일).
- `body`: `--vsc-bg`/`--vsc-fg`, `font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif`, 13px.
- 재스타일 대상 클래스: 버튼(기본/`.accent`/`.danger`), `input/select/textarea`(bg-alt + border), `table.history`(border, thead `--vsc-bg-alt`), `.list`, `.step`(details), `.detail`(코드블록 `--vsc-bg-deep`), `.add-row`, `.field`, `.error`(`--vsc-danger`), `.dim`(`--vsc-fg-muted`).
- `:focus-visible` 링: `1px solid var(--vsc-focus)`.
- `@vscode/codicons/dist/codicon.css` import (로컬 폰트 → Tauri 번들 포함, 외부 요청 없음).
- `index.html`: `lang="ko"`, `<title>` 앱명으로 정리.

### 3. `useRun` 훅 (신규 `src/hooks/useRun.ts`)

한 번에 하나의 실행에 대한 스텝 진행상황 + 이벤트 구독 + run_id 레이스 처리 + 취소를 캡슐화한다. (현 RunView 내부 로직을 이관)

인터페이스:
```ts
interface StepRow { name: string; type: string; status: 'pending'|'running'|'passed'|'failed'|'skipped'; detail: string; duration_ms: number }
interface UseRun {
  rows: StepRow[]
  status: string            // '' | 'running' | 'passed' | 'failed' | 'cancelled'
  error: string
  running: boolean
  activeScenarioId: number | null   // 현재 실행 중인 시나리오 (인라인 표시용)
  start(rec: ScenarioRecord, envId: number): Promise<void>
  cancel(): void
}
```
- `start`: `steps_json` 파싱 → `rows` 초기화 → `runScenario(rec.id, envId)` → run_id 레이스 처리(현행 로직 그대로: pending 중 첫 이벤트 run_id 채택, 취소 큐잉).
- 이벤트(`step-started`/`step-finished`/`run-finished`) 구독은 훅 내부 `useEffect` 1회.
- RunView와 ScenarioBuilder가 각자 `useRun()` 인스턴스를 사용(상태 격리).

### 4. 시나리오 인라인 실행 (`src/views/ScenarioBuilder.tsx`)

- 좌측 목록 상단 툴바: 환경 `select`(전역). `listEnvironments()` 로드. 선택값 `localStorage['run.envId']`에 기억.
- 각 시나리오 행: `[▶실행] [편집] [삭제]`. `running && activeScenarioId !== s.id` 이면 ▶ 비활성(동시 1건).
- ▶ 클릭: `envId` 없으면 안내 후 중단, 있으면 `run.start(s, envId)`.
- 실행 중인 행 아래 인라인 영역: 스텝 진행상황(✅/🔵/❌/⏭️/⚪ + 이름 + 소요ms) + `취소` 버튼 + 최종 상태 배지.

### 5. RunView 리팩터 (`src/views/RunView.tsx`)

- 내부 실행/이벤트 로직을 `useRun`으로 대체. 화면(시나리오·환경 선택 + 스텝 목록)은 동일 유지. 기능 동치.

## 파일 변경

| 파일 | 변경 |
|---|---|
| `src/App.tsx` | 상단 탭 → activity bar 사이드바 구조 |
| `src/App.css` | VS Code 다크 전면 재작성 |
| `index.html` | lang/title 정리 |
| `src/hooks/useRun.ts` (신규) | 실행 이벤트·상태·취소 훅 |
| `src/views/RunView.tsx` | `useRun` 사용하도록 리팩터 |
| `src/views/ScenarioBuilder.tsx` | 전역 환경선택 + 행별 ▶실행 + 인라인 진행 |
| `package.json` | `@vscode/codicons` devDependency 추가 |

## 테스트 / 검증

- `useRun`의 순수부(steps_json 파싱 → rows 매핑, 상태 전이)는 단위 테스트 가능하면 대상. 이벤트 의존부는 앱 실행으로 수동 확인.
- 회귀: `npx tsc --noEmit` 통과, 기존 `vitest`(capture/presets) 통과.
- 수동 E2E: 5개 뷰 렌더 확인, 시나리오 ▶실행 → 진행상황 표시 → 완료 상태, 환경 미선택 시 안내.

## 리스크

- codicons 폰트가 Tauri 번들/CSP에서 로드되는지 확인(로컬 자산이라 정상 예상). 실패 시 인라인 SVG 아이콘으로 폴백.
- `useRun` 추출 시 RunView의 run_id 레이스 처리 로직을 정확히 이관(회귀 주의) — 기존 동작을 1:1로 옮긴다.
