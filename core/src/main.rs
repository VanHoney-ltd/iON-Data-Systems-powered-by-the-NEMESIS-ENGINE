use std::io::{self, Write};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use clap::Parser;

#[derive(Parser)]
#[command(name = "minios")]
#[command(about = "iON Mobile Evidence Extraction Core")]
struct Cli {
    /// Enable NDJSON streaming of agent progress events to stdout
    #[arg(long, default_value_t = false)]
    json: bool,

    /// Agent name to run
    agent: String,

    /// Case name to process
    case: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("MINiOS ERROR: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.json {
        minios::agents::set_json_streaming(true);
    }

    // Dispatch to the correct agent
    minios::agents::dispatch(&cli.agent, &cli.case)?;

    Ok(())
}
