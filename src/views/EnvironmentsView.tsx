import { useEffect, useState } from 'react'
import * as api from '../api'
import type { Environment } from '../types'

const empty: Environment = {
  id: null, name: '', keystone_url: '', user_name: '', user_domain: 'Default',
  project_name: '', project_domain: 'Default', mq_url: '', mq_exchanges: 'nova,neutron,cinder',
  endpoints: {},
}

export default function EnvironmentsView() {
  const [envs, setEnvs] = useState<Environment[]>([])
  const [form, setForm] = useState<Environment>(empty)
  const [password, setPassword] = useState('')
  const [endpointsText, setEndpointsText] = useState('{}')
  const [error, setError] = useState('')

  const reload = () => api.listEnvironments().then(setEnvs).catch(e => setError(String(e)))
  useEffect(() => { reload() }, [])

  const edit = (env: Environment) => {
    setForm(env)
    setEndpointsText(JSON.stringify(env.endpoints, null, 2))
    setPassword('')
  }

  const save = async () => {
    setError('')
    let endpoints: Record<string, string>
    try {
      endpoints = JSON.parse(endpointsText)
    } catch {
      setError('엔드포인트는 JSON 객체여야 합니다. 예: {"nova": "http://host:8774/v2.1"}')
      return
    }
    try {
      await api.saveEnvironment({ ...form, endpoints }, password || null)
      setForm(empty); setEndpointsText('{}'); setPassword('')
      reload()
    } catch (e) { setError(String(e)) }
  }

  const field = (key: keyof Environment, label: string, placeholder = '') => (
    <label className="field">{label}
      <input value={String(form[key] ?? '')} placeholder={placeholder}
        onChange={e => setForm({ ...form, [key]: e.target.value })} />
    </label>
  )

  return (
    <div className="two-col">
      <div>
        <h2>환경 목록</h2>
        <ul className="list">
          {envs.map(env => (
            <li key={env.id}>
              <button onClick={() => edit(env)}>{env.name}</button>
              <button className="danger" onClick={() => api.deleteEnvironment(env.id!).then(reload)}>삭제</button>
            </li>
          ))}
        </ul>
      </div>
      <div>
        <h2>{form.id ? '환경 수정' : '새 환경'}</h2>
        {field('name', '이름', 'dev')}
        {field('keystone_url', 'Keystone URL', 'http://keystone:5000')}
        {field('user_name', '사용자')}
        {field('user_domain', '사용자 도메인')}
        {field('project_name', '프로젝트')}
        {field('project_domain', '프로젝트 도메인')}
        <label className="field">비밀번호 (OS 키체인에 저장)
          <input type="password" value={password} onChange={e => setPassword(e.target.value)}
            placeholder={form.id ? '변경할 때만 입력' : ''} />
        </label>
        {field('mq_url', 'RabbitMQ URL', 'amqp://user:pw@host:5672/%2f')}
        {field('mq_exchanges', 'notification exchange (쉼표 구분)', 'nova,neutron,cinder')}
        <label className="field">서비스 엔드포인트 (JSON)
          <textarea rows={5} value={endpointsText} onChange={e => setEndpointsText(e.target.value)} />
        </label>
        {error && <p className="error">{error}</p>}
        <button onClick={save}>저장</button>
        {form.id && <button onClick={() => edit(empty)}>새로 만들기</button>}
      </div>
    </div>
  )
}
