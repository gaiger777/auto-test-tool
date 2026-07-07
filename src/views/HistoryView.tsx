import { useEffect, useState } from 'react'
import * as api from '../api'
import type { RunRecord, StepResultRecord } from '../types'

export default function HistoryView() {
  const [runs, setRuns] = useState<RunRecord[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [results, setResults] = useState<StepResultRecord[]>([])
  const [error, setError] = useState('')

  useEffect(() => { api.listRuns().then(setRuns).catch(e => setError(String(e))) }, [])

  const show = (runId: number) => {
    setSelected(runId)
    api.listStepResults(runId).then(setResults).catch(e => setError(String(e)))
  }

  return (
    <div>
      <h2>실행 히스토리</h2>
      {error && <p className="error">{error}</p>}
      <table className="history">
        <thead>
          <tr><th>ID</th><th>시나리오</th><th>상태</th><th>시작</th><th>종료</th><th></th></tr>
        </thead>
        <tbody>
          {runs.map(r => (
            <tr key={r.id}>
              <td>{r.id}</td>
              <td>{r.scenario_name}</td>
              <td>{r.status}</td>
              <td>{r.started_at}</td>
              <td>{r.finished_at ?? '-'}</td>
              <td><button onClick={() => show(r.id)}>상세</button></td>
            </tr>
          ))}
        </tbody>
      </table>

      {selected != null && (
        <>
          <h3>실행 #{selected} 스텝 결과</h3>
          <table className="history">
            <thead>
              <tr><th>#</th><th>스텝</th><th>상태</th><th>소요(ms)</th><th>상세</th></tr>
            </thead>
            <tbody>
              {results.map(r => (
                <tr key={r.step_index}>
                  <td>{r.step_index + 1}</td>
                  <td>{r.name}</td>
                  <td>{r.status}</td>
                  <td>{r.duration_ms}</td>
                  <td><pre className="detail">{r.detail}</pre></td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  )
}
