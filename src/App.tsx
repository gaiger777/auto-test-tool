import { useState } from 'react'
import './App.css'
import EnvironmentsView from './views/EnvironmentsView'
import HistoryView from './views/HistoryView'
import RunView from './views/RunView'
import ScenarioBuilder from './views/ScenarioBuilder'

const tabs = [
  { key: 'run', label: '실행', el: <RunView /> },
  { key: 'scenarios', label: '시나리오', el: <ScenarioBuilder /> },
  { key: 'envs', label: '환경', el: <EnvironmentsView /> },
  { key: 'history', label: '히스토리', el: <HistoryView /> },
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
      {tabs.find(t => t.key === tab)?.el}
    </main>
  )
}
