import { useState } from 'react'
import './App.css'
import CaptureView from './views/CaptureView'
import EnvironmentsView from './views/EnvironmentsView'
import HistoryView from './views/HistoryView'
import RunView from './views/RunView'
import ScenarioBuilder from './views/ScenarioBuilder'

const tabs = [
  { key: 'run', label: '실행' },
  { key: 'scenarios', label: '시나리오' },
  { key: 'capture', label: '캡처' },
  { key: 'envs', label: '환경' },
  { key: 'history', label: '히스토리' },
] as const

export default function App() {
  const [tab, setTab] = useState<string>('run')
  return (
    <main>
      <nav className="tabs">
        {tabs.map(t => (
          <button key={t.key} className={tab === t.key ? 'active' : ''} onClick={() => setTab(t.key)}>
            {t.label}
          </button>
        ))}
      </nav>
      <div style={{ display: tab === 'run' ? undefined : 'none' }}>
        <RunView active={tab === 'run'} />
      </div>
      <div style={{ display: tab === 'capture' ? undefined : 'none' }}>
        <CaptureView />
      </div>
      {tab === 'scenarios' && <ScenarioBuilder />}
      {tab === 'envs' && <EnvironmentsView />}
      {tab === 'history' && <HistoryView />}
    </main>
  )
}
