import { Fragment, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import type { UiAction, UiStepResult } from '../types'

interface SuiteItem {
  name: string
  path: string
  actions: UiAction[]
  status: 'idle' | 'running' | 'passed' | 'failed'
  detail: string
  expanded: boolean
  stepResults: Record<number, { status: string; detail: string }>
}

const basename = (p: string) => p.split(/[\\/]/).pop() || p

export default function UiSuiteView() {
  const [items, setItems] = useState<SuiteItem[]>([])
  const [runningAll, setRunningAll] = useState(false)
  const [asc, setAsc] = useState(true)
  const [error, setError] = useState('')

  const itemsRef = useRef<SuiteItem[]>([])
  useEffect(() => { itemsRef.current = items }, [items])
  const runningIdx = useRef<number | null>(null)
  const queue = useRef<number[]>([])

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
    const url = it.actions[0]?.url
    if (!url) { finishItem(i, 'failed', '시작 URL이 없습니다 (첫 동작의 url 없음)'); return }
    try { await api.startUiReplay(url, it.actions) }
    catch (e) { finishItem(i, 'failed', String(e)) }
  }

  useEffect(() => {
    const un = listen<UiStepResult>('ui-replay-step', e => {
      const r = e.payload
      const i = runningIdx.current
      if (i == null) return
      if (r.done) { finishItem(i, r.status, r.detail); return }
      // 실행 중 파일의 스텝별 결과 기록
      setItems(list => list.map((x, j) =>
        j === i ? { ...x, stepResults: { ...x.stepResults, [r.index]: { status: r.status, detail: r.detail } } } : x))
    })
    return () => { un.then(u => u()) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const loadFiles = async () => {
    setError('')
    const paths = await open({ multiple: true, filters: [{ name: 'JSON', extensions: ['json'] }] })
    const list = Array.isArray(paths) ? paths : typeof paths === 'string' ? [paths] : []
    const loaded: SuiteItem[] = []
    for (const p of list) {
      try {
        const acts = await api.loadUiActions(p)
        loaded.push({ name: basename(p), path: p, actions: acts, status: 'idle', detail: '', expanded: false, stepResults: {} })
      } catch (e) { setError(`${basename(p)}: ${e}`) }
    }
    if (loaded.length) setItems(prev => [...prev, ...loaded])
  }

  const sortByName = () => {
    setItems(list => [...list].sort((a, b) => (asc ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name))))
    setAsc(v => !v)
  }
  const move = (i: number, d: -1 | 1) => {
    const j = i + d
    if (j < 0 || j >= items.length) return
    setItems(list => { const n = [...list]; [n[i], n[j]] = [n[j], n[i]]; return n })
  }
  const remove = (i: number) => setItems(list => list.filter((_, j) => j !== i))
  const clearAll = () => { setItems([]); setError('') }
  const toggleExpand = (i: number) => setItems(list => list.map((x, j) => (j === i ? { ...x, expanded: !x.expanded } : x)))

  const runAll = () => {
    if (!items.length || runningIdx.current != null) return
    setError('')
    setItems(list => list.map(x => ({ ...x, status: 'idle', detail: '', stepResults: {} })))
    queue.current = items.map((_, i) => i)
    setRunningAll(true)
    const first = queue.current.shift()!
    startItem(first)
  }
  const runOne = (i: number) => {
    if (runningIdx.current != null) return
    queue.current = []; setRunningAll(false); setError('')
    startItem(i)
  }

  const busy = runningIdx.current != null || runningAll
  const fileIcon = (s: SuiteItem['status']) => ({ idle: '—', running: '⏳', passed: '✅', failed: '❌' })[s]
  const stepIcon = (it: SuiteItem, idx: number) => {
    const r = it.stepResults[idx]
    if (r) return r.status === 'passed' ? '✅' : '❌'
    return it.status === 'running' ? '⏳' : ''
  }

  return (
    <div>
      <h2>UI 테스트 스위트</h2>
      <p className="dim">저장한 UI 동작(JSON)들을 불러와 정렬하고, 개별/전체 실행합니다. 파일명을 눌러 펼치면 개별 동작이 보입니다.</p>
      <div className="add-row">
        <button onClick={loadFiles} disabled={busy}>불러오기 (여러 개)</button>
        <button onClick={sortByName} disabled={!items.length}>파일명순 정렬 {asc ? '↑' : '↓'}</button>
        <button className="accent" onClick={runAll} disabled={!items.length || busy}>
          {runningAll ? '전체 실행 중…' : '▶ 전체 실행'}
        </button>
        <button className="danger" onClick={clearAll} disabled={!items.length || busy}>비우기</button>
        <span className="dim">{items.length}개</span>
      </div>

      {error && <p className="error">{error}</p>}

      <table className="history">
        <thead>
          <tr><th></th><th>#</th><th>파일명</th><th>동작수</th><th>상태</th><th>세부</th><th>실행</th><th>순서/삭제</th></tr>
        </thead>
        <tbody>
          {items.map((it, i) => (
            <Fragment key={it.path + i}>
              <tr>
                <td><button onClick={() => toggleExpand(i)} title="펼치기">{it.expanded ? '▾' : '▸'}</button></td>
                <td>{i + 1}</td>
                <td title={it.path} style={{ cursor: 'pointer' }} onClick={() => toggleExpand(i)}>{it.name}</td>
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
                <tr key={it.path + i + '-exp'}>
                  <td></td>
                  <td colSpan={7} style={{ background: 'var(--vsc-bg-alt)' }}>
                    <table className="history" style={{ margin: 0 }}>
                      <thead><tr><th>#</th><th>동작</th><th>이름</th><th>셀렉터</th><th>값</th><th>결과</th></tr></thead>
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
