import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

interface MqLog { event_type: string; text: string }
interface LogRow { ts: string; event_type: string; text: string }

const pretty = (t: string) => { try { return JSON.stringify(JSON.parse(t), null, 2) } catch { return t } }

// 하단 고정 RabbitMQ 로그 패널. 'mq-log' 이벤트를 수신해 누적 표시하고, 행 클릭 시 JSON 상세를 연다.
// onConnected: '(연결)' 안내가 오면 호출(상위에서 실패 경고를 지우는 용도).
export default function MqLogPanel({ height = 200, onConnected }: { height?: number; onConnected?: () => void }) {
  const [rows, setRows] = useState<LogRow[]>([])
  const [detail, setDetail] = useState<LogRow | null>(null)
  const [exclude, setExclude] = useState(() => localStorage.getItem('mqlog.exclude') ?? '')
  const boxRef = useRef<HTMLDivElement>(null)
  const onConnRef = useRef(onConnected)
  useEffect(() => { onConnRef.current = onConnected }, [onConnected])

  const changeExclude = (v: string) => { setExclude(v); localStorage.setItem('mqlog.exclude', v) }
  // 쉼표로 구분된 제외어. event_type 에 하나라도 포함되면 숨긴다.('(연결)' 등 안내는 항상 표시)
  const excludeTerms = exclude.split(',').map(s => s.trim().toLowerCase()).filter(Boolean)
  const isInfo = (et: string) => et.startsWith('(')
  const visible = rows.filter(r => isInfo(r.event_type) || !excludeTerms.some(t => r.event_type.toLowerCase().includes(t)))

  useEffect(() => {
    const un = listen<MqLog>('mq-log', e => {
      const ts = new Date().toLocaleTimeString()
      if (e.payload.event_type === '(연결)') onConnRef.current?.()
      setRows(prev => [...prev.slice(-300), { ts, event_type: e.payload.event_type, text: e.payload.text }])
    })
    return () => { un.then(u => u()) }
  }, [])

  useEffect(() => {
    const el = boxRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [rows])

  return (
    <div style={{ marginTop: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4, flexWrap: 'wrap' }}>
        <strong style={{ fontSize: 13 }}>RabbitMQ 로그 ({visible.length}/{rows.length})</strong>
        <button onClick={() => setRows([])} disabled={!rows.length}>지우기</button>
        <input value={exclude} onChange={e => changeExclude(e.target.value)}
          placeholder="제외할 event_type (쉼표, 예: identity.authenticate)" style={{ minWidth: 300, fontSize: 12 }} />
        <span className="dim" style={{ fontSize: 11 }}>행 클릭 = JSON 상세</span>
      </div>
      <div ref={boxRef} style={{
        height, overflow: 'auto', border: '1px solid var(--vsc-border)', borderRadius: 6,
        background: 'var(--vsc-bg-alt, #1e1e1e)', padding: 8, fontFamily: 'monospace', fontSize: 12,
      }}>
        {rows.length === 0
          ? <span className="dim">환경(RabbitMQ)이 연결되면 수신 메시지가 여기에 표시됩니다.</span>
          : visible.map((r, i) => (
            <div key={i} onClick={() => setDetail(r)}
              style={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', cursor: 'pointer', padding: '1px 0' }}>
              <span className="dim">{r.ts}</span>{' '}
              <span style={{ color: 'var(--vsc-accent, #4daafc)' }}>{r.event_type}</span>{' '}
              <span className="dim">{r.text}</span>
            </div>
          ))}
      </div>

      {detail && (
        <div onClick={() => setDetail(null)} style={{
          position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)', zIndex: 1000,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}>
          <div onClick={e => e.stopPropagation()} style={{
            width: 'min(900px, 90vw)', maxHeight: '85vh', display: 'flex', flexDirection: 'column',
            background: 'var(--vsc-bg, #1e1e1e)', border: '1px solid var(--vsc-border)', borderRadius: 8, padding: 12,
          }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
              <strong style={{ color: 'var(--vsc-accent, #4daafc)' }}>{detail.event_type}</strong>
              <span className="dim" style={{ fontSize: 12 }}>{detail.ts}</span>
              <span style={{ flex: 1 }} />
              <button onClick={() => { navigator.clipboard?.writeText(pretty(detail.text)).catch(() => {}) }}>복사</button>
              <button onClick={() => setDetail(null)}>닫기 ✕</button>
            </div>
            <pre style={{
              margin: 0, overflow: 'auto', flex: 1, fontFamily: 'monospace', fontSize: 12,
              background: 'var(--vsc-bg-alt, #111)', border: '1px solid var(--vsc-border)', borderRadius: 6, padding: 10,
              whiteSpace: 'pre-wrap', wordBreak: 'break-word',
            }}>{pretty(detail.text)}</pre>
          </div>
        </div>
      )}
    </div>
  )
}
