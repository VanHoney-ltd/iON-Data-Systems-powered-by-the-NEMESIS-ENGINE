//! Cerberus — Unified communications extraction (SMS/MMS, calls, voicemail, contacts).

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{BufWriter, Write};

use crate::agents::cerberus_models::{ContactValueRecord, VoicemailRecord};
use crate::agents::{Agent, AgentCtx};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
use crate::common::target::{
    addressbook_target, call_history_target, sms_target, voicemail_target,
};
use crate::evidence::EvidenceRecord;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CerberusAgent;

impl Agent for CerberusAgent {
    const NAME: &'static str = "Cerberus";
    const SLUG: &'static str = "cerberus";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        let mut contact_count = 0usize;
        let mut contact_identity_count = 0usize;
        let mut call_count = 0usize;
        let mut voicemail_count = 0usize;

        // 1. Contacts (from original contacts agent)
        match extract_contacts_internal(ctx) {
            Ok(mut contact_records) => {
                contact_count = contact_records
                    .iter()
                    .filter(|record| record.record_type == "contact")
                    .count();
                contact_identity_count = contact_records
                    .iter()
                    .filter(|record| record.record_type == "contact_identity")
                    .count();
                records.append(&mut contact_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus contacts extraction skipped: {}", e));
            }
        }

        // 2. Call history (best-effort; original hermes was a stub)
        match extract_call_history(ctx) {
            Ok(mut call_records) => {
                call_count = call_records.len();
                records.append(&mut call_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus call history extraction skipped: {}", e));
            }
        }

        // 3. Voicemail metadata (best-effort; original hermes was a stub)
        match extract_voicemail(ctx) {
            Ok(mut vm_records) => {
                voicemail_count = vm_records.len();
                records.append(&mut vm_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus voicemail extraction skipped: {}", e));
            }
        }

        // 4. SMS / MMS messages and attachments
        let mut message_count = 0usize;
        let mut attachment_count = 0usize;
        match extract_messages_and_attachments(ctx) {
            Ok((mut msg_records, mut att_records)) => {
                message_count = msg_records.len();
                attachment_count = att_records.len();
                records.append(&mut msg_records);
                records.append(&mut att_records);
            }
            Err(e) => {
                ctx.log(&format!("Cerberus SMS/MMS extraction skipped: {}", e));
            }
        }

        records.push(EvidenceRecord {
            schema_version: Self::SCHEMA_VERSION,
            source_agent: Self::NAME.to_string(),
            record_type: "report".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "case_id": ctx.case.name(),
                "state": "complete",
                "contacts_extracted": contact_count,
                "contact_identities_extracted": contact_identity_count,
                "calls_extracted": call_count,
                "voicemails_extracted": voicemail_count,
                "messages_extracted": message_count,
                "attachments_extracted": attachment_count,
                "records_emitted": records.len(),
                "warnings": Vec::<String>::new(),
            }),
        });

        Ok(records)
    }
}

// ---------------------------------------------------------------------------
// Contacts
// ---------------------------------------------------------------------------

fn extract_contacts_internal(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;

    let target = addressbook_target();
    let resolved = resolver.resolve_known_target(&target)?;
    let audit = ResolverAuditRecord {
        artifact_key: target.artifact_key.to_string(),
        candidates: target
            .candidates
            .iter()
            .map(|c| format!("{}/{}", c.domain, c.relative_path))
            .collect(),
        chosen: resolved
            .as_ref()
            .map(|a| a.source_path.display().to_string()),
        method: resolved.as_ref().map(|a| format!("{:?}", a.method)),
    };
    crate::common::resolver::write_resolver_audit(resolver.case_root(), &audit)?;

    let resolved = resolved.context("AddressBook.sqlitedb could not be resolved")?;

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let mut stmt = conn.prepare(
        r#"
        SELECT
            ABPerson.ROWID,
            COALESCE(NULLIF(TRIM(ABPerson.DisplayName), ''),
                     NULLIF(TRIM(COALESCE(ABPerson.First, '') || ' ' || COALESCE(ABPerson.Last, '')), ''),
                     NULLIF(TRIM(ABPerson.Organization), ''),
                     NULLIF(TRIM(ABPerson.CompositeNameFallback), ''),
                     'Unknown') AS display_name,
            NULLIF(TRIM(ABPerson.Organization), '') AS organization,
            NULLIF(TRIM(ABPerson.Note), '') AS note,
            NULLIF(TRIM(ABPerson.Birthday), '') AS birthday,
            NULLIF(TRIM(ABPerson.JobTitle), '') AS job_title,
            NULLIF(TRIM(ABPerson.Nickname), '') AS nickname,
            0 AS blocked,
            NULL AS department
        FROM ABPerson
        ORDER BY display_name
        "#,
    )?;

    let base_records: Vec<(
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<String>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let contact_values = load_contact_values(&conn)?;
    let mut values_by_contact: HashMap<i64, Vec<ContactValueRecord>> = HashMap::new();
    for value in contact_values {
        values_by_contact
            .entry(value.contact_id)
            .or_default()
            .push(value);
    }

    let mut records = Vec::new();
    let mut contacts_export = Vec::new();
    let mut identities_export = Vec::new();

    for (id, name, organization, note, birthday, job_title, nickname, blocked, department) in
        base_records
    {
        let mut values = values_by_contact.remove(&id).unwrap_or_default();
        for value in &mut values {
            value.is_voip_like =
                value.is_voip_like || is_voip_like_identity(Some(&name), &value.value);
        }
        let phones = values_of_type(&values, "phone");
        let emails = values_of_type(&values, "email");
        let urls = values_of_type(&values, "url");
        let blocked = blocked != 0;
        let contact_payload = json!({
            "id": id,
            "name": name,
            "phones": phones,
            "emails": emails,
            "urls": urls,
            "labeled_values": values,
            "organization": organization,
            "note": note,
            "birthday": birthday,
            "job_title": job_title,
            "nickname": nickname,
            "blocked": blocked,
            "department": department,
        });

        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "contact".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: contact_payload.clone(),
        });
        contacts_export.push(contact_payload);

        for value in values {
            let identity_payload = json!({
                "contact_id": id,
                "contact_name": name,
                "value_type": value.value_type,
                "value": value.value,
                "label": value.label,
                "identifier": value.identifier,
                "guid": value.guid,
                "is_voip_like": value.is_voip_like,
                "source": "AddressBook.sqlitedb",
            });
            records.push(EvidenceRecord {
                schema_version: CerberusAgent::SCHEMA_VERSION,
                source_agent: CerberusAgent::NAME.to_string(),
                record_type: "contact_identity".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: identity_payload.clone(),
            });
            identities_export.push(identity_payload);
        }
    }

    export_contact_identity_package(ctx, &contacts_export, &identities_export)?;

    Ok(records)
}

fn load_contact_values(conn: &rusqlite::Connection) -> Result<Vec<ContactValueRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            mv.record_id,
            mv.property,
            mv.value,
            mv.identifier,
            mv.guid,
            label.value AS label_value
        FROM ABMultiValue mv
        LEFT JOIN ABMultiValueLabel label ON label.rowid = mv.label
        WHERE mv.property IN (3, 4, 22)
          AND NULLIF(TRIM(mv.value), '') IS NOT NULL
        ORDER BY mv.record_id, mv.property, mv.identifier, mv.UID
        "#,
    )?;

    let values = stmt
        .query_map([], |row| {
            let property = row.get::<_, i64>(1)?;
            let value = row.get::<_, String>(2)?;
            let label = row.get::<_, Option<String>>(5)?;
            Ok(ContactValueRecord {
                contact_id: row.get(0)?,
                value_type: contact_value_type(property).to_string(),
                is_voip_like: is_voip_like_identity(label.as_deref(), &value),
                value,
                label,
                identifier: row.get(3)?,
                guid: row.get(4)?,
            })
        })?
        .filter_map(|row| row.ok())
        .collect();
    Ok(values)
}

fn contact_value_type(property: i64) -> &'static str {
    match property {
        3 => "phone",
        4 => "email",
        22 => "url",
        _ => "other",
    }
}

fn values_of_type(values: &[ContactValueRecord], value_type: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .filter(|value| value.value_type == value_type)
        .filter_map(|value| {
            if seen.insert(value.value.clone()) {
                Some(value.value.clone())
            } else {
                None
            }
        })
        .collect()
}

fn is_voip_like_identity(label: Option<&str>, value: &str) -> bool {
    let haystack = format!("{} {}", label.unwrap_or_default(), value).to_ascii_lowercase();
    [
        "voip",
        "textnow",
        "text now",
        "google voice",
        "voice",
        "whatsapp",
        "signal",
        "telegram",
        "burner",
        "temp",
        "temporary",
        "old number",
        "other",
        "app",
    ]
    .iter()
    .any(|term| haystack.contains(term))
}

fn export_contact_identity_package(
    ctx: &AgentCtx,
    contacts: &[serde_json::Value],
    identities: &[serde_json::Value],
) -> Result<()> {
    let evidence_dir = ctx.case.evidence_path("cerberus");
    fs::create_dir_all(&evidence_dir)?;
    write_json(evidence_dir.join("contacts_full.json"), contacts)?;
    write_json(evidence_dir.join("contact_identities.json"), identities)?;

    let mut wtr = csv::Writer::from_path(evidence_dir.join("contact_identities.csv"))?;
    wtr.write_record([
        "contact_id",
        "contact_name",
        "value_type",
        "value",
        "label",
        "identifier",
        "guid",
        "is_voip_like",
        "source",
    ])?;
    for identity in identities {
        wtr.write_record([
            json_field(identity, "contact_id"),
            json_field(identity, "contact_name"),
            json_field(identity, "value_type"),
            json_field(identity, "value"),
            json_field(identity, "label"),
            json_field(identity, "identifier"),
            json_field(identity, "guid"),
            json_field(identity, "is_voip_like"),
            json_field(identity, "source"),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn json_field(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(raw)) => raw.clone(),
        Some(serde_json::Value::Number(raw)) => raw.to_string(),
        Some(serde_json::Value::Bool(raw)) => raw.to_string(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Call history (best-effort; no original queries available)
// ---------------------------------------------------------------------------

fn extract_call_history(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = call_history_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    // Try modern ZCALLRECORD schema first, fallback to generic
    let query = r#"
        SELECT
            Z_PK AS id,
            ZDATE AS timestamp,
            ZDURATION AS duration,
            ZCALLTYPE AS call_type,
            ZADDRESS AS phone_number,
            ZANSWERED AS answered,
            ZLOCATION AS location,
            ZSERVICE_PROVIDER AS service_provider
        FROM ZCALLRECORD
        ORDER BY ZDATE DESC
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()), // Schema mismatch — skip silently
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, timestamp, duration, call_type, phone, answered, location, service) = row;
        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "call_history".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "timestamp": timestamp,
                "duration": duration,
                "call_type": call_type,
                "phone_number": phone,
                "answered": answered.map(|v| v != 0),
                "location": location,
                "service_provider": service,
            }),
        });
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// Voicemail (best-effort; no original queries available)
// ---------------------------------------------------------------------------

fn extract_voicemail(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = voicemail_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let query = r#"
        SELECT
            ROWID AS id,
            sender,
            callback_num,
            duration,
            expiration,
            trashed_date,
            date,
            token,
            flags
        FROM voicemail
        ORDER BY date DESC
    "#;

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<i64>>(8)?,
        ))
    })?;

    let mut records = Vec::new();
    let mut voicemail_records: Vec<VoicemailRecord> = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, sender, callback, duration, _expiration, _trashed, date, _token, _flags) = row;
        let timestamp = date.unwrap_or(0);
        let phone = callback.clone().or_else(|| sender.clone());

        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "voicemail".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "sender": sender,
                "callback_num": callback,
                "duration": duration,
                "date": date,
            }),
        });

        voicemail_records.push(VoicemailRecord {
            id: id.unwrap_or(0),
            date,
            timestamp,
            duration,
            phone_number: phone.clone(),
            sender: sender.clone(),
            filename: None,
            file_path: None,
            file_size: None,
            hash: None,
            transcript_path: None,
            diarization_path: None,
            confidence: None,
            speakers: None,
            is_transcribed: false,
            is_diarized: false,
        });
    }

    // Write voicemail records for Echo agent
    if !voicemail_records.is_empty() {
        let voicemail_dir = ctx.case.root_path().join("evidence").join("voicemail");
        fs::create_dir_all(&voicemail_dir)?;
        let path = voicemail_dir.join("voicemail_records.json");
        let json = serde_json::to_string_pretty(&voicemail_records)?;
        fs::write(&path, json)?;
    }

    Ok(records)
}

// ---------------------------------------------------------------------------
// SMS / MMS messages & attachments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct CerberusMessageExport {
    thread_id: String,
    chat_id: Option<i64>,
    chat_identifier: Option<String>,
    chat_display_name: Option<String>,
    message_id: i64,
    guid: String,
    timestamp_raw: Option<i64>,
    timestamp_utc: Option<String>,
    direction: String,
    handle: String,
    service: String,
    text: Option<String>,
    subject: Option<String>,
    attachment_count: usize,
    attachment_paths: Vec<String>,
    attachment_export_paths: Vec<String>,
    attachment_mime_types: Vec<String>,
    is_read: bool,
    is_delivered: bool,
    is_sent: bool,
    is_audio_message: bool,
    item_type: Option<i64>,
    group_title: Option<String>,
    reply_to_guid: Option<String>,
    balloon_bundle_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CerberusAttachmentExport {
    attachment_id: i64,
    message_id: i64,
    thread_id: String,
    guid: String,
    created_raw: Option<i64>,
    created_utc: Option<String>,
    original_filename: Option<String>,
    transfer_name: Option<String>,
    mime_type: Option<String>,
    uti: Option<String>,
    total_bytes: Option<i64>,
    source_path: Option<String>,
    export_path: Option<String>,
    sha256: Option<String>,
    media_class: String,
    missing: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CerberusThreadExport {
    thread_id: String,
    chat_id: Option<i64>,
    chat_identifier: Option<String>,
    display_name: Option<String>,
    message_count: usize,
    attachment_count: usize,
    first_timestamp_utc: Option<String>,
    last_timestamp_utc: Option<String>,
    participants: Vec<String>,
    html_path: String,
}

struct ContactMessageGroup<'a> {
    key: String,
    label: String,
    messages: Vec<&'a CerberusMessageExport>,
}

fn extract_messages_and_attachments(
    ctx: &AgentCtx,
) -> Result<(Vec<EvidenceRecord>, Vec<EvidenceRecord>)> {
    let resolver = BackupResolver::from_case(&ctx.case)?;
    let target = sms_target();
    let resolved = match resolver.resolve_known_target(&target)? {
        Some(r) => r,
        None => return Ok((Vec::new(), Vec::new())),
    };

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: ctx.case.root_path(),
            clean_root: ctx.case.root_path().join("clean"),
            temp_root: ctx.case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let evidence_dir = ctx.case.evidence_path("cerberus");
    fs::create_dir_all(&evidence_dir)?;
    let package_dir = evidence_dir.join("review_package");
    let attachments_root = package_dir.join("threads");
    fs::create_dir_all(&attachments_root)?;

    let attachment_rows = load_attachment_rows(&conn)?;
    let mut attachments_by_message: HashMap<i64, Vec<CerberusAttachmentExport>> = HashMap::new();
    for mut attachment in attachment_rows {
        let source_path =
            resolve_sms_attachment_path(&ctx.backup_root, attachment.original_filename.as_deref());
        attachment.source_path = source_path.as_ref().map(|p| p.display().to_string());
        attachment.missing = source_path.is_none();
        attachments_by_message
            .entry(attachment.message_id)
            .or_default()
            .push(attachment);
    }

    let msg_query = r#"
        SELECT
            m.ROWID,
            m.guid,
            m.text,
            m.date,
            m.date_read,
            m.date_delivered,
            m.is_from_me,
            m.is_read,
            m.is_delivered,
            m.is_sent,
            m.service,
            m.subject,
            m.cache_has_attachments,
            m.item_type,
            m.group_title,
            h.id AS phone_number,
            c.ROWID AS chat_id,
            c.chat_identifier,
            c.display_name,
            m.is_audio_message,
            m.reply_to_guid,
            m.balloon_bundle_id
        FROM message m
        LEFT JOIN chat_message_join cmj ON m.ROWID = cmj.message_id
        LEFT JOIN chat c ON cmj.chat_id = c.ROWID
        LEFT JOIN handle h ON m.handle_id = h.ROWID
        ORDER BY COALESCE(c.ROWID, -m.ROWID), m.date ASC, m.ROWID ASC
    "#;

    let mut stmt = match conn.prepare(msg_query) {
        Ok(s) => s,
        Err(_) => return Ok((Vec::new(), Vec::new())),
    };

    let msg_rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<i64>>(12)?,
            row.get::<_, Option<i64>>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<String>>(15)?,
            row.get::<_, Option<i64>>(16)?,
            row.get::<_, Option<String>>(17)?,
            row.get::<_, Option<String>>(18)?,
            row.get::<_, Option<i64>>(19)?,
            row.get::<_, Option<String>>(20)?,
            row.get::<_, Option<String>>(21)?,
        ))
    })?;

    let mut msg_records = Vec::new();
    let mut att_records = Vec::new();
    let mut all_phones: Vec<String> = Vec::new();
    let mut messages = Vec::new();
    let mut attachments = Vec::new();
    let mut threads: BTreeMap<String, Vec<CerberusMessageExport>> = BTreeMap::new();

    for row in msg_rows.filter_map(|r| r.ok()) {
        let (
            id,
            guid,
            text,
            date,
            date_read,
            date_delivered,
            is_from_me,
            is_read,
            is_delivered,
            is_sent,
            service,
            subject,
            cache_has_attachments,
            item_type,
            group_title,
            phone_number,
            chat_id,
            chat_identifier,
            chat_display_name,
            is_audio_message,
            reply_to_guid,
            balloon_bundle_id,
        ) = row;
        let phone = phone_number.unwrap_or_default();
        let direction = if is_from_me.unwrap_or(0) != 0 {
            "Sent"
        } else {
            "Received"
        };
        let has_attachments = cache_has_attachments.unwrap_or(0) != 0;
        let service_str = service.as_deref().unwrap_or("SMS");
        let thread_id = thread_id_for(chat_id, chat_identifier.as_deref(), &phone, id);

        if !phone.is_empty() && !all_phones.contains(&phone) {
            all_phones.push(phone.clone());
        }

        let mut message_attachments = attachments_by_message.remove(&id).unwrap_or_default();
        for attachment in &mut message_attachments {
            attachment.thread_id = thread_id.clone();
            if let Some(source_path) = attachment.source_path.as_ref().map(PathBuf::from) {
                let export_path = copy_attachment_to_thread(
                    &source_path,
                    &attachments_root,
                    &thread_id,
                    id,
                    attachment,
                )?;
                attachment.sha256 = if export_path.is_file() {
                    crate::common::prepared::compute_sha256(&export_path).ok()
                } else {
                    None
                };
                attachment.export_path = Some(export_path.display().to_string());
                attachment.missing = false;
            }
        }

        let attachment_paths: Vec<String> = message_attachments
            .iter()
            .filter_map(|a| a.source_path.clone())
            .collect();
        let attachment_export_paths: Vec<String> = message_attachments
            .iter()
            .filter_map(|a| a.export_path.clone())
            .collect();
        let attachment_mime_types: Vec<String> = message_attachments
            .iter()
            .filter_map(|a| a.mime_type.clone())
            .collect();

        let exported_message = CerberusMessageExport {
            thread_id: thread_id.clone(),
            chat_id,
            chat_identifier: chat_identifier.clone(),
            chat_display_name: chat_display_name.clone(),
            message_id: id,
            guid: guid.clone(),
            timestamp_raw: date,
            timestamp_utc: date.and_then(imessage_timestamp_to_rfc3339),
            direction: direction.to_string(),
            handle: phone.clone(),
            service: service_str.to_string(),
            text: text.clone(),
            subject: subject.clone(),
            attachment_count: message_attachments.len(),
            attachment_paths,
            attachment_export_paths,
            attachment_mime_types,
            is_read: is_read.unwrap_or(0) != 0,
            is_delivered: is_delivered.unwrap_or(0) != 0,
            is_sent: is_sent.unwrap_or(0) != 0,
            is_audio_message: is_audio_message.unwrap_or(0) != 0,
            item_type,
            group_title: group_title.clone(),
            reply_to_guid,
            balloon_bundle_id,
        };

        msg_records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "message".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "guid": guid,
                "text": text,
                "date": date,
                "date_read": date_read,
                "date_delivered": date_delivered,
                "direction": direction,
                "phone_number": phone,
                "thread_id": thread_id,
                "chat_id": chat_id,
                "chat_identifier": chat_identifier,
                "chat_display_name": chat_display_name,
                "is_read": is_read.unwrap_or(0) != 0,
                "is_delivered": is_delivered.unwrap_or(0) != 0,
                "is_sent": is_sent.unwrap_or(0) != 0,
                "service": service_str,
                "subject": subject,
                "has_attachments": has_attachments,
                "attachment_count": exported_message.attachment_count,
                "attachment_export_paths": exported_message.attachment_export_paths,
                "item_type": item_type,
                "group_title": group_title,
            }),
        });

        for attachment in message_attachments {
            att_records.push(EvidenceRecord {
                schema_version: CerberusAgent::SCHEMA_VERSION,
                source_agent: CerberusAgent::NAME.to_string(),
                record_type: "attachment".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: serde_json::to_value(&attachment)?,
            });
            attachments.push(attachment);
        }

        threads
            .entry(thread_id.clone())
            .or_default()
            .push(exported_message.clone());
        messages.push(exported_message);
    }

    if !all_phones.is_empty() {
        let _ = ctx.case.remember_phone_numbers(&all_phones);
    }

    write_cerberus_review_package(ctx, &package_dir, &messages, &attachments, &threads)?;

    Ok((msg_records, att_records))
}

fn load_attachment_rows(conn: &rusqlite::Connection) -> Result<Vec<CerberusAttachmentExport>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            a.ROWID,
            a.guid,
            a.created_date,
            a.filename,
            a.uti,
            a.mime_type,
            a.transfer_name,
            a.total_bytes,
            maj.message_id
        FROM attachment a
        JOIN message_attachment_join maj ON a.ROWID = maj.attachment_id
        ORDER BY a.created_date ASC, a.ROWID ASC
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        let mime_type = row.get::<_, Option<String>>(5)?;
        Ok(CerberusAttachmentExport {
            attachment_id: row.get(0)?,
            guid: row.get(1)?,
            created_raw: row.get(2)?,
            created_utc: row
                .get::<_, Option<i64>>(2)?
                .and_then(imessage_timestamp_to_rfc3339),
            original_filename: row.get(3)?,
            uti: row.get(4)?,
            media_class: classify_media(
                mime_type.as_deref(),
                row.get::<_, Option<String>>(3)?.as_deref(),
            )
            .to_string(),
            mime_type,
            transfer_name: row.get(6)?,
            total_bytes: row.get(7)?,
            message_id: row.get(8)?,
            thread_id: String::new(),
            source_path: None,
            export_path: None,
            sha256: None,
            missing: true,
        })
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn write_cerberus_review_package(
    ctx: &AgentCtx,
    package_dir: &Path,
    messages: &[CerberusMessageExport],
    attachments: &[CerberusAttachmentExport],
    threads: &BTreeMap<String, Vec<CerberusMessageExport>>,
) -> Result<()> {
    fs::create_dir_all(package_dir)?;

    write_json(package_dir.join("messages.json"), messages)?;
    write_jsonl(package_dir.join("messages.jsonl"), messages)?;
    write_messages_csv(&package_dir.join("messages.csv"), messages)?;
    write_message_html_archive(ctx, package_dir, messages)?;

    write_json(package_dir.join("attachments.json"), attachments)?;
    write_jsonl(package_dir.join("attachments.jsonl"), attachments)?;
    write_attachments_csv(&package_dir.join("attachments.csv"), attachments)?;

    let mut thread_exports = Vec::new();
    for (thread_id, thread_messages) in threads {
        let thread_dir = package_dir.join("threads").join(safe_name(thread_id));
        fs::create_dir_all(&thread_dir)?;
        let html_path = thread_dir.join("thread.html");
        let html = build_thread_html(ctx.case.name(), thread_id, thread_messages);
        fs::write(&html_path, html)?;
        let relative_html_path = format!("threads/{}/thread.html", safe_name(thread_id));

        thread_exports.push(CerberusThreadExport {
            thread_id: thread_id.clone(),
            chat_id: thread_messages.first().and_then(|m| m.chat_id),
            chat_identifier: thread_messages
                .first()
                .and_then(|m| m.chat_identifier.clone()),
            display_name: thread_messages
                .first()
                .and_then(|m| m.chat_display_name.clone()),
            message_count: thread_messages.len(),
            attachment_count: thread_messages.iter().map(|m| m.attachment_count).sum(),
            first_timestamp_utc: thread_messages
                .first()
                .and_then(|m| m.timestamp_utc.clone()),
            last_timestamp_utc: thread_messages.last().and_then(|m| m.timestamp_utc.clone()),
            participants: participants_for_thread(thread_messages),
            html_path: relative_html_path,
        });
    }

    write_json(package_dir.join("threads.json"), &thread_exports)?;
    write_jsonl(package_dir.join("threads.jsonl"), &thread_exports)?;
    write_threads_csv(&package_dir.join("threads.csv"), &thread_exports)?;

    let index_html = build_review_index_html(ctx.case.name(), &thread_exports);
    let index_path = package_dir.join("index.html");
    fs::write(&index_path, index_html)?;
    if thread_exports.len() <= 100 {
        let _ = convert_html_to_pdf_best_effort(
            &index_path,
            &package_dir.join("cerberus_printable_index.pdf"),
        );
    } else {
        ctx.log(&format!(
            "Cerberus printable PDF index skipped for large case ({} threads); use review_package/index.html",
            thread_exports.len()
        ));
    }

    Ok(())
}

fn write_message_html_archive(
    ctx: &AgentCtx,
    package_dir: &Path,
    messages: &[CerberusMessageExport],
) -> Result<()> {
    let html_dir = package_dir.join("messages_html");
    let contacts_dir = html_dir.join("contacts");
    fs::create_dir_all(&contacts_dir)?;

    let groups = group_messages_by_contact(messages);
    write_message_html_index(ctx.case.name(), &html_dir, messages, &groups)?;
    for group in &groups {
        write_contact_message_html(ctx.case.name(), package_dir, &contacts_dir, group)?;
    }

    ctx.log(&format!(
        "Cerberus HTML contact archive written: {} messages across {} contact/conversation page(s)",
        messages.len(),
        groups.len()
    ));

    Ok(())
}

fn write_message_html_index(
    case_name: &str,
    html_dir: &Path,
    messages: &[CerberusMessageExport],
    groups: &[ContactMessageGroup<'_>],
) -> Result<()> {
    let contact_rows: String = groups
        .iter()
        .map(|group| {
            let first_time = group
                .messages
                .first()
                .and_then(|m| m.timestamp_utc.as_deref())
                .unwrap_or("");
            let last_time = group
                .messages
                .last()
                .and_then(|m| m.timestamp_utc.as_deref())
                .unwrap_or("");
            let attachment_count: usize = group.messages.iter().map(|m| m.attachment_count).sum();
            format!(
                r#"<tr>
  <td><a href="contacts/{file}.html">{label}</a></td>
  <td>{count}</td>
  <td>{attachments}</td>
  <td>{first}</td>
  <td>{last}</td>
</tr>"#,
                file = escape_html(&safe_name(&group.key)),
                label = escape_html(&group.label),
                count = group.messages.len(),
                attachments = attachment_count,
                first = escape_html(first_time),
                last = escape_html(last_time)
            )
        })
        .collect();
    let first_time = messages
        .first()
        .and_then(|m| m.timestamp_utc.as_deref())
        .unwrap_or("");
    let last_time = messages
        .last()
        .and_then(|m| m.timestamp_utc.as_deref())
        .unwrap_or("");
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Cerberus Message HTML Archive - {case}</title>
<style>{style}</style>
</head>
<body>
<header>
  <h1>Cerberus Contact Message Archive</h1>
  <p>Case: <strong>{case}</strong> | Messages: <strong>{count}</strong> | Contacts/conversations: <strong>{contact_count}</strong> | Range: {first} to {last}</p>
</header>
<main>
  <section class="panel">
    <h2>Select Contact</h2>
    <p class="hint">Open a contact/conversation to view the complete linked message record for that contact only. No page contains the entire case message corpus.</p>
    <table>
      <thead><tr><th>Contact / Conversation</th><th>Messages</th><th>Attachments</th><th>First</th><th>Last</th></tr></thead>
      <tbody>{contact_rows}</tbody>
    </table>
  </section>
</main>
</body>
</html>"#,
        case = escape_html(case_name),
        count = messages.len(),
        contact_count = groups.len(),
        first = escape_html(first_time),
        last = escape_html(last_time),
        contact_rows = contact_rows,
        style = message_archive_css()
    );
    fs::write(html_dir.join("index.html"), html)?;
    Ok(())
}

fn write_contact_message_html(
    case_name: &str,
    package_dir: &Path,
    contacts_dir: &Path,
    group: &ContactMessageGroup<'_>,
) -> Result<()> {
    let path = contacts_dir.join(format!("{}.html", safe_name(&group.key)));
    let mut file = BufWriter::new(fs::File::create(&path)?);
    write_message_doc_start(
        &mut file,
        &format!("Messages - {}", group.label),
        case_name,
        group.messages.len(),
        None,
        "../index.html",
    )?;
    for message in &group.messages {
        write_message_article(&mut file, package_dir, "../../", message)?;
    }
    write_message_doc_end(&mut file)?;
    Ok(())
}

fn group_messages_by_contact(messages: &[CerberusMessageExport]) -> Vec<ContactMessageGroup<'_>> {
    let mut grouped: BTreeMap<String, ContactMessageGroup<'_>> = BTreeMap::new();
    for msg in messages {
        let key = contact_group_key(msg);
        let label = contact_group_label(msg);
        grouped
            .entry(key.clone())
            .or_insert_with(|| ContactMessageGroup {
                key,
                label,
                messages: Vec::new(),
            })
            .messages
            .push(msg);
    }
    let mut groups: Vec<_> = grouped.into_values().collect();
    groups.sort_by(|a, b| {
        b.messages
            .len()
            .cmp(&a.messages.len())
            .then_with(|| a.label.cmp(&b.label))
    });
    groups
}

fn contact_group_key(msg: &CerberusMessageExport) -> String {
    if !msg.handle.trim().is_empty() {
        return format!("handle:{}", msg.handle.trim());
    }
    if let Some(identifier) = msg
        .chat_identifier
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        return format!("chat_identifier:{}", identifier.trim());
    }
    if let Some(name) = msg
        .chat_display_name
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        return format!("chat_display:{}", name.trim());
    }
    format!("thread:{}", msg.thread_id)
}

fn contact_group_label(msg: &CerberusMessageExport) -> String {
    if let Some(name) = msg
        .chat_display_name
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        if !msg.handle.trim().is_empty() {
            return format!("{} ({})", name.trim(), msg.handle.trim());
        }
        return name.trim().to_string();
    }
    if let Some(identifier) = msg
        .chat_identifier
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        return identifier.trim().to_string();
    }
    if !msg.handle.trim().is_empty() {
        return msg.handle.trim().to_string();
    }
    msg.thread_id.clone()
}

fn write_message_doc_start<W: Write>(
    file: &mut W,
    title: &str,
    case_name: &str,
    message_count: usize,
    page_label: Option<&str>,
    home_link: &str,
) -> Result<()> {
    write!(
        file,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>{style}</style>
</head>
<body>
<header>
  <h1>{title}</h1>
  <p>Case: <strong>{case}</strong> | Messages: <strong>{count}</strong>{page}</p>
  <p><a href="{home}">Back to Cerberus index</a></p>
</header>
<main>
"#,
        title = escape_html(title),
        style = message_archive_css(),
        case = escape_html(case_name),
        count = message_count,
        page = page_label
            .map(|v| format!(" | {}", escape_html(v)))
            .unwrap_or_default(),
        home = escape_html(home_link)
    )?;
    Ok(())
}

fn write_message_doc_end<W: Write>(file: &mut W) -> Result<()> {
    write!(file, "</main>\n</body>\n</html>\n")?;
    Ok(())
}

fn write_message_article<W: Write>(
    file: &mut W,
    package_dir: &Path,
    up_prefix: &str,
    msg: &CerberusMessageExport,
) -> Result<()> {
    let text = msg.text.as_deref().unwrap_or("");
    let subject = msg.subject.as_deref().unwrap_or("");
    let attachments = if msg.attachment_export_paths.is_empty() {
        String::new()
    } else {
        let items: String = msg
            .attachment_export_paths
            .iter()
            .zip(
                msg.attachment_mime_types
                    .iter()
                    .chain(std::iter::repeat(&String::new())),
            )
            .map(|(path, mime)| archive_attachment_html(package_dir, up_prefix, path, mime))
            .collect();
        format!(r#"<div class="attachments">{}</div>"#, items)
    };
    write!(
        file,
        r#"<article class="message {dir}" id="msg-{id}">
  <div class="meta">
    <span>{time}</span>
    <span>{direction}</span>
    <span>{handle}</span>
    <span>{service}</span>
    <span>thread: {thread}</span>
    <span>id: {id}</span>
  </div>
  {subject}
  <div class="body">{text}</div>
  {attachments}
</article>
"#,
        dir = if msg.direction == "Sent" {
            "sent"
        } else {
            "received"
        },
        id = msg.message_id,
        time = escape_html(msg.timestamp_utc.as_deref().unwrap_or("")),
        direction = escape_html(&msg.direction),
        handle = escape_html(&msg.handle),
        service = escape_html(&msg.service),
        thread = escape_html(&msg.thread_id),
        subject = if subject.is_empty() {
            String::new()
        } else {
            format!(r#"<div class="subject">{}</div>"#, escape_html(subject))
        },
        text = escape_html(text).replace('\n', "<br>"),
        attachments = attachments
    )?;
    Ok(())
}

fn archive_attachment_html(package_dir: &Path, up_prefix: &str, path: &str, mime: &str) -> String {
    let link = archive_relative_link(package_dir, up_prefix, path);
    let label = escape_html(
        Path::new(path)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(path),
    );
    if mime.starts_with("image/") && !mime.contains("heic") {
        format!(
            r#"<a class="attachment image" href="{link}"><img src="{link}" alt="{label}"><span>{label}</span></a>"#
        )
    } else {
        format!(r#"<a class="attachment file" href="{link}">{label}</a>"#)
    }
}

fn archive_relative_link(package_dir: &Path, up_prefix: &str, path: &str) -> String {
    let path_ref = Path::new(path);
    let relative = path_ref
        .strip_prefix(package_dir)
        .ok()
        .map(|p| p.display().to_string().replace('\\', "/"))
        .unwrap_or_else(|| path.to_string());
    escape_html(&format!(
        "{}{}",
        up_prefix,
        relative.trim_start_matches('/')
    ))
}

fn message_archive_css() -> &'static str {
    r#"body{font-family:Arial,Helvetica,sans-serif;margin:24px;color:#17212b;background:#f7f8fa}header{margin-bottom:18px}h1{font-size:22px;margin:0 0 8px}h2{font-size:16px}.panel{background:#fff;border:1px solid #d7dde6;border-radius:6px;padding:14px;margin:12px 0}.hint{color:#52606d}table{border-collapse:collapse;width:100%;background:#fff}td,th{border:1px solid #d7dde6;padding:7px;font-size:13px;text-align:left;vertical-align:top}th{background:#edf1f5}.pager{margin:0 0 14px}.pager a{display:inline-block;margin-right:8px}.message{background:#fff;border:1px solid #d7dde6;border-radius:6px;margin:10px 0;padding:10px;break-inside:avoid}.sent{border-left:5px solid #2563eb}.received{border-left:5px solid #16a34a}.meta{display:flex;flex-wrap:wrap;gap:10px;color:#52606d;font-size:12px;margin-bottom:8px}.subject{font-weight:bold;margin-bottom:8px}.body{white-space:pre-wrap;font-size:14px;line-height:1.45}.attachments{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:8px;margin-top:10px}.attachment{border:1px solid #e3e8ef;border-radius:4px;background:#fbfcfd;padding:6px;font-size:12px;color:#243b53;text-decoration:none}.attachment img{display:block;max-width:100%;max-height:240px;object-fit:contain;margin-bottom:4px}@media(max-width:800px){body{margin:12px}.meta{display:block}.meta span{display:block;margin:2px 0}td,th{font-size:12px}}@media print{body{background:#fff;margin:10mm}.message{page-break-inside:avoid}}"#
}

fn write_json<T: Serialize + ?Sized>(path: PathBuf, value: &T) -> Result<()> {
    fs::write(&path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn write_jsonl<T: Serialize>(path: PathBuf, values: &[T]) -> Result<()> {
    let mut file =
        fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    for value in values {
        writeln!(file, "{}", serde_json::to_string(value)?)?;
    }
    Ok(())
}

fn write_messages_csv(path: &Path, messages: &[CerberusMessageExport]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "thread_id",
        "chat_id",
        "chat_identifier",
        "chat_display_name",
        "message_id",
        "guid",
        "timestamp_utc",
        "direction",
        "handle",
        "service",
        "text",
        "subject",
        "attachment_count",
        "attachment_export_paths",
        "attachment_mime_types",
        "is_read",
        "is_delivered",
        "is_sent",
        "is_audio_message",
        "reply_to_guid",
        "balloon_bundle_id",
    ])?;
    for msg in messages {
        wtr.write_record([
            msg.thread_id.clone(),
            msg.chat_id.map(|v| v.to_string()).unwrap_or_default(),
            msg.chat_identifier.clone().unwrap_or_default(),
            msg.chat_display_name.clone().unwrap_or_default(),
            msg.message_id.to_string(),
            msg.guid.clone(),
            msg.timestamp_utc.clone().unwrap_or_default(),
            msg.direction.clone(),
            msg.handle.clone(),
            msg.service.clone(),
            msg.text.clone().unwrap_or_default(),
            msg.subject.clone().unwrap_or_default(),
            msg.attachment_count.to_string(),
            msg.attachment_export_paths.join(";"),
            msg.attachment_mime_types.join(";"),
            msg.is_read.to_string(),
            msg.is_delivered.to_string(),
            msg.is_sent.to_string(),
            msg.is_audio_message.to_string(),
            msg.reply_to_guid.clone().unwrap_or_default(),
            msg.balloon_bundle_id.clone().unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_attachments_csv(path: &Path, attachments: &[CerberusAttachmentExport]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "thread_id",
        "message_id",
        "attachment_id",
        "guid",
        "created_utc",
        "media_class",
        "mime_type",
        "uti",
        "transfer_name",
        "original_filename",
        "total_bytes",
        "source_path",
        "export_path",
        "sha256",
        "missing",
    ])?;
    for att in attachments {
        wtr.write_record([
            att.thread_id.clone(),
            att.message_id.to_string(),
            att.attachment_id.to_string(),
            att.guid.clone(),
            att.created_utc.clone().unwrap_or_default(),
            att.media_class.clone(),
            att.mime_type.clone().unwrap_or_default(),
            att.uti.clone().unwrap_or_default(),
            att.transfer_name.clone().unwrap_or_default(),
            att.original_filename.clone().unwrap_or_default(),
            att.total_bytes.map(|v| v.to_string()).unwrap_or_default(),
            att.source_path.clone().unwrap_or_default(),
            att.export_path.clone().unwrap_or_default(),
            att.sha256.clone().unwrap_or_default(),
            att.missing.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn write_threads_csv(path: &Path, threads: &[CerberusThreadExport]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "thread_id",
        "chat_id",
        "chat_identifier",
        "display_name",
        "message_count",
        "attachment_count",
        "first_timestamp_utc",
        "last_timestamp_utc",
        "participants",
        "html_path",
    ])?;
    for thread in threads {
        wtr.write_record([
            thread.thread_id.clone(),
            thread.chat_id.map(|v| v.to_string()).unwrap_or_default(),
            thread.chat_identifier.clone().unwrap_or_default(),
            thread.display_name.clone().unwrap_or_default(),
            thread.message_count.to_string(),
            thread.attachment_count.to_string(),
            thread.first_timestamp_utc.clone().unwrap_or_default(),
            thread.last_timestamp_utc.clone().unwrap_or_default(),
            thread.participants.join(";"),
            thread.html_path.clone(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn resolve_sms_attachment_path(backup_root: &Path, filename: Option<&str>) -> Option<PathBuf> {
    let filename = filename?;
    let mut candidates = Vec::new();
    if let Some(rest) = filename.strip_prefix("~/") {
        candidates.push(backup_root.join("HomeDomain").join(rest));
        candidates.push(backup_root.join("MediaDomain").join(rest));
    }
    if let Some(rest) = filename.strip_prefix("/var/mobile/") {
        candidates.push(backup_root.join("HomeDomain").join(rest));
        candidates.push(backup_root.join("MediaDomain").join(rest));
    }
    if let Some(rest) = filename.strip_prefix("/var/tmp/com.apple.messages/com.apple.MobileSMS/") {
        candidates.push(
            backup_root
                .join("MediaDomain")
                .join("Library/SMS")
                .join(rest),
        );
    }
    candidates.push(backup_root.join(filename.trim_start_matches('/')));

    candidates.into_iter().find(|p| p.exists())
}

fn copy_attachment_to_thread(
    source_path: &Path,
    threads_root: &Path,
    thread_id: &str,
    message_id: i64,
    attachment: &CerberusAttachmentExport,
) -> Result<PathBuf> {
    let media_dir = threads_root
        .join(safe_name(thread_id))
        .join("attachments")
        .join(&attachment.media_class);
    fs::create_dir_all(&media_dir)?;

    let fallback_name = source_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("attachment.bin");
    let name = attachment
        .transfer_name
        .as_deref()
        .or(attachment
            .original_filename
            .as_deref()
            .and_then(|p| Path::new(p).file_name().and_then(|v| v.to_str())))
        .unwrap_or(fallback_name);
    let dest = media_dir.join(format!(
        "msg_{}_att_{}_{}",
        message_id,
        attachment.attachment_id,
        safe_name(name)
    ));
    if source_path.is_dir() {
        copy_dir_recursive(source_path, &dest).with_context(|| {
            format!(
                "copying attachment dir {} -> {}",
                source_path.display(),
                dest.display()
            )
        })?;
    } else {
        fs::copy(source_path, &dest).with_context(|| {
            format!(
                "copying attachment {} -> {}",
                source_path.display(),
                dest.display()
            )
        })?;
    }
    Ok(dest)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn build_thread_html(
    case_name: &str,
    thread_id: &str,
    messages: &[CerberusMessageExport],
) -> String {
    let rows: String = messages
        .iter()
        .map(|msg| {
            let attachments = if msg.attachment_export_paths.is_empty() {
                String::new()
            } else {
                let items: String = msg
                    .attachment_export_paths
                    .iter()
                    .zip(msg.attachment_mime_types.iter().chain(std::iter::repeat(&String::new())))
                    .map(|(path, mime)| attachment_html(path, mime))
                    .collect();
                format!(r#"<div class="attachments">{}</div>"#, items)
            };
            format!(
                r#"<article class="msg {dir_class}">
  <div class="meta"><span>{time}</span><span>{direction}</span><span>{handle}</span><span>{service}</span></div>
  <div class="body">{text}</div>
  {attachments}
</article>"#,
                dir_class = if msg.direction == "Sent" { "sent" } else { "received" },
                time = escape_html(msg.timestamp_utc.as_deref().unwrap_or("")),
                direction = escape_html(&msg.direction),
                handle = escape_html(&msg.handle),
                service = escape_html(&msg.service),
                text = escape_html(msg.text.as_deref().unwrap_or("")),
                attachments = attachments,
            )
        })
        .collect();

    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>{case} {thread}</title>
<style>
body {{ font-family: Arial, sans-serif; color: #1f2933; margin: 24px; }}
h1 {{ font-size: 18px; margin-bottom: 4px; }}
.subtitle {{ color: #667; font-size: 12px; margin-bottom: 20px; }}
.msg {{ border: 1px solid #d9dee7; border-radius: 6px; padding: 10px; margin: 10px 0; break-inside: avoid; }}
.sent {{ border-left: 5px solid #2563eb; }}
.received {{ border-left: 5px solid #16a34a; }}
.meta {{ display: flex; gap: 12px; flex-wrap: wrap; color: #52606d; font-size: 11px; margin-bottom: 8px; }}
.body {{ white-space: pre-wrap; font-size: 13px; }}
.attachments {{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; margin-top: 10px; }}
.attachment {{ border: 1px solid #edf0f5; padding: 6px; font-size: 11px; }}
.attachment img {{ max-width: 100%; max-height: 280px; object-fit: contain; display: block; }}
.file {{ color: #334e68; }}
@media print {{ body {{ margin: 10mm; }} .msg {{ page-break-inside: avoid; }} }}
</style></head><body>
<h1>Cerberus Thread Export</h1>
<div class="subtitle">Case: {case} | Thread: {thread} | Messages: {count}</div>
{rows}
</body></html>"#,
        case = escape_html(case_name),
        thread = escape_html(thread_id),
        count = messages.len(),
        rows = rows
    )
}

fn attachment_html(path: &str, mime: &str) -> String {
    let escaped_path = escape_html(path);
    let label = escape_html(
        Path::new(path)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(path),
    );
    if mime.starts_with("image/") && !mime.contains("heic") {
        format!(
            r#"<div class="attachment"><img src="{escaped_path}" alt="{label}"><div>{label}</div></div>"#
        )
    } else if mime.starts_with("video/") {
        format!(
            r#"<div class="attachment file"><strong>Video:</strong> <a href="{escaped_path}">{label}</a></div>"#
        )
    } else if mime.starts_with("audio/")
        || label.to_ascii_lowercase().ends_with(".caf")
        || label.to_ascii_lowercase().ends_with(".m4a")
    {
        format!(
            r#"<div class="attachment file"><strong>Audio:</strong> <a href="{escaped_path}">{label}</a></div>"#
        )
    } else if mime.contains("gif") {
        format!(
            r#"<div class="attachment"><img src="{escaped_path}" alt="{label}"><div>{label}</div></div>"#
        )
    } else {
        format!(
            r#"<div class="attachment file"><strong>Attachment:</strong> <a href="{escaped_path}">{label}</a></div>"#
        )
    }
}

fn build_review_index_html(case_name: &str, threads: &[CerberusThreadExport]) -> String {
    let rows: String = threads
        .iter()
        .map(|thread| {
            format!(
                "<tr><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&thread.html_path),
                escape_html(thread.display_name.as_deref().or(thread.chat_identifier.as_deref()).unwrap_or(&thread.thread_id)),
                thread.message_count,
                thread.attachment_count,
                escape_html(thread.first_timestamp_utc.as_deref().unwrap_or("")),
                escape_html(thread.last_timestamp_utc.as_deref().unwrap_or(""))
            )
        })
        .collect();
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Cerberus {case}</title>
<style>body{{font-family:Arial,sans-serif;margin:24px;color:#1f2933}}table{{border-collapse:collapse;width:100%}}td,th{{border:1px solid #d9dee7;padding:6px;font-size:12px}}th{{background:#f3f5f8;text-align:left}}</style>
</head><body><h1>Cerberus Review Package</h1><p>Case: {case}</p><table><thead><tr><th>Thread</th><th>Messages</th><th>Attachments</th><th>First</th><th>Last</th></tr></thead><tbody>{rows}</tbody></table></body></html>"#,
        case = escape_html(case_name),
        rows = rows
    )
}

fn convert_html_to_pdf_best_effort(html_path: &Path, pdf_path: &Path) -> Result<()> {
    if Command::new("weasyprint")
        .args([html_path, pdf_path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let out_dir = pdf_path.parent().unwrap_or_else(|| Path::new("."));
    let status = Command::new("libreoffice")
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(out_dir)
        .arg(html_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("HTML to PDF conversion failed")
    }
}

fn participants_for_thread(messages: &[CerberusMessageExport]) -> Vec<String> {
    let mut participants = Vec::new();
    for msg in messages {
        if !msg.handle.is_empty() && !participants.contains(&msg.handle) {
            participants.push(msg.handle.clone());
        }
    }
    participants
}

fn thread_id_for(
    chat_id: Option<i64>,
    chat_identifier: Option<&str>,
    phone: &str,
    message_id: i64,
) -> String {
    if let Some(chat_id) = chat_id {
        return format!("chat_{}", chat_id);
    }
    if let Some(chat_identifier) = chat_identifier.filter(|v| !v.is_empty()) {
        return format!("chat_{}", safe_name(chat_identifier));
    }
    if !phone.is_empty() {
        return format!("handle_{}", safe_name(phone));
    }
    format!("message_{}", message_id)
}

fn imessage_timestamp_to_rfc3339(raw: i64) -> Option<String> {
    let seconds = if raw > 10_000_000_000_000_000 {
        raw / 1_000_000_000 + 978_307_200
    } else {
        raw + 978_307_200
    };
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(|dt: DateTime<Utc>| dt.to_rfc3339())
}

fn classify_media(mime: Option<&str>, filename: Option<&str>) -> &'static str {
    let lower_name = filename.unwrap_or("").to_ascii_lowercase();
    let mime = mime.unwrap_or("").to_ascii_lowercase();
    if mime.starts_with("image/")
        || lower_name.ends_with(".heic")
        || lower_name.ends_with(".jpg")
        || lower_name.ends_with(".jpeg")
        || lower_name.ends_with(".png")
        || lower_name.ends_with(".gif")
    {
        "images"
    } else if mime.starts_with("video/")
        || lower_name.ends_with(".mov")
        || lower_name.ends_with(".mp4")
    {
        "videos"
    } else if mime.starts_with("audio/")
        || lower_name.ends_with(".caf")
        || lower_name.ends_with(".m4a")
        || lower_name.ends_with(".amr")
    {
        "audio"
    } else if lower_name.ends_with(".vcf") {
        "contacts"
    } else if lower_name.contains("location") || lower_name.ends_with(".loc.vcf") {
        "locations"
    } else {
        "files"
    }
}

fn safe_name(value: &str) -> String {
    let mut out: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_')
        .chars()
        .take(120)
        .collect::<String>()
        .if_empty("unnamed")
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

// ---------------------------------------------------------------------------
// Attachment Catalog PDF Generation
// ---------------------------------------------------------------------------

#[allow(dead_code)]
const ATTACHMENT_CHUNK_SIZE: usize = 500;

#[allow(dead_code)]
fn generate_attachment_catalog(ctx: &AgentCtx, att_records: &[EvidenceRecord]) -> Result<()> {
    let evidence_dir = ctx.case.evidence_path("cerberus");
    fs::create_dir_all(&evidence_dir)?;

    let chunks: Vec<&[EvidenceRecord]> = att_records.chunks(ATTACHMENT_CHUNK_SIZE).collect();
    let mut chunk_pdfs: Vec<PathBuf> = Vec::new();

    for (idx, chunk) in chunks.iter().enumerate() {
        let html_path = evidence_dir.join(format!("attachments_chunk_{}.html", idx));
        let pdf_path = evidence_dir.join(format!("attachments_chunk_{}.pdf", idx));

        let html = build_attachment_catalog_html(ctx.case.name(), chunk, idx, chunks.len());
        fs::write(&html_path, html)?;

        let lo_out = Command::new("libreoffice")
            .args([
                "--headless",
                "--convert-to",
                "pdf",
                "--outdir",
                evidence_dir.to_str().unwrap(),
                html_path.to_str().unwrap(),
            ])
            .output()?;

        let lo_pdf = html_path.with_extension("pdf");
        if lo_out.status.success() && lo_pdf.exists() {
            if lo_pdf != pdf_path {
                fs::rename(&lo_pdf, &pdf_path)?;
            }
        } else {
            let wp_out = Command::new("weasyprint")
                .args([html_path.to_str().unwrap(), pdf_path.to_str().unwrap()])
                .output()?;
            if !wp_out.status.success() {
                anyhow::bail!("PDF conversion failed for attachment chunk {}", idx);
            }
        }

        let _ = fs::remove_file(&html_path);
        chunk_pdfs.push(pdf_path);
    }

    let final_pdf = evidence_dir.join("attachments_catalog.pdf");
    if chunk_pdfs.len() == 1 {
        fs::rename(&chunk_pdfs[0], &final_pdf)?;
    } else {
        let mut args = vec!["--empty".to_string(), "--pages".to_string()];
        for pdf in &chunk_pdfs {
            args.push(pdf.to_str().unwrap().to_string());
        }
        args.push("--".to_string());
        args.push(final_pdf.to_str().unwrap().to_string());

        let out = Command::new("qpdf").args(&args).output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("qpdf merge failed: {}", stderr);
        }

        for pdf in &chunk_pdfs {
            let _ = fs::remove_file(pdf);
        }
    }

    ctx.log(&format!(
        "Cerberus: attachment catalog PDF generated: {} ({} attachments)",
        final_pdf.display(),
        att_records.len()
    ));

    Ok(())
}

#[allow(dead_code)]
fn build_attachment_catalog_html(
    case_name: &str,
    records: &[EvidenceRecord],
    chunk_idx: usize,
    total_chunks: usize,
) -> String {
    let now = chrono::Utc::now().to_rfc3339();

    let items: String = records
        .iter()
        .map(|record| {
            let payload = record.payload.as_object().unwrap_or(&serde_json::Map::new()).clone();
            let filename = payload.get("filename").and_then(|v| v.as_str()).unwrap_or("unknown");
            let transfer_name = payload.get("transfer_name").and_then(|v| v.as_str()).unwrap_or(filename);
            let mime = payload.get("mime_type").and_then(|v| v.as_str()).unwrap_or("");
            let phone = payload.get("phone_number").and_then(|v| v.as_str()).unwrap_or("");
            let date = payload.get("date").and_then(|v| v.as_i64()).unwrap_or(0);
            let file_size = payload.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0);

            let is_video = mime.starts_with("video/");
            let is_image = mime.starts_with("image/");

            let icon = if is_video { "🎬" } else if is_image { "📷" } else { "📎" };
            let type_label = if is_video { "VIDEO" } else if is_image { "IMAGE" } else { "FILE" };

            // Try to find the actual file
            let img_tag = if !filename.is_empty() {
                let possible = vec![
                    PathBuf::from("backup").join(filename),
                    PathBuf::from("clean").join(filename),
                    PathBuf::from(filename),
                ];
                let found = possible.iter().find(|p| p.exists());
                if let Some(path) = found {
                    if is_image {
                        format!(r#"<img src="{}" alt="{}" />"#, escape_html(path.to_str().unwrap_or("")), escape_html(transfer_name))
                    } else if is_video {
                        format!(r#"<div class="placeholder" style="color:#d94a4a;"><div style="font-size:18pt;margin-bottom:4px;">🎬</div>{}</div>"#, escape_html(transfer_name))
                    } else {
                        format!(r#"<div class="placeholder" style="color:#888;"><div style="font-size:18pt;margin-bottom:4px;">📎</div>{}</div>"#, escape_html(transfer_name))
                    }
                } else {
                    let color = if is_video { "#d94a4a" } else if is_image { "#4a90d9" } else { "#888" };
                    format!(r#"<div class="placeholder" style="color:{};"><div style="font-size:18pt;margin-bottom:4px;">{}</div>{}</div>"#, color, icon, escape_html(transfer_name))
                }
            } else {
                format!(r#"<div class="placeholder" style="color:#888;"><div style="font-size:18pt;margin-bottom:4px;">📎</div>No file</div>"#)
            };

            let video_overlay = if is_video {
                r#"<div class="play-overlay">▶</div>"#.to_string()
            } else {
                String::new()
            };

            let size_str = if file_size > 0 {
                if file_size > 1024 * 1024 {
                    format!("{:.1} MB", file_size as f64 / (1024.0 * 1024.0))
                } else if file_size > 1024 {
                    format!("{:.1} KB", file_size as f64 / 1024.0)
                } else {
                    format!("{} B", file_size)
                }
            } else {
                String::new()
            };

            format!(
                r#"<div class="item">
  <div class="thumb">{}{}</div>
  <div class="info">
    <div class="meta-line"><strong>{}</strong></div>
    <div class="meta-line">{} &nbsp;|&nbsp; {}</div>
    <div class="meta-line">{}</div>
    <div class="meta-line">{}</div>
  </div>
</div>"#,
                img_tag,
                video_overlay,
                escape_html(transfer_name),
                type_label,
                size_str,
                if phone.is_empty() { "" } else { phone },
                if date > 0 { format!("{}", date) } else { String::new() }
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Cerberus Attachments — {case}</title>
<style>
  @page {{ size: A4 portrait; margin: 12mm; @bottom-center {{ content: "Page " counter(page); font-size: 7pt; color: #888; }} }}
  body {{ font-family: "Segoe UI", Roboto, Helvetica, Arial, sans-serif; font-size: 8pt; color: #222; margin: 0; padding: 10px; }}
  h1 {{ font-size: 14pt; color: #1a1a2e; margin-bottom: 3px; }}
  .subtitle {{ color: #666; font-size: 8pt; margin-bottom: 12px; }}
  .grid {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }}
  .item {{ border: 1px solid #ddd; border-radius: 4px; padding: 6px; break-inside: avoid; }}
  .thumb {{ position: relative; width: 100%; height: 160px; background: #f0f0f0; display: flex; align-items: center; justify-content: center; border-radius: 3px; overflow: hidden; }}
  .thumb img {{ max-width: 100%; max-height: 160px; object-fit: contain; }}
  .thumb .placeholder {{ color: #999; font-size: 10pt; text-align: center; padding: 8px; }}
  .play-overlay {{ position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); font-size: 28px; color: white; text-shadow: 0 0 6px rgba(0,0,0,0.7); pointer-events: none; }}
  .info {{ margin-top: 5px; }}
  .meta-line {{ font-size: 6.5pt; color: #444; line-height: 1.3; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
</style>
</head>
<body>
  <h1>Cerberus Attachment Catalog</h1>
  <div class="subtitle">Case: {case} &nbsp;|&nbsp; Chunk {chunk} of {total} &nbsp;|&nbsp; Generated: {now}</div>
  <div class="grid">
    {items}
  </div>
</body>
</html>"#,
        case = escape_html(case_name),
        chunk = chunk_idx + 1,
        total = total_chunks,
        now = escape_html(&now),
        items = items,
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Standalone contact extraction for backward compatibility (chronos pipeline)
// ---------------------------------------------------------------------------

/// Standalone contact extraction for backward compatibility.
pub fn extract_contacts(
    case: &crate::case::Case,
) -> Result<Vec<crate::agents::cerberus_models::ContactRecord>> {
    use crate::agents::cerberus_models::ContactRecord;
    use crate::common::prepared::{prepare_artifact, PrepareContext};
    use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
    use crate::common::target::addressbook_target;

    let resolver = BackupResolver::from_case(case)?;
    let target = addressbook_target();
    let resolved = resolver.resolve_known_target(&target)?;
    let audit = ResolverAuditRecord {
        artifact_key: target.artifact_key.to_string(),
        candidates: target
            .candidates
            .iter()
            .map(|c| format!("{}/{}", c.domain, c.relative_path))
            .collect(),
        chosen: resolved
            .as_ref()
            .map(|a| a.source_path.display().to_string()),
        method: resolved.as_ref().map(|a| format!("{:?}", a.method)),
    };
    crate::common::resolver::write_resolver_audit(resolver.case_root(), &audit)?;

    let resolved = resolved.context("AddressBook.sqlitedb could not be resolved")?;

    let prepared = prepare_artifact(
        &PrepareContext {
            case_root: case.root_path(),
            clean_root: case.root_path().join("clean"),
            temp_root: case.root_path().join("tmp"),
        },
        &resolved,
        target.sqlite_like,
    )?;

    let conn = prepared.open_sqlite_ro()?;

    let mut stmt = conn.prepare(
        r#"
        SELECT
            ABPerson.ROWID,
            COALESCE(NULLIF(TRIM(ABPerson.DisplayName), ''),
                     NULLIF(TRIM(COALESCE(ABPerson.First, '') || ' ' || COALESCE(ABPerson.Last, '')), ''),
                     NULLIF(TRIM(ABPerson.Organization), ''),
                     NULLIF(TRIM(ABPerson.CompositeNameFallback), ''),
                     'Unknown') AS display_name,
            NULLIF(TRIM(ABPerson.Organization), '') AS organization,
            NULLIF(TRIM(ABPerson.Note), '') AS note,
            NULLIF(TRIM(ABPerson.Birthday), '') AS birthday,
            NULLIF(TRIM(ABPerson.JobTitle), '') AS job_title,
            NULLIF(TRIM(ABPerson.Nickname), '') AS nickname,
            ABPerson.Flag AS blocked,
            NULLIF(TRIM(ABPerson.Department), '') AS department
        FROM ABPerson
        ORDER BY display_name
        "#,
    )?;

    let base_records: Vec<(
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<String>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let contact_values = load_contact_values(&conn)?;
    let mut values_by_contact: HashMap<i64, Vec<ContactValueRecord>> = HashMap::new();
    for value in contact_values {
        values_by_contact
            .entry(value.contact_id)
            .or_default()
            .push(value);
    }

    let mut records = Vec::new();
    for (id, name, organization, note, birthday, job_title, nickname, blocked, department) in
        base_records
    {
        let mut labeled_values = values_by_contact.remove(&id).unwrap_or_default();
        for value in &mut labeled_values {
            value.is_voip_like =
                value.is_voip_like || is_voip_like_identity(Some(&name), &value.value);
        }
        let phones = values_of_type(&labeled_values, "phone");
        let emails = values_of_type(&labeled_values, "email");
        let urls = values_of_type(&labeled_values, "url");
        let blocked = blocked != 0;

        records.push(ContactRecord {
            id,
            name,
            phones,
            emails,
            urls,
            labeled_values,
            organization,
            note,
            birthday,
            job_title,
            nickname,
            blocked,
            department,
        });
    }

    records.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(records)
}

/// Standalone contact export for backward compatibility.
pub fn export_contacts(
    case: &crate::case::Case,
    contacts: &[crate::agents::cerberus_models::ContactRecord],
) -> Result<std::path::PathBuf> {
    let out_dir = case.evidence_path("cerberus");
    std::fs::create_dir_all(&out_dir)?;

    let json_path = out_dir.join("contacts.json");
    let csv_path = out_dir.join("contacts.csv");

    let json = serde_json::to_string_pretty(contacts)?;
    std::fs::write(&json_path, json)?;

    let mut wtr = csv::Writer::from_path(&csv_path)?;
    wtr.write_record([
        "id",
        "name",
        "phones",
        "emails",
        "urls",
        "labeled_values_json",
        "organization",
        "note",
        "birthday",
        "job_title",
        "nickname",
        "blocked",
        "department",
    ])?;
    for c in contacts {
        let blocked_str = if c.blocked {
            "true".to_string()
        } else {
            "false".to_string()
        };
        wtr.write_record([
            &c.id.to_string(),
            &c.name,
            &c.phones.join("; "),
            &c.emails.join("; "),
            &c.urls.join("; "),
            &serde_json::to_string(&c.labeled_values)?,
            c.organization.as_deref().unwrap_or(""),
            c.note.as_deref().unwrap_or(""),
            c.birthday.as_deref().unwrap_or(""),
            c.job_title.as_deref().unwrap_or(""),
            c.nickname.as_deref().unwrap_or(""),
            &blocked_str,
            c.department.as_deref().unwrap_or(""),
        ])?;
    }
    wtr.flush()?;

    Ok(json_path)
}
