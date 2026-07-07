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

export default function RunView() {
  const [scenarios, setScenarios] = useState<ScenarioRecord[]>([])
  const [envs, setEnvs] = useState<Environment[]>([])
  const [scenarioId, setScenarioId] = useState<number | null>(null)
  const [envId, setEnvId] = useState<number | null>(null)
  const [rows, setRows] = useState<StepRow[]>([])
  const [runStatus, setRunStatus] = useState<string>('')
  const [error, setError] = useState('')
  const runIdRef = useRef<number | null>(null)

  useEffect(() => {
    api.listScenarios().then(setScenarios)
    api.listEnvironments().then(setEnvs)

    const unlistens = [
      listen<{ run_id: number; index: number }>('step-started', e => {
        if (e.payload.run_id !== runIdRef.current) return
        setRows(rows => rows.map((r, i) => (i === e.payload.index ? { ...r, status: 'running' } : r)))
      }),
      listen<{ run_id: number; outcome: StepOutcome }>('step-finished', e => {
        if (e.payload.run_id !== runIdRef.current) return
        const o = e.payload.outcome
        setRows(rows => rows.map((r, i) =>
          i === o.index ? { ...r, status: o.status, detail: o.detail, duration_ms: o.duration_ms } : r))
      }),
      listen<{ run_id: number; status: string }>('run-finished', e => {
        if (e.payload.run_id !== runIdRef.current) return
        setRunStatus(e.payload.status)
      }),
    ]
    return () => { unlistens.forEach(p => p.then(un => un())) }
  }, [])

  const start = async () => {
    if (scenarioId == null || envId == null) { setError('시나리오와 환경을 선택하세요'); return }
    setError('')
    const rec = scenarios.find(s => s.id === scenarioId)!
    const steps: StepDef[] = JSON.parse(rec.steps_json)
    setRows(steps.map(s => ({ name: s.name, type: s.type, status: 'pending', detail: '', duration_ms: 0 })))
    setRunStatus('running')
    try {
      runIdRef.current = await api.runScenario(scenarioId, envId)
    } catch (e) {
      setRunStatus('')
      setError(String(e))
    }
  }

  const cancel = () => {
    if (runIdRef.current != null) api.cancelRun(runIdRef.current).catch(e => setError(String(e)))
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
