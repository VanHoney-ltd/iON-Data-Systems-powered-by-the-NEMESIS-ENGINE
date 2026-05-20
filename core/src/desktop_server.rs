use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::case::Case;
use crate::chronos::device::{probe_live_status, ConnectedDeviceStatus, ToolStatus};
use crate::chronos::{run_chronos, ChronosConfig};

#[derive(Clone)]
struct EventBus {
    clients: Arc<Mutex<Vec<mpsc::Sender<String>>>>,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct CaseEntry {
    case_id: String,
    root_path: String,
    evidence_dir: String,
    manifest_path: Option<String>,
    last_run_status: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeviceProbeResponse {
    detected: bool,
    connected_count: usize,
    paired_count: usize,
    connected_devices: Vec<ConnectedDeviceStatus>,
    tool_status: Vec<ToolStatus>,
    recommended_action: &'static str,
}

#[derive(Debug, Serialize)]
struct EvidenceSearchResponse {
    case_id: String,
    agent: String,
    total_matches: usize,
    offset: usize,
    limit: usize,
    records: Vec<Value>,
}

#[derive(Debug, serde::Deserialize)]
struct StartBackupRequest {
    case_id: String,
    backup_password: Option<String>,
}

pub fn serve(case_arg: &str, args: &[String]) -> Result<()> {
    let bind = desktop_bind_addr(case_arg, args);
    let listener = TcpListener::bind(&bind).with_context(|| format!("binding {bind}"))?;
    let bus = EventBus::default();
    println!("Desktop server listening on http://{bind}");

    for stream in listener.incoming() {
        let stream = stream?;
        let bus = bus.clone();
        thread::spawn(move || {
            if let Err(error) = handle_connection(stream, bus) {
                eprintln!("desktop-serve request failed: {error:#}");
            }
        });
    }

    Ok(())
}

impl Default for EventBus {
    fn default() -> Self {
        Self {
            clients: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl EventBus {
    fn subscribe(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut clients) = self.clients.lock() {
            clients.push(tx);
        }
        rx
    }

    fn publish(&self, event_type: &str, case_id: Option<&str>, payload: Value) {
        let event = json!({
            "v": 1,
            "case_id": case_id,
            "session_id": null,
            "run_id": null,
            "seq": 0,
            "ts": Utc::now().to_rfc3339(),
            "channel": "desktop",
            "type": event_type,
            "agent": null,
            "model": null,
            "message_id": null,
            "payload": payload,
        });
        let line = match serde_json::to_string(&event) {
            Ok(line) => line,
            Err(_) => return,
        };
        if let Ok(mut clients) = self.clients.lock() {
            clients.retain(|client| client.send(line.clone()).is_ok());
        }
    }
}

fn desktop_bind_addr(case_arg: &str, args: &[String]) -> String {
    for arg in args {
        if let Some(value) = arg.strip_prefix("--bind=") {
            return value.to_string();
        }
    }
    if case_arg.contains(':') {
        case_arg.to_string()
    } else {
        "127.0.0.1:17870".to_string()
    }
}

fn handle_connection(mut stream: TcpStream, bus: EventBus) -> Result<()> {
    let request = read_request(&mut stream)?;
    if request.method == "OPTIONS" {
        return write_response(&mut stream, 204, "text/plain", b"");
    }

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/health") => write_json(
            &mut stream,
            200,
            &json!({"status": "ok", "service": "ion-desktop-serve"}),
        ),
        ("GET", "/api/cases") => write_json(&mut stream, 200, &list_cases()?),
        ("GET", "/api/device/status") => write_json(&mut stream, 200, &probe_device_status()),
        ("GET", "/api/events") => stream_events(stream, bus),
        ("POST", "/api/backups/start") => {
            let request = parse_start_backup_request(&request.body)?;
            let case_id = request.case_id.clone();
            start_backup(request, bus);
            write_json(
                &mut stream,
                202,
                &json!({"status": "queued", "case_id": case_id}),
            )
        }
        ("POST", path) if is_run_all_path(path) => {
            let case_id = path_component(path, 3)?;
            start_run_all(case_id, bus);
            write_json(
                &mut stream,
                202,
                &json!({"status": "queued", "case_id": case_id}),
            )
        }
        ("GET", path) if is_manifest_path(path) => {
            let case_id = path_component(path, 3)?;
            let case = Case::new(case_id)?;
            let path = case.evidence_path("_ui").join("case_manifest.json");
            write_json_file(&mut stream, &path)
        }
        ("GET", path) if is_agent_search_path(path) => {
            let case_id = path_component(path, 3)?;
            let agent = path_component(path, 5)?;
            write_json(
                &mut stream,
                200,
                &search_agent_records(case_id, agent, &request.query)?,
            )
        }
        ("GET", path) if is_selection_path(path) => {
            let case_id = path_component(path, 3)?;
            write_json_file(
                &mut stream,
                &Case::new(case_id)?
                    .evidence_path("_ui")
                    .join("selections.json"),
            )
        }
        ("POST", path) if is_selection_path(path) => {
            let case_id = path_component(path, 3)?;
            save_selection(case_id, &request.body)?;
            write_json(
                &mut stream,
                200,
                &json!({"status": "saved", "case_id": case_id}),
            )
        }
        ("GET", path) if is_agent_records_path(path) => {
            let case_id = path_component(path, 3)?;
            let agent = path_component(path, 5)?;
            let case = Case::new(case_id)?;
            let path = case.evidence_path(agent).join("records.json");
            write_json_file(&mut stream, &path)
        }
        ("GET", path) if is_agent_summary_path(path) => {
            let case_id = path_component(path, 3)?;
            let agent = path_component(path, 5)?;
            let case = Case::new(case_id)?;
            let path = case.evidence_path(agent).join("summary.json");
            write_json_file(&mut stream, &path)
        }
        _ => write_json(
            &mut stream,
            404,
            &json!({"error": "not_found", "path": request.path}),
        ),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let raw_path = parts.next().unwrap_or("/").to_string();
    let path = raw_path.split('?').next().unwrap_or("/").to_string();
    let query = raw_path
        .split_once('?')
        .map(|(_, query)| query.to_string())
        .unwrap_or_default();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().unwrap_or_default();
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(HttpRequest {
        method,
        path,
        query,
        body,
    })
}

fn list_cases() -> Result<Vec<CaseEntry>> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cases_root in case_search_roots() {
        collect_cases_from_root(&cases_root, &mut entries, &mut seen)?;
    }
    entries.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    Ok(entries)
}

fn collect_cases_from_root(
    cases_root: &Path,
    entries: &mut Vec<CaseEntry>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<()> {
    if !cases_root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&cases_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let case_id = entry.file_name().to_string_lossy().to_string();
        if !seen.insert(case_id.clone()) {
            continue;
        }
        let case = Case::from_root(&case_id, entry.path());
        let manifest_path = case.evidence_path("_ui").join("case_manifest.json");
        let run_report_path = case.evidence_path("_ui").join("run_all_report.json");
        let last_run_status = read_last_run_status(&run_report_path);
        entries.push(CaseEntry {
            case_id,
            root_path: case.root_path().display().to_string(),
            evidence_dir: case.root_path().join("evidence").display().to_string(),
            manifest_path: manifest_path
                .exists()
                .then(|| manifest_path.display().to_string()),
            last_run_status,
        });
    }
    Ok(())
}

fn case_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![default_cases_root()];
    if let Some(home) = dirs::home_dir() {
        roots.push(
            home.join("iON-Data-Security-Systems")
                .join("backups")
                .join("cases"),
        );
        roots.push(home.join("iON").join("backups").join("cases"));
    }
    roots
}

fn default_cases_root() -> PathBuf {
    if let Ok(root) = std::env::var("iON_CASE_ROOTS") {
        if let Some(first) = root.split([':', ',', ';']).find(|value| !value.is_empty()) {
            return PathBuf::from(first);
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("backups")
        .join("cases")
}

fn read_last_run_status(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("status")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn search_agent_records(case_id: &str, agent: &str, query: &str) -> Result<EvidenceSearchResponse> {
    let params = parse_query(query);
    let record_type = params.get("type").map(String::as_str);
    let direction = params.get("direction").map(String::as_str);
    let source = params.get("source").map(String::as_str);
    let search = params
        .get("q")
        .map(|value| value.to_lowercase())
        .filter(|value| !value.is_empty());
    let offset = params
        .get("offset")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 500);

    let case = Case::new(case_id)?;
    let path = case.evidence_path(agent).join("records.json");
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let records: Vec<Value> = serde_json::from_slice(&bytes)?;
    let mut total_matches = 0usize;
    let mut page = Vec::new();

    for record in records {
        if let Some(kind) = record_type {
            if !kind.is_empty() && record.get("record_type").and_then(Value::as_str) != Some(kind) {
                continue;
            }
        }
        if let Some(direction) = direction {
            if !direction.is_empty()
                && record.get("direction").and_then(Value::as_str) != Some(direction)
            {
                continue;
            }
        }
        if let Some(source) = source {
            if !source.is_empty()
                && !record
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&source.to_lowercase())
            {
                continue;
            }
        }
        if let Some(search) = &search {
            let haystack = searchable_record_text(&record);
            if !haystack.to_lowercase().contains(search) {
                continue;
            }
        }
        if total_matches >= offset && page.len() < limit {
            page.push(record);
        }
        total_matches += 1;
    }

    Ok(EvidenceSearchResponse {
        case_id: case_id.to_string(),
        agent: agent.to_string(),
        total_matches,
        offset,
        limit,
        records: page,
    })
}

fn searchable_record_text(record: &Value) -> String {
    [
        "text",
        "phone_number",
        "thread_id",
        "filename",
        "file_path",
        "original_filename",
        "transfer_name",
        "media_type",
        "mime_type",
        "contact_name",
        "direction",
        "timestamp",
        "created_date",
        "created_utc",
    ]
    .iter()
    .filter_map(|key| record.get(*key).and_then(Value::as_str))
    .collect::<Vec<_>>()
    .join(" ")
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (url_decode(key), url_decode(value)))
        .collect()
}

fn url_decode(value: &str) -> String {
    let mut out = String::new();
    let mut bytes = value.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(' '),
            b'%' => {
                let hi = bytes.next();
                let lo = bytes.next();
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    let hex = [hi, lo];
                    if let Ok(hex) = std::str::from_utf8(&hex) {
                        if let Ok(decoded) = u8::from_str_radix(hex, 16) {
                            out.push(decoded as char);
                            continue;
                        }
                    }
                }
            }
            _ => out.push(byte as char),
        }
    }
    out
}

fn save_selection(case_id: &str, body: &[u8]) -> Result<()> {
    let value: Value = serde_json::from_slice(body).context("selection body must be JSON")?;
    let case = Case::new(case_id)?;
    let path = case.evidence_path("_ui").join("selections.json");
    let mut existing = if path.exists() {
        fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    existing.push(json!({
        "selected_at": Utc::now().to_rfc3339(),
        "selection": value,
    }));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&existing)?)?;
    Ok(())
}

fn probe_device_status() -> DeviceProbeResponse {
    let probe_root = std::env::temp_dir().join("ion-nemesis-device-probe");
    let case = Case::from_root("_device_probe", probe_root);
    let status = probe_live_status(&case);
    let detected = status.connected_count() > 0;
    DeviceProbeResponse {
        detected,
        connected_count: status.connected_count(),
        paired_count: status.paired_count,
        connected_devices: status.connected_devices,
        tool_status: status.tool_status,
        recommended_action: if detected {
            "choose_new_backup_or_existing_case"
        } else {
            "connect_ios_device_or_open_existing_case"
        },
    }
}

fn parse_start_backup_request(body: &[u8]) -> Result<StartBackupRequest> {
    let request: StartBackupRequest = serde_json::from_slice(body)
        .context("expected JSON body with case_id and optional backup_password")?;
    let case_id = request.case_id.trim();
    if case_id.is_empty() {
        anyhow::bail!("case_id is required");
    }
    if !case_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        anyhow::bail!("case_id may only contain letters, numbers, dash, underscore, or dot");
    }
    Ok(StartBackupRequest {
        case_id: case_id.to_string(),
        backup_password: request
            .backup_password
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

fn start_backup(request: StartBackupRequest, bus: EventBus) {
    thread::spawn(move || {
        let case_id = request.case_id;
        bus.publish(
            "backup_started",
            Some(&case_id),
            json!({"command": "chronos", "case_id": case_id}),
        );

        let result = (|| -> Result<()> {
            let case_root = default_cases_root().join(&case_id);
            let case = Case::from_root(&case_id, case_root);
            case.open("chronos")?;

            let mut config = ChronosConfig::default();
            config.backup_password = request
                .backup_password
                .or_else(|| std::env::var("BACKUP_PASSWORD").ok());
            config.verbose = true;

            if config.backup_password.is_none() {
                anyhow::bail!(
                    "Encrypted backups require a password. Enter one in the desktop backup form or set BACKUP_PASSWORD."
                );
            }

            let backup = run_chronos(case, config)?;
            bus.publish(
                "backup_completed",
                Some(&case_id),
                json!({
                    "command": "chronos",
                    "case_id": case_id,
                    "status": "complete",
                    "backup_root": backup.backup_root.display().to_string(),
                    "helios_root": backup.helios_root.map(|path| path.display().to_string()),
                    "orpheus_ok": backup.orpheus_ok,
                }),
            );
            Ok(())
        })();

        if let Err(error) = result {
            bus.publish(
                "error",
                Some(&case_id),
                json!({
                    "code": "BACKUP_FAILED",
                    "severity": "error",
                    "message": format!("{error:#}"),
                    "retryable": true,
                }),
            );
        }
    });
}

fn start_run_all(case_id: &str, bus: EventBus) {
    let case_id = case_id.to_string();
    thread::spawn(move || {
        bus.publish(
            "run_started",
            Some(&case_id),
            json!({"command": "run-all", "case_id": case_id}),
        );
        match crate::run_all::run(&case_id) {
            Ok(()) => bus.publish(
                "run_completed",
                Some(&case_id),
                json!({"command": "run-all", "case_id": case_id, "status": "complete"}),
            ),
            Err(error) => bus.publish(
                "error",
                Some(&case_id),
                json!({
                    "code": "RUN_ALL_FAILED",
                    "severity": "error",
                    "message": format!("{error:#}"),
                    "retryable": true,
                }),
            ),
        }
    });
}

fn stream_events(mut stream: TcpStream, bus: EventBus) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n"
    )?;
    stream.flush()?;

    let rx = bus.subscribe();
    write_sse_event(&mut stream, "connected", &json!({"status": "connected"}))?;
    loop {
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(event) => {
                write!(stream, "event: message\ndata: {event}\n\n")?;
                stream.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                write_sse_event(
                    &mut stream,
                    "heartbeat",
                    &json!({"ts": Utc::now().to_rfc3339()}),
                )?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn write_sse_event(stream: &mut TcpStream, event: &str, payload: &Value) -> Result<()> {
    writeln!(stream, "event: {event}")?;
    writeln!(stream, "data: {}", serde_json::to_string(payload)?)?;
    writeln!(stream)?;
    stream.flush()?;
    Ok(())
}

fn write_json_file(stream: &mut TcpStream, path: &Path) -> Result<()> {
    if !path.exists() {
        return write_json(
            stream,
            404,
            &json!({"error": "missing_file", "path": path.display().to_string()}),
        );
    }
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    write_response(stream, 200, "application/json", &bytes)
}

fn write_json<T: Serialize>(stream: &mut TcpStream, status: u16, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_response(stream, status, "application/json", &bytes)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn is_manifest_path(path: &str) -> bool {
    path.starts_with("/api/cases/") && path.ends_with("/manifest")
}

fn is_run_all_path(path: &str) -> bool {
    path.starts_with("/api/cases/") && path.ends_with("/run-all")
}

fn is_agent_records_path(path: &str) -> bool {
    path.starts_with("/api/cases/") && path.contains("/evidence/") && path.ends_with("/records")
}

fn is_agent_summary_path(path: &str) -> bool {
    path.starts_with("/api/cases/") && path.contains("/evidence/") && path.ends_with("/summary")
}

fn is_agent_search_path(path: &str) -> bool {
    path.starts_with("/api/cases/") && path.contains("/evidence/") && path.ends_with("/search")
}

fn is_selection_path(path: &str) -> bool {
    path.starts_with("/api/cases/") && path.ends_with("/selections")
}

fn path_component<'a>(path: &'a str, index: usize) -> Result<&'a str> {
    path.split('/')
        .nth(index)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing path component {index} in {path}"))
}
