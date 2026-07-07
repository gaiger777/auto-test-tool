import { useEffect, useState } from 'react'
import { open, save } from '@tauri-apps/plugin-dialog'
import * as api from '../api'
import { presets } from '../presets'
import type { ScenarioRecord, StepDef } from '../types'
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

  const reload = () => api.listScenarios().then(setScenarios).catch(e => setError(String(e)))
  useEffect(() => { reload() }, [])

  const edit = (rec: ScenarioRecord) => {
    setCurrent(rec)
    setSteps(JSON.parse(rec.steps_json))
  }

  const newScenario = () => edit({ id: null, name: '', description: '', steps_json: '[]' })

  const saveCurrent = async () => {
    setError('')
    try {
      const id = await api.saveScenario({ ...current, steps_json: JSON.stringify(steps) })
      setCurrent({ ...current, id })
      reload()
    } catch (e) { setError(String(e)) }
  }

  const move = (i: number, delta: -1 | 1) => {
    const j = i + delta
    if (j < 0 || j >= steps.length) return
    const next = [...steps]
    ;[next[i], next[j]] = [next[j], next[i]]
    setSteps(next)
  }

  const addPreset = () => {
    const preset = presets.find(p => p.id === presetId)!
    setSteps([...steps, ...preset.expand(presetInput)])
    setPresetInput({})
  }

  const removeScenario = (s: ScenarioRecord) => {
    if (!window.confirm(`시나리오 "${s.name}"을(를) 삭제할까요?`)) return
    api.deleteScenario(s.id!)
      .then(() => {
        if (current.id === s.id) newScenario()
        reload()
      })
      .catch(e => setError(String(e)))
  }

  const doExport = async () => {
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
        <button onClick={newScenario}>새 시나리오</button>
        <button onClick={doImport}>가져오기</button>
        <ul className="list">
          {scenarios.map(s => (
            <li key={s.id}>
              <button onClick={() => edit(s)}>{s.name}</button>
              <button className="danger" onClick={() => removeScenario(s)}>삭제</button>
            </li>
          ))}
        </ul>
      </div>
      <div>
        <h2>{current.id ? '시나리오 편집' : '새 시나리오'}</h2>
        <label className="field">이름
          <input value={current.name} onChange={e => setCurrent({ ...current, name: e.target.value })} />
        </label>
        <label className="field">설명
          <input value={current.description} onChange={e => setCurrent({ ...current, description: e.target.value })} />
        </label>

        <h3>스텝 ({steps.length})</h3>
        {steps.map((s, i) => (
          <details key={i} className="step">
            <summary>
              {i + 1}. [{s.type}] {s.name} {s.cleanup ? '🧹' : ''}
              <span className="step-actions">
                <button onClick={e => { e.preventDefault(); move(i, -1) }}>↑</button>
                <button onClick={e => { e.preventDefault(); move(i, 1) }}>↓</button>
                <button className="danger" onClick={e => { e.preventDefault(); setSteps(steps.filter((_, j) => j !== i)) }}>삭제</button>
              </span>
            </summary>
            <StepForm step={s} onChange={ns => setSteps(steps.map((old, j) => (j === i ? ns : old)))} />
          </details>
        ))}

        <h3>스텝 추가</h3>
        <div className="add-row">
          {(['http_call', 'wait_event', 'assert', 'sleep'] as const).map(t => (
            <button key={t} onClick={() => setSteps([...steps, blankStep(t)])}>+ {t}</button>
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
        <button onClick={saveCurrent}>저장</button>
        <button onClick={doExport}>내보내기</button>
      </div>
    </div>
  )
}
