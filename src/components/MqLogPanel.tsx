import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { mqLogFor, type LogRow } from '../mqLog'

const pretty = (t: string) => { try { return JSON.stringify(JSON.parse(t), null, 2) } catch { return t } }

// 하단 고정 RabbitMQ 로그 패널. 로그 rows는 전역 스토어(단일 연결)에서 공유하지만,
// 제외 필터는 storageKey별로 화면마다 독립 저장한다.
// 행 클릭 시 JSON 상세 모달. onConnected: '(연결)' 안내가 오면 호출(상위 실패 경고 제거).
export default function MqLogPanel({ height = 200, onConnected, storageKey = 'default' }:
  { height?: number; onConnected?: () => void; storageKey?: string }) {
  const mqLog = mqLogFor(storageKey) // 채널별 독립 로그 스토어
  const { rows, connectSeq } = useSyncExternalStore(mqLog.subscribe, mqLog.getSnapshot)
  const [detail, setDetail] = useState<LogRow | null>(null)
  const exKey = `mqlog.exclude.${storageKey}`
  const [exclude, setExclude] = useState(() => localStorage.getItem(exKey) ?? '') // 적용된 필터
  const [draft, setDraft] = useState(exclude) // 입력 중인 값(적용 버튼/Enter로만 반영)
  const boxRef = useRef<HTMLDivElement>(null)

  // 연결 성공 시 상위 경고 제거 (마운트 이후 새 연결에만 반응)
  const seqRef = useRef(connectSeq)
  useEffect(() => {
    if (connectSeq !== seqRef.current) { seqRef.current = connectSeq; onConnected?.() }
  }, [connectSeq, onConnected])

  const applyExclude = () => { setExclude(draft); localStorage.setItem(exKey, draft) }
  const excludeTerms = exclude.split(',').map(s => s.trim().toLowerCase()).filter(Boolean)
  const isInfo = (et: string) => et.startsWith('(')
  const visible = rows.filter(r => isInfo(r.event_type) || !excludeTerms.some(t => r.event_type.toLowerCase().includes(t)))

  useEffect(() => {
    const el = boxRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [rows])

  return (
    <div style={{ marginTop: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4, flexWrap: 'wrap' }}>
        <strong style={{ fontSize: 13 }}>RabbitMQ 로그 ({visible.length}/{rows.length})</strong>
        <button onClick={() => mqLog.clear()} disabled={!rows.length}>지우기</button>
        <input value={draft} onChange={e => setDraft(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') applyExclude() }}
          placeholder="제외할 event_type (쉼표, 예: identity.authenticate)" style={{ minWidth: 300, fontSize: 12 }} />
        <button onClick={applyExclude} disabled={draft === exclude}>적용</button>
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
