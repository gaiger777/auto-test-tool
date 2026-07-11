import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

interface MqLog { event_type: string; text: string }
interface LogRow { ts: string; event_type: string; text: string }

// 하단 고정 RabbitMQ 로그 패널. 'mq-log' 이벤트를 수신해 누적 표시한다.
export default function MqLogPanel({ height = 200 }: { height?: number }) {
  const [rows, setRows] = useState<LogRow[]>([])
  const boxRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const un = listen<MqLog>('mq-log', e => {
      const ts = new Date().toLocaleTimeString()
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
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
        <strong style={{ fontSize: 13 }}>RabbitMQ 로그 ({rows.length})</strong>
        <button onClick={() => setRows([])} disabled={!rows.length}>지우기</button>
      </div>
      <div ref={boxRef} style={{
        height, overflow: 'auto', border: '1px solid var(--vsc-border)', borderRadius: 6,
        background: 'var(--vsc-bg-alt, #1e1e1e)', padding: 8, fontFamily: 'monospace', fontSize: 12,
      }}>
        {rows.length === 0
          ? <span className="dim">환경(RabbitMQ)이 연결되면 수신 메시지가 여기에 표시됩니다.</span>
          : rows.map((r, i) => (
            <div key={i} style={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }} title={r.text}>
              <span className="dim">{r.ts}</span>{' '}
              <span style={{ color: 'var(--vsc-accent, #4daafc)' }}>{r.event_type}</span>{' '}
              <span className="dim">{r.text}</span>
            </div>
          ))}
      </div>
    </div>
  )
}
