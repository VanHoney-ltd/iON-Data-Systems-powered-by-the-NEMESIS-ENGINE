use anyhow::Result;
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

    /// Extra command arguments for specialized agent subcommands
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
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

    if cli.agent == "hermes-transcribe" {
        minios::agents::hermes::run_transcription_command(&cli.case, &cli.args)?;
    } else if cli.agent == "psyche-tui" {
        minios::agents::psyche::run_tui(&cli.case)?;
    } else if cli.agent == "evidence-review" {
        minios::evidence_review::run(&cli.case)?;
    } else if cli.agent == "run-all" {
        minios::run_all::run(&cli.case)?;
    } else if cli.agent == "desktop-serve" {
        minios::desktop_server::serve(&cli.case, &cli.args)?;
    } else {
        // Dispatch to the correct agent
        minios::agents::dispatch(&cli.agent, &cli.case)?;
    }

    Ok(())
}
