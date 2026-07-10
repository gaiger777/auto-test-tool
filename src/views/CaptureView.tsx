import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '../api'
import { capturesToSteps, type CapturedCall } from '../capture'
import type { ScenarioRecord, UiAction, UiStepResult } from '../types'

export default function CaptureView() {
  const [url, setUrl] = useState('')
  const [tokenHeader, setTokenHeader] = useState('X-Auth-Token')
  const [active, setActive] = useState(false)
  const [calls, setCalls] = useState<CapturedCall[]>([])
  const [uiActions, setUiActions] = useState<UiAction[]>([])
  const [replaying, setReplaying] = useState(false)
  const [replayResults, setReplayResults] = useState<Record<number, { status: string; detail: string }>>({})
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [scenarioName, setScenarioName] = useState('')
  const [flowName, setFlowName] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const startedAt = useRef(0)

  useEffect(() => {
    api.captureSessionActive().then(setActive).catch(() => {})
    const unRec = listen<CapturedCall>('capture-recorded', e => {
      setCalls(prev => [e.payload, ...prev])
    })
    const unUi = listen<UiAction>('ui-recorded', e => {
      setUiActions(prev => [...prev, e.payload])
    })
    const unReplay = listen<UiStepResult>('ui-replay-step', e => {
      const r = e.payload
      if (r.done) {
        setReplaying(false)
        setNotice(r.status === 'passed' ? 'UI 재생 완료' : `UI 재생 중단: ${r.detail}`)
      } else {
        setReplayResults(prev => ({ ...prev, [r.index]: { status: r.status, detail: r.detail } }))
      }
    })
    const unEnd = listen('capture-session-ended', () => {
      setActive(false)
      setReplaying(false)
      setNotice('세션이 종료되었습니다. 목록은 유지됩니다.')
    })
    return () => { unRec.then(u => u()); unUi.then(u => u()); unReplay.then(u => u()); unEnd.then(u => u()) }
  }, [])

  const start = async () => {
    setError(''); setNotice('')
    if (!url) { setError('대상 URL을 입력하세요'); return }
    try {
      await api.startCaptureSession(url)
      setActive(true)
      startedAt.current = Date.now()
      setCalls([]); setUiActions([]); setSelected({}); setReplayResults({})
    } catch (e) { setError(String(e)) }
  }

  const stop = async () => {
    try {
      await api.stopCaptureSession()
      setActive(false)
      if (calls.length === 0 && uiActions.length === 0 && Date.now() - startedAt.current > 3000) {
        setNotice('캡처가 0건입니다. 대상 사이트의 CSP로 후킹이 차단됐을 수 있습니다.')
      }
    } catch (e) { setError(String(e)) }
  }

  const replay = async () => {
    setError(''); setNotice(''); setReplayResults({})
    if (uiActions.length === 0) { setError('재생할 UI 동작이 없습니다'); return }
    setReplaying(true)
    const startUrl = uiActions[0]?.url || url
    try { await api.startUiReplay(startUrl, uiActions) }
    catch (e) { setReplaying(false); setError(String(e)) }
  }

  // UI 동작 편집: 삭제 / 순번 이동 (편집 시 재생 결과는 초기화 — 인덱스가 바뀌므로)
  const delUi = (i: number) => { setUiActions(a => a.filter((_, j) => j !== i)); setReplayResults({}) }
  const moveUi = (i: number, d: -1 | 1) => {
    const j = i + d
    if (j < 0 || j >= uiActions.length) return
    const next = [...uiActions]
    ;[next[i], next[j]] = [next[j], next[i]]
    setUiActions(next); setReplayResults({})
  }

  const saveFlow = async () => {
    setError(''); setNotice('')
    if (uiActions.length === 0) { setError('저장할 UI 동작이 없습니다'); return }
    if (!flowName.trim()) { setError('시나리오 이름을 입력하세요'); return }
    const siteUrl = url || uiActions[0]?.url || ''
    if (!siteUrl) { setError('사이트 URL이 없습니다'); return }
    try {
      await api.saveUiFlow(flowName.trim(), siteUrl, uiActions)
      setNotice(`시나리오 "${flowName.trim()}" DB 저장됨 · 사이트 ${siteUrl}`)
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

  const resultIcon = (i: number) => {
    const r = replayResults[i]
    if (r) return r.status === 'passed' ? '✅' : '❌'
    return replaying ? '⏳' : ''
  }

  return (
    <div>
      <h2>네트워크 캡처 + UI 레코더</h2>
      <div className="add-row">
        <input placeholder="대상 사이트 URL (https://...)" value={url}
          onChange={e => setUrl(e.target.value)} disabled={active || replaying} style={{ minWidth: 320 }} />
        <input placeholder="토큰 헤더명" value={tokenHeader}
          onChange={e => setTokenHeader(e.target.value)} />
        {!active
          ? <button className="accent" onClick={start} disabled={replaying}>세션 시작</button>
          : <button className="danger" onClick={stop}>세션 종료</button>}
      </div>

      {error && <p className="error">{error}</p>}
      {notice && <p className="dim">{notice}</p>}

      <div className="add-row">
        <input placeholder="새 시나리오 이름 (비우면 자동)" value={scenarioName}
          onChange={e => setScenarioName(e.target.value)} style={{ minWidth: 240 }} />
        <button onClick={addToScenario}>선택 네트워크 호출을 시나리오로 저장</button>
        <span className="dim">네트워크 {calls.length}건 · 선택 {Object.values(selected).filter(Boolean).length}건</span>
      </div>

      <div className="two-col" style={{ gridTemplateColumns: '1fr 1fr' }}>
        <div>
          <h3>네트워크 호출 ({calls.length})
            <button style={{ marginLeft: 8 }} onClick={() => { setCalls([]); setSelected({}) }} disabled={calls.length === 0}>전체 삭제</button>
          </h3>
          <table className="history">
            <thead><tr><th></th><th>메서드</th><th>URL</th><th>상태</th></tr></thead>
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

        <div>
          <h3 style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            UI 동작 ({uiActions.length})
            <button className="accent" disabled={active || replaying || uiActions.length === 0} onClick={replay}>
              {replaying ? '재생 중…' : '▶ 재생'}
            </button>
            <input placeholder="시나리오 이름" value={flowName} onChange={e => setFlowName(e.target.value)}
              style={{ width: 150, fontSize: 12 }} />
            <button onClick={saveFlow} disabled={uiActions.length === 0}>DB 저장</button>
            <button className="danger" onClick={() => { setUiActions([]); setReplayResults({}) }}
              disabled={uiActions.length === 0 || replaying}>전체 삭제</button>
          </h3>
          <p className="dim" style={{ marginTop: 0 }}>캡처 창에서 클릭·입력하면 기록됩니다. ↑↓로 순서, ✕로 삭제.</p>
          <table className="history">
            <thead><tr><th>#</th><th>동작</th><th>이름</th><th>셀렉터</th><th>값</th><th>결과</th><th>관리</th></tr></thead>
            <tbody>
              {uiActions.map((a, i) => (
                <tr key={a.id}>
                  <td>{i + 1}</td>
                  <td>{a.kind === 'click' ? '클릭' : '입력'}</td>
                  <td style={{ maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={a.name}>{a.name}</td>
                  <td className="dim" style={{ maxWidth: 180, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                    title={a.selectors.map(s => `${s.strategy}: ${s.value}`).join('\n')}>
                    {a.selectors[0] ? `${a.selectors[0].strategy}: ${a.selectors[0].value}` : ''}
                  </td>
                  <td>{a.value ?? ''}</td>
                  <td title={replayResults[i]?.detail || ''}>{resultIcon(i)}</td>
                  <td style={{ whiteSpace: 'nowrap' }}>
                    <button onClick={() => moveUi(i, -1)} disabled={i === 0} title="위로">↑</button>
                    <button onClick={() => moveUi(i, 1)} disabled={i === uiActions.length - 1} title="아래로">↓</button>
                    <button className="danger" onClick={() => delUi(i)} title="삭제">✕</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}
