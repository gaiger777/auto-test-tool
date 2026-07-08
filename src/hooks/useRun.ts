import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '../api'
import type { ScenarioRecord, StepDef, StepOutcome } from '../types'

export interface StepRow {
  name: string
  type: string
  status: 'pending' | 'running' | 'passed' | 'failed' | 'skipped'
  detail: string
  duration_ms: number
}

export interface UseRun {
  rows: StepRow[]
  status: string // '' | 'running' | 'passed' | 'failed' | 'cancelled'
  error: string
  running: boolean
  activeScenarioId: number | null
  start: (rec: ScenarioRecord, envId: number) => Promise<void>
  cancel: () => void
}

/**
 * 한 번에 하나의 시나리오 실행에 대한 스텝 진행상황·이벤트 구독·취소를 캡슐화한다.
 * (run_id 확정 전에 이벤트가 먼저 도착하는 레이스, 취소 큐잉 처리 포함)
 * RunView(상세 화면)와 ScenarioBuilder(인라인 실행)가 각각 인스턴스를 사용한다.
 */
export function useRun(): UseRun {
  const [rows, setRows] = useState<StepRow[]>([])
  const [status, setStatus] = useState<string>('')
  const [error, setError] = useState('')
  const [activeScenarioId, setActiveScenarioId] = useState<number | null>(null)
  const runIdRef = useRef<number | null>(null)
  const pendingRef = useRef(false)
  const cancelWantedRef = useRef(false)
  const prevRunRef = useRef<number | null>(null)

  // run_id 확정 시점에 pending 중 요청된 취소를 실행한다.
  const flushQueuedCancel = (id: number) => {
    if (cancelWantedRef.current) {
      cancelWantedRef.current = false
      api.cancelRun(id).catch(e => setError(String(e)))
    }
  }

  // 시작 직후 invoke 응답보다 이벤트가 먼저 도착하는 레이스 대응:
  // 실행 요청 대기 중(pending)이면 첫 이벤트의 run_id를 현재 실행으로 채택한다.
  const isCurrentRun = (id: number) => {
    if (id === prevRunRef.current) return false
    if (runIdRef.current === id) return true
    if (runIdRef.current === null && pendingRef.current) {
      runIdRef.current = id
      flushQueuedCancel(id)
      return true
    }
    return false
  }

  useEffect(() => {
    const unlistens = [
      listen<{ run_id: number; index: number }>('step-started', e => {
        if (!isCurrentRun(e.payload.run_id)) return
        setRows(rows => rows.map((r, i) => (i === e.payload.index ? { ...r, status: 'running' } : r)))
      }),
      listen<{ run_id: number; outcome: StepOutcome }>('step-finished', e => {
        if (!isCurrentRun(e.payload.run_id)) return
        const o = e.payload.outcome
        setRows(rows => rows.map((r, i) =>
          i === o.index ? { ...r, status: o.status, detail: o.detail, duration_ms: o.duration_ms } : r))
      }),
      listen<{ run_id: number; status: string }>('run-finished', e => {
        if (!isCurrentRun(e.payload.run_id)) return
        pendingRef.current = false
        setStatus(e.payload.status)
      }),
    ]
    return () => { unlistens.forEach(p => p.then(un => un())) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const start = async (rec: ScenarioRecord, envId: number) => {
    setError('')
    let steps: StepDef[]
    try {
      steps = JSON.parse(rec.steps_json)
    } catch (e) {
      setError(`시나리오 스텝 데이터가 손상됨: ${e}`)
      return
    }
    setRows(steps.map(s => ({ name: s.name, type: s.type, status: 'pending', detail: '', duration_ms: 0 })))
    setStatus('running')
    setActiveScenarioId(rec.id)
    prevRunRef.current = runIdRef.current
    runIdRef.current = null
    pendingRef.current = true
    cancelWantedRef.current = false
    try {
      const id = await api.runScenario(rec.id!, envId)
      if (runIdRef.current === null) runIdRef.current = id
      flushQueuedCancel(id)
    } catch (e) {
      pendingRef.current = false
      setStatus('')
      setError(String(e))
    }
  }

  const cancel = () => {
    if (runIdRef.current != null) api.cancelRun(runIdRef.current).catch(e => setError(String(e)))
    else cancelWantedRef.current = true
  }

  return { rows, status, error, running: status === 'running', activeScenarioId, start, cancel }
}
