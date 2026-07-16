//! HELiOS: CLI wrapper for decrypting/reconstructing encrypted iOS backups.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use STYGiON2::case::Case;
use STYGiON2::chronos::ios_backup_decrypt as ios_backup;

const DEFAULT_OUTPUT_DIR_ENV: &str = "ION_HELIOS_OUTPUT_DIR";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliOptions {
    case_name: String,
    backup_dir: PathBuf,
    output_dir: PathBuf,
    password: String,
    incremental: bool,
    profile: ExtractProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ExtractProfile {
    Full,
    Messages,
    Contacts,
    Calls,
    Voicemail,
    Mail,
    Location,
    Photos,
    AppData,
    Finance,
}

impl ExtractProfile {
    fn parse(raw: &str) -> Result<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "messages" | "sms" | "imessage" => Ok(Self::Messages),
            "contacts" | "addressbook" => Ok(Self::Contacts),
            "calls" | "callhistory" => Ok(Self::Calls),
            "voicemail" | "voicemails" => Ok(Self::Voicemail),
            "mail" | "email" | "emails" => Ok(Self::Mail),
            "location" | "locations" => Ok(Self::Location),
            "photos" | "media" => Ok(Self::Photos),
            "appdata" | "apps" => Ok(Self::AppData),
            "finance" | "money" | "banking" => Ok(Self::Finance),
            other => Err(anyhow!("unknown HELiOS extraction profile: {}", other)),
        }
    }

    fn specs(self, incremental: bool) -> Vec<ios_backup::ExtractSpec> {
        match self {
            Self::Full => vec![],

            Self::Messages => vec![spec("HomeDomain%", "Library/SMS/%", incremental)],

            Self::Contacts => vec![spec("HomeDomain%", "Library/AddressBook/%", incremental)],

            Self::Calls => vec![
                spec("HomeDomain%", "Library/CallHistoryDB/%", incremental),
                spec("WirelessDomain%", "Library/CallHistoryDB/%", incremental),
            ],

            Self::Voicemail => vec![spec("HomeDomain%", "Library/Voicemail/%", incremental)],

            Self::Mail => vec![spec("HomeDomain%", "Library/Mail/%", incremental)],

            Self::Location => vec![
                spec("RootDomain%", "Library/Caches/locationd/%", incremental),
                spec(
                    "RootDomain%",
                    "private/var/root/Library/Caches/locationd/%",
                    incremental,
                ),
                spec(
                    "HomeDomain%",
                    "Library/Caches/com.apple.routined/%",
                    incremental,
                ),
                spec("HomeDomain%", "Library/Caches/locationd/%", incremental),
            ],

            Self::Photos => vec![
                spec("MediaDomain%", "DCIM/%", incremental),
                spec("MediaDomain%", "PhotoData/%", incremental),
                spec("MediaDomain%", "Photos/%", incremental),
            ],

            Self::AppData => vec![
                spec("AppDomain-%", "%", incremental),
                spec("AppDomainGroup-%", "%", incremental),
                spec("SysContainerDomain-%", "%", incremental),
            ],

            Self::Finance => vec![
                spec("AppDomain-%", "%", incremental),
                spec("AppDomainGroup-%", "%", incremental),
            ],
        }
    }
}

fn spec(
    domain_like: &str,
    relative_paths_like: &str,
    incremental: bool,
) -> ios_backup::ExtractSpec {
    ios_backup::ExtractSpec {
        domain_like: domain_like.to_string(),
        relative_paths_like: relative_paths_like.to_string(),
        preserve_folders: true,
        domain_subfolders: true,
        incremental,
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("HELiOS ERROR: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let opts = parse_args()?;

    let case = Case::new(opts.case_name.clone())?;
    case.open("helios")?;

    if opts.password.trim().is_empty() {
        return Err(anyhow!("Decrypt password is empty"));
    }

    fs::create_dir_all(&opts.output_dir)
        .with_context(|| format!("Failed to create output dir {}", opts.output_dir.display()))?;

    println!("HELiOS decrypt starting...");
    println!("Case:   {}", opts.case_name);
    println!("Backup: {}", opts.backup_dir.display());
    println!("Output: {}", opts.output_dir.display());
    println!("Mode:   {:?}", opts.profile);

    let result = if opts.profile == ExtractProfile::Full {
        ios_backup::extract_full_backup(
            &opts.backup_dir,
            &opts.output_dir,
            &opts.password,
            opts.incremental,
        )
        .context("Full backup decrypt failed")?
    } else {
        let specs = opts.profile.specs(opts.incremental);

        ios_backup::extract_from_encrypted_backup(
            &opts.backup_dir,
            &opts.output_dir,
            &opts.password,
            &specs,
        )
        .with_context(|| format!("{:?} profile decrypt failed", opts.profile))?
    };

    println!();
    println!("HELiOS complete");
    println!("  Extracted: {}", result.extracted);
    println!("  Skipped:   {}", result.skipped);
    println!("  Errors:    {}", result.errors);

    Ok(())
}

fn parse_args() -> Result<CliOptions> {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        std::process::exit(0);
    }

    let case_name = args
        .get(1)
        .filter(|v| !v.starts_with('-'))
        .cloned()
        .ok_or_else(|| {
            print_usage();
            anyhow!("missing case name")
        })?;

    let mut input_raw: Option<String> = None;
    let mut output_raw: Option<String> = None;
    let mut password_raw: Option<String> = None;
    let mut password_file_raw: Option<String> = None;
    let mut incremental = true;
    let mut profile = ExtractProfile::Full;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--input" | "--backup-dir" => {
                input_raw = take_value(&args, i, "--input")?;
                i += 1;
            }
            "-o" | "--out" | "--output" => {
                output_raw = take_value(&args, i, "--output")?;
                i += 1;
            }
            "-p" | "--password" => {
                password_raw = take_value(&args, i, "--password")?;
                i += 1;
            }
            "-pf" | "--password-file" => {
                password_file_raw = take_value(&args, i, "--password-file")?;
                i += 1;
            }
            "--prompt" => {
                // Accepted for compatibility. Prompting is the default fallback.
            }
            "--overwrite" => incremental = false,
            "--incremental" => incremental = true,
            "--sms-only" => profile = ExtractProfile::Messages,
            "--profile" => {
                let raw = take_value(&args, i, "--profile")?
                    .ok_or_else(|| anyhow!("--profile requires a value"))?;
                profile = ExtractProfile::parse(&raw)?;
                i += 1;
            }
            unknown => return Err(anyhow!("unknown argument: {}", unknown)),
        }

        i += 1;
    }

    let backup_dir =
        normalize_path(&input_raw.ok_or_else(|| anyhow!("missing -i/--input <backup_dir>"))?);

    let output_dir = match output_raw {
        Some(raw) => normalize_path(&raw),
        None => default_output_dir(&case_name),
    };

    let password = if let Some(p) = password_raw {
        p
    } else if let Some(path) = password_file_raw {
        read_password_file(&normalize_path(&path))?
    } else {
        prompt_password()?
    };

    Ok(CliOptions {
        case_name,
        backup_dir,
        output_dir,
        password,
        incremental,
        profile,
    })
}

fn take_value(args: &[String], index: usize, flag: &str) -> Result<Option<String>> {
    args.get(index + 1)
        .filter(|v| !v.starts_with('-'))
        .cloned()
        .map(Some)
        .ok_or_else(|| anyhow!("{} requires a value", flag))
}

fn prompt_password() -> Result<String> {
    let password = rpassword::prompt_password("Backup password: ")?;
    if password.trim().is_empty() {
        return Err(anyhow!("Decrypt password is empty"));
    }
    Ok(password)
}

fn read_password_file(path: &Path) -> Result<String> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;

    let password = contents.trim_end_matches(['\r', '\n']).to_string();

    if password.trim().is_empty() {
        return Err(anyhow!("Password file is empty: {}", path.display()));
    }

    Ok(password)
}

fn default_output_dir(case_name: &str) -> PathBuf {
    let base = if let Ok(raw) = env::var(DEFAULT_OUTPUT_DIR_ENV) {
        normalize_path(&raw)
    } else if let Some(home) = dirs::home_dir() {
        home.join("iON").join("prepared").join("helios")
    } else {
        PathBuf::from("./prepared/helios")
    };

    base.join(case_name)
}

fn normalize_path(raw: &str) -> PathBuf {
    if cfg!(windows) {
        return PathBuf::from(raw);
    }

    if let Some((drive, rest)) = raw.split_once(':') {
        if rest.starts_with('\\') || rest.starts_with('/') {
            let drive = drive.to_lowercase();
            let rest = rest.trim_start_matches(['\\', '/']);
            let rest = rest.replace('\\', "/");
            return PathBuf::from(format!("/mnt/{}/{}", drive, rest));
        }
    }

    PathBuf::from(raw)
}

fn print_usage() {
    eprintln!("HELiOS - encrypted iOS backup decrypt/reconstruct CLI");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  helios <case> -i <backup_dir> -o <output_dir> -pf <password_file>");
    eprintln!("  helios <case> -i <backup_dir> -o <output_dir> --prompt");
    eprintln!("  helios <case> -i <backup_dir> --profile messages");
    eprintln!("  helios <case> -i <backup_dir> --sms-only");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -i,  --input <path>          Encrypted backup root");
    eprintln!("  -o,  --out <path>            Output folder");
    eprintln!("  -pf, --password-file <path>  Read password from file");
    eprintln!("  -p,  --password <value>      Provide password directly");
    eprintln!("       --prompt                Prompt for password");
    eprintln!("       --overwrite             Overwrite output files");
    eprintln!("       --incremental           Skip newer files/default incremental mode");
    eprintln!("       --profile <name>        Extraction profile");
    eprintln!("                                full, messages, contacts, calls, voicemail, mail,");
    eprintln!("                                location, photos, appdata, finance");
    eprintln!("       --sms-only              Deprecated alias for --profile messages");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  ION_HELIOS_OUTPUT_DIR        Default HELiOS output base directory");
}
