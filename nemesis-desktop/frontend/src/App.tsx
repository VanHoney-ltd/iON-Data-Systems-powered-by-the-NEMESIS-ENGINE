import {useEffect, useMemo, useState} from 'react';
import './App.css';
import {GetBootstrapConfig} from '../wailsjs/go/main/App';

type CaseEntry = {
  case_id: string;
  root_path: string;
  evidence_dir: string;
  manifest_path?: string;
  last_run_status?: string;
};

type ReviewSummary = {
  findings_total: number;
  high: number;
  medium: number;
  low: number;
};

type AgentEntry = {
  slug: string;
  name: string;
  category: string;
  ui_route: string;
  status: string;
  record_count: number;
  record_types: Record<string, number>;
};

type CaseManifest = {
  case_id: string;
  generated_at: string;
  agents: AgentEntry[];
  navigation: Array<{
    label: string;
    slug: string;
    route: string;
    category: string;
    enabled: boolean;
    record_count: number;
  }>;
  review?: ReviewSummary;
};

type StreamEvent = {
  type: string;
  ts?: string;
  case_id?: string;
  payload?: Record<string, unknown>;
};

function App() {
  const [serverUrl, setServerUrl] = useState('http://127.0.0.1:17870');
  const [cases, setCases] = useState<CaseEntry[]>([]);
  const [selectedCase, setSelectedCase] = useState<string>('');
  const [manifest, setManifest] = useState<CaseManifest | null>(null);
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    GetBootstrapConfig()
      .then((config) => {
        setServerUrl(config.serverUrl);
        return loadCases(config.serverUrl);
      })
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    const source = new EventSource(`${serverUrl}/api/events`);
    source.addEventListener('message', (event) => {
      const parsed = JSON.parse(event.data) as StreamEvent;
      setEvents((current) => [parsed, ...current].slice(0, 80));
      if (parsed.type === 'run_completed' && parsed.case_id) {
        loadManifest(serverUrl, parsed.case_id);
        loadCases(serverUrl);
      }
    });
    source.addEventListener('connected', () => {
      setEvents((current) => [
        {type: 'connected', ts: new Date().toISOString(), payload: {status: 'connected'}},
        ...current,
      ]);
    });
    source.onerror = () => {
      setEvents((current) => [
        {type: 'stream_error', ts: new Date().toISOString(), payload: {status: 'disconnected'}},
        ...current,
      ].slice(0, 80));
    };
    return () => source.close();
  }, [serverUrl]);

  const totals = useMemo(() => {
    if (!manifest) return {records: 0, agents: 0};
    return {
      records: manifest.agents.reduce((sum, agent) => sum + agent.record_count, 0),
      agents: manifest.agents.length,
    };
  }, [manifest]);

  async function loadCases(baseUrl = serverUrl) {
    setError(null);
    const response = await fetch(`${baseUrl}/api/cases`);
    if (!response.ok) throw new Error(`case list failed: ${response.status}`);
    const data = await response.json() as CaseEntry[];
    setCases(data);
    if (!selectedCase && data.length > 0) {
      setSelectedCase(data[0].case_id);
      await loadManifest(baseUrl, data[0].case_id);
    }
  }

  async function loadManifest(baseUrl = serverUrl, caseId = selectedCase) {
    if (!caseId) return;
    setError(null);
    const response = await fetch(`${baseUrl}/api/cases/${caseId}/manifest`);
    if (!response.ok) throw new Error(`manifest failed: ${response.status}`);
    setManifest(await response.json() as CaseManifest);
  }

  async function runAll() {
    if (!selectedCase) return;
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(`${serverUrl}/api/cases/${selectedCase}/run-all`, {method: 'POST'});
      if (!response.ok) throw new Error(`run-all failed: ${response.status}`);
      setEvents((current) => [
        {type: 'run_queued', ts: new Date().toISOString(), case_id: selectedCase},
        ...current,
      ].slice(0, 80));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  async function selectCase(caseId: string) {
    setSelectedCase(caseId);
    await loadManifest(serverUrl, caseId);
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="mark">iON</span>
          <span>Nemesis Engine</span>
        </div>
        <button className="primary" disabled={!selectedCase || loading} onClick={runAll}>
          Run All Agents
        </button>
        <div className="case-list">
          <div className="section-title">Cases</div>
          {cases.map((entry) => (
            <button
              key={entry.case_id}
              className={entry.case_id === selectedCase ? 'case active' : 'case'}
              onClick={() => selectCase(entry.case_id)}
            >
              <span>{entry.case_id}</span>
              <small>{entry.last_run_status || 'not staged'}</small>
            </button>
          ))}
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <h1>{selectedCase || 'Select a case'}</h1>
            <p>{manifest ? `Manifest generated ${manifest.generated_at}` : 'Waiting for case manifest'}</p>
          </div>
          <div className={error ? 'status bad' : 'status'}>
            {error || `${serverUrl}`}
          </div>
        </header>

        <section className="metrics">
          <div className="metric">
            <span>Total Records</span>
            <strong>{totals.records.toLocaleString()}</strong>
          </div>
          <div className="metric">
            <span>Agents</span>
            <strong>{totals.agents}</strong>
          </div>
          <div className="metric">
            <span>Review Findings</span>
            <strong>{manifest?.review?.findings_total ?? 0}</strong>
          </div>
          <div className="metric">
            <span>High / Medium</span>
            <strong>{manifest?.review ? `${manifest.review.high} / ${manifest.review.medium}` : '0 / 0'}</strong>
          </div>
        </section>

        <section className="content-grid">
          <div className="panel wide">
            <div className="panel-head">
              <h2>Agent Status</h2>
            </div>
            <div className="agent-grid">
              {manifest?.agents.map((agent) => (
                <div className="agent-card" key={agent.slug}>
                  <div>
                    <strong>{agent.name}</strong>
                    <span>{agent.category}</span>
                  </div>
                  <div className="agent-count">{agent.record_count.toLocaleString()}</div>
                  <div className="agent-route">{agent.ui_route}</div>
                </div>
              ))}
            </div>
          </div>

          <div className="panel">
            <div className="panel-head">
              <h2>Navigation</h2>
            </div>
            <div className="nav-list">
              {manifest?.navigation.map((item) => (
                <div className={item.enabled ? 'nav-item' : 'nav-item disabled'} key={item.slug}>
                  <span>{item.label}</span>
                  <small>{item.record_count.toLocaleString()}</small>
                </div>
              ))}
            </div>
          </div>

          <div className="panel">
            <div className="panel-head">
              <h2>Event Stream</h2>
            </div>
            <div className="event-list">
              {events.map((event, index) => (
                <div className="event" key={`${event.type}-${index}`}>
                  <span>{event.type}</span>
                  <small>{event.case_id || ''}</small>
                </div>
              ))}
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}

export default App;
