export interface Capture { var: string; json_path: string }
export interface Condition { json_path: string; equals: string }
export type AssertOp = 'eq' | 'contains' | 'regex'

export type Action =
  | { type: 'http_call'; method: string; url: string; headers?: Record<string, string>; body?: string | null; expect_status?: number | null; captures?: Capture[] }
  | { type: 'wait_event'; event_type: string; conditions?: Condition[]; timeout_secs: number }
  | { type: 'assert'; left: string; op: AssertOp; right: string }
  | { type: 'sleep'; seconds: number }

export type StepDef = { name: string; cleanup?: boolean } & Action

export interface Environment {
  id: number | null
  name: string
  keystone_url: string
  user_name: string
  user_domain: string
  project_name: string
  project_domain: string
  mq_url: string
  mq_exchanges: string
  endpoints: Record<string, string>
  mq_hosts: string
  mq_user: string
  mq_password: string
  mq_vhost: string
}

export interface ScenarioRecord { id: number | null; name: string; description: string; steps_json: string }

export type RunStatus = 'running' | 'passed' | 'failed' | 'cancelled' | 'interrupted'

export interface RunRecord {
  id: number
  scenario_id: number
  scenario_name: string
  env_id: number
  status: RunStatus
  started_at: string
  finished_at: string | null
}

export type StepStatus = 'passed' | 'failed' | 'skipped'

export interface StepResultRecord {
  run_id: number
  step_index: number
  name: string
  status: StepStatus
  detail: string
  duration_ms: number
}

export interface StepOutcome { index: number; name: string; status: StepStatus; detail: string; duration_ms: number }

// UI 레코더: 캡처 창에서 사용자의 클릭/입력을 기록한 것
export interface UiSelector { strategy: string; value: string }
export interface UiCall {
  method: string
  url: string
  status: number
  request_headers?: Record<string, string>
  request_body?: string | null
}

// UI 동작 종류: 웹뷰에서 재생하는 UI 스텝 + 흐름에 끼워넣는 프로그램 스텝.
export type UiKind = 'click' | 'input' | 'hover' | 'http_call' | 'wait_event' | 'assert' | 'sleep'
export const UI_STEP_KINDS: UiKind[] = ['click', 'input', 'hover']
export const PROG_STEP_KINDS: UiKind[] = ['http_call', 'wait_event', 'assert', 'sleep']
export const isProgKind = (k: UiKind) => PROG_STEP_KINDS.includes(k)

// 프로그램 스텝의 설정. kind에 따라 사용하는 필드가 다르다.
export interface UiProgStep {
  // http_call
  method?: string
  url?: string
  headers?: Record<string, string>
  body?: string | null
  expect_status?: number | null
  // http_call / wait_event 는 url/event_type 사용
  event_type?: string
  conditions?: Condition[]
  timeout_secs?: number
  // assert
  left?: string
  op?: AssertOp
  right?: string
  // sleep
  seconds?: number
}

export interface UiAction {
  id: string
  kind: UiKind
  selectors: UiSelector[]
  name: string
  value: string | null
  href?: string | null
  api?: UiCall[]
  url: string
  timestamp: number
  // 프로그램 스텝(http_call/wait_event/assert/sleep)일 때의 설정
  step?: UiProgStep | null
}

// UI 재생 스텝 결과 (index = -1 은 재생 종료 신호, status 'delegate' 는 백엔드 위임 요청)
export interface UiStepResult { index: number; status: 'passed' | 'failed' | 'delegate'; detail: string; done: boolean }

// DB에 저장된 UI 플로우 (사이트 URL별)
export interface UiFlowRecord { id: number | null; name: string; site_url: string; grp: string; actions_json: string }
export interface UiFlowSite { site_url: string; count: number }

// UI 스위트/레코더 실행 히스토리
export interface UiRunRecord {
  id: number
  flow_id: number | null
  flow_name: string
  site_url: string
  status: string
  started_at: string
  finished_at: string | null
}
export interface UiRunStepRecord {
  run_id: number
  step_index: number
  kind: string
  name: string
  status: string
  detail: string
}
