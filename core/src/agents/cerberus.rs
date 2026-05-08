//! Cerberus — Unified communications extraction (SMS/MMS, calls, voicemail, contacts).

use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;

use crate::agents::{Agent, AgentCtx};
use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
use crate::common::prepared::{prepare_artifact, PrepareContext};
use crate::common::target::{addressbook_target, call_history_target, voicemail_target, sms_target};
use crate::agents::cerberus_models::{Message, Attachment, VoicemailRecord};
use crate::evidence::EvidenceRecord;
use std::path::PathBuf;
use std::path::Path;
use std::process::Command;

pub struct CerberusAgent;

impl Agent for CerberusAgent {
    const NAME: &'static str = "Cerberus";
    const SLUG: &'static str = "cerberus";
    const SCHEMA_VERSION: u32 = 1;

    fn extract(ctx: &AgentCtx) -> Result<Vec<EvidenceRecord>> {
        let mut records = Vec::new();
        let mut contact_count = 0usize;
        let mut call_count = 0usize;
        let mut voicemail_count = 0usize;

        // 1. Contacts (from original contacts agent)
        match extract_contacts_internal(ctx) {
            Ok(mut contact_records) => {
                contact_count = contact_records.len();
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
        method: resolved
            .as_ref()
            .map(|a| format!("{:?}", a.method)),
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
            ABPerson.Flag AS blocked,
            NULLIF(TRIM(ABPerson.Department), '') AS department
        FROM ABPerson
        ORDER BY display_name
        "#,
    )?;

    let base_records: Vec<(i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>)> = stmt
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

    let mut all_phones_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 3")?;
    let mut all_phones: HashMap<i64, Vec<String>> = all_phones_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_emails_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 4")?;
    let mut all_emails: HashMap<i64, Vec<String>> = all_emails_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_urls_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 22")?;
    let mut all_urls: HashMap<i64, Vec<String>> = all_urls_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut records = Vec::new();

    for (id, name, organization, note, birthday, job_title, nickname, blocked, department) in base_records {
        let phones = all_phones.remove(&id).unwrap_or_default();
        let emails = all_emails.remove(&id).unwrap_or_default();
        let urls = all_urls.remove(&id).unwrap_or_default();
        let blocked = blocked != 0;

        records.push(EvidenceRecord {
            schema_version: CerberusAgent::SCHEMA_VERSION,
            source_agent: CerberusAgent::NAME.to_string(),
            record_type: "contact".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            payload: json!({
                "id": id,
                "name": name,
                "phones": phones,
                "emails": emails,
                "urls": urls,
                "organization": organization,
                "note": note,
                "birthday": birthday,
                "job_title": job_title,
                "nickname": nickname,
                "blocked": blocked,
                "department": department,
            }),
        });
    }

    Ok(records)
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

fn extract_messages_and_attachments(ctx: &AgentCtx) -> Result<(Vec<EvidenceRecord>, Vec<EvidenceRecord>)> {
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

    // Messages with associated handle (phone number)
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
            h.id AS phone_number
        FROM message m
        LEFT JOIN handle h ON m.handle_id = h.ROWID
        ORDER BY m.date DESC
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
        ))
    })?;

    let mut msg_records = Vec::new();
    let mut all_phones: Vec<String> = Vec::new();

    for row in msg_rows.filter_map(|r| r.ok()) {
        let (id, guid, text, date, date_read, date_delivered, is_from_me, is_read, is_delivered, is_sent, service, subject, cache_has_attachments, item_type, group_title, phone_number) = row;
        let phone = phone_number.unwrap_or_default();
        let direction = if is_from_me.unwrap_or(0) != 0 { "Sent" } else { "Received" };
        let has_attachments = cache_has_attachments.unwrap_or(0) != 0;
        let service_str = service.as_deref().unwrap_or("SMS");

        if !phone.is_empty() && !all_phones.contains(&phone) {
            all_phones.push(phone.clone());
        }

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
                "is_read": is_read.unwrap_or(0) != 0,
                "is_delivered": is_delivered.unwrap_or(0) != 0,
                "is_sent": is_sent.unwrap_or(0) != 0,
                "service": service_str,
                "subject": subject,
                "has_attachments": has_attachments,
                "item_type": item_type,
                "group_title": group_title,
            }),
        });
    }

    // Persist phone numbers to case registry for downstream agents
    if !all_phones.is_empty() {
        let _ = ctx.case.remember_phone_numbers(&all_phones);
    }

    // Attachments
    let att_query = r#"
        SELECT
            a.ROWID,
            a.guid,
            a.created_date,
            a.filename,
            a.uti,
            a.mime_type,
            a.transfer_name,
            a.total_bytes,
            m.ROWID AS message_id,
            h.id AS phone_number
        FROM attachment a
        JOIN message_attachment_join maj ON a.ROWID = maj.attachment_id
        JOIN message m ON maj.message_id = m.ROWID
        LEFT JOIN handle h ON m.handle_id = h.ROWID
        ORDER BY a.created_date DESC
    "#;

    let mut att_records = Vec::new();
    if let Ok(mut att_stmt) = conn.prepare(att_query) {
        let att_rows = att_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })?;

        for row in att_rows.filter_map(|r| r.ok()) {
            let (id, guid, created_date, filename, uti, mime_type, transfer_name, total_bytes, message_id, phone_number) = row;
            att_records.push(EvidenceRecord {
                schema_version: CerberusAgent::SCHEMA_VERSION,
                source_agent: CerberusAgent::NAME.to_string(),
                record_type: "attachment".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                payload: json!({
                    "id": id,
                    "guid": guid,
                    "date": created_date,
                    "filename": filename,
                    "uti": uti,
                    "mime_type": mime_type,
                    "transfer_name": transfer_name,
                    "file_size": total_bytes,
                    "message_id": message_id,
                    "phone_number": phone_number.unwrap_or_default(),
                }),
            });
        }
    }

    // Also write flat files for backward compatibility
    let out_dir = ctx.case.evidence_path("cerberus");
    std::fs::create_dir_all(&out_dir)?;

    let messages_json: Vec<_> = msg_records.iter().map(|r| r.payload.clone()).collect();
    fs::write(out_dir.join("messages.json"), serde_json::to_string_pretty(&messages_json)?)?;

    let attachments_json: Vec<_> = att_records.iter().map(|r| r.payload.clone()).collect();
    fs::write(out_dir.join("attachments.json"), serde_json::to_string_pretty(&attachments_json)?)?;

    // Generate attachment catalog PDF
    if !att_records.is_empty() {
        if let Err(e) = generate_attachment_catalog(ctx, &att_records) {
            ctx.log(&format!("Cerberus attachment catalog failed: {}", e));
        }
    }

    Ok((msg_records, att_records))
}

// ---------------------------------------------------------------------------
// Attachment Catalog PDF Generation
// ---------------------------------------------------------------------------

const ATTACHMENT_CHUNK_SIZE: usize = 500;

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

fn build_attachment_catalog_html(case_name: &str, records: &[EvidenceRecord], chunk_idx: usize, total_chunks: usize) -> String {
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
pub fn extract_contacts(case: &crate::case::Case) -> Result<Vec<crate::agents::cerberus_models::ContactRecord>> {
    use crate::agents::cerberus_models::ContactRecord;
    use crate::common::resolver::{ArtifactResolver, BackupResolver, ResolverAuditRecord};
    use crate::common::prepared::{prepare_artifact, PrepareContext};
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
        chosen: resolved.as_ref().map(|a| a.source_path.display().to_string()),
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

    let base_records: Vec<(i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<String>)> = stmt
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

    let mut all_phones_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 3")?;
    let mut all_phones: HashMap<i64, Vec<String>> = all_phones_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_emails_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 4")?;
    let mut all_emails: HashMap<i64, Vec<String>> = all_emails_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut all_urls_stmt = conn.prepare("SELECT record_id, value FROM ABMultiValue WHERE property = 22")?;
    let mut all_urls: HashMap<i64, Vec<String>> = all_urls_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .fold(HashMap::new(), |mut acc, (id, value)| {
            acc.entry(id).or_default().push(value);
            acc
        });

    let mut records = Vec::new();
    for (id, name, organization, note, birthday, job_title, nickname, blocked, department) in base_records {
        let phones = all_phones.remove(&id).unwrap_or_default();
        let emails = all_emails.remove(&id).unwrap_or_default();
        let urls = all_urls.remove(&id).unwrap_or_default();
        let blocked = blocked != 0;

        records.push(ContactRecord {
            id,
            name,
            phones,
            emails,
            urls,
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
pub fn export_contacts(case: &crate::case::Case, contacts: &[crate::agents::cerberus_models::ContactRecord]) -> Result<std::path::PathBuf> {
    let out_dir = case.evidence_path("cerberus");
    std::fs::create_dir_all(&out_dir)?;

    let json_path = out_dir.join("contacts.json");
    let csv_path = out_dir.join("contacts.csv");

    let json = serde_json::to_string_pretty(contacts)?;
    std::fs::write(&json_path, json)?;

    let mut wtr = csv::Writer::from_path(&csv_path)?;
    wtr.write_record(["id", "name", "phones", "emails", "urls", "organization", "note", "birthday", "job_title", "nickname", "blocked", "department"])?;
    for c in contacts {
        let blocked_str = if c.blocked { "true".to_string() } else { "false".to_string() };
        wtr.write_record([
            &c.id.to_string(),
            &c.name,
            &c.phones.join("; "),
            &c.emails.join("; "),
            &c.urls.join("; "),
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
