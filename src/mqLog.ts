import { listen } from '@tauri-apps/api/event'
import * as api from './api'

// RabbitMQ 로그/세션을 화면(채널)별로 독립 관리한다. 채널 키: "env" | "runner" | "capture".
// 컴포넌트 밖(모듈 전역)에 두어 탭 전환·언마운트에도 유지한다.

export interface LogRow { ts: string; event_type: string; text: string }

interface Channel {
  rows: LogRow[]
  connectSeq: number
  snapshot: { rows: LogRow[]; connectSeq: number }
  subs: Set<() => void>
  activeEnvId: number | null
  sessSubs: Set<() => void>
}

const channels = new Map<string, Channel>()
function chan(key: string): Channel {
  let c = channels.get(key)
  if (!c) {
    c = { rows: [], connectSeq: 0, snapshot: { rows: [], connectSeq: 0 }, subs: new Set(), activeEnvId: null, sessSubs: new Set() }
    channels.set(key, c)
  }
  return c
}

let started = false
function ensureStarted() {
  if (started) return
  started = true
  // 단일 리스너에서 payload.channel로 라우팅해 각 채널 버퍼에 누적한다.
  listen<{ channel?: string; event_type: string; text: string }>('mq-log', e => {
    const c = chan(e.payload.channel || 'default')
    const ts = new Date().toLocaleTimeString()
    if (e.payload.event_type === '(연결)') c.connectSeq++
    c.rows = [...c.rows.slice(-500), { ts, event_type: e.payload.event_type, text: e.payload.text }]
    c.snapshot = { rows: c.rows, connectSeq: c.connectSeq }
    c.subs.forEach(f => f())
  })
}

export interface MqLog {
  subscribe(cb: () => void): () => void
  getSnapshot(): { rows: LogRow[]; connectSeq: number }
  clear(): void
}
export interface MqSession {
  subscribe(cb: () => void): () => void
  getEnvId(): number | null
  start(envId: number): Promise<void>
  stop(): Promise<void>
}

// useSyncExternalStore는 안정된 subscribe/getSnapshot 참조가 필요하므로 채널별로 인스턴스를 캐시한다.
const logApis = new Map<string, MqLog>()
const sessApis = new Map<string, MqSession>()

export function mqLogFor(key: string): MqLog {
  let a = logApis.get(key)
  if (!a) {
    a = {
      subscribe(cb) { ensureStarted(); const c = chan(key); c.subs.add(cb); return () => { c.subs.delete(cb) } },
      getSnapshot() { return chan(key).snapshot },
      clear() { const c = chan(key); c.rows = []; c.snapshot = { rows: [], connectSeq: c.connectSeq }; c.subs.forEach(f => f()) },
    }
    logApis.set(key, a)
  }
  return a
}

export function mqSessionFor(key: string): MqSession {
  let a = sessApis.get(key)
  if (!a) {
    a = {
      subscribe(cb) { const c = chan(key); c.sessSubs.add(cb); return () => { c.sessSubs.delete(cb) } },
      getEnvId() { return chan(key).activeEnvId },
      async start(envId) { await api.startReplayMq(envId, key); const c = chan(key); c.activeEnvId = envId; c.sessSubs.forEach(f => f()) },
      async stop() { try { await api.stopReplayMq(key) } catch { /* noop */ } const c = chan(key); c.activeEnvId = null; c.sessSubs.forEach(f => f()) },
    }
    sessApis.set(key, a)
  }
  return a
}
