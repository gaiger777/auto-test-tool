import { useState } from 'react'

// 캡처 라이브(CapturedCall)와 저장된 UiCall 모두 만족하는 최소 형태
export interface CallLike {
  method: string
  url: string
  status: number
  request_headers?: Record<string, string>
  request_body?: string | null
}

function pretty(body?: string | null): string {
  if (!body) return ''
  try { return JSON.stringify(JSON.parse(body), null, 2) } catch { return body }
}

export default function ApiCallsModal({ title, calls, onClose }: { title: string; calls: CallLike[]; onClose: () => void }) {
  const [open, setOpen] = useState<number | null>(calls.length === 1 ? 0 : null)

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-panel" onClick={e => e.stopPropagation()}>
        <div className="modal-head">
          <strong>API 호출 · {title}</strong>
          <button onClick={onClose}>닫기 ✕</button>
        </div>
        <div className="modal-body">
          {calls.length === 0 && <p className="dim">호출 정보가 없습니다.</p>}
          {calls.map((c, i) => {
            const headers = c.request_headers || {}
            const hkeys = Object.keys(headers)
            const body = pretty(c.request_body)
            return (
              <div key={i} className="call-item">
                <button className="call-row" onClick={() => setOpen(open === i ? null : i)}>
                  <span className="mono">{open === i ? '▾' : '▸'}</span>
                  <span className="method">{c.method}</span>
                  <span className={c.status >= 400 && c.status !== 401 ? 'status-bad' : 'status-ok'}>{c.status}</span>
                  <span className="url" title={c.url}>{c.url}</span>
                </button>
                {open === i && (
                  <div className="call-detail">
                    <div className="detail-label">Request Headers ({hkeys.length})</div>
                    {hkeys.length === 0
                      ? <p className="dim">헤더 없음</p>
                      : <table className="kv"><tbody>
                          {hkeys.map(k => <tr key={k}><td className="kv-k">{k}</td><td className="kv-v">{headers[k]}</td></tr>)}
                        </tbody></table>}
                    <div className="detail-label">Request Body</div>
                    {body ? <pre className="body-pre">{body}</pre> : <p className="dim">본문 없음</p>}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
