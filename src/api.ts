import { invoke } from '@tauri-apps/api/core'
import type { Condition, Environment, RunRecord, ScenarioRecord, StepResultRecord, UiAction, UiFlowRecord, UiFlowSite, UiRunRecord, UiRunStepRecord } from './types'

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

export const runScenario = (scenarioId: number, envId: number | null, vars?: Record<string, string>) =>
  invoke<number>('run_scenario', { scenarioId, envId, vars })
export const cancelRun = (runId: number) => invoke<void>('cancel_run', { runId })

export const startCaptureSession = (url: string) => invoke<void>('start_capture_session', { url })
export const stopCaptureSession = () => invoke<void>('stop_capture_session')
export const captureSessionActive = () => invoke<boolean>('capture_session_active')
export const setUiRecording = (enabled: boolean) => invoke<void>('set_ui_recording', { enabled })

export const startUiReplay = (url: string, actions: UiAction[]) =>
  invoke<void>('start_ui_replay', { url, actions })
export const saveUiActions = (path: string, actions: UiAction[]) =>
  invoke<void>('save_ui_actions', { path, actions })
export const loadUiActions = (path: string) => invoke<UiAction[]>('load_ui_actions', { path })

// UI 플로우 DB (사이트 URL별)
export const saveUiFlow = (name: string, siteUrl: string, group: string, actions: UiAction[]) =>
  invoke<number>('save_ui_flow', { name, siteUrl, group, actions })
export const listUiFlowSites = () => invoke<UiFlowSite[]>('list_ui_flow_sites')
export const listUiFlows = (siteUrl: string) => invoke<UiFlowRecord[]>('list_ui_flows', { siteUrl })
export const listAllUiFlows = () => invoke<UiFlowRecord[]>('list_all_ui_flows')
export const deleteUiFlow = (id: number) => invoke<void>('delete_ui_flow', { id })
export const renameUiFlow = (id: number, newName: string) => invoke<void>('rename_ui_flow', { id, newName })
export const renameUiGroup = (siteUrl: string, oldGroup: string, newGroup: string) =>
  invoke<number>('rename_ui_group', { siteUrl, oldGroup, newGroup })
export const stopUiReplay = () => invoke<void>('stop_ui_replay')
export const exportUiFlows = (path: string) => invoke<void>('export_ui_flows', { path })
export const importUiFlows = (path: string) => invoke<number>('import_ui_flows', { path })

// 인터리브 재생: wait_event 위임 후 같은 창에서 재개
export const resumeUiReplay = (nextIdx: number, prevStatus: string, prevDetail: string) =>
  invoke<void>('resume_ui_replay', { nextIdx, prevStatus, prevDetail })
export const startReplayMq = (envId: number, channel: string) => invoke<void>('start_replay_mq', { envId, channel })
export const stopReplayMq = (channel: string) => invoke<void>('stop_replay_mq', { channel })
export const continueUiReplay = (actions: UiAction[]) => invoke<void>('continue_ui_replay', { actions })
export const runWaitEvent = (eventType: string, conditions: Condition[], timeoutSecs: number, channel: string) =>
  invoke<string>('run_wait_event', { eventType, conditions, timeoutSecs, channel })

// UI 실행 히스토리
export const createUiRun = (flowId: number | null, flowName: string, siteUrl: string) =>
  invoke<number>('create_ui_run', { flowId, flowName, siteUrl })
export const saveUiRunStep = (runId: number, stepIndex: number, kind: string, name: string, status: string, detail: string) =>
  invoke<void>('save_ui_run_step', { runId, stepIndex, kind, name, status, detail })
export const finishUiRun = (runId: number, status: string) => invoke<void>('finish_ui_run', { runId, status })
export const listUiRuns = () => invoke<UiRunRecord[]>('list_ui_runs')
export const listUiRunSteps = (runId: number) => invoke<UiRunStepRecord[]>('list_ui_run_steps', { runId })
