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

export interface RunRecord {
  id: number
  scenario_id: number
  scenario_name: string
  env_id: number
  status: string
  started_at: string
  finished_at: string | null
}

export interface StepResultRecord {
  run_id: number
  step_index: number
  name: string
  status: string
  detail: string
  duration_ms: number
}

export type StepStatus = 'passed' | 'failed' | 'skipped'

export interface StepOutcome { index: number; name: string; status: StepStatus; detail: string; duration_ms: number }
