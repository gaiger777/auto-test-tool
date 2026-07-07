import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '../api'
import type { Environment, ScenarioRecord, StepDef, StepOutcome } from '../types'

interface StepRow {
  name: string
  type: string
  status: 'pending' | 'running' | 'passed' | 'failed' | 'skipped'
  detail: string
  duration_ms: number
}

export default function RunView({ active }: { active: boolean }) {
  const [scenarios, setScenarios] = useState<ScenarioRecord[]>([])
  const [envs, setEnvs] = useState<Environment[]>([])
  const [scenarioId, setScenarioId] = useState<number | null>(null)
  const [envId, setEnvId] = useState<number | null>(null)
  const [rows, setRows] = useState<StepRow[]>([])
  const [runStatus, setRunStatus] = useState<string>('')
  const [error, setError] = useState('')
  const runIdRef = useRef<number | null>(null)
  const pendingRef = useRef(false)
  const cancelWantedRef = useRef(false)
  const prevRunRef = useRef<number | null>(null)

  // run_id 확정 시점에 pending 중 요청된 취소를 실행한다.
  const flushQueuedCancel = (id: number) => {
    if (cancelWantedRef.current) { cancelWantedRef.current = false; api.cancelRun(id).catch(e => setError(String(e))) }
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
    if (!active) return
    api.listScenarios().then(setScenarios)
    api.listEnvironments().then(setEnvs)
  }, [active])

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
        setRunStatus(e.payload.status)
      }),
    ]
    return () => { unlistens.forEach(p => p.then(un => un())) }
  }, [])

  const start = async () => {
    if (scenarioId == null || envId == null) { setError('시나리오와 환경을 선택하세요'); return }
    setError('')
    const rec = scenarios.find(s => s.id === scenarioId)!
    let steps: StepDef[]
    try {
      steps = JSON.parse(rec.steps_json)
    } catch (e) {
      setError(`시나리오 스텝 데이터가 손상됨: ${e}`)
      return
    }
    setRows(steps.map(s => ({ name: s.name, type: s.type, status: 'pending', detail: '', duration_ms: 0 })))
    setRunStatus('running')
    prevRunRef.current = runIdRef.current
    runIdRef.current = null
    pendingRef.current = true
    cancelWantedRef.current = false
    try {
      const id = await api.runScenario(scenarioId, envId)
      if (runIdRef.current === null) runIdRef.current = id
      flushQueuedCancel(id)
    } catch (e) {
      pendingRef.current = false
      setRunStatus('')
      setError(String(e))
    }
  }

  const cancel = () => {
    if (runIdRef.current != null) api.cancelRun(runIdRef.current).catch(e => setError(String(e)))
    else cancelWantedRef.current = true
  }

  const icon = (s: StepRow['status']) =>
    ({ pending: '⚪', running: '🔵', passed: '✅', failed: '❌', skipped: '⏭️' })[s]

  return (
    <div>
      <h2>시나리오 실행</h2>
      <div className="add-row">
        <select value={scenarioId ?? ''} onChange={e => setScenarioId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">시나리오 선택</option>
          {scenarios.map(s => <option key={s.id} value={s.id!}>{s.name}</option>)}
        </select>
        <select value={envId ?? ''} onChange={e => setEnvId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">환경 선택</option>
          {envs.map(e2 => <option key={e2.id} value={e2.id!}>{e2.name}</option>)}
        </select>
        <button onClick={start} disabled={runStatus === 'running'}>실행</button>
        {runStatus === 'running' && <button className="danger" onClick={cancel}>취소</button>}
      </div>

      {runStatus && <p>상태: <strong>{runStatus}</strong></p>}
      {error && <p className="error">{error}</p>}

      <ol className="run-steps">
        {rows.map((r, i) => (
          <li key={i}>
            <div>{icon(r.status)} [{r.type}] {r.name}
              {r.duration_ms > 0 && <span className="dim"> — {r.duration_ms}ms</span>}
            </div>
            {r.detail && <pre className="detail">{r.detail}</pre>}
          </li>
        ))}
      </ol>
    </div>
  )
}
