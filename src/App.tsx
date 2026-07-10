import { useState } from 'react'
import './App.css'
import '@vscode/codicons/dist/codicon.css'
import CaptureView from './views/CaptureView'
import EnvironmentsView from './views/EnvironmentsView'
import HistoryView from './views/HistoryView'
import RunView from './views/RunView'
import ScenarioBuilder from './views/ScenarioBuilder'
import UiSuiteView from './views/UiSuiteView'

const tabs = [
  { key: 'run', label: '실행', icon: 'play' },
  { key: 'scenarios', label: '시나리오', icon: 'list-tree' },
  { key: 'capture', label: '캡처', icon: 'record' },
  { key: 'suite', label: 'UI 테스트', icon: 'beaker' },
  { key: 'envs', label: '환경', icon: 'server-environment' },
  { key: 'history', label: '히스토리', icon: 'history' },
] as const

export default function App() {
  const [tab, setTab] = useState<string>('run')
  return (
    <div className="app">
      <div className="workspace">
        <nav className="activitybar" aria-label="주 메뉴">
          {tabs.map(t => (
            <button
              key={t.key}
              className={`activity-item ${tab === t.key ? 'active' : ''}`}
              title={t.label}
              aria-label={t.label}
              onClick={() => setTab(t.key)}
            >
              <span className={`codicon codicon-${t.icon}`} aria-hidden="true" />
            </button>
          ))}
        </nav>
        <main className="main">
          <div style={{ display: tab === 'run' ? undefined : 'none' }}>
            <RunView active={tab === 'run'} />
          </div>
          <div style={{ display: tab === 'capture' ? undefined : 'none' }}>
            <CaptureView />
          </div>
          {tab === 'scenarios' && <ScenarioBuilder />}
          {tab === 'suite' && <UiSuiteView />}
          {tab === 'envs' && <EnvironmentsView />}
          {tab === 'history' && <HistoryView />}
        </main>
      </div>
    </div>
  )
}
