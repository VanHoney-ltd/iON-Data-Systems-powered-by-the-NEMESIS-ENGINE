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

type AgentEntry = {
  slug: string;
  name: string;
  category: string;
  status: string;
  record_count: number;
  record_types: Record<string, number>;
};

type CaseManifest = {
  case_id: string;
  generated_at: string;
  agents: AgentEntry[];
};

type EvidenceRecord = Record<string, unknown>;

type SearchResponse = {
  total_matches: number;
  offset: number;
  limit: number;
  records: EvidenceRecord[];
};

type ReviewMode =
  | 'messages'
  | 'contacts'
  | 'notes'
  | 'photos'
  | 'videos'
  | 'locations'
  | 'apps'
  | 'finances'
  | 'safari'
  | 'google'
  | 'files';

type DeviceStatus = {
  detected: boolean;
  connected_count: number;
  paired_count: number;
  connected_devices: Array<{
    udid: string;
    pairing_state: string;
    device_info?: {
      device_name?: string;
      product_type?: string;
      product_version?: string;
    };
  }>;
};

const modeConfig: Record<ReviewMode, {label: string; agent: string; type: string; hint: string; source?: string; defaultQuery?: string}> = {
  messages: {
    label: 'Text Messages',
    agent: 'cerberus',
    type: 'message',
    hint: 'Search sent and received conversations, phone numbers, thread IDs, and message text.',
  },
  contacts: {
    label: 'Address Book',
    agent: 'cerberus',
    type: 'contact',
    hint: 'Search contacts, phone numbers, emails, labels, and contact notes.',
  },
  notes: {
    label: 'Notes',
    agent: 'obolus',
    type: 'note',
    hint: 'Search full extracted Apple Notes content and money-related note references.',
  },
  photos: {
    label: 'Pictures',
    agent: 'charon',
    type: 'asset',
    hint: 'Review photo metadata, filenames, dates, locations, and identifiers.',
  },
  videos: {
    label: 'Videos',
    agent: 'vigil',
    type: 'video',
    hint: 'Review video files, timestamps, durations, locations, and source paths.',
  },
  locations: {
    label: 'Locations',
    agent: 'atlas',
    type: 'location',
    hint: 'Search location activity from Maps, Google Maps, location caches, and related databases.',
  },
  apps: {
    label: 'Apps',
    agent: 'orpheus',
    type: 'app',
    hint: 'Review installed apps, app containers, databases, plists, and high-value targets.',
  },
  finances: {
    label: 'Finances',
    agent: 'plutus',
    type: 'transaction',
    hint: 'Search transactions, Cash App records, financial sources, and money movement.',
  },
  safari: {
    label: 'Safari History',
    agent: 'nyx',
    type: 'web_activity',
    source: 'Safari',
    hint: 'Search Safari browser history, titles, URLs, domains, and visit records.',
  },
  google: {
    label: 'Google History',
    agent: 'nyx',
    type: 'web_activity',
    defaultQuery: 'google',
    hint: 'Search Google-related browser activity, URLs, domains, titles, and queries.',
  },
  files: {
    label: 'Device Files',
    agent: 'orpheus',
    type: 'database',
    hint: 'Review files surfaced from app containers, databases, plists, and high-value device artifacts.',
  },
};

function App() {
  const [serverUrl, setServerUrl] = useState('http://127.0.0.1:17870');
  const [cases, setCases] = useState<CaseEntry[]>([]);
  const [selectedCase, setSelectedCase] = useState('pcr');
  const [deviceStatus, setDeviceStatus] = useState<DeviceStatus | null>(null);
  const [newCaseId, setNewCaseId] = useState(() => `case-${new Date().toISOString().slice(0, 10)}`);
  const [backupPassword, setBackupPassword] = useState('');
  const [manifest, setManifest] = useState<CaseManifest | null>(null);
  const [mode, setMode] = useState<ReviewMode>('messages');
  const [messageDirection, setMessageDirection] = useState<'all' | 'Sent' | 'Received'>('all');
  const [query, setQuery] = useState('');
  const [page, setPage] = useState<SearchResponse | null>(null);
  const [selected, setSelected] = useState<EvidenceRecord | null>(null);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const active = modeConfig[mode];
  const offset = page?.offset ?? 0;
  const limit = page?.limit ?? 100;
  const total = page?.total_matches ?? 0;

  const totals = useMemo(() => {
    const agents = manifest?.agents ?? [];
    return {
      messages: countType(agents, 'cerberus', 'message'),
      photos: countType(agents, 'charon', 'asset'),
      videos: countType(agents, 'vigil', 'video'),
      contacts: countType(agents, 'cerberus', 'contact'),
      notes: countType(agents, 'obolus', 'note'),
      locations: countType(agents, 'atlas', 'location'),
      apps: countType(agents, 'orpheus', 'app'),
      finances: countType(agents, 'plutus', 'transaction'),
      safari: countType(agents, 'nyx', 'web_activity'),
      google: countType(agents, 'nyx', 'web_activity'),
      files: countType(agents, 'orpheus', 'database') + countType(agents, 'orpheus', 'container') + countType(agents, 'orpheus', 'high_value_target'),
    };
  }, [manifest]);

  useEffect(() => {
    GetBootstrapConfig()
      .then(async (config) => {
        setServerUrl(config.serverUrl);
        await loadDeviceStatus(config.serverUrl);
        const foundCases = await loadCases(config.serverUrl);
        const initial = foundCases[0]?.case_id || '';
        setSelectedCase(initial);
        if (initial) {
          await loadManifest(config.serverUrl, initial);
          await loadRecords(config.serverUrl, initial, mode, query, 0);
        }
      })
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    if (!selectedCase) return;
    loadRecords(serverUrl, selectedCase, mode, query, 0);
  }, [mode]);

  async function loadCases(baseUrl = serverUrl) {
    const response = await fetch(`${baseUrl}/api/cases`);
    if (!response.ok) throw new Error(`case list failed: ${response.status}`);
    const data = await response.json() as CaseEntry[];
    setCases(data);
    return data;
  }

  async function loadDeviceStatus(baseUrl = serverUrl) {
    const response = await fetch(`${baseUrl}/api/device/status`);
    if (!response.ok) throw new Error(`device check failed: ${response.status}`);
    const data = await response.json() as DeviceStatus;
    setDeviceStatus(data);
    return data;
  }

  async function loadManifest(baseUrl = serverUrl, caseId = selectedCase) {
    const response = await fetch(`${baseUrl}/api/cases/${caseId}/manifest`);
    if (!response.ok) throw new Error(`manifest failed: ${response.status}`);
    setManifest(await response.json() as CaseManifest);
  }

  async function loadRecords(baseUrl = serverUrl, caseId = selectedCase, nextMode = mode, q = query, nextOffset = 0) {
    if (!caseId) return;
    setBusy(true);
    setError(null);
    const config = modeConfig[nextMode];
    try {
      const params = new URLSearchParams({
        type: config.type,
        limit: '100',
        offset: String(nextOffset),
      });
      if (nextMode === 'messages' && messageDirection !== 'all') params.set('direction', messageDirection);
      if (config.source) params.set('source', config.source);
      const effectiveQuery = q.trim() || config.defaultQuery || '';
      if (effectiveQuery) params.set('q', effectiveQuery);
      const response = await fetch(`${baseUrl}/api/cases/${caseId}/evidence/${config.agent}/search?${params}`);
      if (!response.ok) throw new Error(`${config.label} search failed: ${response.status}`);
      const data = await response.json() as SearchResponse;
      setPage(data);
      setSelected(data.records[0] || null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function selectCase(caseId: string) {
    setSelectedCase(caseId);
    await loadManifest(serverUrl, caseId);
    await loadRecords(serverUrl, caseId, mode, query, 0);
  }

  async function startNewBackup() {
    if (!newCaseId.trim()) {
      setError('Enter a case name first.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const latest = await loadDeviceStatus(serverUrl);
      if (!latest.detected) {
        throw new Error('No iOS device detected. Plug the iPhone in, unlock it, and trust this computer if prompted.');
      }
      const response = await fetch(`${serverUrl}/api/backups/start`, {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({
          case_id: newCaseId.trim(),
          backup_password: backupPassword,
        }),
      });
      if (!response.ok) {
        const body = await response.text();
        throw new Error(`backup start failed: ${response.status} ${body}`);
      }
      setSelectedCase(newCaseId.trim());
      setManifest(null);
      setPage(null);
      setSelected(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function saveSelection() {
    if (!selected || !selectedCase) return;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`${serverUrl}/api/cases/${selectedCase}/selections`, {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({
          mode,
          agent: active.agent,
          record_type: active.type,
          note,
          record: selected,
        }),
      });
      if (!response.ok) throw new Error(`selection save failed: ${response.status}`);
      setNote('');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="review-shell">
      <aside className="review-sidebar">
        <div className="brand">
          <span className="mark">iON</span>
          <span>Evidence Review</span>
        </div>

        <label className="field">
          <span>Case</span>
          <select value={selectedCase} onChange={(event) => selectCase(event.target.value)}>
            {!cases.length && <option value="">No staged cases yet</option>}
            {cases.map((entry) => (
              <option key={entry.case_id} value={entry.case_id}>{entry.case_id}</option>
            ))}
          </select>
        </label>

        <nav className="mode-list">
          {([
            ['messages', totals.messages],
            ['contacts', totals.contacts],
            ['notes', totals.notes],
            ['photos', totals.photos],
            ['videos', totals.videos],
            ['locations', totals.locations],
            ['apps', totals.apps],
            ['finances', totals.finances],
            ['safari', totals.safari],
            ['google', totals.google],
            ['files', totals.files],
          ] as Array<[ReviewMode, number]>).map(([key, count]) => (
            <button key={key} className={mode === key ? 'mode active' : 'mode'} onClick={() => setMode(key)}>
              <span>{modeConfig[key].label}</span>
              <strong>{count.toLocaleString()}</strong>
            </button>
          ))}
        </nav>
      </aside>

      <main className="review-main">
        <header className="review-top">
          <div>
            <h1>{active.label}</h1>
            <p>{active.hint}</p>
          </div>
          <div className={error ? 'status bad' : 'status'}>
            {error || `${selectedCase} · ${total.toLocaleString()} matches`}
          </div>
        </header>

        <section className={deviceStatus?.detected ? 'backup-hero ready' : 'backup-hero'}>
          <div>
            <span className="section-title">New Backup</span>
            <h2>{deviceStatus?.detected ? 'iPhone detected. Ready to start.' : 'Plug in an iPhone to begin.'}</h2>
            <p>
              This creates the case folder, starts the encrypted iOS backup through Chronos, and stages the case for review when the backup finishes.
            </p>
            {deviceStatus?.connected_devices.map((device) => (
              <div className="device-line" key={device.udid}>
                <strong>{device.device_info?.device_name || device.device_info?.product_type || 'iOS Device'}</strong>
                <span>{device.device_info?.product_version || 'iOS'} · {device.pairing_state}</span>
              </div>
            ))}
          </div>
          <div className="backup-actions">
            <label className="field">
              <span>Case Name</span>
              <input value={newCaseId} onChange={(event) => setNewCaseId(event.target.value)} />
            </label>
            <label className="field">
              <span>Backup Password</span>
              <input
                value={backupPassword}
                onChange={(event) => setBackupPassword(event.target.value)}
                type="password"
                placeholder="required for encrypted backup"
              />
            </label>
            <button className="secondary" disabled={busy} onClick={() => loadDeviceStatus()}>
              Check Phone
            </button>
            <button className="primary big-action" disabled={busy || !deviceStatus?.detected} onClick={startNewBackup}>
              Start New Backup
            </button>
          </div>
        </section>

        <section className="search-row">
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') loadRecords(serverUrl, selectedCase, mode, query, 0);
            }}
            placeholder="Search evidence"
          />
          <button className="primary" disabled={busy} onClick={() => loadRecords(serverUrl, selectedCase, mode, query, 0)}>
            Search
          </button>
          {mode === 'messages' && (
            <select
              className="direction-select"
              value={messageDirection}
              onChange={(event) => {
                const value = event.target.value as 'all' | 'Sent' | 'Received';
                setMessageDirection(value);
                setTimeout(() => loadRecords(serverUrl, selectedCase, mode, query, 0), 0);
              }}
            >
              <option value="all">All</option>
              <option value="Sent">Sent</option>
              <option value="Received">Received</option>
            </select>
          )}
          <button className="secondary" disabled={busy || offset === 0} onClick={() => loadRecords(serverUrl, selectedCase, mode, query, Math.max(0, offset - limit))}>
            Previous
          </button>
          <button className="secondary" disabled={busy || offset + limit >= total} onClick={() => loadRecords(serverUrl, selectedCase, mode, query, offset + limit)}>
            Next
          </button>
        </section>

        <section className="review-grid">
          <div className="result-list">
            {(page?.records ?? []).map((record, index) => (
              <button
                key={recordKey(record, index)}
                className={selected === record ? 'result active' : 'result'}
                onClick={() => setSelected(record)}
              >
                <strong>{recordTitle(record, mode)}</strong>
                <span>{recordSubtitle(record, mode)}</span>
                <small>{recordMeta(record, mode)}</small>
              </button>
            ))}
          </div>

          <article className="detail-panel">
            {selected ? (
              <>
                <div className="detail-head">
                  <div>
                    <h2>{recordTitle(selected, mode)}</h2>
                    <p>{recordSubtitle(selected, mode)}</p>
                  </div>
                  <button className="primary" disabled={busy} onClick={saveSelection}>
                    Select For Court Packet
                  </button>
                </div>
                {mode === 'messages' ? <MessageDetail record={selected} /> : <GenericDetail record={selected} mode={mode} />}
                <label className="field note-field">
                  <span>Why this matters</span>
                  <textarea
                    value={note}
                    onChange={(event) => setNote(event.target.value)}
                    placeholder="Example: This message contradicts the statement given to police."
                  />
                </label>
                <pre className="raw-json">{JSON.stringify(selected, null, 2)}</pre>
              </>
            ) : (
              <div className="empty">No record selected.</div>
            )}
          </article>
        </section>
      </main>
    </div>
  );
}

function MessageDetail({record}: {record: EvidenceRecord}) {
  return (
    <div className="message-detail">
      <div className={record.direction === 'Sent' ? 'bubble sent' : 'bubble received'}>
        {stringValue(record.text) || '[no message text]'}
      </div>
      <dl>
        <dt>Direction</dt><dd>{stringValue(record.direction)}</dd>
        <dt>Phone</dt><dd>{stringValue(record.phone_number)}</dd>
        <dt>Thread</dt><dd>{stringValue(record.thread_id)}</dd>
        <dt>Service</dt><dd>{stringValue(record.service)}</dd>
        <dt>Attachments</dt><dd>{String(record.attachment_count ?? 0)}</dd>
      </dl>
    </div>
  );
}

function GenericDetail({record, mode}: {record: EvidenceRecord; mode: ReviewMode}) {
  if (mode === 'photos' || mode === 'videos') return <MediaDetail record={record} />;
  return (
    <div className="media-detail">
      <dl>
        {detailFields(record).map(([key, value]) => (
          <><dt key={`${key}-dt`}>{key}</dt><dd key={`${key}-dd`}>{value}</dd></>
        ))}
      </dl>
    </div>
  );
}

function MediaDetail({record}: {record: EvidenceRecord}) {
  return (
    <div className="media-detail">
      <div className="media-frame">
        <strong>{stringValue(record.filename) || stringValue(record.file_path) || 'Media item'}</strong>
        <span>{stringValue(record.media_type)} · {stringValue(record.mime_type) || stringValue(record.container)}</span>
      </div>
      <dl>
        <dt>Created</dt><dd>{stringValue(record.created_date) || stringValue(record.timestamp)}</dd>
        <dt>Path</dt><dd>{stringValue(record.file_path) || stringValue(record.source_path) || stringValue(record.resolved_source_path)}</dd>
        <dt>Dimensions</dt><dd>{`${record.width ?? '?'} x ${record.height ?? '?'}`}</dd>
        <dt>Duration</dt><dd>{record.duration_seconds ? `${record.duration_seconds}s` : ''}</dd>
        <dt>Location</dt><dd>{locationValue(record)}</dd>
      </dl>
    </div>
  );
}

function countType(agents: AgentEntry[], agent: string, type: string) {
  return agents.find((entry) => entry.slug === agent)?.record_types?.[type] ?? 0;
}

function recordKey(record: EvidenceRecord, index: number) {
  return String(record.guid || record.uuid || record.asset_id || record.id || `${record.filename}-${index}`);
}

function recordTitle(record: EvidenceRecord, mode: ReviewMode) {
  if (mode === 'messages') return stringValue(record.phone_number) || stringValue(record.thread_id) || 'Message';
  return stringValue(record.name)
    || stringValue(record.display_name)
    || stringValue(record.title)
    || stringValue(record.filename)
    || stringValue(record.file_path)
    || stringValue(record.bundle_id)
    || stringValue(record.domain)
    || stringValue(record.counterparty)
    || stringValue(record.asset_id)
    || `${modeConfig[mode].label} record`;
}

function recordSubtitle(record: EvidenceRecord, mode: ReviewMode) {
  if (mode === 'messages') return stringValue(record.text) || '[no text]';
  return stringValue(record.text)
    || stringValue(record.note)
    || stringValue(record.body)
    || stringValue(record.url)
    || stringValue(record.file_path)
    || stringValue(record.source_path)
    || stringValue(record.container_path)
    || stringValue(record.display_text)
    || stringValue(record.media_type);
}

function recordMeta(record: EvidenceRecord, mode: ReviewMode) {
  if (mode === 'messages') return `${stringValue(record.direction)} · ${stringValue(record.service)} · attachments ${record.attachment_count ?? 0}`;
  return `${stringValue(record.created_date) || stringValue(record.timestamp)} · ${record.width ?? '?'}x${record.height ?? '?'}`;
}

function detailFields(record: EvidenceRecord) {
  return Object.entries(record)
    .filter(([_, value]) => value != null && typeof value !== 'object')
    .slice(0, 24)
    .map(([key, value]) => [key, stringValue(value)] as [string, string]);
}

function stringValue(value: unknown) {
  return typeof value === 'string' ? value : value == null ? '' : String(value);
}

function locationValue(record: EvidenceRecord) {
  const lat = record.latitude ?? (record.location_metadata as Record<string, unknown> | undefined)?.latitude;
  const lon = record.longitude ?? (record.location_metadata as Record<string, unknown> | undefined)?.longitude;
  if (lat == null || lon == null) return '';
  return `${lat}, ${lon}`;
}

export default App;
