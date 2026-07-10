import { Fragment, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import type { UiAction, UiStepResult, UiFlowSite } from '../types'

interface SuiteItem {
  id: number
  name: string
  siteUrl: string
  actions: UiAction[]
  status: 'idle' | 'running' | 'passed' | 'failed'
  detail: string
  expanded: boolean
  stepResults: Record<number, { status: string; detail: string }>
}

export default function UiSuiteView() {
  const [sites, setSites] = useState<UiFlowSite[]>([])
  const [selectedSite, setSelectedSite] = useState('')
  const [items, setItems] = useState<SuiteItem[]>([])
  const [runningAll, setRunningAll] = useState(false)
  const [error, setError] = useState('')
  const [info, setInfo] = useState('')

  const itemsRef = useRef<SuiteItem[]>([])
  useEffect(() => { itemsRef.current = items }, [items])
  const runningIdx = useRef<number | null>(null)
  const queue = useRef<number[]>([])

  const loadSites = async () => {
    try { setSites(await api.listUiFlowSites()) } catch (e) { setError(String(e)) }
  }
  useEffect(() => {
    loadSites()
    const h = () => loadSites() // 캡처 탭에서 DB 저장 시 자동 갱신
    window.addEventListener('ui-flows-changed', h)
    return () => window.removeEventListener('ui-flows-changed', h)
  }, [])

  const finishItem = (i: number, status: 'passed' | 'failed', detail: string) => {
    setItems(list => list.map((x, j) => (j === i ? { ...x, status, detail } : x)))
    runningIdx.current = null
    if (queue.current.length) { const next = queue.current.shift()!; startItem(next) }
    else setRunningAll(false)
  }
  const startItem = async (i: number) => {
    const it = itemsRef.current[i]
    if (!it) return
    runningIdx.current = i
    setItems(list => list.map((x, j) => (j === i ? { ...x, status: 'running', detail: '', stepResults: {} } : x)))
    const url = it.actions[0]?.url || it.siteUrl
    if (!url) { finishItem(i, 'failed', '시작 URL이 없습니다'); return }
    try { await api.startUiReplay(url, it.actions) }
    catch (e) { finishItem(i, 'failed', String(e)) }
  }

  useEffect(() => {
    const un = listen<UiStepResult>('ui-replay-step', e => {
      const r = e.payload
      const i = runningIdx.current
      if (i == null) return
      if (r.done) { finishItem(i, r.status, r.detail); return }
      setItems(list => list.map((x, j) =>
        j === i ? { ...x, stepResults: { ...x.stepResults, [r.index]: { status: r.status, detail: r.detail } } } : x))
    })
    return () => { un.then(u => u()) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const loadSite = async () => {
    setError(''); setInfo('')
    if (!selectedSite) { setError('사이트 URL을 선택하세요'); return }
    try {
      const flows = await api.listUiFlows(selectedSite)
      setItems(flows.map(f => ({
        id: f.id!, name: f.name, siteUrl: f.site_url,
        actions: JSON.parse(f.actions_json) as UiAction[],
        status: 'idle' as const, detail: '', expanded: false, stepResults: {},
      })))
    } catch (e) { setError(String(e)) }
  }

  const move = (i: number, d: -1 | 1) => {
    const j = i + d
    if (j < 0 || j >= items.length) return
    setItems(list => { const n = [...list]; [n[i], n[j]] = [n[j], n[i]]; return n })
  }
  const remove = async (i: number) => {
    const it = items[i]
    if (!window.confirm(`"${it.name}" 시나리오를 DB에서 삭제할까요?`)) return
    try { await api.deleteUiFlow(it.id); setItems(list => list.filter((_, j) => j !== i)); loadSites() }
    catch (e) { setError(String(e)) }
  }
  const toggleExpand = (i: number) => setItems(list => list.map((x, j) => (j === i ? { ...x, expanded: !x.expanded } : x)))

  // 스위트 내에서 동작별 개별 삭제 (DB에도 반영)
  const delAction = async (i: number, k: number) => {
    const it = itemsRef.current[i]
    if (!it) return
    const next = it.actions.filter((_, x) => x !== k)
    setItems(list => list.map((y, j) => (j === i ? { ...y, actions: next, stepResults: {} } : y)))
    try { await api.saveUiFlow(it.name, it.siteUrl, next) } catch (e) { setError(String(e)) }
  }

  const cancelRun = async () => {
    queue.current = []
    const i = runningIdx.current
    runningIdx.current = null
    setRunningAll(false)
    try { await api.stopUiReplay() } catch { /* noop */ }
    if (i != null) setItems(list => list.map((x, j) => (j === i ? { ...x, status: 'idle', detail: '취소됨' } : x)))
  }

  const runAll = () => {
    if (!items.length || runningIdx.current != null) return
    setError('')
    setItems(list => list.map(x => ({ ...x, status: 'idle', detail: '', stepResults: {} })))
    queue.current = items.map((_, i) => i)
    setRunningAll(true)
    startItem(queue.current.shift()!)
  }
  const runOne = (i: number) => {
    if (runningIdx.current != null) return
    queue.current = []; setRunningAll(false); setError('')
    startItem(i)
  }

  const doExport = async () => {
    setError(''); setInfo('')
    const path = await save({ defaultPath: 'recap-ui-flows.json', filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (path) { try { await api.exportUiFlows(path); setInfo('DB의 UI 플로우를 파일로 내보냈습니다.') } catch (e) { setError(String(e)) } }
  }
  const doImport = async () => {
    setError(''); setInfo('')
    const path = await open({ multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (typeof path === 'string') {
      try { const n = await api.importUiFlows(path); await loadSites(); setInfo(`${n}개 플로우를 DB로 가져왔습니다.`) }
      catch (e) { setError(String(e)) }
    }
  }

  const busy = runningIdx.current != null || runningAll
  const fileIcon = (s: SuiteItem['status']) => ({ idle: '—', running: '⏳', passed: '✅', failed: '❌' })[s]
  const stepIcon = (it: SuiteItem, k: number) => {
    const r = it.stepResults[k]
    if (r) return r.status === 'passed' ? '✅' : '❌'
    return it.status === 'running' ? '⏳' : ''
  }

  return (
    <div>
      <h2>UI 테스트 스위트</h2>
      <p className="dim">DB에서 사이트 URL을 골라 저장된 시나리오를 불러와 개별/전체 실행합니다. 파일명을 눌러 펼치면 개별 동작이 보입니다.</p>
      <div className="add-row">
        <select value={selectedSite} onChange={e => setSelectedSite(e.target.value)} style={{ minWidth: 320 }}>
          <option value="">사이트 URL 선택</option>
          {sites.map(s => <option key={s.site_url} value={s.site_url}>{s.site_url} ({s.count})</option>)}
        </select>
        <button onClick={loadSite} disabled={!selectedSite || busy}>불러오기</button>
        <button onClick={loadSites} disabled={busy}>사이트 새로고침</button>
        <button className="accent" onClick={runAll} disabled={!items.length || busy}>
          {runningAll ? '전체 실행 중…' : '▶ 전체 실행'}
        </button>
        {busy && <button className="danger" onClick={cancelRun}>취소</button>}
        <span style={{ flex: 1 }} />
        <button onClick={doExport} disabled={busy}>DB 내보내기</button>
        <button onClick={doImport} disabled={busy}>DB 가져오기</button>
      </div>

      {error && <p className="error">{error}</p>}
      {info && <p className="dim">{info}</p>}

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
                      <thead><tr><th>#</th><th>동작</th><th>이름</th><th>셀렉터</th><th>값</th><th>결과</th><th>삭제</th></tr></thead>
                      <tbody>
                        {it.actions.map((a, k) => (
                          <tr key={a.id + k}>
                            <td>{k + 1}</td>
                            <td>{a.kind === 'click' ? '클릭' : '입력'}</td>
                            <td style={{ maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={a.name}>{a.name}</td>
                            <td className="dim" style={{ maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                              title={a.selectors.map(s => `${s.strategy}: ${s.value}`).join('\n')}>
                              {a.selectors[0] ? `${a.selectors[0].strategy}: ${a.selectors[0].value}` : ''}
                            </td>
                            <td>{a.value ?? ''}</td>
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
  )
}
