import { describe, expect, it } from 'vitest'
import { presets } from './presets'

const byId = (id: string) => {
  const p = presets.find(p => p.id === id)
  if (!p) throw new Error(`preset ${id} 없음`)
  return p
}

describe('presets', () => {
  it('인스턴스 생성 = http_call + wait_event, server_id 캡처와 참조가 연결된다', () => {
    const steps = byId('create_instance').expand({
      name: 'vm1', image_ref: 'img-1', flavor_ref: 'f-1', network_id: 'net-1',
    })
    expect(steps).toHaveLength(2)
    const [call, wait] = steps
    if (call.type !== 'http_call' || wait.type !== 'wait_event') throw new Error('스텝 타입 불일치')
    expect(call.captures?.[0]).toEqual({ var: 'server_id', json_path: '$.server.id' })
    expect(call.body).toContain('vm1')
    expect(wait.event_type).toBe('compute.instance.create.end')
    expect(wait.conditions?.[0].equals).toBe('{{server_id}}')
  })

  it('인스턴스 삭제 프리셋은 cleanup으로 표시된다', () => {
    const steps = byId('delete_instance').expand({ server_id_var: 'server_id' })
    expect(steps.every(s => s.cleanup)).toBe(true)
  })

  it('모든 프리셋이 최소 1개 스텝을 만든다', () => {
    for (const p of presets) {
      const input = Object.fromEntries(p.fields.map(f => [f.key, 'x']))
      expect(p.expand(input).length).toBeGreaterThan(0)
    }
  })
})
