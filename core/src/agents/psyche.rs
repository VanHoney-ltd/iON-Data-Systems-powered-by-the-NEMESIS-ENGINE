//! Psyche — contact-centered dossier and relationship review.
//!
//! Psyche is intentionally not a whole-catalogue summarizer. It starts with
//! Cerberus contacts/identities, links evidence by contact handles, and only
//! then produces per-contact dossiers. Optional Ollama synthesis is bounded to
//! the selected dossier and never replaces source-linked counts and excerpts.

use anyhow::{Context, Result};
use chrono::Utc;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    style::Print,
    terminal::{self, ClearType},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use crate::agents::{Agent, AgentCtx};
use crate::case::Case;
use crate::evidence::EvidenceRecord;

const DEFAULT_REVIEW_MODEL: &str = "llama3.1:8b";
const MAX_EXCERPTS_PER_CONTACT: usize = 12;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PsycheIdentity {
    value: String,
    value_type: String,
    label: Option<String>,
    is_voip_like: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PsycheEvidenceRef {
    source_agent: String,
    record_type: String,
    record_id: Option<String>,
    timestamp: Option<String>,
    direction: Option<String>,
    handle: Option<String>,
    excerpt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PsycheDossier {
    contact_id: i64,
    contact_name: String,
    normalized_names: Vec<String>,
    identities: Vec<PsycheIdentity>,
    message_count: usize,
    sent_messages: usize,
    received_messages: usize,
    voicemail_count: usize,
    financial_thread_mentions: usize,
    note_mentions: usize,
    first_observed: Option<String>,
    last_observed: Option<String>,
    flags: Vec<String>,
    evidence_refs: Vec<PsycheEvidenceRef>,
    llm_model: Option<String>,
    llm_summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PsycheReport {
    schema_version: u32,
    case_id: String,
    generated_at: String,
    state: String,
    contacts_loaded: usize,
    contacts_with_evidence: usize,
    dossiers_written: usize,
    review_model: String,
    llm_enabled: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Default)]
struct ContactBuilder {
    id: i64,
    name: String,
    identities: Vec<PsycheIdentity>,
    handles: BTreeSet<String>,
    name_keys: BTreeSet<String>,
}

pub struct PsycheAgent;

impl Agent for PsycheAgent {
    const NAME: &'static str = "Psyche";
    const SLUG: &'static str = "psyche";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let evidence_dir = ctx.case.evidence_path(Self::SLUG);
        fs::create_dir_all(&evidence_dir)?;

        let mut warnings = Vec::new();
        let review_model =
            std::env::var("PSYCHE_REVIEW_MODEL").unwrap_or_else(|_| DEFAULT_REVIEW_MODEL.into());
        let llm_enabled = std::env::var("PSYCHE_LLM")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
        let llm_limit = std::env::var("PSYCHE_LLM_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(5);

        let contacts = load_contacts(ctx, &mut warnings)?;
        let contact_count = contacts.len();
        let mut dossiers = build_dossiers(ctx, contacts, &mut warnings)?;
        dossiers.sort_by(|left, right| {
            right
                .message_count
                .cmp(&left.message_count)
                .then_with(|| {
                    right
                        .financial_thread_mentions
                        .cmp(&left.financial_thread_mentions)
                })
                .then_with(|| left.contact_name.cmp(&right.contact_name))
        });

        if llm_enabled {
            for dossier in dossiers.iter_mut().take(llm_limit) {
                match synthesize_with_ollama(&review_model, dossier) {
                    Ok(summary) => {
                        dossier.llm_model = Some(review_model.clone());
                        dossier.llm_summary = Some(summary);
                    }
                    Err(error) => warnings.push(format!(
                        "Ollama synthesis failed for {}: {}",
                        dossier.contact_name, error
                    )),
                }
            }
        }

        let contacts_with_evidence = dossiers
            .iter()
            .filter(|dossier| {
                dossier.message_count > 0
                    || dossier.voicemail_count > 0
                    || dossier.financial_thread_mentions > 0
                    || dossier.note_mentions > 0
            })
            .count();

        write_json(&evidence_dir.join("contact_dossiers.json"), &dossiers)?;
        write_dossiers_csv(&evidence_dir.join("contact_dossiers.csv"), &dossiers)?;
        write_index_html(&evidence_dir.join("index.html"), ctx.case.name(), &dossiers)?;

        let report = PsycheReport {
            schema_version: Self::SCHEMA_VERSION,
            case_id: ctx.case.name().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            state: "complete".to_string(),
            contacts_loaded: contact_count,
            contacts_with_evidence,
            dossiers_written: dossiers.len(),
            review_model,
            llm_enabled,
            warnings,
        };
        write_json(&evidence_dir.join("report.json"), &report)?;

        let mut records = vec![EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            payload: serde_json::to_value(&report)?,
        }];
        for dossier in dossiers {
            records.push(EvidenceRecord {
                schema_version: Self::SCHEMA_VERSION,
                source_agent: Self::NAME.to_string(),
                record_type: "contact_dossier".to_string(),
                timestamp: dossier
                    .last_observed
                    .clone()
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
                payload: serde_json::to_value(dossier)?,
            });
        }
        Ok(records)
    }
}

pub fn run_tui(case_name: &str) -> Result<()> {
    let case = Case::new(case_name)?;
    let path = case.evidence_path("psyche").join("contact_dossiers.json");
    if !path.exists() {
        anyhow::bail!(
            "No Psyche dossiers found at {}. Run `minios psyche {}` first.",
            path.display(),
            case_name
        );
    }
    let dossiers: Vec<PsycheDossier> = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    if dossiers.is_empty() {
        anyhow::bail!("Psyche dossier file is empty");
    }

    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    let actions_dir = case.evidence_path("psyche").join("actions");
    let result = tui_loop(&mut stdout, &dossiers, &actions_dir);
    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    result
}

fn tui_loop(stdout: &mut io::Stdout, dossiers: &[PsycheDossier], actions_dir: &Path) -> Result<()> {
    let mut selected = 0usize;
    let mut screen = TuiScreen::Contacts;
    let mut action_selected = 0usize;
    let mut status = String::new();
    loop {
        draw_tui(stdout, dossiers, selected, screen, action_selected, &status)?;
        if event::poll(Duration::from_millis(500))? {
            if let Event::Key(key) = event::read()? {
                match screen {
                    TuiScreen::Contacts => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Down | KeyCode::Char('j') => {
                            selected = (selected + 1).min(dossiers.len().saturating_sub(1));
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            selected = selected.saturating_sub(1);
                        }
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => {
                            screen = TuiScreen::Actions;
                            action_selected = 0;
                        }
                        _ => {}
                    },
                    TuiScreen::Actions => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Esc | KeyCode::Left | KeyCode::Char('b') => {
                            screen = TuiScreen::Contacts;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            action_selected =
                                (action_selected + 1).min(PSYCHE_ACTIONS.len().saturating_sub(1));
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            action_selected = action_selected.saturating_sub(1);
                        }
                        KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') => {
                            match run_contact_action(
                                actions_dir,
                                &dossiers[selected],
                                action_selected,
                            ) {
                                Ok(path) => status = format!("Wrote {}", path.display()),
                                Err(error) => status = format!("Action failed: {error:#}"),
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum TuiScreen {
    Contacts,
    Actions,
}

const PSYCHE_ACTIONS: &[&str] = &[
    "Standardize legal jargon",
    "Behavioral concern review (non-diagnostic)",
    "Generate contact dossier summary",
    "Extract contradiction points",
    "Build communication timeline",
    "Find financial pressure references",
    "Find note references",
    "Export selected contact packet",
    "Run bounded Ollama review",
];

fn draw_tui(
    stdout: &mut io::Stdout,
    dossiers: &[PsycheDossier],
    selected: usize,
    screen: TuiScreen,
    action_selected: usize,
    status: &str,
) -> Result<()> {
    let (cols, rows) = terminal::size()?;
    execute!(
        stdout,
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::All)
    )?;
    let cols_usize = cols as usize;
    let list_width = (cols_usize / 3).max(32).min(48);
    let detail_x = list_width as u16 + 2;
    let detail_width = cols_usize.saturating_sub(detail_x as usize + 1);
    draw_line(
        stdout,
        0,
        0,
        cols_usize,
        "Psyche Contact Review | arrows/j/k move | Enter select | Esc/back | q quit",
    )?;
    draw_line(stdout, 0, 1, cols_usize, &"-".repeat(cols_usize))?;
    draw_line(
        stdout,
        0,
        2,
        cols_usize,
        "Step 1: choose a contact from the full list. Step 2: choose an action for that contact.",
    )?;
    draw_line(stdout, 0, 3, cols_usize, &"-".repeat(cols_usize))?;

    let visible = rows.saturating_sub(6) as usize;
    let start = selected.saturating_sub(visible / 2);
    for (line, dossier) in dossiers.iter().enumerate().skip(start).take(visible) {
        let marker = if line == selected { ">" } else { " " };
        let name = tui_text(&dossier.contact_name);
        draw_line(stdout, 0, 4 + (line - start) as u16, list_width, marker)?;
        draw_line(
            stdout,
            2,
            4 + (line - start) as u16,
            list_width.saturating_sub(2),
            &format!(
                "{:<width$} {:>6} msg",
                truncate_chars(&name, list_width.saturating_sub(13)),
                dossier.message_count,
                width = list_width.saturating_sub(13)
            ),
        )?;
    }

    let selected_dossier = &dossiers[selected];
    draw_line(
        stdout,
        detail_x,
        4,
        detail_width,
        &tui_text(&selected_dossier.contact_name),
    )?;

    match screen {
        TuiScreen::Contacts => {
            draw_contact_preview(stdout, detail_x, detail_width, selected_dossier)?
        }
        TuiScreen::Actions => draw_action_menu(
            stdout,
            detail_x,
            detail_width,
            selected_dossier,
            action_selected,
            status,
        )?,
    }
    stdout.flush()?;
    Ok(())
}

fn draw_contact_preview(
    stdout: &mut io::Stdout,
    detail_x: u16,
    detail_width: usize,
    dossier: &PsycheDossier,
) -> Result<()> {
    draw_line(
        stdout,
        detail_x,
        6,
        detail_width,
        &format!(
            "messages={} sent={} received={} voicemail={} financial={} notes={}",
            dossier.message_count,
            dossier.sent_messages,
            dossier.received_messages,
            dossier.voicemail_count,
            dossier.financial_thread_mentions,
            dossier.note_mentions
        ),
    )?;
    let handles = dossier
        .identities
        .iter()
        .map(|identity| identity.value.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    draw_wrapped(
        stdout,
        detail_x,
        8,
        detail_width,
        3,
        &format!("handles: {handles}"),
    )?;
    draw_wrapped(
        stdout,
        detail_x,
        12,
        detail_width,
        2,
        &format!("flags: {}", dossier.flags.join(", ")),
    )?;
    draw_line(
        stdout,
        detail_x,
        16,
        detail_width,
        "Press Enter to open actions for this contact.",
    )?;
    Ok(())
}

fn draw_action_menu(
    stdout: &mut io::Stdout,
    detail_x: u16,
    detail_width: usize,
    dossier: &PsycheDossier,
    action_selected: usize,
    status: &str,
) -> Result<()> {
    draw_line(
        stdout,
        detail_x,
        6,
        detail_width,
        "Actions for selected contact",
    )?;
    for (idx, action) in PSYCHE_ACTIONS.iter().enumerate() {
        let marker = if idx == action_selected { ">" } else { " " };
        draw_line(
            stdout,
            detail_x,
            8 + idx as u16,
            detail_width,
            &format!("{marker} {action}"),
        )?;
    }
    draw_line(
        stdout,
        detail_x,
        18,
        detail_width,
        "Enter runs selected action. Output is evidence-bound under evidence/psyche/actions/.",
    )?;
    draw_wrapped(
        stdout,
        detail_x,
        20,
        detail_width,
        3,
        &format!(
            "Selected contact has {} messages, {} financial mentions, {} note mentions.",
            dossier.message_count, dossier.financial_thread_mentions, dossier.note_mentions
        ),
    )?;
    if !status.is_empty() {
        draw_wrapped(stdout, detail_x, 23, detail_width, 2, status)?;
    }
    Ok(())
}

fn run_contact_action(
    actions_dir: &Path,
    dossier: &PsycheDossier,
    action_index: usize,
) -> Result<std::path::PathBuf> {
    fs::create_dir_all(actions_dir)?;
    let action = PSYCHE_ACTIONS
        .get(action_index)
        .copied()
        .unwrap_or("contact_action");
    let path = actions_dir.join(format!(
        "contact_{}_{}.md",
        dossier.contact_id,
        action_slug(action)
    ));
    let body = match action {
        "Standardize legal jargon" => render_standardized_legal_jargon(dossier),
        "Behavioral concern review (non-diagnostic)" => render_behavioral_concern_review(dossier),
        "Generate contact dossier summary" => render_contact_summary(dossier),
        "Extract contradiction points" => render_contradiction_points(dossier),
        "Build communication timeline" => render_timeline(dossier),
        "Find financial pressure references" => render_financial_refs(dossier),
        "Find note references" => render_note_refs(dossier),
        "Export selected contact packet" => serde_json::to_string_pretty(dossier)
            .unwrap_or_else(|_| render_contact_summary(dossier)),
        "Run bounded Ollama review" => render_bounded_ollama_review(dossier)?,
        _ => render_contact_summary(dossier),
    };
    fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn render_standardized_legal_jargon(dossier: &PsycheDossier) -> String {
    format!(
        "# Standardized Legal Jargon\n\nContact: {}\n\nPreferred phrasing:\n\n- Use \"contact-linked communication records\" instead of informal labels.\n- Use \"financial-context references\" instead of accusations about motive.\n- Use \"observed pattern\" only when supported by repeated source records.\n- Use \"possible inconsistency\" instead of \"lie\" unless a record directly proves falsity.\n- Use \"non-diagnostic behavioral indicators\" instead of mental-health labels.\n\nEvidence posture:\n\n{}\n",
        dossier.contact_name,
        evidence_bullets(dossier)
    )
}

fn render_behavioral_concern_review(dossier: &PsycheDossier) -> String {
    format!(
        "# Behavioral Concern Review (Non-Diagnostic)\n\nContact: {}\n\nScope and limits:\n\nThis is not a clinical diagnosis, forensic psychological diagnosis, or finding of mental illness. It is an evidence-bound review of communication behavior visible in this selected contact dossier. Any mental-health conclusion would require evaluation by a qualified clinician with proper records, interviews, and collateral sources.\n\nSender/receiver separation required:\n\n- Sent messages and received messages must be analyzed separately before drafting conclusions.\n- Quoted evidence should retain direction, timestamp, and record ID when used in documentation.\n\nObserved behavioral indicators from selected evidence:\n\n{}\n\nAlternative explanations to preserve:\n\n- Stress, intoxication, conflict, grief, misunderstanding, legal pressure, financial pressure, or incomplete record context may explain isolated statements.\n\nDocumentation rule:\n\nUse cautious language: \"the records may indicate,\" \"the selected messages show,\" and \"this warrants review,\" not diagnostic labels.\n",
        dossier.contact_name,
        evidence_bullets(dossier)
    )
}

fn render_contact_summary(dossier: &PsycheDossier) -> String {
    format!(
        "# Contact Dossier Summary\n\nContact: {}\n\nMessages: {}\nSent: {}\nReceived: {}\nVoicemail: {}\nFinancial mentions: {}\nNote mentions: {}\nFlags: {}\n\nEvidence excerpts:\n\n{}\n",
        dossier.contact_name,
        dossier.message_count,
        dossier.sent_messages,
        dossier.received_messages,
        dossier.voicemail_count,
        dossier.financial_thread_mentions,
        dossier.note_mentions,
        dossier.flags.join(", "),
        evidence_bullets(dossier)
    )
}

fn render_contradiction_points(dossier: &PsycheDossier) -> String {
    format!(
        "# Contradiction Points\n\nContact: {}\n\nThis action requires comparing selected excerpts against a specific claim. Current evidence excerpts for manual review:\n\n{}\n",
        dossier.contact_name,
        evidence_bullets(dossier)
    )
}

fn render_timeline(dossier: &PsycheDossier) -> String {
    format!(
        "# Communication Timeline\n\nContact: {}\n\nFirst observed: {}\nLast observed: {}\n\nSelected timestamped excerpts:\n\n{}\n",
        dossier.contact_name,
        dossier.first_observed.as_deref().unwrap_or("unknown"),
        dossier.last_observed.as_deref().unwrap_or("unknown"),
        evidence_bullets(dossier)
    )
}

fn render_financial_refs(dossier: &PsycheDossier) -> String {
    let refs = dossier
        .evidence_refs
        .iter()
        .filter(|evidence| evidence.record_type.contains("financial"))
        .map(format_evidence_bullet)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# Financial Pressure References\n\nContact: {}\n\n{}\n",
        dossier.contact_name, refs
    )
}

fn render_note_refs(dossier: &PsycheDossier) -> String {
    let refs = dossier
        .evidence_refs
        .iter()
        .filter(|evidence| evidence.record_type.contains("note"))
        .map(format_evidence_bullet)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# Note References\n\nContact: {}\n\n{}\n",
        dossier.contact_name, refs
    )
}

fn render_bounded_ollama_review(dossier: &PsycheDossier) -> Result<String> {
    let model =
        std::env::var("PSYCHE_REVIEW_MODEL").unwrap_or_else(|_| DEFAULT_REVIEW_MODEL.into());
    let summary = synthesize_with_ollama(&model, dossier)?;
    Ok(format!(
        "# Bounded Ollama Review\n\nModel: {model}\nContact: {}\n\n{}\n",
        dossier.contact_name, summary
    ))
}

fn evidence_bullets(dossier: &PsycheDossier) -> String {
    dossier
        .evidence_refs
        .iter()
        .take(MAX_EXCERPTS_PER_CONTACT)
        .map(format_evidence_bullet)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_evidence_bullet(evidence: &PsycheEvidenceRef) -> String {
    format!(
        "- [{}:{}] {}",
        evidence.source_agent,
        evidence.record_type,
        truncate_chars(&evidence.excerpt.replace('\n', " "), 700)
    )
}

fn action_slug(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn draw_line(stdout: &mut io::Stdout, x: u16, y: u16, width: usize, text: &str) -> Result<()> {
    let clean = truncate_chars(&tui_text(text), width);
    let padded = format!("{clean:<width$}");
    execute!(stdout, cursor::MoveTo(x, y), Print(padded))?;
    Ok(())
}

fn draw_wrapped(
    stdout: &mut io::Stdout,
    x: u16,
    y: u16,
    width: usize,
    max_lines: u16,
    text: &str,
) -> Result<u16> {
    let clean = tui_text(text);
    let mut line_count = 0u16;
    let mut current = String::new();
    for word in clean.split_whitespace() {
        let next_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };
        if next_len > width && !current.is_empty() {
            draw_line(stdout, x, y + line_count, width, &current)?;
            line_count += 1;
            current.clear();
            if line_count >= max_lines {
                return Ok(line_count);
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if line_count < max_lines && !current.is_empty() {
        draw_line(stdout, x, y + line_count, width, &current)?;
        line_count += 1;
    }
    Ok(line_count)
}

fn tui_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn load_contacts(
    ctx: &AgentCtx,
    warnings: &mut Vec<String>,
) -> Result<HashMap<i64, ContactBuilder>> {
    let path = ctx
        .case
        .evidence_path("cerberus")
        .join("contact_identities.json");
    let values = read_json_array(&path)?;
    let mut contacts: HashMap<i64, ContactBuilder> = HashMap::new();
    for value in values {
        let Some(id) = value.get("contact_id").and_then(|value| value.as_i64()) else {
            continue;
        };
        let name = string_field(&value, "contact_name").unwrap_or_else(|| format!("contact_{id}"));
        let entry = contacts.entry(id).or_insert_with(|| ContactBuilder {
            id,
            name: name.clone(),
            ..Default::default()
        });
        entry.name_keys.insert(normalize_name_key(&name));
        let raw = string_field(&value, "value").unwrap_or_default();
        let value_type = string_field(&value, "value_type").unwrap_or_else(|| "unknown".into());
        if raw.trim().is_empty() {
            continue;
        }
        let handle = normalize_handle(&raw, &value_type);
        entry.handles.insert(handle);
        entry.identities.push(PsycheIdentity {
            value: raw,
            value_type,
            label: value
                .get("label")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
            is_voip_like: value
                .get("is_voip_like")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        });
    }
    if contacts.is_empty() {
        warnings.push(format!("No contacts loaded from {}", path.display()));
    }
    Ok(contacts)
}

fn build_dossiers(
    ctx: &AgentCtx,
    contacts: HashMap<i64, ContactBuilder>,
    warnings: &mut Vec<String>,
) -> Result<Vec<PsycheDossier>> {
    let mut handle_to_contact = HashMap::new();
    let mut name_to_contact = HashMap::new();
    for contact in contacts.values() {
        for handle in &contact.handles {
            handle_to_contact.insert(handle.clone(), contact.id);
        }
        for key in &contact.name_keys {
            if !key.is_empty() {
                name_to_contact.insert(key.clone(), contact.id);
            }
        }
    }

    let mut dossiers = contacts
        .into_iter()
        .map(|(id, contact)| {
            (
                id,
                PsycheDossier {
                    contact_id: id,
                    contact_name: contact.name,
                    normalized_names: contact.name_keys.into_iter().collect(),
                    identities: contact.identities,
                    message_count: 0,
                    sent_messages: 0,
                    received_messages: 0,
                    voicemail_count: 0,
                    financial_thread_mentions: 0,
                    note_mentions: 0,
                    first_observed: None,
                    last_observed: None,
                    flags: Vec::new(),
                    evidence_refs: Vec::new(),
                    llm_model: None,
                    llm_summary: None,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    link_cerberus(ctx, &handle_to_contact, &mut dossiers, warnings)?;
    link_obolus(
        ctx,
        &handle_to_contact,
        &name_to_contact,
        &mut dossiers,
        warnings,
    )?;

    for dossier in dossiers.values_mut() {
        if dossier
            .identities
            .iter()
            .any(|identity| identity.is_voip_like || identity.label.as_deref() == Some("TextNow"))
        {
            dossier.flags.push("voip_or_textnow_identity".to_string());
        }
        if dossier.message_count > 500 {
            dossier.flags.push("high_volume_contact".to_string());
        }
        if dossier.financial_thread_mentions > 0 {
            dossier.flags.push("financial_context_present".to_string());
        }
    }

    Ok(dossiers.into_values().collect())
}

fn link_cerberus(
    ctx: &AgentCtx,
    handle_to_contact: &HashMap<String, i64>,
    dossiers: &mut HashMap<i64, PsycheDossier>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let path = ctx.case.evidence_path("cerberus").join("records.json");
    let values = read_json_array(&path)?;
    for value in values {
        let record_type = string_field(&value, "record_type").unwrap_or_default();
        if record_type != "message" && record_type != "voicemail" {
            continue;
        }
        let raw_handle = string_field(&value, "phone_number")
            .or_else(|| string_field(&value, "sender"))
            .or_else(|| string_field(&value, "callback_num"))
            .unwrap_or_default();
        let handle = normalize_handle(&raw_handle, "phone");
        let Some(contact_id) = handle_to_contact.get(&handle).copied() else {
            continue;
        };
        let Some(dossier) = dossiers.get_mut(&contact_id) else {
            continue;
        };
        let timestamp = string_field(&value, "timestamp");
        observe_time(dossier, timestamp.as_deref());
        if record_type == "message" {
            dossier.message_count += 1;
            match string_field(&value, "direction").as_deref() {
                Some("Sent") => dossier.sent_messages += 1,
                Some("Received") => dossier.received_messages += 1,
                _ => {}
            }
        } else {
            dossier.voicemail_count += 1;
        }
        push_evidence_ref(
            dossier,
            PsycheEvidenceRef {
                source_agent: "Cerberus".to_string(),
                record_type,
                record_id: string_field(&value, "id"),
                timestamp,
                direction: string_field(&value, "direction"),
                handle: Some(raw_handle),
                excerpt: string_field(&value, "text")
                    .or_else(|| string_field(&value, "transcript"))
                    .unwrap_or_else(|| compact_json(&value)),
            },
        );
    }
    if dossiers.values().all(|dossier| dossier.message_count == 0) {
        warnings.push(format!(
            "No contact-linked messages found from {}",
            path.display()
        ));
    }
    Ok(())
}

fn link_obolus(
    ctx: &AgentCtx,
    handle_to_contact: &HashMap<String, i64>,
    name_to_contact: &HashMap<String, i64>,
    dossiers: &mut HashMap<i64, PsycheDossier>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let thread_path = ctx
        .case
        .evidence_path("obolus")
        .join("thread_mentions.json");
    if thread_path.exists() {
        for value in read_json_array(&thread_path)? {
            let raw_handle = string_field(&value, "phone_number").unwrap_or_default();
            let handle = normalize_handle(&raw_handle, "phone");
            let Some(contact_id) = handle_to_contact.get(&handle).copied() else {
                continue;
            };
            if let Some(dossier) = dossiers.get_mut(&contact_id) {
                dossier.financial_thread_mentions += 1;
                push_evidence_ref(
                    dossier,
                    PsycheEvidenceRef {
                        source_agent: "Obolus".to_string(),
                        record_type: "financial_thread_mention".to_string(),
                        record_id: string_field(&value, "record_id"),
                        timestamp: None,
                        direction: string_field(&value, "direction"),
                        handle: Some(raw_handle),
                        excerpt: string_field(&value, "text")
                            .unwrap_or_else(|| compact_json(&value)),
                    },
                );
            }
        }
    } else {
        warnings.push(format!("Missing {}", thread_path.display()));
    }

    let notes_path = ctx.case.evidence_path("obolus").join("notes.json");
    if notes_path.exists() {
        for value in read_json_array(&notes_path)? {
            let haystack = format!(
                "{} {}",
                string_field(&value, "title").unwrap_or_default(),
                string_field(&value, "body").unwrap_or_default()
            )
            .to_lowercase();
            for (name_key, contact_id) in name_to_contact {
                if name_key.len() < 4 || !haystack.contains(name_key) {
                    continue;
                }
                if let Some(dossier) = dossiers.get_mut(contact_id) {
                    dossier.note_mentions += 1;
                    push_evidence_ref(
                        dossier,
                        PsycheEvidenceRef {
                            source_agent: "Obolus".to_string(),
                            record_type: "note_mention".to_string(),
                            record_id: string_field(&value, "id"),
                            timestamp: string_field(&value, "modified"),
                            direction: None,
                            handle: None,
                            excerpt: string_field(&value, "body")
                                .unwrap_or_else(|| compact_json(&value)),
                        },
                    );
                }
            }
        }
    }
    Ok(())
}

fn synthesize_with_ollama(model: &str, dossier: &PsycheDossier) -> Result<String> {
    let prompt = format!(
        "You are Psyche, an evidence-bound contact dossier reviewer.\n\
         Use only the source snippets below. Do not infer motive as fact.\n\
         Return concise bullets under: Facts, Patterns, Open Questions, Caution.\n\n\
         Contact: {}\nHandles: {}\nCounts: messages={}, sent={}, received={}, voicemail={}, financial_mentions={}, notes={}\n\nEvidence:\n{}",
        dossier.contact_name,
        dossier
            .identities
            .iter()
            .map(|identity| identity.value.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        dossier.message_count,
        dossier.sent_messages,
        dossier.received_messages,
        dossier.voicemail_count,
        dossier.financial_thread_mentions,
        dossier.note_mentions,
        dossier
            .evidence_refs
            .iter()
            .take(MAX_EXCERPTS_PER_CONTACT)
            .enumerate()
            .map(|(index, evidence)| format!(
                "{}. [{}:{}] {}",
                index + 1,
                evidence.source_agent,
                evidence.record_type,
                truncate_chars(&evidence.excerpt.replace('\n', " "), 800)
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let response: serde_json::Value = client
        .post("http://127.0.0.1:11434/api/generate")
        .json(&json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.1,
                "top_p": 0.9
            }
        }))
        .send()
        .context("calling Ollama /api/generate")?
        .json()
        .context("parsing Ollama response")?;
    response
        .get("response")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Ollama response did not include response text"))
}

fn read_json_array(path: &Path) -> Result<Vec<serde_json::Value>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn write_dossiers_csv(path: &Path, dossiers: &[PsycheDossier]) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "contact_id",
        "contact_name",
        "identities",
        "message_count",
        "sent_messages",
        "received_messages",
        "voicemail_count",
        "financial_thread_mentions",
        "note_mentions",
        "first_observed",
        "last_observed",
        "flags",
        "llm_model",
        "llm_summary",
    ])?;
    for dossier in dossiers {
        writer.write_record([
            dossier.contact_id.to_string(),
            dossier.contact_name.clone(),
            dossier
                .identities
                .iter()
                .map(|identity| identity.value.clone())
                .collect::<Vec<_>>()
                .join("; "),
            dossier.message_count.to_string(),
            dossier.sent_messages.to_string(),
            dossier.received_messages.to_string(),
            dossier.voicemail_count.to_string(),
            dossier.financial_thread_mentions.to_string(),
            dossier.note_mentions.to_string(),
            dossier.first_observed.clone().unwrap_or_default(),
            dossier.last_observed.clone().unwrap_or_default(),
            dossier.flags.join("; "),
            dossier.llm_model.clone().unwrap_or_default(),
            dossier.llm_summary.clone().unwrap_or_default(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

fn write_index_html(path: &Path, case_name: &str, dossiers: &[PsycheDossier]) -> Result<()> {
    let mut html = String::new();
    html.push_str(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Psyche Contact Dossiers</title>",
    );
    html.push_str("<style>body{font-family:system-ui,sans-serif;margin:24px;color:#1f2937}table{border-collapse:collapse;width:100%;margin-top:16px}th,td{border-bottom:1px solid #ddd;padding:6px 8px;text-align:left;vertical-align:top}th{background:#f3f4f6}.num{text-align:right}.excerpt{max-width:760px}pre{white-space:pre-wrap;font-family:inherit;margin:0}</style></head><body>");
    html.push_str(&format!(
        "<h1>Psyche Contact Dossiers - {}</h1>",
        html_escape(case_name)
    ));
    html.push_str("<table><thead><tr><th>Contact</th><th>Handles</th><th class=\"num\">Messages</th><th class=\"num\">Financial</th><th class=\"num\">Notes</th><th>Flags</th><th>Evidence Excerpts</th></tr></thead><tbody>");
    for dossier in dossiers {
        let excerpts = dossier
            .evidence_refs
            .iter()
            .take(5)
            .map(|evidence| {
                format!(
                    "[{}:{}] {}",
                    evidence.source_agent, evidence.record_type, evidence.excerpt
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>{}</td><td class=\"excerpt\"><pre>{}</pre></td></tr>",
            html_escape(&dossier.contact_name),
            html_escape(
                &dossier
                    .identities
                    .iter()
                    .map(|identity| identity.value.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            dossier.message_count,
            dossier.financial_thread_mentions,
            dossier.note_mentions,
            html_escape(&dossier.flags.join(", ")),
            html_escape(&excerpts)
        ));
    }
    html.push_str("</tbody></table></body></html>");
    fs::write(path, html).with_context(|| format!("writing {}", path.display()))
}

fn push_evidence_ref(dossier: &mut PsycheDossier, evidence: PsycheEvidenceRef) {
    if dossier.evidence_refs.len() < 50 {
        dossier.evidence_refs.push(evidence);
    }
}

fn observe_time(dossier: &mut PsycheDossier, timestamp: Option<&str>) {
    let Some(timestamp) = timestamp.filter(|value| !value.is_empty()) else {
        return;
    };
    if dossier
        .first_observed
        .as_deref()
        .map(|current| timestamp < current)
        .unwrap_or(true)
    {
        dossier.first_observed = Some(timestamp.to_string());
    }
    if dossier
        .last_observed
        .as_deref()
        .map(|current| timestamp > current)
        .unwrap_or(true)
    {
        dossier.last_observed = Some(timestamp.to_string());
    }
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key)? {
        serde_json::Value::String(raw) => Some(raw.trim().to_string()).filter(|s| !s.is_empty()),
        serde_json::Value::Number(raw) => Some(raw.to_string()),
        _ => None,
    }
}

fn normalize_handle(raw: &str, value_type: &str) -> String {
    if value_type == "email" || raw.contains('@') {
        return raw.trim().to_lowercase();
    }
    let digits: String = raw.chars().filter(|ch| ch.is_ascii_digit()).collect();
    let without_star67 = digits.strip_prefix("67").unwrap_or(&digits);
    if without_star67.len() > 10 {
        without_star67[without_star67.len() - 10..].to_string()
    } else {
        without_star67.to_string()
    }
}

fn normalize_name_key(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut iter = value.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
