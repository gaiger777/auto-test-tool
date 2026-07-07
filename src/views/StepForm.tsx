import type { AssertOp, StepDef } from '../types'

interface Props {
  step: StepDef
  onChange: (s: StepDef) => void
}

export default function StepForm({ step, onChange }: Props) {
  const set = (patch: Partial<StepDef & { [k: string]: unknown }>) =>
    onChange({ ...step, ...patch } as StepDef)

  const common = (
    <>
      <label className="field">스텝 이름
        <input value={step.name} onChange={e => set({ name: e.target.value })} />
      </label>
      <label className="check">
        <input type="checkbox" checked={!!step.cleanup}
          onChange={e => set({ cleanup: e.target.checked })} /> cleanup (실패해도 항상 실행)
      </label>
    </>
  )

  switch (step.type) {
    case 'http_call':
      return (
        <div className="step-form">
          {common}
          <label className="field">메서드
            <select value={step.method} onChange={e => set({ method: e.target.value })}>
              {['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map(m => <option key={m}>{m}</option>)}
            </select>
          </label>
          <label className="field">URL
            <input value={step.url} onChange={e => set({ url: e.target.value })}
              placeholder="{{base_url.nova}}/servers" />
          </label>
          <label className="field">헤더 (JSON)
            <textarea rows={2} value={JSON.stringify(step.headers ?? {})}
              onChange={e => { try { set({ headers: JSON.parse(e.target.value) }) } catch { /* 입력 중 무시 */ } }} />
          </label>
          <label className="field">바디
            <textarea rows={4} value={step.body ?? ''} onChange={e => set({ body: e.target.value || null })} />
          </label>
          <label className="field">기대 상태코드
            <input value={step.expect_status ?? ''} placeholder="예: 202 (비우면 검사 안 함)"
              onChange={e => set({ expect_status: e.target.value ? Number(e.target.value) : null })} />
          </label>
          <label className="field">변수 캡처 (JSON 배열)
            <textarea rows={2} value={JSON.stringify(step.captures ?? [])}
              placeholder='[{"var":"server_id","json_path":"$.server.id"}]'
              onChange={e => { try { set({ captures: JSON.parse(e.target.value) }) } catch { /* 입력 중 무시 */ } }} />
          </label>
        </div>
      )
    case 'wait_event':
      return (
        <div className="step-form">
          {common}
          <label className="field">이벤트 타입
            <input value={step.event_type} placeholder="compute.instance.create.end"
              onChange={e => set({ event_type: e.target.value })} />
          </label>
          <label className="field">조건 (JSON 배열)
            <textarea rows={2} value={JSON.stringify(step.conditions ?? [])}
              placeholder='[{"json_path":"$.payload.instance_id","equals":"{{server_id}}"}]'
              onChange={e => { try { set({ conditions: JSON.parse(e.target.value) }) } catch { /* 입력 중 무시 */ } }} />
          </label>
          <label className="field">타임아웃(초)
            <input value={step.timeout_secs} onChange={e => set({ timeout_secs: Number(e.target.value) || 0 })} />
          </label>
        </div>
      )
    case 'assert':
      return (
        <div className="step-form">
          {common}
          <label className="field">좌변 <input value={step.left} placeholder="{{server_id}}"
            onChange={e => set({ left: e.target.value })} /></label>
          <label className="field">연산
            <select value={step.op} onChange={e => set({ op: e.target.value as AssertOp })}>
              <option value="eq">같음</option>
              <option value="contains">포함</option>
              <option value="regex">정규식</option>
            </select>
          </label>
          <label className="field">우변 <input value={step.right}
            onChange={e => set({ right: e.target.value })} /></label>
        </div>
      )
    case 'sleep':
      return (
        <div className="step-form">
          {common}
          <label className="field">대기(초)
            <input value={step.seconds} onChange={e => set({ seconds: Number(e.target.value) || 0 })} />
          </label>
        </div>
      )
  }
}
