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
export interface UiAction {
  id: string
  kind: 'click' | 'input'
  selectors: UiSelector[]
  name: string
  value: string | null
  url: string
  timestamp: number
}

// UI 재생 스텝 결과 (index = -1 은 재생 종료 신호)
export interface UiStepResult { index: number; status: 'passed' | 'failed'; detail: string; done: boolean }
