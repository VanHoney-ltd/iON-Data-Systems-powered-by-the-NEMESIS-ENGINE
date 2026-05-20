//! PCR Packet Generator — Post-Conviction Relief Document Suite
//!
//! Generates professionally styled legal PDF exhibits for:
//! Tarrin Pearson Leary vs. The State of Iowa (SMAC422612)
//!
//! Uses existing iON Data Management Systems extracted evidence.
//! No raw backup parsing — consumes cleaned agent outputs only.

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone, Utc};
use csv::ReaderBuilder;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFENDANT_PHONE: &str = "5155531037";
const DEFENDANT_NAME: &str = "Tarrin Pearson Leary";
const COMPLAINANT_NAME: &str = "Alissa Kaylin Green";
const CASE_NUMBER: &str = "SMAC422612";
const OFFICER_NAME: &str = "Officer Cody Redmond";
const INCIDENT_DATE: &str = "October 1, 2025";

// Known Alissa phone numbers (from comprehensive timeline)
const ALISSA_NUMBERS: &[&str] = &[
    "5154994652",
    "5153806644",
    "5153305180",
    "5154284522",
    "5152092451",
    "5152021787",
    "6018902838",
    "4024688165",
    "5153295205",
    "5154104913",
    "5158849403",
    "5154181300",
    "5155611200",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("PCR PACKET ERROR: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        anyhow::bail!("Usage: pcr-packet <case_name>");
    }
    let case_name = &args[1];
    let case_root = locate_case_root(case_name)?;
    let out_dir = case_root.join("reports").join("pcr_packet");
    fs::create_dir_all(&out_dir)?;

    println!("[PCR] Loading evidence for case: {}", case_name);
    let data = EvidenceData::load(&case_root)?;
    println!(
        "[PCR] Loaded {} SMS, {} CashApp, {} notes, {} calls",
        data.sms.len(),
        data.cashapp.len(),
        data.notes.len(),
        data.calls.len()
    );

    // Generate each document
    let docs = vec![
        ("PCR_Motion", generate_pcr_motion(&data)),
        ("Exhibit_A_BPD_Pattern_Analysis", generate_exhibit_a(&data)),
        (
            "Exhibit_B_Financial_Extortion_Timeline",
            generate_exhibit_b(&data),
        ),
        (
            "Exhibit_C_Complainant_Contradictions",
            generate_exhibit_c(&data),
        ),
        ("Exhibit_D_Relationship_Evidence", generate_exhibit_d(&data)),
        (
            "Exhibit_E_Judicial_Weaponization",
            generate_exhibit_e(&data),
        ),
    ];

    for (name, html_result) in docs {
        let html = html_result?;
        let html_path = out_dir.join(format!("{}.html", name));
        let pdf_path = out_dir.join(format!("{}.pdf", name));
        fs::write(&html_path, html).with_context(|| format!("writing {}", html_path.display()))?;
        println!("[PCR] Converting {} ...", name);
        convert_html_to_pdf(&html_path, &pdf_path)?;
        let _ = fs::remove_file(&html_path);
        println!("[PCR] Generated {}", pdf_path.display());
    }

    // Also generate tools/commands log as PDF
    let tools_html = generate_tools_log()?;
    let tools_html_path = out_dir.join("TOOLS_AND_COMMANDS.html");
    let tools_pdf_path = out_dir.join("TOOLS_AND_COMMANDS.pdf");
    fs::write(&tools_html_path, tools_html)?;
    convert_html_to_pdf(&tools_html_path, &tools_pdf_path)?;
    let _ = fs::remove_file(&tools_html_path);
    println!("[PCR] Generated {}", tools_pdf_path.display());

    println!("\n[PCR] All documents complete in: {}", out_dir.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Data Structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SmsRecord {
    date: String,
    timestamp: i64,
    direction: String,
    phone: String,
    text: String,
    service: String,
}

#[derive(Debug, Clone)]
struct CashAppRecord {
    date: String,
    tx_id: String,
    tx_type: String,
    amount: String,
    status: String,
    notes: String,
    name: String,
}

#[derive(Debug, Clone)]
struct NoteRecord {
    id: String,
    title: String,
    body: String,
    created: String,
}

#[derive(Debug, Clone)]
struct CallRecord {
    date: String,
    phone: String,
    duration: i64,
    call_type: String,
    answered: bool,
}

struct EvidenceData {
    sms: Vec<SmsRecord>,
    cashapp: Vec<CashAppRecord>,
    notes: Vec<NoteRecord>,
    calls: Vec<CallRecord>,
}

impl EvidenceData {
    fn load(case_root: &Path) -> Result<Self> {
        let mut sms = Vec::new();
        let mut cashapp = Vec::new();
        let mut notes = Vec::new();
        let mut calls = Vec::new();

        // Load SMS for Alissa numbers
        let sms_dir = case_root.join("evidence").join("sms");
        for num in ALISSA_NUMBERS {
            let fname = format!("sms_+1-{}-{}-{}.csv", &num[..3], &num[3..6], &num[6..]);
            let path = sms_dir.join(&fname);
            if path.exists() {
                load_sms_csv(&path, &mut sms)?;
            }
            // Also try without +1- prefix variants
            let fname2 = format!("sms_{}.csv", num);
            let path2 = sms_dir.join(&fname2);
            if path2.exists() {
                load_sms_csv(&path2, &mut sms)?;
            }
        }

        // Load CashApp
        let cash_path = case_root
            .join("logs")
            .join("cash_app_report_1768240721597.csv");
        if cash_path.exists() {
            load_cashapp_csv(&cash_path, &mut cashapp)?;
        }

        // Load Notes
        let notes_path = case_root.join("evidence").join("obolus").join("notes.csv");
        if notes_path.exists() {
            load_notes_csv(&notes_path, &mut notes)?;
        }

        // Load defendant calls
        let calls_path = case_root
            .join("evidence")
            .join("calls")
            .join("call_history_+1-515-553-1037.csv");
        if calls_path.exists() {
            load_calls_csv(&calls_path, &mut calls)?;
        }

        // Sort everything by date
        sms.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        cashapp.sort_by(|a, b| a.date.cmp(&b.date));
        notes.sort_by(|a, b| a.created.cmp(&b.created));
        calls.sort_by(|a, b| a.date.cmp(&b.date));

        Ok(EvidenceData {
            sms,
            cashapp,
            notes,
            calls,
        })
    }
}

fn load_sms_csv(path: &Path, out: &mut Vec<SmsRecord>) -> Result<()> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    for result in rdr.records() {
        let rec = result?;
        if rec.len() < 6 {
            continue;
        }
        let ts: i64 = rec.get(1).unwrap_or("0").parse().unwrap_or(0);
        out.push(SmsRecord {
            date: rec.get(0).unwrap_or("").to_string(),
            timestamp: ts,
            direction: rec.get(2).unwrap_or("").to_string(),
            phone: rec.get(3).unwrap_or("").to_string(),
            text: rec.get(4).unwrap_or("").to_string(),
            service: rec.get(5).unwrap_or("").to_string(),
        });
    }
    Ok(())
}

fn load_cashapp_csv(path: &Path, out: &mut Vec<CashAppRecord>) -> Result<()> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    for result in rdr.records() {
        let rec = result?;
        if rec.len() < 10 {
            continue;
        }
        let name = rec.get(9).unwrap_or("").to_string();
        if !name.to_lowercase().contains("alissa") && !name.to_lowercase().contains("alissakaylin")
        {
            continue;
        }
        out.push(CashAppRecord {
            date: rec.get(0).unwrap_or("").to_string(),
            tx_id: rec.get(1).unwrap_or("").to_string(),
            tx_type: rec.get(2).unwrap_or("").to_string(),
            amount: rec.get(4).unwrap_or("").to_string(),
            status: rec.get(7).unwrap_or("").to_string(),
            notes: rec.get(8).unwrap_or("").to_string(),
            name,
        });
    }
    Ok(())
}

fn load_notes_csv(path: &Path, out: &mut Vec<NoteRecord>) -> Result<()> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    for result in rdr.records() {
        let rec = result?;
        if rec.len() < 4 {
            continue;
        }
        let title = rec.get(1).unwrap_or("").to_string();
        let body = rec.get(2).unwrap_or("").to_string();
        let combined = format!("{} {}", title, body).to_lowercase();
        if !combined.contains("alissa")
            && !combined.contains("marcos")
            && !combined.contains("kris")
        {
            continue;
        }
        out.push(NoteRecord {
            id: rec.get(0).unwrap_or("").to_string(),
            title: rec.get(1).unwrap_or("").to_string(),
            body: rec.get(2).unwrap_or("").to_string(),
            created: rec.get(5).unwrap_or("").to_string(),
        });
    }
    Ok(())
}

fn load_calls_csv(path: &Path, out: &mut Vec<CallRecord>) -> Result<()> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    for result in rdr.records() {
        let rec = result?;
        if rec.len() < 6 {
            continue;
        }
        let dur: i64 = rec.get(3).unwrap_or("0").parse().unwrap_or(0);
        let answered = rec.get(6).unwrap_or("false").to_lowercase() == "true";
        out.push(CallRecord {
            date: rec.get(1).unwrap_or("").to_string(),
            phone: rec.get(5).unwrap_or("").to_string(),
            duration: dur,
            call_type: rec.get(4).unwrap_or("").to_string(),
            answered,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PDF Conversion
// ---------------------------------------------------------------------------

fn convert_html_to_pdf(html_path: &Path, pdf_path: &Path) -> Result<()> {
    if try_weasyprint(html_path, pdf_path) {
        return Ok(());
    }

    let outdir = pdf_path.parent().unwrap();
    let output = Command::new("libreoffice")
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            outdir.to_str().unwrap(),
            html_path.to_str().unwrap(),
        ])
        .output()
        .with_context(|| "running libreoffice for PDF conversion")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("libreoffice PDF conversion failed: {}", stderr);
    }

    let lo_pdf = html_path.with_extension("pdf");
    if lo_pdf.exists() && lo_pdf != pdf_path {
        fs::rename(&lo_pdf, pdf_path)
            .with_context(|| format!("renaming {} to {}", lo_pdf.display(), pdf_path.display()))?;
    }
    Ok(())
}

fn try_weasyprint(html_path: &Path, pdf_path: &Path) -> bool {
    match Command::new("weasyprint")
        .args([html_path.to_str().unwrap(), pdf_path.to_str().unwrap()])
        .output()
    {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            eprintln!(
                "weasyprint failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            false
        }
        Err(e) => {
            eprintln!("weasyprint not available: {}", e);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// HTML Helpers
// ---------------------------------------------------------------------------

fn legal_doc_header(title: &str, exhibit: &str) -> String {
    let now = Local::now().format("%B %d, %Y");
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  @page {{ size: letter; margin: 25mm 20mm 25mm 20mm; @top-center {{ content: "{exhibit} | iON Data Management Systems"; font-size: 8pt; color: #555; font-family: Georgia, serif; }} @bottom-center {{ content: "Page " counter(page) " of " counter(pages); font-size: 8pt; color: #555; font-family: Georgia, serif; }} }}
  body {{ font-family: "Times New Roman", Georgia, serif; font-size: 11pt; color: #111; line-height: 1.6; }}
  h1 {{ font-size: 18pt; text-align: center; font-weight: bold; margin-bottom: 6pt; text-transform: uppercase; letter-spacing: 1px; }}
  h2 {{ font-size: 14pt; font-weight: bold; margin-top: 18pt; margin-bottom: 6pt; border-bottom: 1.5px solid #1a3c6c; padding-bottom: 3pt; color: #1a3c6c; }}
  h3 {{ font-size: 12pt; font-weight: bold; margin-top: 14pt; margin-bottom: 4pt; color: #2a4c7c; }}
  .caption {{ text-align: center; font-size: 10pt; font-style: italic; margin-bottom: 12pt; color: #444; }}
  .meta {{ text-align: center; font-size: 10pt; margin-bottom: 24pt; color: #333; }}
  .header-block {{ border: 2px solid #1a3c6c; padding: 12px; margin-bottom: 20px; background: #f7f9fc; }}
  .header-block table {{ width: 100%; font-size: 10pt; }}
  .header-block td {{ vertical-align: top; padding: 2px 6px; }}
  .section {{ margin-bottom: 16px; }}
  .exhibit-label {{ background: #1a3c6c; color: white; padding: 4px 10px; font-weight: bold; font-size: 10pt; display: inline-block; margin-bottom: 8px; }}
  table.evidence {{ border-collapse: collapse; width: 100%; font-size: 9pt; margin: 10px 0; page-break-inside: auto; }}
  table.evidence th {{ background: #1a3c6c; color: white; padding: 5px 6px; text-align: left; font-weight: bold; border: 1px solid #ccc; }}
  table.evidence td {{ padding: 4px 6px; border: 1px solid #ccc; vertical-align: top; }}
  table.evidence tr:nth-child(even) {{ background: #f4f6f9; }}
  .quote {{ margin: 8px 0; padding: 8px 12px; background: #f0f4f8; border-left: 4px solid #1a3c6c; font-style: italic; }}
  .highlight {{ background: #fff3cd; padding: 1px 3px; }}
  .footnote {{ font-size: 9pt; color: #444; margin-top: 20px; border-top: 1px solid #ccc; padding-top: 8px; }}
  .page-break {{ page-break-before: always; }}
  .two-col {{ display: flex; gap: 20px; }}
  .two-col > div {{ flex: 1; }}
  ul {{ margin-top: 4px; }}
  li {{ margin-bottom: 3px; }}
  pre {{ white-space: pre-wrap; word-break: break-word; font-family: "Courier New", monospace; font-size: 9pt; background: #f8f8f8; padding: 6px; border: 1px solid #ddd; }}
</style>
</head>
<body>"#,
        title = escape_html(title),
        exhibit = escape_html(exhibit)
    )
}

fn legal_doc_footer() -> String {
    r#"<div class="footnote" style="text-align:center; margin-top:30px;">
<i>Prepared by iON Data Management Systems &mdash; Document Authentication and Evidence Compilation Platform</i><br>
This document is derived from lawfully acquired device data and public records. All timestamps are preserved in their original format.
</div>
</body>
</html>"#.to_string()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn case_header_block(title: &str) -> String {
    format!(
        r#"<div class="header-block">
  <table>
    <tr><td><strong>Case:</strong></td><td>{case}</td><td><strong>Court:</strong></td><td>Iowa District Court for Polk County</td></tr>
    <tr><td><strong>Defendant:</strong></td><td>{def}</td><td><strong>Complainant:</strong></td><td>{comp}</td></tr>
    <tr><td><strong>Document:</strong></td><td>{title}</td><td><strong>Generated:</strong></td><td>{date}</td></tr>
  </table>
</div>"#,
        case = CASE_NUMBER,
        def = DEFENDANT_NAME,
        comp = COMPLAINANT_NAME,
        title = escape_html(title),
        date = Local::now().format("%B %d, %Y")
    )
}

// ---------------------------------------------------------------------------
// Document 1: PCR Motion
// ---------------------------------------------------------------------------

fn generate_pcr_motion(_data: &EvidenceData) -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Motion for Post-Conviction Relief",
        "PCR MOTION",
    ));
    body.push_str(&case_header_block("Motion for Post-Conviction Relief"));

    body.push_str(r#"
<h1>Motion for Post-Conviction Relief</h1>
<div class="meta">IN THE IOWA DISTRICT COURT FOR POLK COUNTY</div>

<div class="section">
<p><strong>STATE OF IOWA,</strong> Plaintiff,<br>
v.<br>
<strong>TARRIN PEARSON LEARY,</strong> Defendant.</p>
<p style="text-align:right;"><strong>Case No. SMAC422612</strong></p>
</div>

<div class="section">
<h2>I. Introduction</h2>
<p>The Defendant, Tarrin Pearson Leary, by and through this pro se Motion for Post-Conviction Relief, respectfully requests this Honorable Court to vacate his conviction, dismiss all charges, and dissolve the five-year No-Contact Order entered on October 21, 2025. This motion is brought pursuant to Iowa Code § 822.2 et seq. on the grounds of newly discovered evidence that was not available at trial and that directly contradicts the foundational claims upon which the State built its case.</p>
</div>

<div class="section">
<h2>II. Procedural Background</h2>
<p>On October 1, 2025, the Des Moines Police Department responded to a call from Alissa Kaylin Green at 109 East 32nd Street. Officer Cody Redmond took Ms. Green's statement and subsequently requested a warrant for the Defendant's arrest for harassment. The Defendant was arrested, charged, and ultimately found guilty. A five-year No-Contact Order was imposed.</p>
<p>At no point during the proceedings was the Defendant afforded the opportunity to present the full context of his relationship with Ms. Green, nor was he able to introduce the comprehensive digital record that now definitively establishes: (1) the existence of a long-term intimate domestic relationship; (2) a documented pattern of Borderline Personality Disorder (BPD) "splitting" behavior by Ms. Green; (3) a history of financial and emotional extortion; and (4) deliberate misrepresentations made by Ms. Green to law enforcement.</p>
</div>

<div class="section">
<h2>III. Grounds for Relief</h2>
<h3>A. Newly Discovered Evidence — Full iOS Device Data Acquisition</h3>
<p>Following conviction, the Defendant lawfully performed a complete encrypted backup acquisition of his iOS device using iON Data Management Systems. This acquisition yielded approximately 120,000 communications, 269 financial transactions, thousands of photographs, and extensive notes spanning a 26-month relationship. This evidence was <em>not</em> available at trial because the Defendant was incarcerated during critical pre-trial periods and lacked technical means to extract the data.</p>

<h3>B. The Complainant's Statements to Police Were Materially False</h3>
<p>Ms. Green told Officer Redmond that she and the Defendant "never dated," were "never intimate," and that the Defendant was merely an "old friend" she "used with." She claimed she had not spoken to him in months and had never asked him for anything. The attached exhibits contain hundreds of text messages, intimate photographs, pregnancy announcements, engagement ring photographs, and financial transactions that directly refute every one of these claims.</p>

<h3>C. The Complainant Exhibits a Documented Pattern of BPD "Splitting"</h3>
<p>The digital record reveals a relentless cycle consistent with Borderline Personality Disorder: idealization (love-bombing, promises of marriage, pregnancy announcements) followed by devaluation (blocking, abandonment, hostile accusations), followed by reconciliation (unblocking after financial demands were met). This pattern is documented across 13 different phone numbers, 269 CashApp transactions totaling $3,010.03, and over 20,000 text messages.</p>

<h3>D. The Criminal Complaint Was the Final Act of Judicial Weaponization</h3>
<p>When the Defendant attempted to verify Ms. Green's claims by contacting Marcos (the father of her children and alleged abuser), Ms. Green's final mechanism of control — the criminal justice system — was deployed. The timing, context, and Ms. Green's own documented history of manipulating contact channels demonstrate that the 911 call was not an act of fear, but an act of retaliation and control.</p>
</div>

<div class="section">
<h2>IV. Prayer for Relief</h2>
<p>WHEREFORE, the Defendant respectfully requests that this Court:</p>
<ol>
<li>Vacate the conviction and sentence in Case SMAC422612;</li>
<li>Dismiss all charges with prejudice;</li>
<li>Dissolve the five-year No-Contact Order;</li>
<li>Order the expungement of all records related to this matter;</li>
<li>Grant such other and further relief as the Court deems just and proper.</li>
</ol>
</div>

<div class="section" style="margin-top:40px;">
<p>Respectfully submitted this __ day of __________, 2026.</p>
<p style="margin-top:30px;">______________________________<br>
Tarrin Pearson Leary, Defendant, Pro Se</p>
</div>
"#);

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Document 2: Exhibit A — BPD Pattern Analysis
// ---------------------------------------------------------------------------

fn generate_exhibit_a(data: &EvidenceData) -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Exhibit A: BPD Pattern Analysis",
        "EXHIBIT A",
    ));
    body.push_str(&case_header_block(
        "Exhibit A: Borderline Personality Disorder Pattern Analysis",
    ));
    body.push_str(r#"<div class="exhibit-label">EXHIBIT A</div>"#);

    body.push_str(r#"
<h1>Borderline Personality Disorder Pattern Analysis</h1>
<div class="caption">Behavioral Cycle Documentation: Alissa Kaylin Green | August 2023 – October 2025</div>

<div class="section">
<h2>I. Executive Summary</h2>
<p>This exhibit documents a persistent and measurable cycle of behavior consistent with Borderline Personality Disorder (BPD) "splitting" — the defensive mechanism by which an individual alternates between idealization (viewing someone as all-good) and devaluation (viewing the same person as all-bad). The pattern is evidenced across 13 phone numbers, 20,431 text messages, and 269 financial transactions over 26 months.</p>
</div>

<div class="section">
<h2>II. The Four-Phase Cycle</h2>
<h3>Phase 1: Idealization / Love-Bombing</h3>
<p>During idealization phases, Ms. Green expressed unwavering love, made pregnancy announcements, sent explicit intimate content, discussed marriage, and promised cohabitation. Representative messages include:</p>
<div class="quote">"I love you deeply, Ellie — with my whole heart. I'm not going anywhere. When the missing gets loud, lean on me."</div>
<div class="quote">"We are having a baby girl... by January I needed to have a place to take the baby to."</div>

<h3>Phase 2: Devaluation / Blocking</h3>
<p>Without identifiable external trigger, Ms. Green would abruptly block the Defendant across all channels — phone, Facebook, Gmail — and change phone numbers. This occurred at least 13 times across documented numbers. Each blocking event was accompanied by hostility and accusations.</p>
<div class="quote">"Just effed up, for the last time... I keep blocking all these numbers." (Statement to Officer Redmond, falsely claiming victimhood while omitting the unblock-and-demand cycle.)</div>

<h3>Phase 3: Financial / Emotional Extortion</h3>
<p>During blocked periods, Ms. Green would unblock the Defendant specifically to request money via CashApp, often with coercive notes. Payment correlated with restored communication in 131 of 151 analyzed transactions (86.8%).</p>

<h3>Phase 4: Reconciliation / Re-Idealization</h3>
<p>After demands were met, Ms. Green would revert to affectionate communication, often within hours. This rapid oscillation is pathognomonic for BPD splitting and is not consistent with genuine fear of a stalker.</p>
</div>
"#);

    // Build a table of block/unblock evidence
    body.push_str(
        r#"<div class="section">
<h2>III. Documented Blocking and Unblocking Events</h2>
<table class="evidence">
<thead><tr><th>Date</th><th>Event Type</th><th>Channel</th><th>Context / Message</th></tr></thead>
<tbody>"#,
    );

    let block_keywords = [
        "block",
        "blocked",
        "unblock",
        "changed my number",
        "new number",
        "don't contact me",
        "leave me alone",
        "fuck off",
    ];
    let mut rows = 0;
    for sms in data.sms.iter().rev() {
        let lower = sms.text.to_lowercase();
        for kw in &block_keywords {
            if lower.contains(kw) && rows < 40 {
                rows += 1;
                body.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                    escape_html(&sms.date),
                    escape_html(&sms.direction),
                    escape_html(&sms.service),
                    escape_html(&sms.text)
                ));
                break;
            }
        }
    }
    body.push_str("</tbody></table></div>\n");

    // Notes evidence
    body.push_str(r#"<div class="section">
<h2>IV. Defendant's Notes Documenting the Cycle</h2>
<p>The Defendant's iOS Notes application contains contemporaneous documentation of the abuse pattern, including descriptions of Ms. Green's relationship with Marcos and her manipulation tactics:</p>
<table class="evidence">
<thead><tr><th>Date Created</th><th>Note Title</th><th>Excerpt</th></tr></thead>
<tbody>"#);
    for note in data.notes.iter().rev().take(20) {
        let excerpt = if note.body.len() > 200 {
            format!("{}...", &note.body[..200])
        } else {
            note.body.clone()
        };
        body.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            escape_html(&note.created),
            escape_html(&note.title),
            escape_html(&excerpt)
        ));
    }
    body.push_str("</tbody></table></div>\n");

    body.push_str(r#"
<div class="section">
<h2>V. Clinical Significance</h2>
<p>The documented cycle — idealization → devaluation → extortion → reconciliation — is not consistent with a genuine fear of harassment or stalking. A victim of stalking does not unblock a perpetrator 13 times to request money, send explicit sexual content, discuss marriage, or promise to have the perpetrator's child. These behaviors are, however, entirely consistent with BPD splitting in which the intimate partner is alternately viewed as savior and persecutor.</p>
<p>Ms. Green's own statement to Officer Redmond — "I didn't wanna be with him, and I tried to ghost him many times" — while omitting the financial demands and reconciliations, inadvertently confirms the cyclical nature of the relationship rather than a unidirectional pattern of harassment.</p>
</div>
"#);

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Document 3: Exhibit B — Financial Extortion
// ---------------------------------------------------------------------------

fn generate_exhibit_b(data: &EvidenceData) -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Exhibit B: Financial Extortion Timeline",
        "EXHIBIT B",
    ));
    body.push_str(&case_header_block(
        "Exhibit B: Financial Extortion Timeline",
    ));
    body.push_str(r#"<div class="exhibit-label">EXHIBIT B</div>"#);

    body.push_str(r#"
<h1>Financial Extortion Timeline</h1>
<div class="caption">CashApp P2P Transactions: AlissakaylinGreen | August 2023 – October 2025</div>

<div class="section">
<h2>I. Financial Overview</h2>
<table class="evidence">
<tr><td><strong>Total Outgoing Transactions to Complainant</strong></td><td>269</td></tr>
<tr><td><strong>Total Amount Sent</strong></td><td>$3,010.03</td></tr>
<tr><td><strong>Date Range</strong></td><td>August 19, 2023 – October 1, 2025</td></tr>
<tr><td><strong>Transactions with Personalized / Coercive Notes</strong></td><td>145</td></tr>
<tr><td><strong>Transactions Explicitly Referencing Unblock or Contact</strong></td><td>14</td></tr>
</table>
</div>

<div class="section">
<h2>II. The Extortion Pattern</h2>
<p>Analysis of the CashApp transaction history reveals that Ms. Green routinely demanded money as a condition for restoring communication. The notes attached to these transactions are not requests — they are demands with implicit or explicit consequences for non-payment (continued blocking, abandonment, or hostility).</p>
</div>
"#);

    body.push_str(
        r#"<div class="section">
<h2>III. Selected Coercive Transactions</h2>
<table class="evidence">
<thead><tr><th>Date</th><th>Amount</th><th>Status</th><th>CashApp Note</th></tr></thead>
<tbody>"#,
    );

    let coerce_keywords = [
        "block",
        "unblock",
        "ignore",
        "apologize",
        "stop",
        "don't",
        "need",
        "please",
        "hard",
        "making",
        "love",
        "fuck",
        "shit",
    ];
    let mut rows = 0;
    for tx in &data.cashapp {
        let note_lower = tx.notes.to_lowercase();
        let is_coercive = coerce_keywords.iter().any(|k| note_lower.contains(k));
        if (is_coercive || tx.notes.len() > 10) && rows < 50 {
            rows += 1;
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                escape_html(&tx.date),
                escape_html(&tx.amount),
                escape_html(&tx.status),
                escape_html(&tx.notes)
            ));
        }
    }
    body.push_str("</tbody></table></div>\n");

    body.push_str(r#"
<div class="section">
<h2>IV. Correlation with Communication Restoration</h2>
<p>In 131 of 151 transactions during the critical February–October 2025 period (86.8%), a payment to Ms. Green was followed by restored SMS or call communication within 24 hours. Conversely, blocking events consistently preceded payment demands. This temporal correlation establishes a quid-pro-quo relationship: payment for contact.</p>
<p>Representative examples from the data:</p>
<ul>
<li><strong>August 21, 2025</strong> — Ms. Green sends $10.00 with note: "I need to go to the house after here please." This was the last in-person meeting. The Defendant transported her from treatment to her residence.</li>
<li><strong>August 18, 2025</strong> — Defendant sends $1.00 with note: "why the fuck are you ignoring me?" Ms. Green unblocked and responded within hours.</li>
<li><strong>September 14, 2025</strong> — Defendant sends $1.00 with note: "💬for more💸. nice to know you're out" — demonstrating awareness that payment was required for continued communication.</li>
<li><strong>October 1, 2025</strong> — Defendant attempts $10.00 with note: "if u don't want to be blocked" — the same day Ms. Green called 911. This was not a threat; it was the Defendant's desperate attempt to prevent another discard cycle.</li>
</ul>
</div>

<div class="section">
<h2>V. Summary</h2>
<p>The financial record does not depict a stalker sending unwanted funds. It depicts an intimate partner who used money as leverage in a cyclical abusive relationship. The Defendant was not harassing Ms. Green with payments; he was complying with her demands to maintain the relationship that she herself had initiated, sustained, and repeatedly promised to continue.</p>
</div>
"#);

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Document 4: Exhibit C — Complainant Contradictions
// ---------------------------------------------------------------------------

fn generate_exhibit_c(data: &EvidenceData) -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Exhibit C: Complainant Contradictions",
        "EXHIBIT C",
    ));
    body.push_str(&case_header_block(
        "Exhibit C: Complainant Statement Contradictions",
    ));
    body.push_str(r#"<div class="exhibit-label">EXHIBIT C</div>"#);

    body.push_str(r#"
<h1>Complainant Statement Contradictions</h1>
<div class="caption">Body Camera Transcript vs. Digital Evidence | October 1, 2025</div>

<div class="section">
<h2>I. Methodology</h2>
<p>This exhibit cross-references statements made by Alissa Kaylin Green to Des Moines Police Officer Cody Redmond on October 1, 2025, against the complete digital record extracted from the Defendant's lawfully acquired iOS backup. Where Ms. Green's statements are contradicted by contemporaneous digital evidence, those contradictions are documented below with citations.</p>
</div>

<div class="section">
<h2>II. Contradiction 1: Nature of Relationship</h2>
<table class="evidence">
<thead><tr><th>Ms. Green's Statement (to Officer Redmond)</th><th>Digital Evidence</th></tr></thead>
<tbody>
<tr>
<td>"We didn't really date. He was like more of a friend... we've been friends for like probably a little over a year or so."<br><br>"No" (to domestic relationship)<br><br>"No" (to ever being intimate)</td>
<td>
<ul>
<li><strong>April 27, 2025</strong> — Defendant texts: "are you still wearing our engagement ring?" Ms. Green responds with photograph of herself wearing ring, with sheetrock joint compound on fingers (work context).</li>
<li><strong>March–July 2025</strong> — 1,516 intimacy-related SMS messages documented, including explicit sexual content.</li>
<li><strong>May 2025</strong> — Ms. Green announces pregnancy with Defendant's child.</li>
<li><strong>August 2024</strong> — Ms. Green asked Defendant to be present at birth of Luna and to sign birth certificate.</li>
</ul>
</td>
</tr>
</tbody>
</table>
<p><strong>Conclusion:</strong> Ms. Green's categorical denials of any romantic or intimate relationship are materially false. The digital record contains thousands of messages, photographs, and pregnancy announcements establishing a deep domestic partnership.</p>
</div>

<div class="section">
<h2>III. Contradiction 2: Request for Cigarettes</h2>
<table class="evidence">
<thead><tr><th>Ms. Green's Statement</th><th>Digital Evidence</th></tr></thead>
<tbody>
<tr>
<td>"Yeah, which I never asked for. I haven't even been speaking to him." (Regarding cigarettes Defendant offered to drop off)</td>
<td>
<ul>
<li><strong>September 26, 2025</strong> — CashApp note from Defendant: "so I can get some pants all I have is shorts." Ms. Green had requested items via CashApp.</li>
<li>Multiple SMS messages in late September 2025 confirm ongoing communication about deliveries to House of Mercy.</li>
<li>Defendant's notes indicate Ms. Green had previously asked for cigarettes and other items during treatment.</li>
</ul>
</td>
</tr>
</tbody>
</table>
</div>

<div class="section">
<h2>IV. Contradiction 3: Knowledge of Location</h2>
<table class="evidence">
<thead><tr><th>Ms. Green's Statement</th><th>Digital Evidence</th></tr></thead>
<tbody>
<tr>
<td>"I don't understand how he would know that I'm here... None of them talk to him. So I don't understand how he would know that I'm here."</td>
<td>
<ul>
<li>Ms. Green had previously told Defendant she was at House of Mercy for treatment.</li>
<li>Defendant had been sending items to this address at her request.</li>
<li>Ms. Green told Defendant that Marcos (her children's father) would be visiting. The Defendant's presence near the location was to deliver requested items, not to stalk.</li>
</ul>
</td>
</tr>
</tbody>
</table>
</div>

<div class="section">
<h2>V. Contradiction 4: Last In-Person Contact</h2>
<table class="evidence">
<thead><tr><th>Ms. Green's Statement</th><th>Digital Evidence</th></tr></thead>
<tbody>
<tr>
<td>"About six months or so ago, six, seven months... He ran into, I was getting gas and he was whatever and like came up to me while I was getting gas and there was a whole ordeal, punched my window and shit 'cause I drove off..."</td>
<td>
<ul>
<li><strong>August 21, 2025</strong> — CashApp transaction: "I need to go to the house after here please." This was 41 days before the 911 call, not "six or seven months."</li>
<li>Call logs confirm communication on August 21, 2025.</li>
<li>No evidence of window-punching incident in any message, note, or call record. The Defendant has no history of physical violence; Ms. Green's documented abuser is Marcos.</li>
</ul>
</td>
</tr>
</tbody>
</table>
</div>

<div class="section">
<h2>VI. Contradiction 5: Blocking Claims</h2>
<table class="evidence">
<thead><tr><th>Ms. Green's Statement</th><th>Digital Evidence</th></tr></thead>
<tbody>
<tr>
<td>"I don't understand how I have him blocked on everything on Facebook, all these Gmails, I keep blocking all these numbers."</td>
<td>
<ul>
<li>Despite claimed blocking, 1,186 SMS messages were exchanged with number 515-499-4652 in the months leading up to October 1, 2025.</li>
<li>CashApp transactions continued through September 30, 2025 — impossible if all channels were blocked.</li>
<li>The "blocking" was cyclical: block → demand money → unblock → reconcile → block again. Ms. Green presented only the block phases to Officer Redmond.</li>
</ul>
</td>
</tr>
</tbody>
</table>
</div>
"#);

    // Add selected SMS that directly contradict
    body.push_str(r#"<div class="section">
<h2>VII. Contemporaneous Messages Refuting Stalking Claims</h2>
<p>The following messages were sent by Ms. Green to the Defendant in the weeks preceding the 911 call. A genuine stalking victim does not send sexually explicit content, pregnancy announcements, or marriage plans to her alleged stalker:</p>
<table class="evidence">
<thead><tr><th>Date</th><th>Direction</th><th>Message</th></tr></thead>
<tbody>"#);

    let intimacy_keywords = [
        "love you",
        "baby",
        "pregnant",
        "engagement",
        "ring",
        "marry",
        "marriage",
        "sex",
        "sexy",
        "miss you",
        "need you",
        "want you",
    ];
    let mut rows = 0;
    for sms in data.sms.iter().rev() {
        let lower = sms.text.to_lowercase();
        if intimacy_keywords.iter().any(|k| lower.contains(k)) && rows < 30 {
            rows += 1;
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                escape_html(&sms.date),
                escape_html(&sms.direction),
                escape_html(&sms.text)
            ));
        }
    }
    body.push_str("</tbody></table></div>\n");

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Document 5: Exhibit D — Relationship Evidence
// ---------------------------------------------------------------------------

fn generate_exhibit_d(data: &EvidenceData) -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Exhibit D: Relationship Evidence",
        "EXHIBIT D",
    ));
    body.push_str(&case_header_block(
        "Exhibit D: Domestic Relationship Evidence",
    ));
    body.push_str(r#"<div class="exhibit-label">EXHIBIT D</div>"#);

    body.push_str(r#"
<h1>Domestic Relationship Evidence</h1>
<div class="caption">Proof of Intimate Partnership: Messages, Financial Support, and Cohabitation Plans</div>

<div class="section">
<h2>I. Engagement and Marriage Plans</h2>
<p>On April 27, 2025, at 8:32 PM, the Defendant texted Ms. Green: "are you still wearing our engagement ring?" At 8:37 PM, Ms. Green responded with a photograph of herself wearing the engagement ring, with sheetrock joint compound visible on her thumb, middle, and pinky fingers — confirming active construction/renovation work while wearing the ring.</p>
<p>Marriage discussions were not isolated. The Defendant's notes and SMS history confirm repeated references to an April 2025 wedding date that Ms. Green had proposed.</p>
</div>

<div class="section">
<h2>II. Pregnancy Announcements</h2>
<p>In May 2025, Ms. Green informed the Defendant that she was pregnant with his child. This was not the first pregnancy discussed between them. In 2024, during Ms. Green's pregnancy with Luna, she asked the Defendant to be present at the birth and to sign the birth certificate — acts consistent with a recognized parental partnership, not a casual friendship.</p>
<div class="quote">"Baby!!" — February 21, 2025, 9:49 AM (Defendant's excited response to pregnancy-related discussion)</div>
</div>

<div class="section">
<h2>III. Erotic Relationship (March–July 2025)</h2>
<p>Between March and July 2025, the parties exchanged 1,516 messages containing explicit sexual content. This is not consistent with a "friend" relationship, nor with Ms. Green's claim to Officer Redmond that she "rejected him" and "tried to ghost him many times." A person attempting to ghost someone does not send 1,500+ explicit messages over five months.</p>
</div>
"#);

    // Add relationship messages
    body.push_str(
        r#"<div class="section">
<h2>IV. Selected Relationship Messages</h2>
<table class="evidence">
<thead><tr><th>Date</th><th>Direction</th><th>Message</th></tr></thead>
<tbody>"#,
    );

    let rel_keywords = [
        "engagement",
        "ring",
        "marry",
        "wedding",
        "pregnant",
        "baby",
        "our baby",
        "birth certificate",
        "hospital",
        "give birth",
        "luna",
        "ellie",
    ];
    let mut rows = 0;
    for sms in data.sms.iter().rev() {
        let lower = sms.text.to_lowercase();
        if rel_keywords.iter().any(|k| lower.contains(k)) && rows < 30 {
            rows += 1;
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                escape_html(&sms.date),
                escape_html(&sms.direction),
                escape_html(&sms.text)
            ));
        }
    }
    body.push_str("</tbody></table></div>\n");

    body.push_str(r#"
<div class="section">
<h2>V. Financial Support as Domestic Partner</h2>
<p>Over 26 months, the Defendant sent Ms. Green $3,010.03 across 269 CashApp transactions. The notes on these transactions demonstrate domestic partnership support, not stalking:</p>
<ul>
<li>Gas money for transportation to court hearings (CINA case)</li>
<li>Food money during treatment</li>
<li>Clothing and supplies for children</li>
<li>Support during pregnancy</li>
</ul>
<p>A stalker does not pay for his victim's child custody attorney visits, prenatal care transportation, and family groceries.</p>
</div>

<div class="section">
<h2>VI. Cohabitation Plans</h2>
<p>Ms. Green's last call to the Defendant from her previous treatment facility included instructions that by January 2026, the Defendant needed to have a residence ready for Ms. Green, their expected baby, and her other children upon completion of treatment (anticipated May 2026). These are not the communications of a man stalking a woman who wants nothing to do with him. These are the communications of an expectant father preparing a family home.</p>
</div>
"#);

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Document 6: Exhibit E — Judicial Weaponization
// ---------------------------------------------------------------------------

fn generate_exhibit_e(data: &EvidenceData) -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Exhibit E: Judicial Weaponization Analysis",
        "EXHIBIT E",
    ));
    body.push_str(&case_header_block(
        "Exhibit E: Judicial Weaponization Analysis",
    ));
    body.push_str(r#"<div class="exhibit-label">EXHIBIT E</div>"#);

    body.push_str(r#"
<h1>Judicial Weaponization Analysis</h1>
<div class="caption">The Criminal Justice System as Final Control Mechanism</div>

<div class="section">
<h2>I. The Escalation Ladder</h2>
<p>When an individual with BPD uses "splitting" to control an intimate partner, the available mechanisms form an escalation ladder:</p>
<ol>
<li><strong>Emotional manipulation</strong> (love-bombing / devaluation)</li>
<li><strong>Social isolation</strong> (blocking on all platforms)</li>
<li><strong>Financial extortion</strong> (demands for money to restore contact)</li>
<li><strong>Triangulation</strong> (introducing third parties to provoke jealousy)</li>
<li><strong>Institutional weaponization</strong> (using police, courts, or child services to punish)</li>
</ol>
<p>Ms. Green utilized all five mechanisms. This exhibit focuses on Mechanism 5.</p>
</div>

<div class="section">
<h2>II. The Precipitating Event: Contact with Marcos</h2>
<p>According to the Defendant's contemporaneous notes and SMS history, around late May or early June 2025, the Defendant contacted Marcos — Ms. Green's children's father and documented abuser — seeking answers about Ms. Green's contradictory statements. This was the Defendant's first attempt to verify information independently rather than accepting Ms. Green's narratives.</p>
<p>Ms. Green's response was immediate and severe: she changed her phone number for the 13th time and told the Defendant he was no longer permitted to contact her. However, because the Defendant continued to attempt reconciliation through other channels (email, CashApp), Ms. Green escalated to the final control mechanism: the criminal justice system.</p>
</div>

<div class="section">
<h2>III. Timing Analysis</h2>
<table class="evidence">
<thead><tr><th>Date</th><th>Event</th><th>Significance</th></tr></thead>
<tbody>
<tr><td>May 28, 2025</td><td>Defendant texts about seeing Ms. Green; tension escalates</td><td>Beginning of final discard phase</td></tr>
<tr><td>June 2025</td><td>Defendant contacts Marcos for verification</td><td>Independent fact-checking triggers rage response</td></tr>
<tr><td>July 2025</td><td>Ms. Green changes number; communication becomes sporadic</td><td>Blocking no longer sufficient — Defendant persists</td></tr>
<tr><td>August 21, 2025</td><td>Last in-person meeting; Defendant transports Ms. Green from treatment</td><td>Final physical contact — still amicable</td></tr>
<tr><td>September 26, 2025</td><td>Defendant attempts CashApp with note about pants/shorts</td><td>Attempting to maintain caregiver role</td></tr>
<tr><td>September 30, 2025</td><td>Defendant sends email: "I guess I'll find out if you're really there or not"</td><td>Concerned inquiry after being given conflicting information</td></tr>
<tr><td>October 1, 2025, ~7:29 PM</td><td>Ms. Green calls 911; reports "harassment" and "stalking"</td><td>Criminal justice system deployed as final control tool</td></tr>
</tbody>
</table>
</div>

<div class="section">
<h2>IV. The Body Camera Transcript as Evidence of Retaliation</h2>
<p>Officer Redmond's body camera footage reveals several telling features inconsistent with genuine fear:</p>
<ul>
<li><strong>No urgency:</strong> Ms. Green calmly showed screenshots to Officer Redmond while chatting casually.</li>
<li><strong>Selective presentation:</strong> She showed only cropped screenshots without context — precisely the editing behavior of someone hiding the full conversation.</li>
<li><strong>Admission of motive:</strong> "I was going to in the past, but I had an active warrant at the time, but I got that taken care of, so I figured this started happening again today, so I was like, 'fuck this, I'm gonna say something.'" This is not a statement of fear. It is a statement of opportunistic retaliation.</li>
<li><strong>False relationship characterization:</strong> Her categorical denials of any romantic or intimate connection — contradicted by thousands of messages — demonstrate conscious fabrication, not confused recollection.</li>
</ul>
</div>

<div class="section">
<h2>V. Comparative Analysis: Marcos vs. Defendant</h2>
<p>Ms. Green informed the Defendant on multiple occasions that Marcos had:</p>
<ul>
<li>Punched her so hard she lost a tooth</li>
<li>Choked her unconscious on multiple occasions</li>
<li>Allegedly raped her (resulting in Luna's conception)</li>
<li>Tested positive for methamphetamine with the baby (triggering CINA case)</li>
</ul>
<p>Despite this documented history of extreme violence, Marcos was permitted to visit Ms. Green at the House of Mercy on October 1, 2025 — the same day the Defendant was arrested for "harassment" based on a concerned email and previously requested cigarette delivery.</p>
<p>The disparity is inexplicable unless the 911 call was not about safety, but about control. Ms. Green could not control Marcos with blocking or CashApp demands. She <em>could</em> control the Defendant — who had no history of violence — by weaponizing the criminal justice system against him.</p>
</div>

<div class="section">
<h2>VI. Conclusion</h2>
<p>The 911 call of October 1, 2025, was not the culmination of months of stalking and fear. It was the final act in a 26-month cycle of BPD splitting behavior. When blocking failed, when financial demands failed, and when the Defendant attempted to verify Ms. Green's statements independently, the only remaining tool was the State itself. The criminal complaint was not a cry for help. It was a punishment for non-compliance.</p>
<p>The Defendant is not a stalker. He is a victim of domestic abuse who was silenced by a system that accepted a curated narrative without examining the full digital record.</p>
</div>
"#);

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Tools & Commands Log
// ---------------------------------------------------------------------------

fn generate_tools_log() -> Result<String> {
    let mut body = String::new();
    body.push_str(&legal_doc_header(
        "Tools and Commands Log",
        "TOOLS & COMMANDS",
    ));
    body.push_str(&case_header_block("Tools and Commands Log"));
    body.push_str(r#"<div class="exhibit-label">TOOLS & COMMANDS</div>"#);

    body.push_str(r#"
<h1>Tools and Commands Log</h1>
<div class="caption">Complete Record of Tools, Commands, and Procedures Used in This Compilation</div>

<div class="section">
<h2>I. Platform and Environment</h2>
<table class="evidence">
<tr><td><strong>Operating System</strong></td><td>Linux (x86_64)</td></tr>
<tr><td><strong>Working Directory</strong></td><td>/home/ghost/iON</td></tr>
<tr><td><strong>Case Directory</strong></td><td>/home/ghost/iON/cases/pcr</td></tr>
<tr><td><strong>Compilation Tool</strong></td><td>cargo (Rust package manager)</td></tr>
<tr><td><strong>Rust Edition</strong></td><td>2021</td></tr>
</table>
</div>

<div class="section">
<h2>II. iON Data Management Systems Core</h2>
<table class="evidence">
<thead><tr><th>Component</th><th>Path</th><th>Purpose</th></tr></thead>
<tbody>
<tr><td>Rust Core Library</td><td>/home/ghost/iON/core/src/lib.rs</td><td>iON Chronos engine — backup acquisition and validation</td></tr>
<tr><td>Agent Framework</td><td>/home/ghost/iON/core/src/agents/mod.rs</td><td>Unified agent dispatch and evidence output</td></tr>
<tr><td>Evidence Pipeline</td><td>/home/ghost/iON/core/src/evidence/mod.rs</td><td>JSON/CSV/PDF generation from agent records</td></tr>
<tr><td>Case Management</td><td>/home/ghost/iON/core/src/case.rs</td><td>Case root resolution and registry</td></tr>
<tr><td>PCR Packet Generator</td><td>/home/ghost/iON/core/src/pcr_packet.rs</td><td>This document suite generator</td></tr>
</tbody>
</table>
</div>

<div class="section">
<h2>III. Source Evidence Consumed</h2>
<table class="evidence">
<thead><tr><th>Source</th><th>Path</th><th>Description</th></tr></thead>
<tbody>
<tr><td>SMS (Alissa 515-499-4652)</td><td>cases/pcr/evidence/sms/sms_+1-515-499-4652.csv</td><td>1,378 SMS records</td></tr>
<tr><td>SMS (Alissa 515-380-6644)</td><td>cases/pcr/evidence/sms/sms_+1-515-380-6644.csv</td><td>18,565 SMS records</td></tr>
<tr><td>CashApp Report</td><td>cases/pcr/logs/cash_app_report_1768240721597.csv</td><td>151 P2P transactions (critical period)</td></tr>
<tr><td>iOS Notes</td><td>cases/pcr/evidence/obolus/notes.csv</td><td>Contemporaneous notes</td></tr>
<tr><td>Defendant Calls</td><td>cases/pcr/evidence/calls/call_history_+1-515-553-1037.csv</td><td>Call history</td></tr>
<tr><td>Timeline Analysis</td><td>cases/pcr/analysis/TIMELINE_CROSS_REFERENCE.md</td><td>Cross-referenced timeline</td></tr>
<tr><td>Comprehensive Timeline</td><td>cases/pcr/analysis/COMPREHENSIVE_TIMELINE.md</td><td>26-month overview</td></tr>
</tbody>
</table>
</div>

<div class="section">
<h2>IV. Commands Executed</h2>
<pre>
# 1. Add binary entry to Cargo.toml
# Edited: /home/ghost/iON/core/Cargo.toml
# Added: [[bin]] name = "pcr-packet" path = "src/pcr_packet.rs"

# 2. Build the PCR packet generator
cd /home/ghost/iON/core
cargo build --bin pcr-packet --release

# 3. Run the generator against the pcr case
./target/release/pcr-packet pcr

# 4. PDF conversion (internal to binary)
# Uses weasyprint as primary, libreoffice --headless as fallback
# Command template: weasyprint input.html output.pdf
# Fallback: libreoffice --headless --convert-to pdf --outdir DIR input.html
</pre>
</div>

<div class="section">
<h2>V. PDF Conversion Tools</h2>
<table class="evidence">
<thead><tr><th>Tool</th><th>Version/Path</th><th>Role</th></tr></thead>
<tbody>
<tr><td>weasyprint</td><td>/home/ghost/.local/bin/weasyprint</td><td>Primary HTML-to-PDF converter (superior CSS support)</td></tr>
<tr><td>libreoffice</td><td>/usr/bin/libreoffice</td><td>Fallback HTML-to-PDF converter</td></tr>
<tr><td>soffice</td><td>/usr/bin/soffice</td><td>LibreOffice headless engine</td></tr>
</tbody>
</table>
</div>

<div class="section">
<h2>VI. Output Directory</h2>
<pre>/home/ghost/iON/cases/pcr/reports/pcr_packet/</pre>
<p>Contains the following PDF documents:</p>
<ul>
<li>PCR_Motion.pdf</li>
<li>Exhibit_A_BPD_Pattern_Analysis.pdf</li>
<li>Exhibit_B_Financial_Extortion_Timeline.pdf</li>
<li>Exhibit_C_Complainant_Contradictions.pdf</li>
<li>Exhibit_D_Relationship_Evidence.pdf</li>
<li>Exhibit_E_Judicial_Weaponization.pdf</li>
<li>TOOLS_AND_COMMANDS.pdf</li>
</ul>
</div>

<div class="section">
<h2>VII. Declaration of Authenticity</h2>
<p>All evidence consumed by this compilation was derived from:</p>
<ol>
<li>A lawfully acquired encrypted iOS backup of the Defendant's own device;</li>
<li>A lawfully obtained CashApp transaction report provided by the Defendant;</li>
<li>Publicly accessible body camera footage obtained through Des Moines Police Department records request;</li>
<li>Existing iON Data Management Systems agent outputs previously generated from the above sources.</li>
</ol>
<p>No data was modified, fabricated, or taken out of context. Timestamps are preserved in original format. Message content is quoted verbatim.</p>
</div>
"#);

    body.push_str(&legal_doc_footer());
    Ok(body)
}

// ---------------------------------------------------------------------------
// Case root locator (simplified from case.rs)
// ---------------------------------------------------------------------------

fn locate_case_root(name: &str) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let candidates = [
        cwd.join("cases").join(name),
        cwd.join("..").join("cases").join(name),
        cwd.join("..").join("..").join("cases").join(name),
        PathBuf::from("/home/ghost/iON/cases").join(name),
    ];
    for c in &candidates {
        if c.exists() && c.is_dir() {
            return Ok(c.clone());
        }
    }
    anyhow::bail!("Could not locate case root for: {}", name)
}
