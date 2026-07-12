import { useEffect, useRef, useState } from 'react'
import * as api from '../api'
import { kindLabel } from '../uiStep'
import type { UiRunRecord, UiRunStepRecord } from '../types'

export default function HistoryView() {
  const [runs, setRuns] = useState<UiRunRecord[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [steps, setSteps] = useState<UiRunStepRecord[]>([])
  const [error, setError] = useState('')
  const selectedRef = useRef<number | null>(null)

  const reload = () => {
    setError('')
    api.listUiRuns().then(setRuns).catch(e => setError(String(e)))
    if (selectedRef.current != null) {
      const id = selectedRef.current
      api.listUiRunSteps(id)
        .then(rs => { if (selectedRef.current === id) setSteps(rs) })
        .catch(e => setError(String(e)))
    }
  }

  useEffect(() => {
    reload()
    // 스위트에서 실행이 끝나면 목록/상세를 자동 갱신
    const h = () => reload()
    window.addEventListener('ui-run-finished', h)
    return () => window.removeEventListener('ui-run-finished', h)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const close = () => { setSelected(null); selectedRef.current = null; setSteps([]) }

  const removeRun = async (id: number) => {
    setError('')
    if (!window.confirm(`실행 #${id} 기록을 삭제할까요?`)) return
    try { await api.deleteUiRun(id); if (selected === id) close(); reload() } catch (e) { setError(String(e)) }
  }
  const clearAll = async () => {
    setError('')
    if (!window.confirm('모든 실행 히스토리를 삭제할까요?')) return
    try { await api.clearUiRuns(); close(); reload() } catch (e) { setError(String(e)) }
  }
  // 스텝 상세 문자열에서 API 성공/실패 판정 (플레이어가 '· API N건' / '· ⚠ API오류 …'로 남김)
  const apiCell = (detail: string) => {
    if (/API오류/.test(detail)) return <span title={detail} style={{ color: 'var(--vsc-danger)' }}>⚠ 실패</span>
    const m = detail.match(/API\s+(\d+)건/)
    if (m && Number(m[1]) > 0) return <span title={detail} style={{ color: 'var(--vsc-ok)' }}>✅ {m[1]}</span>
    return <span className="dim">-</span>
  }

  const show = (runId: number) => {
    setError('')
    if (selected === runId) { close(); return } // 같은 행을 다시 누르면 닫기(토글)
    setSelected(runId)
    selectedRef.current = runId
    api.listUiRunSteps(runId)
      .then(rs => { if (selectedRef.current === runId) setSteps(rs) })
      .catch(e => setError(String(e)))
  }

  return (
    <div>
      <h2>실행 히스토리 <button onClick={reload}>새로고침</button>
        {runs.length > 0 && <button className="danger" onClick={clearAll} style={{ marginLeft: 8 }}>전체 삭제</button>}
      </h2>
      <p className="dim">'시나리오 실행'에서 실행한 기록입니다.</p>
      {error && <p className="error">{error}</p>}
      <table className="history">
        <thead>
          <tr><th>ID</th><th>시나리오</th><th>사이트</th><th>상태</th><th>시작</th><th>종료</th><th>상세</th><th>삭제</th></tr>
        </thead>
        <tbody>
          {runs.map(r => (
            <tr key={r.id}>
              <td>{r.id}</td>
              <td>{r.flow_name}</td>
              <td className="dim" style={{ maxWidth: 220, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={r.site_url}>{r.site_url}</td>
              <td>{r.status}</td>
              <td>{r.started_at}</td>
              <td>{r.finished_at ?? '-'}</td>
              <td><button onClick={() => show(r.id)}>{selected === r.id ? '닫기' : '상세'}</button></td>
              <td><button className="danger" onClick={() => removeRun(r.id)}>✕</button></td>
            </tr>
          ))}
        </tbody>
      </table>

      {selected != null && (
        <>
          <h3>실행 #{selected} 스텝 결과 <button onClick={close}>닫기</button></h3>
          <table className="history">
            <thead>
              <tr><th>#</th><th>동작</th><th>이름</th><th>상태</th><th>API</th><th>상세</th></tr>
            </thead>
            <tbody>
              {steps.map(r => (
                <tr key={r.step_index}>
                  <td>{r.step_index + 1}</td>
                  <td>{kindLabel(r.kind)}</td>
                  <td>{r.name}</td>
                  <td>{r.status === 'passed' ? '✅' : r.status === 'failed' ? '❌' : r.status}</td>
                  <td>{apiCell(r.detail)}</td>
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
