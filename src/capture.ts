import type { StepDef } from './types'

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
