import { useMemo, useState } from 'react'
import type { UiFlowRecord } from '../types'

const GROUP_FALLBACK = '기본'
export const groupOf = (f: UiFlowRecord) => (f.grp && f.grp.trim()) || GROUP_FALLBACK
const grpLabel = (raw: string) => raw || GROUP_FALLBACK

interface Props {
  flows: UiFlowRecord[]
  selectedId?: number | null
  onPickFlow?: (f: UiFlowRecord) => void
  onPickMany?: (flows: UiFlowRecord[], label: string) => void
  onRenameFlow?: (f: UiFlowRecord, newName: string) => void
  onRenameGroup?: (siteUrl: string, oldGroup: string, newGroup: string) => void
}

// URL → 그룹 → 시나리오 트리. 리프 클릭은 onPickFlow, URL/그룹의 ▶ 는 onPickMany, ✎ 는 인라인 이름 변경.
export default function FlowTree({ flows, selectedId, onPickFlow, onPickMany, onRenameFlow, onRenameGroup }: Props) {
  const tree = useMemo(() => {
    const bySite = new Map<string, Map<string, UiFlowRecord[]>>()
    for (const f of flows) {
      if (!bySite.has(f.site_url)) bySite.set(f.site_url, new Map())
      const g = bySite.get(f.site_url)!
      const key = (f.grp || '').trim()
      if (!g.has(key)) g.set(key, [])
      g.get(key)!.push(f)
    }
    return bySite
  }, [flows])

  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const toggle = (k: string) => setCollapsed(c => ({ ...c, [k]: !c[k] }))

  // 인라인 편집: window.prompt 는 WKWebView 에서 막혀 있어 입력창을 직접 렌더한다.
  const [editKey, setEditKey] = useState<string | null>(null)
  const [editVal, setEditVal] = useState('')
  const beginEdit = (key: string, current: string) => { setEditKey(key); setEditVal(current) }
  const cancelEdit = () => { setEditKey(null); setEditVal('') }

  const commitFlow = (f: UiFlowRecord) => {
    const v = editVal.trim()
    cancelEdit()
    if (v && v !== f.name) onRenameFlow?.(f, v)
  }
  const commitGroup = (site: string, rawGrp: string) => {
    const v = editVal.trim()
    cancelEdit()
    if (v && v !== grpLabel(rawGrp)) onRenameGroup?.(site, rawGrp, v)
  }

  const editInput = (onCommit: () => void) => (
    <input autoFocus value={editVal} onChange={e => setEditVal(e.target.value)}
      onKeyDown={e => { if (e.key === 'Enter') onCommit(); else if (e.key === 'Escape') cancelEdit() }}
      onBlur={onCommit} style={{ flex: 1, fontSize: 13 }} />
  )

  if (flows.length === 0) return <p className="dim" style={{ fontSize: 12 }}>저장된 시나리오가 없습니다.</p>

  return (
    <div style={{ fontSize: 13, userSelect: 'none' }}>
      {[...tree.entries()].map(([site, groups]) => {
        const siteKey = 's:' + site
        const siteOpen = !collapsed[siteKey]
        const siteFlows = [...groups.values()].flat()
        return (
          <div key={site}>
            <div className="tree-row" style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '2px 0' }}>
              <button onClick={() => toggle(siteKey)} title="펼치기/접기" style={{ padding: '0 4px' }}>{siteOpen ? '▾' : '▸'}</button>
              <span className="codicon codicon-globe" aria-hidden="true" />
              <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={site}>{site}</span>
              {onPickMany && <button onClick={() => onPickMany(siteFlows, site)} title="이 사이트 전체 불러오기">▶</button>}
            </div>
            {siteOpen && [...groups.entries()].map(([rawGrp, list]) => {
              const grpKey = 'g:' + site + '::' + rawGrp
              const grpOpen = !collapsed[grpKey]
              const editing = editKey === grpKey
              return (
                <div key={rawGrp} style={{ marginLeft: 16 }}>
                  <div className="tree-row" style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '2px 0' }}>
                    <button onClick={() => toggle(grpKey)} style={{ padding: '0 4px' }}>{grpOpen ? '▾' : '▸'}</button>
                    <span className="codicon codicon-folder" aria-hidden="true" />
                    {editing
                      ? editInput(() => commitGroup(site, rawGrp))
                      : <span style={{ flex: 1 }}>{grpLabel(rawGrp)} <span className="dim">({list.length})</span></span>}
                    {onRenameGroup && !editing && <button onClick={() => beginEdit(grpKey, grpLabel(rawGrp))} title="그룹명 변경">✎</button>}
                    {onPickMany && !editing && <button onClick={() => onPickMany(list, grpLabel(rawGrp))} title="이 그룹 불러오기">▶</button>}
                  </div>
                  {grpOpen && list.map(f => {
                    const fKey = 'f:' + f.id
                    const fe = editKey === fKey
                    return (
                      <div key={f.id} className="tree-row" style={{ marginLeft: 20, display: 'flex', alignItems: 'center', gap: 4, padding: '2px 0' }}>
                        <span className="codicon codicon-file" aria-hidden="true" />
                        {fe
                          ? editInput(() => commitFlow(f))
                          : <span
                              onClick={() => onPickFlow?.(f)}
                              style={{ cursor: onPickFlow ? 'pointer' : 'default', flex: 1, fontWeight: selectedId === f.id ? 700 : 400,
                                color: selectedId === f.id ? 'var(--vsc-accent, #4daafc)' : undefined }}
                              title={f.name}>
                              {f.name}
                            </span>}
                        {onRenameFlow && !fe && <button onClick={() => beginEdit(fKey, f.name)} title="이름 변경">✎</button>}
                      </div>
                    )
                  })}
                </div>
              )
            })}
          </div>
        )
      })}
    </div>
  )
}
