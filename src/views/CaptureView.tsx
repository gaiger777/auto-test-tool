import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '../api'
import { capturesToSteps, type CapturedCall } from '../capture'
import type { ScenarioRecord } from '../types'

export default function CaptureView() {
  const [url, setUrl] = useState('')
  const [tokenHeader, setTokenHeader] = useState('X-Auth-Token')
  const [active, setActive] = useState(false)
  const [calls, setCalls] = useState<CapturedCall[]>([])
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [scenarioName, setScenarioName] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const startedAt = useRef(0)

  useEffect(() => {
    api.captureSessionActive().then(setActive).catch(() => {})
    const unRec = listen<CapturedCall>('capture-recorded', e => {
      setCalls(prev => [e.payload, ...prev])
    })
    const unEnd = listen('capture-session-ended', () => {
      setActive(false)
      setNotice('캡처 세션이 종료되었습니다. 목록은 유지되며 변환할 수 있습니다.')
    })
    return () => { unRec.then(u => u()); unEnd.then(u => u()) }
  }, [])

  const start = async () => {
    setError(''); setNotice('')
    if (!url) { setError('대상 URL을 입력하세요'); return }
    try {
      await api.startCaptureSession(url)
      setActive(true)
      startedAt.current = Date.now()
      setCalls([]); setSelected({})
    } catch (e) { setError(String(e)) }
  }

  const stop = async () => {
    try {
      await api.stopCaptureSession()
      setActive(false)
      if (calls.length === 0 && Date.now() - startedAt.current > 3000) {
        setNotice('캡처가 0건입니다. 대상 사이트의 CSP로 후킹이 차단됐을 수 있습니다.')
      }
    } catch (e) { setError(String(e)) }
  }

  const toggle = (id: string) => setSelected(s => ({ ...s, [id]: !s[id] }))

  const addToScenario = async () => {
    setError(''); setNotice('')
    const chosen = calls.filter(c => selected[c.id]).reverse()
    if (chosen.length === 0) { setError('추가할 호출을 선택하세요'); return }
    const steps = capturesToSteps(chosen, tokenHeader)
    const rec: ScenarioRecord = {
      id: null,
      name: scenarioName || `캡처 시나리오 ${new Date().toISOString().slice(0, 19)}`,
      description: `${url} 캡처에서 생성`,
      steps_json: JSON.stringify(steps),
    }
    try {
      await api.saveScenario(rec)
      setNotice(`시나리오 "${rec.name}" 생성됨. 시나리오 탭에서 열어 편집하세요.`)
      setSelected({})
    } catch (e) { setError(String(e)) }
  }

  return (
    <div>
      <h2>네트워크 캡처</h2>
      <div className="add-row">
        <input placeholder="대상 사이트 URL (https://...)" value={url}
          onChange={e => setUrl(e.target.value)} disabled={active} style={{ minWidth: 320 }} />
        <input placeholder="토큰 헤더명" value={tokenHeader}
          onChange={e => setTokenHeader(e.target.value)} />
        {!active
          ? <button onClick={start}>세션 시작</button>
          : <button className="danger" onClick={stop}>세션 종료</button>}
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="dim">{notice}</p>}

      <div className="add-row">
        <input placeholder="새 시나리오 이름 (비우면 자동)" value={scenarioName}
          onChange={e => setScenarioName(e.target.value)} style={{ minWidth: 240 }} />
        <button onClick={addToScenario}>선택 항목을 시나리오로 저장</button>
        <span className="dim">캡처 {calls.length}건 · 선택 {Object.values(selected).filter(Boolean).length}건</span>
      </div>

      <table className="history">
        <thead>
          <tr><th></th><th>메서드</th><th>URL</th><th>상태</th></tr>
        </thead>
        <tbody>
          {calls.map(c => (
            <tr key={c.id}>
              <td><input type="checkbox" checked={!!selected[c.id]} onChange={() => toggle(c.id)} /></td>
              <td>{c.method}</td>
              <td>{c.url}</td>
              <td>{c.status}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
