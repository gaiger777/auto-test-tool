import { useEffect, useState } from 'react'
import * as api from '../api'
import MqLogPanel from '../components/MqLogPanel'
import type { Environment } from '../types'

// RabbitMQ 설정만 사용하는 간소화된 환경. (Keystone/엔드포인트 등은 빈 값으로 저장)
const empty: Environment = {
  id: null, name: '', keystone_url: '', user_name: '', user_domain: 'Default',
  project_name: '', project_domain: 'Default', mq_url: '', mq_exchanges: 'nova,neutron,cinder',
  endpoints: {}, mq_hosts: '', mq_user: 'openstack', mq_password: '', mq_vhost: '/',
}

const hostsToList = (s: string) => (s ? s.split(',').map(h => h.trim()).filter(Boolean) : [])

export default function EnvironmentsView() {
  const [envs, setEnvs] = useState<Environment[]>([])
  const [form, setForm] = useState<Environment>(empty)
  const [hosts, setHosts] = useState<string[]>([''])
  const [showPw, setShowPw] = useState(false)
  const [error, setError] = useState('')
  const [logEnvId, setLogEnvId] = useState<number | null>(null)

  const reload = () => api.listEnvironments().then(setEnvs).catch(e => setError(String(e)))
  useEffect(() => {
    reload()
    return () => { api.stopReplayMq().catch(() => {}) }
  }, [])

  const edit = (env: Environment) => {
    setForm(env)
    const list = hostsToList(env.mq_hosts)
    setHosts(list.length ? list : (env.mq_url ? [env.mq_url] : ['']))
  }
  const reset = () => { setForm(empty); setHosts(['']) }

  const setHost = (i: number, v: string) => setHosts(h => h.map((x, j) => (j === i ? v : x)))
  const addHost = () => setHosts(h => [...h, ''])
  const delHost = (i: number) => setHosts(h => (h.length > 1 ? h.filter((_, j) => j !== i) : h))

  const save = async () => {
    setError('')
    if (!form.name.trim()) { setError('이름을 입력하세요'); return }
    const cleanHosts = hosts.map(h => h.trim()).filter(Boolean)
    if (cleanHosts.length === 0) { setError('RabbitMQ 호스트(host:port)를 1개 이상 입력하세요'); return }
    try {
      await api.saveEnvironment({ ...form, mq_hosts: cleanHosts.join(','), mq_url: '' }, null)
      reset()
      reload()
    } catch (e) { setError(String(e)) }
  }

  const remove = (env: Environment) => {
    if (!window.confirm(`환경 "${env.name}"을(를) 삭제할까요?`)) return
    api.deleteEnvironment(env.id!)
      .then(() => {
        if (form.id === env.id) reset()
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

  return (
    <div>
      <h2>환경 (RabbitMQ)</h2>
      <p className="dim">wait_event 스텝과 실시간 RabbitMQ 로그에 사용할 클러스터 접속 정보를 관리합니다. 호스트는 여러 개(클러스터) 등록 시 순서대로 접속을 시도합니다.</p>
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
          <label className="field">이름
            <input value={form.name} placeholder="dev" onChange={e => setForm({ ...form, name: e.target.value })} />
          </label>

          <label className="field">RabbitMQ URL (host:port) *</label>
          {hosts.map((h, i) => (
            <div className="add-row" key={i} style={{ marginBottom: 4 }}>
              <input value={h} placeholder="10.255.40.2:5672" onChange={e => setHost(i, e.target.value)} style={{ minWidth: 260 }} />
              <button className="danger" onClick={() => delHost(i)} disabled={hosts.length === 1} title="삭제">🗑</button>
            </div>
          ))}
          <button onClick={addHost} style={{ marginBottom: 8 }}>+ 필드 추가</button>

          <label className="field">인증 아이디
            <input value={form.mq_user} placeholder="openstack" onChange={e => setForm({ ...form, mq_user: e.target.value })} />
          </label>
          <label className="field">인증 비밀번호
            <span style={{ display: 'flex', gap: 4 }}>
              <input type={showPw ? 'text' : 'password'} value={form.mq_password}
                onChange={e => setForm({ ...form, mq_password: e.target.value })} style={{ flex: 1 }} />
              <button onClick={() => setShowPw(s => !s)} title="표시/숨김">{showPw ? '🙈' : '👁'}</button>
            </span>
          </label>
          <label className="field">vhost
            <input value={form.mq_vhost} placeholder="/" onChange={e => setForm({ ...form, mq_vhost: e.target.value })} />
          </label>
          <label className="field">notification exchange (쉼표 구분)
            <input value={form.mq_exchanges} placeholder="nova,neutron,cinder" onChange={e => setForm({ ...form, mq_exchanges: e.target.value })} />
          </label>

          {error && <p className="error">{error}</p>}
          <button className="accent" onClick={save}>저장</button>
          {form.id && <button onClick={reset}>새로 만들기</button>}
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
