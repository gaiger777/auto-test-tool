import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import { correlateCalls, type CapturedCall } from '../capture'
import { runDelegatedStep } from '../replayDelegate'
import { mqSessionFor } from '../mqLog'
import MqLogPanel from '../components/MqLogPanel'

const mqSession = mqSessionFor('capture') // 이 화면 전용 독립 MQ 세션
import { kindLabel, stepSummary } from '../uiStep'
import ApiCallsModal, { type CallLike } from '../components/ApiCallsModal'
import ProgStepAdder from '../components/ProgStepAdder'
import FlowTree, { groupOf } from '../components/FlowTree'
import type { Environment, UiAction, UiStepResult, UiFlowRecord } from '../types'

export default function CaptureView() {
  const [url, setUrl] = useState('')
  const [active, setActive] = useState(false)
  const [recording, setRecording] = useState(false)
  const [calls, setCalls] = useState<CapturedCall[]>([]) // 화면엔 안 보이지만 동작별 API 상관에 사용
  const [uiActions, setUiActions] = useState<UiAction[]>([])
  const [replaying, setReplaying] = useState(false)
  const [replayResults, setReplayResults] = useState<Record<number, { status: string; detail: string }>>({})
  const [flowName, setFlowName] = useState('')
  const [group, setGroup] = useState('')
  const [loadedFlowId, setLoadedFlowId] = useState<number | null>(null)
  const [allFlows, setAllFlows] = useState<UiFlowRecord[]>([])
  const [envs, setEnvs] = useState<Environment[]>([])
  const [envId, setEnvId] = useState<number | null>(null)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [modalCalls, setModalCalls] = useState<{ title: string; calls: CallLike[] } | null>(null)
  const startedAt = useRef(0)
  const envIdRef = useRef<number | null>(null)
  useEffect(() => { envIdRef.current = envId }, [envId])
  const connectedEnv = useSyncExternalStore(mqSession.subscribe, mqSession.getEnvId)

  // 이 화면의 RabbitMQ 로그(연결은 전역 1개 — 마지막에 켠 화면이 소유)
  const startLog = async () => {
    if (envId == null) { setError('환경을 먼저 선택하세요'); return }
    setError('')
    try { await mqSession.start(envId) } catch (e) { setError('RabbitMQ 연결 실패: ' + String(e)) }
  }
  const stopLog = async () => { try { await mqSession.stop() } catch { /* noop */ } }
  const replayingRef = useRef(false)
  useEffect(() => { replayingRef.current = replaying }, [replaying])

  const corr = useMemo(() => correlateCalls(uiActions, calls), [uiActions, calls])
  const knownGroups = useMemo(() => [...new Set(allFlows.map(groupOf))], [allFlows])

  const reloadFlows = () => api.listAllUiFlows().then(setAllFlows).catch(() => {})

  useEffect(() => {
    api.captureSessionActive().then(setActive).catch(() => {})
    api.listEnvironments().then(setEnvs).catch(() => {})
    reloadFlows()
    const unRec = listen<CapturedCall>('capture-recorded', e => setCalls(prev => [e.payload, ...prev]))
    const unUi = listen<UiAction>('ui-recorded', e => setUiActions(prev => [...prev, e.payload]))
    const unReplay = listen<UiStepResult>('ui-replay-step', e => {
      if (!replayingRef.current) return // 다른 화면이 시작한 재생은 무시
      const r = e.payload
      if (r.status === 'delegate') { runDelegatedStep(r.index, r.detail, envIdRef.current, 'capture'); return }
      if (r.done) {
        setReplaying(false) // MQ 로그 연결은 이 화면 소유 — 재생 끝나도 끊지 않음(사용자가 ■ 로그 중단)
        setNotice(r.status === 'passed' ? 'UI 재생 완료' : `UI 재생 중단: ${r.detail}`)
      } else setReplayResults(prev => ({ ...prev, [r.index]: { status: r.status, detail: r.detail } }))
    })
    const unEnd = listen('capture-session-ended', () => {
      setActive(false); setReplaying(false); setRecording(false)
      setNotice('세션이 종료되었습니다. 목록은 유지됩니다.')
    })
    const onFlows = () => reloadFlows()
    window.addEventListener('ui-flows-changed', onFlows)
    // 이 화면은 상시 마운트(display 토글)라 환경 추가/수정 시 이벤트로 드롭다운 갱신
    const onEnvs = () => api.listEnvironments().then(setEnvs).catch(() => {})
    window.addEventListener('environments-changed', onEnvs)
    return () => {
      unRec.then(u => u()); unUi.then(u => u()); unReplay.then(u => u()); unEnd.then(u => u())
      window.removeEventListener('ui-flows-changed', onFlows)
      window.removeEventListener('environments-changed', onEnvs)
    }
  }, [])

  const start = async () => {
    setError(''); setNotice('')
    if (!url) { setError('대상 URL을 입력하세요'); return }
    try {
      await api.startCaptureSession(url)
      setActive(true)
      setRecording(false) // 레코드는 꺼진 채 시작 — 로그인 등은 사용자가 레코드 시작 전까지 기록 안 됨
      startedAt.current = Date.now()
      setCalls([]); setReplayResults({})
    } catch (e) { setError(String(e)) }
  }

  const toggleRecord = async () => {
    const next = !recording
    try { await api.setUiRecording(next); setRecording(next) }
    catch (e) { setError(String(e)) }
  }

  const stop = async () => {
    try {
      await api.stopCaptureSession()
      setActive(false); setRecording(false)
      if (calls.length === 0 && uiActions.length === 0 && Date.now() - startedAt.current > 3000) {
        setNotice('캡처가 0건입니다. 대상 사이트의 CSP로 후킹이 차단됐을 수 있습니다.')
      }
    } catch (e) { setError(String(e)) }
  }

  const replay = async () => {
    setError(''); setNotice(''); setReplayResults({})
    if (uiActions.length === 0) { setError('재생할 UI 동작이 없습니다'); return }
    const needsMq = uiActions.some(a => a.kind === 'wait_event')
    if (needsMq && envId == null) { setError('wait_event 스텝이 있어 환경(MQ) 선택이 필요합니다'); return }
    setReplaying(true)
    if (needsMq && envId != null && mqSession.getEnvId() !== envId) {
      try { await mqSession.start(envId) }
      catch (e) { setReplaying(false); setError('MQ 연결 실패: ' + String(e)); return }
    }
    const startUrl = uiActions.find(a => a.url)?.url || url
    if (!startUrl) { setReplaying(false); setError('시작 URL이 없습니다. 대상 사이트 URL을 입력하세요'); return }
    try { await api.startUiReplay(startUrl, uiActions) }
    catch (e) { setReplaying(false); setError(String(e)) }
  }
  const cancelReplay = async () => {
    try { await api.stopUiReplay() } catch { /* noop */ }
    setReplaying(false) // MQ 로그 연결은 유지(이 화면 소유)
  }

  const newScenario = () => {
    setUiActions([]); setReplayResults({}); setFlowName(''); setLoadedFlowId(null); setNotice('새 시나리오 — 세션을 시작해 기록하거나 스텝을 추가하세요.')
  }
  const addStep = (a: UiAction) => { setUiActions(prev => [...prev, a]); setReplayResults({}) }
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
        setNotice(`${n}개 플로우를 DB로 가져왔습니다. 왼쪽 트리에서 선택하세요.`)
      } catch (e) { setError(String(e)) }
    }
  }

  const deleteLoaded = async () => {
    setError(''); setNotice('')
    const f = allFlows.find(x => x.id === loadedFlowId)
    if (!f) { setError('삭제할 시나리오를 트리에서 먼저 선택하세요'); return }
    if (!window.confirm(`"${f.name}" 시나리오를 DB에서 삭제할까요?`)) return
    try {
      await api.deleteUiFlow(f.id!)
      setLoadedFlowId(null); newScenario()
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
      setNotice(`"${f.name}" 삭제됨`)
    } catch (e) { setError(String(e)) }
  }

  const renameFlow = async (f: UiFlowRecord, newName: string) => {
    setError('')
    try {
      await api.renameUiFlow(f.id!, newName)
      if (loadedFlowId === f.id) setFlowName(newName)
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
    } catch (e) { setError(String(e)) }
  }
  const renameGroup = async (site: string, oldGroup: string, newGroup: string) => {
    setError('')
    try {
      await api.renameUiGroup(site, oldGroup, newGroup)
      if (loadedFlowId != null && (group || '') === oldGroup) setGroup(newGroup)
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
    } catch (e) { setError(String(e)) }
  }
  // 트리에서 URL/그룹/시나리오 삭제(DB). 확인 후 일괄 삭제.
  const deleteFlows = async (flows: UiFlowRecord[], label: string) => {
    setError('')
    if (!flows.length) return
    if (!window.confirm(`'${label}' 시나리오 ${flows.length}개를 DB에서 삭제할까요?`)) return
    try {
      for (const f of flows) await api.deleteUiFlow(f.id!)
      if (loadedFlowId != null && flows.some(f => f.id === loadedFlowId)) setLoadedFlowId(null)
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
    } catch (e) { setError(String(e)) }
  }

  const loadFlow = (f: UiFlowRecord) => {
    setError(''); setNotice('')
    try {
      setUiActions(JSON.parse(f.actions_json) as UiAction[])
      setFlowName(f.name); setGroup(f.grp || ''); setUrl(f.site_url); setLoadedFlowId(f.id ?? null); setReplayResults({})
      setNotice(`"${f.name}" 불러옴 — 수정 후 DB 저장하면 덮어씁니다.`)
    } catch (e) { setError(String(e)) }
  }

  const saveFlow = async () => {
    setError(''); setNotice('')
    if (uiActions.length === 0) { setError('저장할 UI 동작이 없습니다'); return }
    if (!flowName.trim()) { setError('시나리오 이름을 입력하세요'); return }
    const siteUrl = (url || uiActions.find(a => a.url)?.url || '').replace(/\/+$/, '')
    if (!siteUrl) { setError('사이트 URL이 없습니다'); return }
    const grpNorm = group.trim() === '기본' ? '' : group.trim()
    const dup = allFlows.find(f => f.site_url.replace(/\/+$/, '') === siteUrl && (f.grp || '') === grpNorm && f.name === flowName.trim())
    if (dup && !window.confirm(`"${grpNorm || '기본'}" 그룹에 "${flowName.trim()}" 시나리오가 이미 있습니다. 덮어쓸까요?`)) return
    const withApi: UiAction[] = uiActions.map(a => {
      const linked = corr[a.id]?.length ? corr[a.id] : (a.api || [])
      return {
        ...a,
        api: linked.map(c => ({
          method: c.method, url: c.url, status: c.status,
          request_headers: (c as CapturedCall).request_headers ?? {},
          request_body: (c as CapturedCall).request_body ?? null,
        })),
      }
    })
    try {
      await api.saveUiFlow(flowName.trim(), siteUrl, group.trim(), withApi)
      setNotice(`시나리오 "${flowName.trim()}" 저장됨 · 그룹 ${group.trim() || '기본'} · ${siteUrl}`)
      await reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
    } catch (e) { setError(String(e)) }
  }

  const resultIcon = (i: number) => {
    const r = replayResults[i]
    if (r) return r.status === 'passed' ? '✅' : '❌'
    return replaying ? '⏳' : ''
  }

  return (
    <div>
      <h2>UI 레코더</h2>
      <p className="dim">세션을 시작해 클릭·입력·호버를 기록하고, http_call·wait_event·assert·sleep 스텝을 끼워넣어 그룹별로 저장·재생합니다.</p>
      <div className="two-col" style={{ gridTemplateColumns: '280px 1fr', alignItems: 'start', gap: 16 }}>
        {/* 좌측: 시나리오 트리 */}
        <div style={{ borderRight: '1px solid var(--vsc-border)', paddingRight: 12 }}>
          <div className="add-row" style={{ marginBottom: 8 }}>
            <strong style={{ fontSize: 13 }}>시나리오</strong>
            <button onClick={newScenario}>+ 새 시나리오</button>
          </div>
          <FlowTree flows={allFlows} selectedId={loadedFlowId} onPickFlow={loadFlow}
            onRenameFlow={renameFlow} onRenameGroup={renameGroup} onDelete={deleteFlows} />
          <div className="add-row" style={{ marginTop: 8 }}>
            <button onClick={doImport} disabled={active || replaying}>DB 가져오기</button>
          </div>
        </div>

        {/* 우측: 편집기 */}
        <div>
          <div className="add-row">
            <input placeholder="대상 사이트 URL (https://...)" value={url}
              onChange={e => setUrl(e.target.value)} disabled={active || replaying} style={{ minWidth: 300 }} />
            {!active
              ? <button className="accent" onClick={start} disabled={replaying}>세션 시작</button>
              : <button className="danger" onClick={stop}>세션 종료</button>}
            {active && (recording
              ? <button className="danger" onClick={toggleRecord}>■ 레코드 정지</button>
              : <button className="accent" onClick={toggleRecord}>● 레코드 시작</button>)}
            {active && <span className="dim">{recording ? '레코드 중 — 클릭/입력이 기록됩니다' : '세션 열림 — 레코드 시작 전까지 UI 동작은 기록 안 됨'}</span>}
          </div>

          {error && <p className="error">{error}</p>}
          {notice && <p className="dim">{notice}</p>}

          <h3 style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            UI 동작 ({uiActions.length})
            <button className="accent" disabled={active || replaying || uiActions.length === 0} onClick={replay}>
              {replaying ? '재생 중…' : '▶ 재생'}
            </button>
            {replaying && <button className="danger" onClick={cancelReplay}>취소</button>}
            <select value={envId ?? ''} onChange={e => setEnvId(e.target.value ? Number(e.target.value) : null)}
              disabled={replaying} title="wait_event 스텝·RabbitMQ 로그용 환경">
              <option value="">환경 없음</option>
              {envs.map(en => <option key={en.id} value={en.id!}>{en.name}</option>)}
            </select>
            {envId != null && (connectedEnv === envId
              ? <button className="danger" onClick={stopLog}>■ 로그 중단</button>
              : <button onClick={startLog}>▶ 로그</button>)}
          </h3>
          <div className="add-row">
            <input placeholder="그룹 (예: 로그인)" value={group} onChange={e => setGroup(e.target.value)} list="grp-list" style={{ width: 140 }} />
            <datalist id="grp-list">{knownGroups.map(g => <option key={g} value={g} />)}</datalist>
            <input placeholder="시나리오 이름" value={flowName} onChange={e => setFlowName(e.target.value)} style={{ width: 160 }} />
            <button className="accent" onClick={saveFlow} disabled={uiActions.length === 0}>DB 저장</button>
            <button className="danger" onClick={deleteLoaded} disabled={loadedFlowId == null}>선택 삭제</button>
            <button className="danger" onClick={() => { setUiActions([]); setReplayResults({}) }}
              disabled={uiActions.length === 0 || replaying}>동작 비우기</button>
          </div>

          <table className="history">
            <thead><tr><th>#</th><th>동작</th><th>이름</th><th>셀렉터/설정</th><th>값</th><th>API</th><th>결과</th><th>관리</th></tr></thead>
            <tbody>
              {uiActions.map((a, i) => {
                const linked = corr[a.id]?.length ? corr[a.id] : (a.api || [])
                return (
                  <tr key={a.id}>
                    <td>{i + 1}</td>
                    <td>{kindLabel(a.kind)}</td>
                    <td style={{ maxWidth: 150, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={a.name}>{a.name}</td>
                    <td className="dim" style={{ maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                      title={a.selectors.map(s => `${s.strategy}: ${s.value}`).join('\n') || stepSummary(a)}>
                      {stepSummary(a)}
                    </td>
                    <td>{a.value ?? ''}</td>
                    <td>{linked.length > 0
                      ? <button onClick={() => setModalCalls({ title: a.name || `동작 ${i + 1}`, calls: linked as CallLike[] })} title="유발된 네트워크 호출 보기">▸ {linked.length}</button>
                      : <span className="dim">0</span>}</td>
                    <td title={replayResults[i]?.detail || ''}>{resultIcon(i)}</td>
                    <td style={{ whiteSpace: 'nowrap' }}>
                      <button onClick={() => moveUi(i, -1)} disabled={i === 0}>↑</button>
                      <button onClick={() => moveUi(i, 1)} disabled={i === uiActions.length - 1}>↓</button>
                      <button className="danger" onClick={() => delUi(i)}>✕</button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>

          <ProgStepAdder onAdd={addStep} />
        </div>
      </div>

      {modalCalls && <ApiCallsModal title={modalCalls.title} calls={modalCalls.calls} onClose={() => setModalCalls(null)} />}

      {envId != null && <MqLogPanel storageKey="capture" onConnected={() => setError('')} />}
    </div>
  )
}
