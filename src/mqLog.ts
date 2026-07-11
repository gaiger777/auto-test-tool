import { listen } from '@tauri-apps/api/event'
import * as api from './api'

// RabbitMQ 로그/세션을 React 컴포넌트 밖(모듈 전역)에 두어 탭 전환·언마운트에도 유지한다.

export interface LogRow { ts: string; event_type: string; text: string }

let rows: LogRow[] = []
let connectSeq = 0
let snapshot = { rows, connectSeq } // useSyncExternalStore 용 안정 참조
const subs = new Set<() => void>()
let started = false

function commit() { snapshot = { rows, connectSeq }; subs.forEach(f => f()) }

function ensureStarted() {
  if (started) return
  started = true
  // 앱 생애주기 동안 유지되는 단일 리스너. 어떤 화면이 떠 있든 계속 누적한다.
  listen<{ event_type: string; text: string }>('mq-log', e => {
    const ts = new Date().toLocaleTimeString()
    if (e.payload.event_type === '(연결)') connectSeq++
    rows = [...rows.slice(-500), { ts, event_type: e.payload.event_type, text: e.payload.text }]
    commit()
  })
}

export const mqLog = {
  subscribe(cb: () => void) { ensureStarted(); subs.add(cb); return () => { subs.delete(cb) } },
  getSnapshot() { return snapshot },
  clear() { rows = []; commit() },
}

// 활성 MQ 세션(환경 id). 시작/중단을 이 헬퍼로 하면 화면 간 상태가 공유되고 탭 전환에도 유지된다.
let activeEnvId: number | null = null
const sessSubs = new Set<() => void>()
function sessCommit() { sessSubs.forEach(f => f()) }

export const mqSession = {
  subscribe(cb: () => void) { sessSubs.add(cb); return () => { sessSubs.delete(cb) } },
  getEnvId() { return activeEnvId },
  async start(envId: number) { await api.startReplayMq(envId); activeEnvId = envId; sessCommit() },
  async stop() { try { await api.stopReplayMq() } catch { /* noop */ } activeEnvId = null; sessCommit() },
}
