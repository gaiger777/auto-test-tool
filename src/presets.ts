import type { StepDef } from './types'

export interface PresetField { key: string; label: string; placeholder?: string }

export interface Preset {
  id: string
  label: string
  fields: PresetField[]
  expand: (input: Record<string, string>) => StepDef[]
}

const token = { 'X-Auth-Token': '{{auth_token}}' }

export const presets: Preset[] = [
  {
    id: 'create_instance',
    label: '인스턴스 생성',
    fields: [
      { key: 'name', label: '서버 이름' },
      { key: 'image_ref', label: '이미지 ID' },
      { key: 'flavor_ref', label: '플레이버 ID' },
      { key: 'network_id', label: '네트워크 ID' },
    ],
    expand: i => [
      {
        name: `인스턴스 생성: ${i.name}`,
        type: 'http_call',
        method: 'POST',
        url: '{{base_url.nova}}/servers',
        headers: token,
        body: JSON.stringify({
          server: {
            name: i.name, imageRef: i.image_ref, flavorRef: i.flavor_ref,
            networks: [{ uuid: i.network_id }],
          },
        }),
        expect_status: 202,
        captures: [{ var: 'server_id', json_path: '$.server.id' }],
      },
      {
        name: '인스턴스 생성 완료 대기',
        type: 'wait_event',
        event_type: 'compute.instance.create.end',
        conditions: [{ json_path: '$.payload.instance_id', equals: '{{server_id}}' }],
        timeout_secs: 600,
      },
    ],
  },
  {
    id: 'delete_instance',
    label: '인스턴스 삭제 (cleanup)',
    fields: [{ key: 'server_id_var', label: '서버 ID 변수명', placeholder: 'server_id' }],
    expand: i => {
      const v = i.server_id_var || 'server_id'
      return [
        {
          name: '인스턴스 삭제', cleanup: true, type: 'http_call', method: 'DELETE',
          url: `{{base_url.nova}}/servers/{{${v}}}`, headers: token, expect_status: 204,
        },
        {
          name: '인스턴스 삭제 완료 대기', cleanup: true, type: 'wait_event',
          event_type: 'compute.instance.delete.end',
          conditions: [{ json_path: '$.payload.instance_id', equals: `{{${v}}}` }],
          timeout_secs: 120,
        },
      ]
    },
  },
  {
    id: 'create_network',
    label: '네트워크 생성',
    fields: [{ key: 'name', label: '네트워크 이름' }],
    expand: i => [
      {
        name: `네트워크 생성: ${i.name}`, type: 'http_call', method: 'POST',
        url: '{{base_url.neutron}}/v2.0/networks', headers: token,
        body: JSON.stringify({ network: { name: i.name } }),
        expect_status: 201,
        captures: [{ var: 'network_id', json_path: '$.network.id' }],
      },
    ],
  },
  {
    id: 'delete_network',
    label: '네트워크 삭제 (cleanup)',
    fields: [{ key: 'network_id_var', label: '네트워크 ID 변수명', placeholder: 'network_id' }],
    expand: i => {
      const v = i.network_id_var || 'network_id'
      return [{
        name: '네트워크 삭제', cleanup: true, type: 'http_call', method: 'DELETE',
        url: `{{base_url.neutron}}/v2.0/networks/{{${v}}}`, headers: token, expect_status: 204,
      }]
    },
  },
  {
    id: 'create_volume',
    label: '볼륨 생성',
    fields: [
      { key: 'name', label: '볼륨 이름' },
      { key: 'size', label: '크기(GB)', placeholder: '10' },
    ],
    expand: i => [
      {
        name: `볼륨 생성: ${i.name}`, type: 'http_call', method: 'POST',
        url: '{{base_url.cinder}}/volumes', headers: token,
        body: JSON.stringify({ volume: { name: i.name, size: Number(i.size) || 1 } }),
        expect_status: 202,
        captures: [{ var: 'volume_id', json_path: '$.volume.id' }],
      },
      {
        name: '볼륨 생성 완료 대기', type: 'wait_event',
        event_type: 'volume.create.end',
        conditions: [{ json_path: '$.payload.volume_id', equals: '{{volume_id}}' }],
        timeout_secs: 300,
      },
    ],
  },
  {
    id: 'delete_volume',
    label: '볼륨 삭제 (cleanup)',
    fields: [{ key: 'volume_id_var', label: '볼륨 ID 변수명', placeholder: 'volume_id' }],
    expand: i => {
      const v = i.volume_id_var || 'volume_id'
      return [
        {
          name: '볼륨 삭제', cleanup: true, type: 'http_call', method: 'DELETE',
          url: `{{base_url.cinder}}/volumes/{{${v}}}`, headers: token, expect_status: 202,
        },
        {
          name: '볼륨 삭제 완료 대기', cleanup: true, type: 'wait_event',
          event_type: 'volume.delete.end',
          conditions: [{ json_path: '$.payload.volume_id', equals: `{{${v}}}` }],
          timeout_secs: 120,
        },
      ]
    },
  },
]
