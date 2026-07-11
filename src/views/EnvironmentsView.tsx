import { useEffect, useState } from 'react'
import * as api from '../api'
import MqLogPanel from '../components/MqLogPanel'
import type { Environment } from '../types'

// RabbitMQ 설정만 사용하는 간소화된 환경. (Keystone/엔드포인트 등은 빈 값으로 저장)
const empty: Environment = {
  id: null, name: '', keystone_url: '', user_name: '', user_domain: 'Default',
  project_name: '', project_domain: 'Default', mq_url: '', mq_exchanges: 'nova,neutron,cinder',
  endpoints: {},
}

export default function EnvironmentsView() {
  const [envs, setEnvs] = useState<Environment[]>([])
  const [form, setForm] = useState<Environment>(empty)
  const [error, setError] = useState('')
  const [logEnvId, setLogEnvId] = useState<number | null>(null)

  const reload = () => api.listEnvironments().then(setEnvs).catch(e => setError(String(e)))
  useEffect(() => {
    reload()
    return () => { api.stopReplayMq().catch(() => {}) }
  }, [])

  const edit = (env: Environment) => setForm(env)

  const save = async () => {
    setError('')
    if (!form.name.trim()) { setError('이름을 입력하세요'); return }
    if (!form.mq_url.trim()) { setError('RabbitMQ URL을 입력하세요'); return }
    try {
      await api.saveEnvironment(form, null)
      setForm(empty)
      reload()
    } catch (e) { setError(String(e)) }
  }

  const remove = (env: Environment) => {
    if (!window.confirm(`환경 "${env.name}"을(를) 삭제할까요?`)) return
    api.deleteEnvironment(env.id!)
      .then(() => {
        if (form.id === env.id) setForm(empty)
        if (logEnvId === env.id) stopLog()
        reload()
      })
      .catch(e => setError(String(e)))
  }

  const startLog = async (env: Environment) => {
    setError('')
    try { await api.startReplayMq(env.id!); setLogEnvId(env.id!) }
    catch (e) { setError('RabbitMQ 연결 실패: ' + String(e)) }
  }
  const stopLog = async () => { try { await api.stopReplayMq() } catch { /* noop */ } setLogEnvId(null) }

  const field = (key: 'name' | 'mq_url' | 'mq_exchanges', label: string, placeholder = '') => (
    <label className="field">{label}
      <input value={String(form[key] ?? '')} placeholder={placeholder}
        onChange={e => setForm({ ...form, [key]: e.target.value })} />
    </label>
  )

  return (
    <div>
      <h2>환경 (RabbitMQ)</h2>
      <p className="dim">wait_event 스텝과 실시간 RabbitMQ 로그에 사용할 접속 정보를 관리합니다.</p>
      <div className="two-col">
        <div>
          <h3>환경 목록</h3>
          <ul className="list">
            {envs.map(env => (
              <li key={env.id}>
                <button onClick={() => edit(env)}>{env.name}</button>
                {logEnvId === env.id
                  ? <button className="danger" onClick={stopLog}>■ 로그 중단</button>
                  : <button onClick={() => startLog(env)}>▶ 로그</button>}
                <button className="danger" onClick={() => remove(env)}>삭제</button>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h3>{form.id ? '환경 수정' : '새 환경'}</h3>
          {field('name', '이름', 'dev')}
          {field('mq_url', 'RabbitMQ URL', 'amqp://user:pw@host:5672/%2f')}
          {field('mq_exchanges', 'notification exchange (쉼표 구분)', 'nova,neutron,cinder')}
          {error && <p className="error">{error}</p>}
          <button className="accent" onClick={save}>저장</button>
          {form.id && <button onClick={() => setForm(empty)}>새로 만들기</button>}
        </div>
      </div>

      {logEnvId != null && (
        <div style={{ marginTop: 12 }}>
          <p className="dim">"{envs.find(e => e.id === logEnvId)?.name}" 환경의 RabbitMQ 실시간 로그</p>
          <MqLogPanel height={260} />
        </div>
      )}
    </div>
  )
}
