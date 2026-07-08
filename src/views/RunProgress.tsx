import type { StepRow } from '../hooks/useRun'

const ICON: Record<StepRow['status'], string> = {
  pending: '⚪', running: '🔵', passed: '✅', failed: '❌', skipped: '⏭️',
}

/** 실행 스텝 진행상황 렌더. RunView와 시나리오 인라인 실행이 공유한다. */
export function RunProgress({ rows }: { rows: StepRow[] }) {
  if (rows.length === 0) return null
  return (
    <ol className="run-steps">
      {rows.map((r, i) => (
        <li key={i}>
          <div>
            {ICON[r.status]} <span className="dim">[{r.type}]</span> {r.name}
            {r.duration_ms > 0 && <span className="dim"> — {r.duration_ms}ms</span>}
          </div>
          {r.detail && <pre className="detail">{r.detail}</pre>}
        </li>
      ))}
    </ol>
  )
}
