import type { UiAction, UiKind } from './types'

export const kindLabel = (k: UiKind | string): string =>
  ({ click: '클릭', input: '입력', hover: '호버', http_call: 'HTTP', wait_event: '이벤트대기', assert: '검증', sleep: '대기' } as Record<string, string>)[k] || k

// 표의 셀렉터 열에 보여줄 요약: 프로그램 스텝은 설정 요약, UI 스텝은 첫 셀렉터.
export const stepSummary = (a: UiAction): string => {
  const s = a.step || {}
  switch (a.kind) {
    case 'http_call': return `${s.method || 'GET'} ${s.url || ''}${s.expect_status != null ? ' → ' + s.expect_status : ''}`
    case 'wait_event': {
      const conds = (s.conditions || []).map(c => `${c.json_path}=${c.equals}`).join(', ')
      return `${s.event_type || ''} (${s.timeout_secs ?? 30}s)${conds ? ` [${conds}]` : ''}`
    }
    case 'assert': return `${s.left || ''} ${s.op || 'eq'} ${s.right || ''}`
    case 'sleep': return `${s.seconds ?? 0}초`
    default: return a.selectors[0] ? `${a.selectors[0].strategy}: ${a.selectors[0].value}` : ''
  }
}
