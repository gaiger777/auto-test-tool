import { useState } from 'react'
import type { AssertOp, UiAction, UiKind, UiProgStep } from '../types'

// http_call/wait_event/assert/sleep 프로그램 스텝을 만들거나(추가) 기존 스텝을 수정한다.
// initial이 주어지면 그 값으로 채워 '수정' 모드로 동작(같은 id 유지 → 부모가 교체).
export default function ProgStepAdder({ onAdd, initial, onCancel }:
  { onAdd: (a: UiAction) => void; initial?: UiAction | null; onCancel?: () => void }) {
  const s: UiProgStep = initial?.step || {}
  const editing = !!initial
  const [kind, setKind] = useState<UiKind>(initial?.kind ?? 'http_call')
  // http_call
  const [method, setMethod] = useState(s.method ?? 'GET')
  const [url, setUrl] = useState(s.url ?? '')
  const [expectStatus, setExpectStatus] = useState(s.expect_status != null ? String(s.expect_status) : '')
  const [body, setBody] = useState(s.body ?? '')
  // wait_event
  const [eventType, setEventType] = useState(s.event_type ?? '')
  const [timeoutSecs, setTimeoutSecs] = useState(s.timeout_secs != null ? String(s.timeout_secs) : '30')
  const [condPath, setCondPath] = useState(s.conditions?.[0]?.json_path ?? '')
  const [condEq, setCondEq] = useState(s.conditions?.[0]?.equals ?? '')
  // assert
  const [left, setLeft] = useState(s.left ?? '')
  const [op, setOp] = useState<AssertOp>(s.op ?? 'eq')
  const [right, setRight] = useState(s.right ?? '')
  // sleep
  const [seconds, setSeconds] = useState(s.seconds != null ? String(s.seconds) : '1')

  const mkId = () => 'p' + Math.random().toString(36).slice(2, 10)

  const submit = () => {
    let step: UiProgStep = {}
    let name = ''
    if (kind === 'http_call') {
      if (!url.trim()) return
      step = { method, url: url.trim(), expect_status: expectStatus ? Number(expectStatus) : null, body: body || null }
      name = `${method} ${url.trim()}`
    } else if (kind === 'wait_event') {
      if (!eventType.trim()) return
      step = {
        event_type: eventType.trim(),
        timeout_secs: Number(timeoutSecs) || 30,
        conditions: condPath.trim() ? [{ json_path: condPath.trim(), equals: condEq }] : [],
      }
      name = eventType.trim()
    } else if (kind === 'assert') {
      step = { left, op, right }
      name = `assert ${op}`
    } else if (kind === 'sleep') {
      step = { seconds: Number(seconds) || 0 }
      name = `${Number(seconds) || 0}초 대기`
    }
    // 수정이면 id/timestamp 유지 → 부모가 같은 위치로 교체
    onAdd({ id: initial?.id ?? mkId(), kind, selectors: [], name, value: null, url: '', timestamp: initial?.timestamp ?? 0, step })
  }

  return (
    <div style={{ border: '1px solid var(--vsc-border)', borderRadius: 6, padding: 8, marginTop: 8 }}>
      <div className="add-row" style={{ marginBottom: 6 }}>
        <strong style={{ fontSize: 12 }}>{editing ? '스텝 수정:' : '스텝 추가:'}</strong>
        {(['http_call', 'wait_event', 'assert', 'sleep'] as UiKind[]).map(k => (
          <button key={k} className={kind === k ? 'accent' : ''} onClick={() => setKind(k)}>+ {k}</button>
        ))}
      </div>

      {kind === 'http_call' && (
        <div className="add-row">
          <select value={method} onChange={e => setMethod(e.target.value)}>
            {['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map(m => <option key={m}>{m}</option>)}
          </select>
          <input placeholder="URL (https://... 또는 상대경로)" value={url} onChange={e => setUrl(e.target.value)} style={{ minWidth: 280 }} />
          <input placeholder="기대 상태(선택)" value={expectStatus} onChange={e => setExpectStatus(e.target.value)} style={{ width: 110 }} />
          <input placeholder="body(선택, {{status}}/{{body}} 치환)" value={body ?? ''} onChange={e => setBody(e.target.value)} style={{ minWidth: 200 }} />
        </div>
      )}
      {kind === 'wait_event' && (
        <div className="add-row">
          <input placeholder="event_type (예: compute.instance.create.end)" value={eventType} onChange={e => setEventType(e.target.value)} style={{ minWidth: 300 }} />
          <input placeholder="타임아웃(초)" value={timeoutSecs} onChange={e => setTimeoutSecs(e.target.value)} style={{ width: 100 }} />
          <input placeholder="조건 json_path(선택)" value={condPath} onChange={e => setCondPath(e.target.value)} style={{ minWidth: 160 }} />
          <input placeholder="= 값" value={condEq} onChange={e => setCondEq(e.target.value)} style={{ width: 120 }} />
        </div>
      )}
      {kind === 'assert' && (
        <div className="add-row">
          <input placeholder="좌변 (예: {{status}})" value={left} onChange={e => setLeft(e.target.value)} style={{ minWidth: 200 }} />
          <select value={op} onChange={e => setOp(e.target.value as AssertOp)}>
            <option value="eq">eq</option><option value="contains">contains</option><option value="regex">regex</option>
          </select>
          <input placeholder="우변 (예: 200)" value={right} onChange={e => setRight(e.target.value)} style={{ minWidth: 200 }} />
        </div>
      )}
      {kind === 'sleep' && (
        <div className="add-row">
          <input placeholder="초" value={seconds} onChange={e => setSeconds(e.target.value)} style={{ width: 100 }} />
        </div>
      )}

      <div className="add-row" style={{ marginTop: 6 }}>
        <button className="accent" onClick={submit}>{editing ? '수정 저장' : '이 스텝 추가'}</button>
        {editing && onCancel && <button onClick={onCancel}>취소</button>}
        {!editing && <span className="dim" style={{ fontSize: 12 }}>흐름 맨 끝에 추가됩니다. 순서는 표에서 번호로 조정.</span>}
      </div>
    </div>
  )
}
