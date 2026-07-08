import { useEffect, useState } from 'react'
import { open, save } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import { presets } from '../presets'
import { useRun } from '../hooks/useRun'
import { RunProgress } from './RunProgress'
import type { Environment, ScenarioRecord, StepDef } from '../types'
import StepForm from './StepForm'

const blankStep = (type: StepDef['type']): StepDef => {
  switch (type) {
    case 'http_call': return { name: '새 HTTP 호출', type, method: 'GET', url: '', headers: { 'X-Auth-Token': '{{auth_token}}' } }
    case 'wait_event': return { name: '새 이벤트 대기', type, event_type: '', conditions: [], timeout_secs: 300 }
    case 'assert': return { name: '새 검증', type, left: '', op: 'eq', right: '' }
    case 'sleep': return { name: '새 대기', type, seconds: 5 }
  }
}

export default function ScenarioBuilder() {
  const [scenarios, setScenarios] = useState<ScenarioRecord[]>([])
  const [current, setCurrent] = useState<ScenarioRecord>({ id: null, name: '', description: '', steps_json: '[]' })
  const [steps, setSteps] = useState<StepDef[]>([])
  const [presetId, setPresetId] = useState(presets[0].id)
  const [presetInput, setPresetInput] = useState<Record<string, string>>({})
  const [error, setError] = useState('')
  const [dirty, setDirty] = useState(false)

  // 시나리오 목록에서 바로 실행 — 전역 환경 선택(선택값 기억) + 인라인 진행
  const [envs, setEnvs] = useState<Environment[]>([])
  const [envId, setEnvId] = useState<number | null>(() => {
    const v = localStorage.getItem('run.envId')
    return v ? Number(v) : null
  })
  const run = useRun()

  const reload = () => api.listScenarios().then(setScenarios).catch(e => setError(String(e)))
  useEffect(() => { reload() }, [])
  useEffect(() => {
    api.listEnvironments().then(list => {
      setEnvs(list)
      setEnvId(prev => prev ?? list[0]?.id ?? null) // 미선택 시 첫 환경 자동 선택
    }).catch(() => {})
  }, [])

  const changeEnv = (id: number | null) => {
    setEnvId(id)
    if (id == null) localStorage.removeItem('run.envId')
    else localStorage.setItem('run.envId', String(id))
  }

  const runScenario = (s: ScenarioRecord) => {
    const eid = envId ?? envs[0]?.id ?? null // 환경 미선택 시 첫 환경으로 실행
    if (eid == null) { setError('실행하려면 환경을 먼저 등록하세요 (환경 탭)'); return }
    setError('')
    run.start(s, eid)
  }

  const changeSteps = (next: StepDef[]) => {
    setSteps(next)
    setDirty(true)
  }

  const applyEdit = (rec: ScenarioRecord) => {
    setCurrent(rec)
    setSteps(JSON.parse(rec.steps_json))
    setDirty(false)
  }

  const edit = (rec: ScenarioRecord) => {
    if (dirty && !window.confirm('저장하지 않은 변경이 있습니다. 버리고 이동할까요?')) return
    applyEdit(rec)
  }

  const newScenario = () => edit({ id: null, name: '', description: '', steps_json: '[]' })

  const saveCurrent = async () => {
    setError('')
    try {
      const id = await api.saveScenario({ ...current, steps_json: JSON.stringify(steps) })
      setCurrent({ ...current, id })
      setDirty(false)
      reload()
    } catch (e) { setError(String(e)) }
  }

  const move = (i: number, delta: -1 | 1) => {
    const j = i + delta
    if (j < 0 || j >= steps.length) return
    const next = [...steps]
    ;[next[i], next[j]] = [next[j], next[i]]
    changeSteps(next)
  }

  const addPreset = () => {
    const preset = presets.find(p => p.id === presetId)!
    changeSteps([...steps, ...preset.expand(presetInput)])
    setPresetInput({})
  }

  const removeScenario = (s: ScenarioRecord) => {
    if (!window.confirm(`시나리오 "${s.name}"을(를) 삭제할까요?`)) return
    api.deleteScenario(s.id!)
      .then(() => {
        if (current.id === s.id) applyEdit({ id: null, name: '', description: '', steps_json: '[]' })
        reload()
      })
      .catch(e => setError(String(e)))
  }

  const doExport = async () => {
    if (dirty) { setError('저장하지 않은 변경이 있습니다. 먼저 저장하세요.'); return }
    if (current.id == null) { setError('먼저 저장하세요'); return }
    const path = await save({ defaultPath: `${current.name || 'scenario'}.json` })
    if (path) await api.exportScenario(current.id, path).catch(e => setError(String(e)))
  }

  const doImport = async () => {
    const path = await open({ multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] })
    if (typeof path === 'string') {
      await api.importScenario(path).catch(e => setError(String(e)))
      reload()
    }
  }

  const preset = presets.find(p => p.id === presetId)!

  return (
    <div className="two-col">
      <div>
        <h2>시나리오</h2>
        <div className="add-row">
          <button onClick={newScenario}>새 시나리오</button>
          <button onClick={doImport}>가져오기</button>
        </div>
        <label className="field">실행 환경
          <select value={envId ?? ''} onChange={e => changeEnv(e.target.value ? Number(e.target.value) : null)}>
            <option value="">환경 선택</option>
            {envs.map(e2 => <option key={e2.id} value={e2.id!}>{e2.name}</option>)}
          </select>
        </label>
        {run.error && <p className="error">{run.error}</p>}
        <ul className="list">
          {scenarios.map(s => (
            <li key={s.id}>
              <div className="scenario-row-main">
                <button className="run-btn accent" title="실행" disabled={run.running}
                  onClick={() => runScenario(s)}>▶</button>
                <button className="scenario-name" onClick={() => edit(s)}>{s.name}</button>
                <button className="danger" onClick={() => removeScenario(s)}>삭제</button>
              </div>
              {run.activeScenarioId === s.id && (
                <div className="scenario-run">
                  <div className="scenario-run-head">
                    <span className={`status-badge ${run.status}`}>{run.status}</span>
                    {run.running && <button className="danger" onClick={run.cancel}>취소</button>}
                  </div>
                  <RunProgress rows={run.rows} />
                </div>
              )}
            </li>
          ))}
        </ul>
      </div>
      <div>
        <h2>{current.id ? '시나리오 편집' : '새 시나리오'}</h2>
        <label className="field">이름
          <input value={current.name} onChange={e => { setCurrent({ ...current, name: e.target.value }); setDirty(true) }} />
        </label>
        <label className="field">설명
          <input value={current.description} onChange={e => { setCurrent({ ...current, description: e.target.value }); setDirty(true) }} />
        </label>

        <h3>스텝 ({steps.length})</h3>
        {steps.map((s, i) => (
          <details key={i} className="step">
            <summary>
              {i + 1}. [{s.type}] {s.name} {s.cleanup ? '🧹' : ''}
              <span className="step-actions">
                <button onClick={e => { e.preventDefault(); move(i, -1) }}>↑</button>
                <button onClick={e => { e.preventDefault(); move(i, 1) }}>↓</button>
                <button className="danger" onClick={e => { e.preventDefault(); changeSteps(steps.filter((_, j) => j !== i)) }}>삭제</button>
              </span>
            </summary>
            <StepForm step={s} onChange={ns => changeSteps(steps.map((old, j) => (j === i ? ns : old)))} />
          </details>
        ))}

        <h3>스텝 추가</h3>
        <div className="add-row">
          {(['http_call', 'wait_event', 'assert', 'sleep'] as const).map(t => (
            <button key={t} onClick={() => changeSteps([...steps, blankStep(t)])}>+ {t}</button>
          ))}
        </div>
        <div className="add-row">
          <select value={presetId} onChange={e => setPresetId(e.target.value)}>
            {presets.map(p => <option key={p.id} value={p.id}>{p.label}</option>)}
          </select>
          {preset.fields.map(f => (
            <input key={f.key} placeholder={f.placeholder || f.label} value={presetInput[f.key] ?? ''}
              onChange={e => setPresetInput({ ...presetInput, [f.key]: e.target.value })} />
          ))}
          <button onClick={addPreset}>프리셋 추가</button>
        </div>

        {error && <p className="error">{error}</p>}
        <div className="add-row">
          <button className="accent" onClick={saveCurrent}>저장</button>
          <button onClick={doExport}>내보내기</button>
        </div>
      </div>
    </div>
  )
}
