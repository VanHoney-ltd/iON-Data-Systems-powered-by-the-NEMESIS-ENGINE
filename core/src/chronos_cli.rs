use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::path::PathBuf;

use ion::case::Case;
use ion::chronos::{run_chronos, ChronosConfig};

#[derive(Parser, Debug)]
#[command(name = "chronos")]
#[command(about = "iON acquisition and preparation agent")]
struct Cli {
    /// Case name to acquire or prepare
    case: String,

    /// Use an existing backup instead of pairing and pulling from a connected device
    #[arg(long, alias = "use-existing", default_value_t = false)]
    offline: bool,

    /// Use a specific existing backup root
    #[arg(short = 'i', long = "input", alias = "backup-path")]
    backup_path: Option<PathBuf>,

    /// Backup/decrypt password. Prefer --password-file or BACKUP_PASSWORD for shell history.
    #[arg(short = 'p', long)]
    password: Option<String>,

    /// Read backup/decrypt password from a file
    #[arg(long = "password-file")]
    password_file: Option<PathBuf>,

    /// Enable verbose Chronos logging
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("CHRONOS ERROR: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let case = Case::new(&cli.case)?;
    case.open("chronos")?;

    let mut config = ChronosConfig::default();
    config.offline = cli.offline || cli.backup_path.is_some();
    config.backup_path_override = cli.backup_path;
    config.backup_password = resolve_password(cli.password, cli.password_file)?;
    config.verbose = cli.verbose;

    run_chronos(case, config)?;
    Ok(())
}

fn resolve_password(inline: Option<String>, file: Option<PathBuf>) -> Result<Option<String>> {
    if inline.is_some() && file.is_some() {
        return Err(anyhow!(
            "Use either --password or --password-file, not both"
        ));
    }

    if let Some(password) = inline {
        return Ok(Some(password));
    }

    if let Some(path) = file {
        let password = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?
            .trim_end_matches(['\r', '\n'])
            .to_string();

        if password.trim().is_empty() {
            return Err(anyhow!("Password file is empty: {}", path.display()));
        }

        return Ok(Some(password));
    }

    Ok(std::env::var("BACKUP_PASSWORD").ok())
}
