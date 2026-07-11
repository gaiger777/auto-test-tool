import * as api from './api'
import type { UiProgStep } from './types'

// 재생 중 wait_event 위임 신호(status='delegate')를 백엔드 MQ로 실행한 뒤,
// 같은 재생 창에서 다음 스텝부터 재개시킨다. 실제 스텝 결과 표시는 재개된
// 플레이어가 다시 보고(ui-replay-step)하므로 여기서 화면 상태는 건드리지 않는다.
export async function runDelegatedStep(index: number, detailJson: string, envId: number | null) {
  let name = ''
  let step: UiProgStep = {}
  try {
    const p = JSON.parse(detailJson)
    name = p.name || ''
    step = p.step || {}
  } catch { /* detail 파싱 실패 시 빈 스텝 */ }

  let status = 'passed'
  let detail = ''
  if (envId == null) {
    status = 'failed'
    detail = (name ? name + ' · ' : '') + 'wait_event: 환경(MQ)이 선택되지 않았습니다'
  } else {
    try {
      const ev = await api.runWaitEvent(step.event_type || '', step.conditions || [], step.timeout_secs ?? 30)
      detail = (name ? name + ' · ' : '') + '이벤트 수신: ' + ev.slice(0, 200)
    } catch (e) {
      status = 'failed'
      detail = (name ? name + ' · ' : '') + String(e)
    }
  }
  try { await api.resumeUiReplay(index + 1, status, detail) } catch { /* 창이 닫혔으면 무시 */ }
}
