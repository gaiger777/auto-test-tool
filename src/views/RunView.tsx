import { useEffect, useState } from 'react'
import * as api from '../api'
import { useRun } from '../hooks/useRun'
import { RunProgress } from './RunProgress'
import type { Environment, ScenarioRecord } from '../types'

export default function RunView({ active }: { active: boolean }) {
  const [scenarios, setScenarios] = useState<ScenarioRecord[]>([])
  const [envs, setEnvs] = useState<Environment[]>([])
  const [scenarioId, setScenarioId] = useState<number | null>(null)
  const [envId, setEnvId] = useState<number | null>(null)
  const [token, setToken] = useState<string>(() => localStorage.getItem('run.token') ?? '')
  const [error, setError] = useState('')
  const run = useRun()

  useEffect(() => {
    if (!active) return
    api.listScenarios().then(setScenarios)
    api.listEnvironments().then(setEnvs)
  }, [active])

  const changeToken = (t: string) => { setToken(t); localStorage.setItem('run.token', t) }

  const start = () => {
    if (scenarioId == null) { setError('시나리오를 선택하세요'); return }
    setError('')
    const rec = scenarios.find(s => s.id === scenarioId)!
    const vars = token.trim() ? { auth_token: token.trim() } : undefined
    run.start(rec, envId, vars) // 환경 미선택(null) → Postman식 단순 실행
  }

  return (
    <div>
      <h2>시나리오 실행</h2>
      <div className="add-row">
        <select value={scenarioId ?? ''} onChange={e => setScenarioId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">시나리오 선택</option>
          {scenarios.map(s => <option key={s.id} value={s.id!}>{s.name}</option>)}
        </select>
        <select value={envId ?? ''} onChange={e => setEnvId(e.target.value ? Number(e.target.value) : null)}>
          <option value="">환경 없음 (단순 실행)</option>
          {envs.map(e2 => <option key={e2.id} value={e2.id!}>{e2.name}</option>)}
        </select>
        <button className="accent" onClick={start} disabled={run.running}>실행</button>
        {run.running && <button className="danger" onClick={run.cancel}>취소</button>}
      </div>
      <div className="add-row">
        <input
          style={{ minWidth: 360 }}
          placeholder="인증 토큰 (auth_token — 환경 없이 실행 시 사용)"
          value={token}
          onChange={e => changeToken(e.target.value)}
        />
      </div>

      {run.status && <p>상태: <strong>{run.status}</strong></p>}
      {(error || run.error) && <p className="error">{error || run.error}</p>}

      <RunProgress rows={run.rows} />
    </div>
  )
}
