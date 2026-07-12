import { Fragment, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'
import { save } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import { runDelegatedStep } from '../replayDelegate'
import { kindLabel, stepSummary } from '../uiStep'
import { mqSession } from '../mqLog'
import ApiCallsModal, { type CallLike } from '../components/ApiCallsModal'
import FlowTree from '../components/FlowTree'
import MqLogPanel from '../components/MqLogPanel'
import type { Environment, UiAction, UiStepResult, UiFlowRecord } from '../types'

interface SuiteItem {
  id: number
  name: string
  siteUrl: string
  grp: string
  actions: UiAction[]
  status: 'idle' | 'running' | 'passed' | 'failed'
  detail: string
  expanded: boolean
  stepResults: Record<number, { status: string; detail: string }>
}

const toItem = (f: UiFlowRecord): SuiteItem => ({
  id: f.id!, name: f.name, siteUrl: f.site_url, grp: f.grp || '',
  actions: JSON.parse(f.actions_json) as UiAction[],
  status: 'idle', detail: '', expanded: false, stepResults: {},
})

export default function UiSuiteView({ active }: { active?: boolean }) {
  const [allFlows, setAllFlows] = useState<UiFlowRecord[]>([])
  const [loadedLabel, setLoadedLabel] = useState('')
  const [items, setItems] = useState<SuiteItem[]>([])
  const [envs, setEnvs] = useState<Environment[]>([])
  // 이 화면의 환경 선택(화면별 독립, localStorage로 지속). MQ 연결 자체는 mqSession(공유).
  const [envId, setEnvId] = useState<number | null>(() => {
    const s = localStorage.getItem('runner.envId'); return s ? Number(s) : null
  })
  const setEnv = (v: number | null) => {
    setEnvId(v)
    if (v == null) localStorage.removeItem('runner.envId'); else localStorage.setItem('runner.envId', String(v))
  }
  const connectedEnv = useSyncExternalStore(mqSession.subscribe, mqSession.getEnvId)
  const [runningAll, setRunningAll] = useState(false)
  const [error, setError] = useState('')
  const [info, setInfo] = useState('')
  const [modalCalls, setModalCalls] = useState<{ title: string; calls: CallLike[] } | null>(null)

  const itemsRef = useRef<SuiteItem[]>([])
  useEffect(() => { itemsRef.current = items }, [items])
  const runningIdx = useRef<number | null>(null)
  const runIdRef = useRef<number | null>(null)
  const queue = useRef<number[]>([])
  const envIdRef = useRef<number | null>(null)
  useEffect(() => { envIdRef.current = envId }, [envId])

  const reloadFlows = () => api.listAllUiFlows().then(setAllFlows).catch(e => setError(String(e)))
  useEffect(() => {
    reloadFlows()
    api.listEnvironments().then(setEnvs).catch(() => {})
    // 앱 시작 시 저장된 환경 선택이 있으면 자동 연결해 연결/바인딩 정보를 바로 보여준다.
    if (envId != null && mqSession.getEnvId() !== envId) {
      mqSession.start(envId).catch(e => setError('MQ 연결 실패: ' + String(e)))
    }
    const h = () => reloadFlows() // 레코더에서 DB 저장 시 자동 갱신
    window.addEventListener('ui-flows-changed', h)
    // 상시 마운트(display 토글)라 환경 추가/수정 시 이벤트로 드롭다운 갱신
    const onEnvs = () => api.listEnvironments().then(setEnvs).catch(() => {})
    window.addEventListener('environments-changed', onEnvs)
    return () => {
      window.removeEventListener('ui-flows-changed', h)
      window.removeEventListener('environments-changed', onEnvs)
    }
    // MQ 세션은 전역 유지(탭 전환에도 로그·연결 보존) — 언마운트에서 끊지 않는다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const finishItem = (i: number, status: 'passed' | 'failed', detail: string) => {
    setItems(list => list.map((x, j) => (j === i ? { ...x, status, detail } : x)))
    const runId = runIdRef.current
    if (runId != null) { api.finishUiRun(runId, status).catch(() => {}); window.dispatchEvent(new CustomEvent('ui-run-finished')) }
    runningIdx.current = null; runIdRef.current = null
    if (queue.current.length) { const next = queue.current.shift()!; startItem(next) }
    else setRunningAll(false)
  }
  const startItem = async (i: number) => {
    const it = itemsRef.current[i]
    if (!it) return
    runningIdx.current = i
    setItems(list => list.map((x, j) => (j === i ? { ...x, status: 'running', detail: '', stepResults: {} } : x)))
    const url = it.actions.find(a => a.url)?.url || it.siteUrl
    if (!url) { finishItem(i, 'failed', '시작 URL이 없습니다'); return }
    try {
      runIdRef.current = await api.createUiRun(it.id, it.name, it.siteUrl)
      await api.startUiReplay(url, it.actions)
    } catch (e) { finishItem(i, 'failed', String(e)) }
  }

  useEffect(() => {
    const un = listen<UiStepResult>('ui-replay-step', e => {
      const r = e.payload
      const i = runningIdx.current
      if (i == null) return
      if (r.status === 'delegate') { runDelegatedStep(r.index, r.detail, envIdRef.current); return }
      if (r.done) { finishItem(i, r.status, r.detail); return }
      setItems(list => list.map((x, j) =>
        j === i ? { ...x, stepResults: { ...x.stepResults, [r.index]: { status: r.status, detail: r.detail } } } : x))
      const it = itemsRef.current[i]
      const runId = runIdRef.current
      if (it && runId != null && r.index >= 0) {
        const a = it.actions[r.index]
        api.saveUiRunStep(runId, r.index, a?.kind || '', a?.name || '', r.status, r.detail).catch(() => {})
      }
    })
    return () => { un.then(u => u()) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const pickFlow = (f: UiFlowRecord) => { setItems([toItem(f)]); setLoadedLabel(f.name); setError('') }
  const pickMany = (flows: UiFlowRecord[], label: string) => { setItems(flows.map(toItem)); setLoadedLabel(label); setError('') }
  const renameFlow = async (f: UiFlowRecord, newName: string) => {
    try { await api.renameUiFlow(f.id!, newName); await reloadFlows(); window.dispatchEvent(new CustomEvent('ui-flows-changed')) } catch (e) { setError(String(e)) }
  }
  const renameGroup = async (site: string, oldGroup: string, newGroup: string) => {
    try { await api.renameUiGroup(site, oldGroup, newGroup); await reloadFlows(); window.dispatchEvent(new CustomEvent('ui-flows-changed')) } catch (e) { setError(String(e)) }
  }

  const changeEnv = async (v: number | null) => {
    setError(''); setEnv(v)
    try {
      if (v != null) await mqSession.start(v)
      else await mqSession.stop()
    } catch (e) { setError('MQ 연결 실패: ' + String(e)) }
  }

  const move = (i: number, d: -1 | 1) => {
    const j = i + d
    if (j < 0 || j >= items.length) return
    setItems(list => { const n = [...list]; [n[i], n[j]] = [n[j], n[i]]; return n })
  }
  const remove = async (i: number) => {
    const it = items[i]
    if (!window.confirm(`"${it.name}" 시나리오를 DB에서 삭제할까요?`)) return
    try {
      await api.deleteUiFlow(it.id)
      setItems(list => list.filter((_, j) => j !== i))
      reloadFlows()
      window.dispatchEvent(new CustomEvent('ui-flows-changed'))
    } catch (e) { setError(String(e)) }
  }
  const toggleExpand = (i: number) => setItems(list => list.map((x, j) => (j === i ? { ...x, expanded: !x.expanded } : x)))

  const delAction = async (i: number, k: number) => {
    const it = itemsRef.current[i]
    if (!it) return
    const next = it.actions.filter((_, x) => x !== k)
    setItems(list => list.map((y, j) => (j === i ? { ...y, actions: next, stepResults: {} } : y)))
    try { await api.saveUiFlow(it.name, it.siteUrl, it.grp, next) } catch (e) { setError(String(e)) }
  }

  const cancelRun = async () => {
    queue.current = []
    const i = runningIdx.current
    runningIdx.current = null; runIdRef.current = null
    setRunningAll(false)
    try { await api.stopUiReplay() } catch { /* noop */ }
    if (i != null) setItems(list => list.map((x, j) => (j === i ? { ...x, status: 'idle', detail: '취소됨' } : x)))
  }

  // wait_event 스텝이 있으면 MQ 세션 필요 → 환경 확인/연결.
  const ensureMq = async (idxs: number[]): Promise<boolean> => {
    const needsMq = idxs.some(i => itemsRef.current[i]?.actions.some(a => a.kind === 'wait_event'))
    if (!needsMq) return true
    if (envId == null) { setError('wait_event 스텝이 있어 환경(MQ) 선택이 필요합니다'); return false }
    // 공유 연결이 이 환경이 아니면(다른 화면이 중단/변경했을 수 있음) 다시 연결한다.
    if (mqSession.getEnvId() !== envId) {
      try { await mqSession.start(envId) } catch (e) { setError('MQ 연결 실패: ' + String(e)); return false }
    }
    return true
  }

  const runAll = async () => {
    if (!items.length || runningIdx.current != null) return
    setError('')
    if (!(await ensureMq(items.map((_, i) => i)))) return
    setItems(list => list.map(x => ({ ...x, status: 'idle', detail: '', stepResults: {} })))
    queue.current = items.map((_, i) => i)
    setRunningAll(true)
    startItem(queue.current.shift()!)
  }
  const runOne = async (i: number) => {
    if (runningIdx.current != null) return
    queue.current = []; setRunningAll(false); setError('')
    if (!(await ensureMq([i]))) return
    startItem(i)
  }

  const doExport = async () => {
    setError(''); setInfo('')
    const path = await save({ defaultPath: 'recap-ui-flows.json', filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (path) { try { await api.exportUiFlows(path); setInfo('DB의 UI 플로우를 파일로 내보냈습니다.') } catch (e) { setError(String(e)) } }
  }

  const busy = runningIdx.current != null || runningAll
  const fileIcon = (s: SuiteItem['status']) => ({ idle: '—', running: '⏳', passed: '✅', failed: '❌' })[s]
  const stepIcon = (it: SuiteItem, k: number) => {
    const r = it.stepResults[k]
    if (r) return r.status === 'passed' ? '✅' : '❌'
    return it.status === 'running' ? '⏳' : ''
  }
  void active

  return (
    <div>
      <h2>시나리오 실행</h2>
      <p className="dim">왼쪽 트리에서 사이트·그룹·시나리오를 골라 불러온 뒤 개별/전체 실행합니다. 실행 결과는 히스토리 탭에 기록됩니다.</p>
      <div className="add-row">
        <select value={envId ?? ''} onChange={e => changeEnv(e.target.value ? Number(e.target.value) : null)} disabled={busy} title="wait_event·RabbitMQ 로그용 환경">
          <option value="">환경 없음</option>
          {envs.map(en => <option key={en.id} value={en.id!}>{en.name}</option>)}
        </select>
        {envId != null && (connectedEnv === envId
          ? <span className="dim">RabbitMQ 연결됨</span>
          : <span className="dim">연결 대기(실행 시 연결)</span>)}
        <span style={{ flex: 1 }} />
        <button onClick={reloadFlows} disabled={busy}>트리 새로고침</button>
        <button onClick={doExport} disabled={busy}>DB 내보내기</button>
      </div>

      {error && <p className="error">{error}</p>}
      {info && <p className="dim">{info}</p>}

      <div className="two-col" style={{ gridTemplateColumns: '280px 1fr', alignItems: 'start', gap: 16 }}>
        {/* 좌측 트리 */}
        <div style={{ borderRight: '1px solid var(--vsc-border)', paddingRight: 12 }}>
          <strong style={{ fontSize: 13 }}>시나리오 트리</strong>
          <p className="dim" style={{ fontSize: 11, margin: '4px 0' }}>리프 클릭=단일, ▶=그룹/사이트 전체 불러오기</p>
          <FlowTree flows={allFlows} onPickFlow={pickFlow} onPickMany={pickMany}
            onRenameFlow={renameFlow} onRenameGroup={renameGroup} />
        </div>

        {/* 우측 실행 목록 */}
        <div>
          <div className="add-row">
            <strong>{loadedLabel ? `불러옴: ${loadedLabel} (${items.length})` : '시나리오를 트리에서 불러오세요'}</strong>
            <button className="accent" onClick={runAll} disabled={!items.length || busy}>
              {runningAll ? '전체 실행 중…' : '▶ 전체 실행'}
            </button>
            {busy && <button className="danger" onClick={cancelRun}>취소</button>}
          </div>

          <table className="history">
            <thead>
              <tr><th></th><th>#</th><th>시나리오</th><th>동작수</th><th>상태</th><th>세부</th><th>실행</th><th>순서/삭제</th></tr>
            </thead>
            <tbody>
              {items.map((it, i) => (
                <Fragment key={it.id}>
                  <tr>
                    <td><button onClick={() => toggleExpand(i)} title="펼치기">{it.expanded ? '▾' : '▸'}</button></td>
                    <td>{i + 1}</td>
                    <td style={{ cursor: 'pointer' }} onClick={() => toggleExpand(i)}>{it.name}</td>
                    <td>{it.actions.length}</td>
                    <td>{fileIcon(it.status)}</td>
                    <td className="dim" title={it.detail} style={{ maxWidth: 220, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{it.detail}</td>
                    <td><button onClick={() => runOne(i)} disabled={busy}>개별 실행</button></td>
                    <td style={{ whiteSpace: 'nowrap' }}>
                      <button onClick={() => move(i, -1)} disabled={i === 0 || busy}>↑</button>
                      <button onClick={() => move(i, 1)} disabled={i === items.length - 1 || busy}>↓</button>
                      <button className="danger" onClick={() => remove(i)} disabled={busy}>✕</button>
                    </td>
                  </tr>
                  {it.expanded && (
                    <tr>
                      <td></td>
                      <td colSpan={7} style={{ background: 'var(--vsc-bg-alt)' }}>
                        <table className="history" style={{ margin: 0 }}>
                          <thead><tr><th>#</th><th>동작</th><th>이름</th><th>셀렉터/설정</th><th>값</th><th>API</th><th>결과</th><th>삭제</th></tr></thead>
                          <tbody>
                            {it.actions.map((a, k) => (
                              <tr key={a.id + k}>
                                <td>{k + 1}</td>
                                <td>{kindLabel(a.kind)}</td>
                                <td style={{ maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={a.name}>{a.name}</td>
                                <td className="dim" style={{ maxWidth: 220, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                                  title={a.selectors.map(s => `${s.strategy}: ${s.value}`).join('\n') || stepSummary(a)}>
                                  {stepSummary(a)}
                                </td>
                                <td>{a.value ?? ''}</td>
                                <td>{(a.api?.length || 0) > 0
                                  ? <button onClick={() => setModalCalls({ title: a.name || `동작 ${k + 1}`, calls: a.api as CallLike[] })} title="유발된 네트워크 호출 보기">▸ {a.api!.length}</button>
                                  : <span className="dim">0</span>}</td>
                                <td title={it.stepResults[k]?.detail || ''}>{stepIcon(it, k)}</td>
                                <td><button className="danger" onClick={() => delAction(i, k)} disabled={busy}>✕</button></td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {envId != null && <MqLogPanel storageKey="runner" onConnected={() => setError('')} />}

      {modalCalls && <ApiCallsModal title={modalCalls.title} calls={modalCalls.calls} onClose={() => setModalCalls(null)} />}
    </div>
  )
}
