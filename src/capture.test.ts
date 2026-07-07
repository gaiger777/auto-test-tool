import { describe, expect, it } from 'vitest'
import { capturesToSteps, type CapturedCall } from './capture'

const call = (over: Partial<CapturedCall> = {}): CapturedCall => ({
  id: '1',
  method: 'POST',
  url: 'https://contrabass.example.com/api/servers',
  request_headers: { 'X-Auth-Token': 'live-token-abc', 'Content-Type': 'application/json' },
  request_body: '{"name":"vm1"}',
  status: 202,
  response_body: '{"id":"srv-1"}',
  timestamp: 0,
  ...over,
})

describe('capturesToSteps', () => {
  it('http_call 스텝으로 변환하고 method/url/body를 그대로 둔다', () => {
    const [step] = capturesToSteps([call()], 'X-Auth-Token')
    expect(step.type).toBe('http_call')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.method).toBe('POST')
    expect(step.url).toBe('https://contrabass.example.com/api/servers')
    expect(step.body).toBe('{"name":"vm1"}')
  })

  it('토큰 헤더만 {{auth_token}}으로 치환한다 (대소문자 무시)', () => {
    const [step] = capturesToSteps([call({ request_headers: { 'x-auth-token': 'live', 'Accept': 'application/json' } })], 'X-Auth-Token')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.headers!['x-auth-token']).toBe('{{auth_token}}')
    expect(step.headers!['Accept']).toBe('application/json')
  })

  it('응답 상태코드를 expect_status로 설정한다', () => {
    const [step] = capturesToSteps([call({ status: 201 })], 'X-Auth-Token')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.expect_status).toBe(201)
  })

  it('스텝 이름을 METHOD + 경로로 만든다 (쿼리 제외)', () => {
    const [step] = capturesToSteps([call({ url: 'https://h.example.com/api/servers?x=1' })], 'X-Auth-Token')
    expect(step.name).toBe('POST /api/servers')
  })

  it('토큰 헤더명이 없으면 헤더를 그대로 둔다', () => {
    const [step] = capturesToSteps([call()], 'Authorization')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.headers!['X-Auth-Token']).toBe('live-token-abc')
  })

  it('여러 캡처를 순서대로 변환한다', () => {
    const steps = capturesToSteps([call({ id: 'a', method: 'GET' }), call({ id: 'b', method: 'DELETE' })], 'X-Auth-Token')
    expect(steps.map(s => (s.type === 'http_call' ? s.method : ''))).toEqual(['GET', 'DELETE'])
  })

  it('URL 파싱 실패 시 원본 문자열을 이름 경로로 쓴다', () => {
    const [step] = capturesToSteps([call({ url: 'not a url' })], 'X-Auth-Token')
    expect(step.name).toBe('POST not a url')
    if (step.type !== 'http_call') throw new Error('타입 불일치')
    expect(step.url).toBe('not a url')
  })
})
