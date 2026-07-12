import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import * as api from '../api'
import MqLogPanel from '../components/MqLogPanel'
import { mqSession } from '../mqLog'
import type { Environment } from '../types'

// RabbitMQ 설정만 사용하는 간소화된 환경. (Keystone/엔드포인트 등은 빈 값으로 저장)
const empty: Environment = {
  id: null, name: '', keystone_url: '', user_name: '', user_domain: 'Default',
  project_name: '', project_domain: 'Default', mq_url: '', mq_exchanges: 'nova,neutron,cinder',
  endpoints: {}, mq_hosts: '', mq_user: 'openstack', mq_password: '', mq_vhost: '/',
}

interface HostRow { id: number; v: string }
const hostsToList = (s: string) => (s ? s.split(',').map(h => h.trim()).filter(Boolean) : [])

export default function EnvironmentsView() {
  const [envs, setEnvs] = useState<Environment[]>([])
  const [form, setForm] = useState<Environment>(empty)
  const [hosts, setHosts] = useState<HostRow[]>([{ id: 0, v: '' }])
  const [showPw, setShowPw] = useState(false)
  const [error, setError] = useState('')
  // 로그 대상 = 실제 연결된 MQ 세션을 따라간다(앱 시작 시 자동 연결하지 않음).
  // 탭 전환 시에도 전역 mqSession이 연결을 유지하므로 재마운트해도 상태가 복원된다.
  const [logEnvId, setLogEnvId] = useState<number | null>(() => mqSession.getEnvId())
  const setLog = (id: number | null) => setLogEnvId(id)
  const connectedEnv = useSyncExternalStore(mqSession.subscribe, mqSession.getEnvId)
  const nextId = useRef(1)
  const mkRows = (vals: string[]): HostRow[] =>
    (vals.length ? vals : ['']).map(v => ({ id: nextId.current++, v }))

  const reload = () => api.listEnvironments().then(list => {
    setEnvs(list)
    window.dispatchEvent(new CustomEvent('environments-changed')) // 레코더/스위트 드롭다운 갱신
  }).catch(e => setError(String(e)))
  useEffect(() => {
    reload()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const edit = (env: Environment) => {
    setForm(env)
    const list = hostsToList(env.mq_hosts)
    setHosts(mkRows(list.length ? list : (env.mq_url ? [env.mq_url] : [''])))
  }
  const reset = () => { setForm(empty); setHosts(mkRows([''])) }

  const setHost = (id: number, v: string) => setHosts(rows => rows.map(r => (r.id === id ? { ...r, v } : r)))
  const addHost = () => setHosts(rows => [...rows, { id: nextId.current++, v: '' }])
  const delHost = (id: number) => setHosts(rows => (rows.length > 1 ? rows.filter(r => r.id !== id) : rows))

  const save = async () => {
    setError('')
    if (!form.name.trim()) { setError('이름을 입력하세요'); return }
    const cleanHosts = hosts.map(r => r.v.trim()).filter(Boolean)
    if (cleanHosts.length === 0) { setError('RabbitMQ 호스트(host:port)를 1개 이상 입력하세요'); return }
    const savedId = form.id // reset 전에 편집 대상 id 보관
    try {
      await api.saveEnvironment({ ...form, mq_hosts: cleanHosts.join(','), mq_url: '' }, null)
      reset()
      reload()
      // 편집한 환경의 로그가 켜져 있으면 새 접속 정보로 재연결(로그 새로고침)
      if (savedId != null && logEnvId === savedId) {
        await mqSession.stop()
        await mqSession.start(savedId).catch(e => setError('RabbitMQ 재연결 실패: ' + String(e)))
      }
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
    setError(''); setLog(env.id!)
    try { await mqSession.start(env.id!) }
    catch (e) { setError('RabbitMQ 연결 실패: ' + String(e)) }
  }
  const stopLog = async () => { setLog(null); await mqSession.stop() }

  return (
    <div>
      <h2>환경 (RabbitMQ)</h2>
      <p className="dim">wait_event 스텝과 실시간 RabbitMQ 로그에 사용할 클러스터 접속 정보를 관리합니다. 호스트는 여러 개(클러스터) 등록 시 순서대로 접속을 시도합니다.</p>
      <div className="two-col">
        <div>
          <h3>환경 목록</h3>
          {envs.length === 0 && <p className="dim" style={{ fontSize: 12 }}>저장된 환경이 없습니다.</p>}
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

          <div className="field">RabbitMQ 호스트 (host:port) *
            {hosts.map((r, i) => (
              <div className="add-row" key={r.id} style={{ marginTop: 4 }}>
                <span className="dim" style={{ width: 16 }}>{i + 1}</span>
                <input value={r.v} placeholder="host:port" onChange={e => setHost(r.id, e.target.value)} style={{ minWidth: 240 }} />
                <button className="danger" onClick={() => delHost(r.id)} disabled={hosts.length === 1} title="이 호스트 삭제">🗑</button>
              </div>
            ))}
            <button onClick={addHost} style={{ marginTop: 4 }}>+ 필드 추가</button>
          </div>

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
          <p className="dim">
            "{envs.find(e => e.id === logEnvId)?.name}" 환경의 RabbitMQ 실시간 로그
            {connectedEnv === logEnvId ? '' : ' · (다른 환경이 연결 중이거나 끊김 — ▶ 로그로 재연결)'}
          </p>
          <MqLogPanel height={260} storageKey="env" onConnected={() => setError('')} />
        </div>
      )}
    </div>
  )
}
