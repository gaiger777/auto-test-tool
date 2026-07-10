import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import { capturesToSteps, correlateCalls, type CapturedCall } from '../capture'
import type { ScenarioRecord, UiAction, UiStepResult, UiFlowRecord } from '../types'

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
  const [selectedFlowId, setSelectedFlowId] = useState('')
  const [allFlows, setAllFlows] = useState<UiFlowRecord[]>([])
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [openUi, setOpenUi] = useState<Record<string, boolean>>({})
  const startedAt = useRef(0)

  // 각 UI 동작이 유발한 네트워크 호출을 timestamp로 묶는다 (상관보기).
  const corr = useMemo(() => correlateCalls(uiActions, calls), [uiActions, calls])

  const reloadFlows = () => api.listAllUiFlows().then(setAllFlows).catch(() => {})

  useEffect(() => {
    api.captureSessionActive().then(setActive).catch(() => {})
    reloadFlows()
    const unRec = listen<CapturedCall>('capture-recorded', e => setCalls(prev => [e.payload, ...prev]))
    const unUi = listen<UiAction>('ui-recorded', e => setUiActions(prev => [...prev, e.payload]))
    const unReplay = listen<UiStepResult>('ui-replay-step', e => {
      const r = e.payload
      if (r.done) { setReplaying(false); setNotice(r.status === 'passed' ? 'UI 재생 완료' : `UI 재생 중단: ${r.detail}`) }
      else setReplayResults(prev => ({ ...prev, [r.index]: { status: r.status, detail: r.detail } }))
    })
    const unEnd = listen('capture-session-ended', () => {
      setActive(false); setReplaying(false)
      setNotice('세션이 종료되었습니다. 목록은 유지됩니다.')
    })
    const onFlows = () => reloadFlows() // 스위트 등 다른 화면에서 DB 변경 시 드롭다운 갱신
    window.addEventListener('ui-flows-changed', onFlows)
    return () => {
      unRec.then(u => u()); unUi.then(u => u()); unReplay.then(u => u()); unEnd.then(u => u())
      window.removeEventListener('ui-flows-changed', onFlows)
    }
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
  const cancelReplay = async () => { try { await api.stopUiReplay() } catch { /* noop */ } setReplaying(false) }

  const delUi = (i: number) => { setUiActions(a => a.filter((_, j) => j !== i)); setReplayResults({}) }
  const moveUi = (i: number, d: -1 | 1) => {
    const j = i + d
    if (j < 0 || j >= uiActions.length) return
    setUiActions(a => { const n = [...a]; [n[i], n[j]] = [n[j], n[i]]; return n }); setReplayResults({})
  }

  const doImport = async () => {
    setError(''); setNotice('')
    const path = await open({ multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (typeof path === 'string') {
      try {
        const n = await api.importUiFlows(path)
        await reloadFlows()
        window.dispatchEvent(new CustomEvent('ui-flows-changed'))
        setNotice(`${n}개 플로우를 DB로 가져왔습니다. "저장된 시나리오 불러오기"에서 선택하세요.`)
      } catch (e) { setError(String(e)) }
    }
  }

  const deleteSelectedFlow = async () => {
    setError(''); setNotice('')
    const f = allFlows.find(x => String(x.id) === selectedFlowId)
    if (!f) return
    if (!window.confirm(`"${f.name}" 시나리오를 DB에서 삭제할까요?`)) return
    try {
      await api.deleteUiFlow(f.id!)
      setSelectedFlowId('')
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
      setNotice(`"${f.name}" 삭제됨`)
    } catch (e) { setError(String(e)) }
  }

  const loadFlow = (f: UiFlowRecord) => {
    setError(''); setNotice('')
    try {
      setUiActions(JSON.parse(f.actions_json) as UiAction[])
      setFlowName(f.name); setUrl(f.site_url); setReplayResults({})
      setNotice(`"${f.name}" 불러옴 — 수정 후 DB 저장하면 덮어씁니다.`)
    } catch (e) { setError(String(e)) }
  }

  const saveFlow = async () => {
    setError(''); setNotice('')
    if (uiActions.length === 0) { setError('저장할 UI 동작이 없습니다'); return }
    if (!flowName.trim()) { setError('시나리오 이름을 입력하세요'); return }
    const siteUrl = (url || uiActions[0]?.url || '').replace(/\/+$/, '')
    if (!siteUrl) { setError('사이트 URL이 없습니다'); return }
    const dup = allFlows.find(f => f.site_url.replace(/\/+$/, '') === siteUrl && f.name === flowName.trim())
    if (dup && !window.confirm(`"${flowName.trim()}" 시나리오가 이미 있습니다. 덮어쓸까요?`)) return
    // 각 동작이 유발한 네트워크 호출(상관 결과)을 함께 저장 → 스위트에서 API 표시.
    const withApi: UiAction[] = uiActions.map(a => {
      const linked = corr[a.id]?.length ? corr[a.id] : (a.api || [])
      return { ...a, api: linked.map(c => ({ method: c.method, url: c.url, status: c.status })) }
    })
    try {
      await api.saveUiFlow(flowName.trim(), siteUrl, withApi)
      setNotice(`시나리오 "${flowName.trim()}" DB 저장됨 · 사이트 ${siteUrl}`)
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
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
        <input placeholder="토큰 헤더명" value={tokenHeader} onChange={e => setTokenHeader(e.target.value)} />
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
                  <td>{c.method}</td><td>{c.url}</td><td>{c.status}</td>
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
            {replaying && <button className="danger" onClick={cancelReplay}>취소</button>}
            <input placeholder="시나리오 이름" value={flowName} onChange={e => setFlowName(e.target.value)}
              style={{ width: 150, fontSize: 12 }} />
            <button onClick={saveFlow} disabled={uiActions.length === 0}>DB 저장</button>
            <button className="danger" onClick={() => { setUiActions([]); setReplayResults({}) }}
              disabled={uiActions.length === 0 || replaying}>전체 삭제</button>
          </h3>
          <div className="add-row">
            <select value={selectedFlowId} disabled={active || replaying}
              onChange={e => { setSelectedFlowId(e.target.value); const f = allFlows.find(x => String(x.id) === e.target.value); if (f) loadFlow(f) }}>
              <option value="">저장된 시나리오 불러오기…</option>
              {allFlows.map(f => <option key={f.id} value={f.id!}>{f.site_url} — {f.name}</option>)}
            </select>
            <button className="danger" onClick={deleteSelectedFlow} disabled={!selectedFlowId || active || replaying}>삭제</button>
            <button onClick={doImport} disabled={active || replaying}>DB 가져오기</button>
          </div>
          <table className="history">
            <thead><tr><th>#</th><th>동작</th><th>이름</th><th>셀렉터</th><th>값</th><th>API</th><th>결과</th><th>관리</th></tr></thead>
            <tbody>
              {uiActions.map((a, i) => {
                const linked = corr[a.id]?.length ? corr[a.id] : (a.api || [])
                return (
                <Fragment key={a.id}>
                <tr>
                  <td>{i + 1}</td>
                  <td>{a.kind === 'click' ? '클릭' : a.kind === 'input' ? '입력' : '호버'}</td>
                  <td style={{ maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={a.name}>{a.name}</td>
                  <td className="dim" style={{ maxWidth: 180, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                    title={a.selectors.map(s => `${s.strategy}: ${s.value}`).join('\n')}>
                    {a.selectors[0] ? `${a.selectors[0].strategy}: ${a.selectors[0].value}` : ''}
                  </td>
                  <td>{a.value ?? ''}</td>
                  <td>{linked.length > 0
                    ? <button onClick={() => setOpenUi(s => ({ ...s, [a.id]: !s[a.id] }))} title="유발된 네트워크 호출 보기">
                        {openUi[a.id] ? '▾' : '▸'} {linked.length}
                      </button>
                    : <span className="dim">0</span>}</td>
                  <td title={replayResults[i]?.detail || ''}>{resultIcon(i)}</td>
                  <td style={{ whiteSpace: 'nowrap' }}>
                    <button onClick={() => moveUi(i, -1)} disabled={i === 0}>↑</button>
                    <button onClick={() => moveUi(i, 1)} disabled={i === uiActions.length - 1}>↓</button>
                    <button className="danger" onClick={() => delUi(i)}>✕</button>
                  </td>
                </tr>
                {openUi[a.id] && linked.length > 0 && (
                  <tr>
                    <td></td>
                    <td colSpan={7} style={{ background: 'var(--vsc-bg-alt)' }}>
                      <table className="history" style={{ margin: 0 }}>
                        <thead><tr><th>메서드</th><th>URL</th><th>상태</th></tr></thead>
                        <tbody>
                          {linked.map((c, ci) => (
                            <tr key={ci}>
                              <td>{c.method}</td>
                              <td style={{ maxWidth: 320, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={c.url}>{c.url}</td>
                              <td>{c.status}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </td>
                  </tr>
                )}
                </Fragment>
              )})}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}
