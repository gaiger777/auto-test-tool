import { invoke } from '@tauri-apps/api/core'
import type { Environment, RunRecord, ScenarioRecord, StepResultRecord } from './types'

export const listEnvironments = () => invoke<Environment[]>('list_environments')
export const saveEnvironment = (env: Environment, password: string | null) =>
  invoke<number>('save_environment', { env, password })
export const deleteEnvironment = (id: number) => invoke<void>('delete_environment', { id })

export const listScenarios = () => invoke<ScenarioRecord[]>('list_scenarios')
export const saveScenario = (rec: ScenarioRecord) => invoke<number>('save_scenario', { rec })
export const deleteScenario = (id: number) => invoke<void>('delete_scenario', { id })
export const exportScenario = (id: number, path: string) => invoke<void>('export_scenario', { id, path })
export const importScenario = (path: string) => invoke<number>('import_scenario', { path })

export const listRuns = () => invoke<RunRecord[]>('list_runs')
export const listStepResults = (runId: number) => invoke<StepResultRecord[]>('list_step_results', { runId })

export const runScenario = (scenarioId: number, envId: number) =>
  invoke<number>('run_scenario', { scenarioId, envId })
export const cancelRun = (runId: number) => invoke<void>('cancel_run', { runId })
