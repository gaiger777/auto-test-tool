import type { StepDef, UiAction } from './types'

export interface CapturedCall {
  id: string
  method: string
  url: string
  request_headers: Record<string, string>
  request_body: string | null
  status: number
  response_body: string | null
  timestamp: number
}

function pathOf(url: string): string {
  try {
    return new URL(url).pathname
  } catch {
    return url
  }
}

/** URL을 실행마다 달라지는 부분(쿼리·UUID·숫자 id)을 * 로 지운 경로 패턴으로 정규화한다.
 *  재생 검증에서 "같은 API 호출"을 매칭하는 데 쓴다. */
export function normalizePath(url: string): string {
  const p = pathOf(url)
  return p
    .split('/')
    .map(seg =>
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(seg) ? '*'
        : /^\d+$/.test(seg) ? '*'
          : /^[0-9a-f]{24,}$/i.test(seg) ? '*'
            : seg)
    .join('/')
}

/** 각 UI 동작에, 그 동작 직후~다음 동작 직전 사이에 발생한 네트워크 호출을 묶는다.
 *  (첫 동작 이전의 호출 = 초기 로드는 버린다.) 반환: 동작 id -> 호출 목록 (시간순). */
export function correlateCalls(actions: UiAction[], calls: CapturedCall[]): Record<string, CapturedCall[]> {
  const acts = [...actions].sort((a, b) => a.timestamp - b.timestamp)
  const map: Record<string, CapturedCall[]> = {}
  acts.forEach(a => { map[a.id] = [] })
  for (const c of calls) {
    let owner: UiAction | null = null
    for (const a of acts) {
      if (a.timestamp <= c.timestamp) owner = a
      else break
    }
    if (owner) map[owner.id].push(c)
  }
  for (const id of Object.keys(map)) map[id].sort((a, b) => a.timestamp - b.timestamp)
  return map
}

/** 선택된 캡처들을 http_call 스텝으로 변환한다.
 *  tokenHeaderName과 (대소문자 무시) 일치하는 헤더 값만 {{auth_token}}으로 치환하고
 *  method/url/body는 그대로 둔다. */
export function capturesToSteps(calls: CapturedCall[], tokenHeaderName: string): StepDef[] {
  const tokenLower = tokenHeaderName.toLowerCase()
  return calls.map(c => {
    const headers: Record<string, string> = {}
    for (const [k, v] of Object.entries(c.request_headers)) {
      headers[k] = k.toLowerCase() === tokenLower ? '{{auth_token}}' : v
    }
    return {
      name: `${c.method} ${pathOf(c.url)}`,
      type: 'http_call',
      method: c.method,
      url: c.url,
      headers,
      body: c.request_body,
      expect_status: c.status,
      captures: [],
    }
  })
}
