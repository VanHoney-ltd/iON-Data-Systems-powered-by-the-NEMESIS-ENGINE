use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use minios::case::Case;
use minios::chronos::device::probe_live_status;
use minios::chronos::{run_chronos, ChronosConfig};
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Read};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ion")]
#[command(about = "iON Data Security Systems - Nemesis Engine CLI")]
#[command(version)]
struct Cli {
    /// Enable NDJSON streaming of agent progress events to stdout.
    #[arg(long, global = true, default_value_t = false)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List or run extraction agents.
    Agents(AgentsCommand),
    /// Run every staged evidence agent and prepare UI-ready case files.
    #[command(name = "run-all")]
    RunAll(CaseArg),
    /// Nemesis orchestration commands.
    Nemesis(NemesisCommand),
    /// iOS device detection and backup acquisition.
    Device(DeviceCommand),
    /// Start or inspect iOS encrypted backups.
    Backup(BackupCommand),
    /// Psyche contact review commands.
    Psyche(PsycheCommand),
    /// Hermes media transcription commands.
    Hermes(HermesCommand),
    /// Evidence-review commands.
    #[command(name = "evidence-review")]
    EvidenceReview(CaseArg),
    /// Start the local HTTP/SSE bridge used by desktop shells.
    Serve(ServeArgs),
    /// Local LLM runtime checks.
    Llm(LlmCommand),
    /// Backward-compatible command shape: ion <agent> <case> [args]...
    #[command(external_subcommand)]
    Legacy(Vec<OsString>),
}

#[derive(Args)]
struct CaseArg {
    case: String,
}

#[derive(Args)]
struct AgentsCommand {
    #[command(subcommand)]
    command: AgentsSubcommand,
}

#[derive(Subcommand)]
enum AgentsSubcommand {
    /// List installed agents.
    List,
    /// Run one agent against a case.
    Run { agent: String, case: String },
}

#[derive(Args)]
struct NemesisCommand {
    #[command(subcommand)]
    command: NemesisSubcommand,
}

#[derive(Subcommand)]
enum NemesisSubcommand {
    /// Run the production case pipeline.
    Run(CaseArg),
    /// List the embedded dependency pipeline.
    Pipeline,
}

#[derive(Args)]
struct DeviceCommand {
    #[command(subcommand)]
    command: DeviceSubcommand,
}

#[derive(Subcommand)]
enum DeviceSubcommand {
    /// Probe for a connected iOS device.
    Status {
        /// Emit the full probe result as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct BackupCommand {
    #[command(subcommand)]
    command: BackupSubcommand,
}

#[derive(Subcommand)]
enum BackupSubcommand {
    /// Start a new encrypted iOS backup for a case.
    Start {
        case: String,
        /// Backup password. Prefer BACKUP_PASSWORD or the hidden prompt for shell history safety.
        #[arg(long)]
        password: Option<String>,
        /// Environment variable to read the backup password from.
        #[arg(long, default_value = "BACKUP_PASSWORD")]
        password_env: String,
        /// Do not prompt interactively when no password is available.
        #[arg(long)]
        no_prompt: bool,
    },
}

#[derive(Args)]
struct PsycheCommand {
    #[command(subcommand)]
    command: PsycheSubcommand,
}

#[derive(Subcommand)]
enum PsycheSubcommand {
    /// Run Psyche over contact dossiers.
    Run {
        case: String,
        /// Enable bounded Ollama synthesis during the run.
        #[arg(long)]
        llm: bool,
        /// Ollama model name for Psyche review.
        #[arg(long)]
        model: Option<String>,
        /// Maximum number of contacts to synthesize with the LLM.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Launch the selectable Psyche TUI.
    Tui(CaseArg),
}

#[derive(Args)]
struct HermesCommand {
    #[command(subcommand)]
    command: HermesSubcommand,
}

#[derive(Subcommand)]
enum HermesSubcommand {
    /// Run Hermes transcription subcommand arguments.
    Transcribe {
        case: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Args)]
struct ServeArgs {
    /// Bind address, for example 127.0.0.1:17870.
    #[arg(default_value = "127.0.0.1:17870")]
    bind: String,
}

#[derive(Args)]
struct LlmCommand {
    #[command(subcommand)]
    command: LlmSubcommand,
}

#[derive(Subcommand)]
enum LlmSubcommand {
    /// Check the local Ollama runtime and list installed models.
    Status {
        #[arg(long, default_value = "http://127.0.0.1:11434")]
        endpoint: String,
    },
    /// Generate text with a local Ollama model.
    Generate {
        /// Ollama model name.
        #[arg(long)]
        model: String,
        /// Prompt text. If omitted, stdin is read.
        #[arg(long)]
        prompt: Option<String>,
        /// Read prompt text from a file.
        #[arg(long)]
        prompt_file: Option<PathBuf>,
        /// Stream chunks as they arrive.
        #[arg(long)]
        stream: bool,
        #[arg(long, default_value = "http://127.0.0.1:11434")]
        endpoint: String,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("iON ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.json {
        minios::agents::set_json_streaming(true);
    }

    match cli.command {
        Command::Agents(command) => run_agents_command(command),
        Command::RunAll(arg) => minios::run_all::run(&arg.case),
        Command::Nemesis(command) => run_nemesis_command(command),
        Command::Device(command) => run_device_command(command),
        Command::Backup(command) => run_backup_command(command),
        Command::Psyche(command) => run_psyche_command(command),
        Command::Hermes(command) => run_hermes_command(command),
        Command::EvidenceReview(arg) => minios::evidence_review::run(&arg.case),
        Command::Serve(args) => minios::desktop_server::serve(&args.bind, &[]),
        Command::Llm(command) => run_llm_command(command),
        Command::Legacy(args) => run_legacy(args),
    }
}

fn run_agents_command(command: AgentsCommand) -> Result<()> {
    match command.command {
        AgentsSubcommand::List => {
            for agent in minios::agents::get_agent_definitions() {
                println!(
                    "{:<14} {:<16} {}",
                    agent.slug, agent.category, agent.description
                );
            }
            Ok(())
        }
        AgentsSubcommand::Run { agent, case } => minios::agents::dispatch(&agent, &case),
    }
}

fn run_nemesis_command(command: NemesisCommand) -> Result<()> {
    match command.command {
        NemesisSubcommand::Run(arg) => minios::run_all::run(&arg.case),
        NemesisSubcommand::Pipeline => {
            let runner = minios::nemesis::PipelineRunner::load()?;
            for agent in runner.agents() {
                if let Some(step) = runner.step_info(agent) {
                    let required = if step.required {
                        "required"
                    } else {
                        "optional"
                    };
                    println!(
                        "{:<12} {:<8} depends_on=[{}] {}",
                        step.agent,
                        required,
                        step.depends_on.join(","),
                        step.description.as_deref().unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
    }
}

fn run_device_command(command: DeviceCommand) -> Result<()> {
    match command.command {
        DeviceSubcommand::Status { json } => {
            let case = Case::from_root(
                "_device_probe",
                std::env::temp_dir().join("ion-nemesis-device-probe"),
            );
            let status = probe_live_status(&case);
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
                return Ok(());
            }
            println!("connected_devices={}", status.connected_count());
            println!("paired_devices={}", status.paired_count);
            for tool in &status.tool_status {
                println!(
                    "tool {:<14} {} ({})",
                    tool.name,
                    if tool.available { "ok" } else { "missing" },
                    tool.detail
                );
            }
            for device in &status.connected_devices {
                let name = device
                    .device_info
                    .as_ref()
                    .map(|info| info.device_name.as_str())
                    .unwrap_or("iOS Device");
                println!(
                    "device {} udid={} pairing={}",
                    name,
                    device.udid,
                    device.pairing_state.label()
                );
            }
            Ok(())
        }
    }
}

fn run_backup_command(command: BackupCommand) -> Result<()> {
    match command.command {
        BackupSubcommand::Start {
            case,
            password,
            password_env,
            no_prompt,
        } => {
            let mut config = ChronosConfig::default();
            config.backup_password = password
                .or_else(|| std::env::var(&password_env).ok())
                .or_else(|| prompt_password(no_prompt).ok().flatten());
            config.verbose = true;

            if config.backup_password.is_none() {
                anyhow::bail!(
                    "Encrypted backups require a password. Use --password, set {}, or run interactively.",
                    password_env
                );
            }

            let case = Case::new(case)?;
            case.workspace().ensure_layout()?;
            let result = run_chronos(case, config)?;
            println!("backup_root={}", result.backup_root.display());
            if let Some(root) = result.helios_root {
                println!("helios_root={}", root.display());
            }
            println!("orpheus_ok={}", result.orpheus_ok);
            Ok(())
        }
    }
}

fn run_psyche_command(command: PsycheCommand) -> Result<()> {
    match command.command {
        PsycheSubcommand::Run {
            case,
            llm,
            model,
            limit,
        } => {
            if llm {
                std::env::set_var("PSYCHE_LLM", "1");
            }
            if let Some(model) = model {
                std::env::set_var("PSYCHE_REVIEW_MODEL", model);
            }
            if let Some(limit) = limit {
                std::env::set_var("PSYCHE_LLM_LIMIT", limit.to_string());
            }
            minios::agents::dispatch("psyche", &case)
        }
        PsycheSubcommand::Tui(arg) => minios::agents::psyche::run_tui(&arg.case),
    }
}

fn run_hermes_command(command: HermesCommand) -> Result<()> {
    match command.command {
        HermesSubcommand::Transcribe { case, args } => {
            minios::agents::hermes::run_transcription_command(&case, &args)
        }
    }
}

fn run_llm_command(command: LlmCommand) -> Result<()> {
    match command.command {
        LlmSubcommand::Status { endpoint } => {
            let endpoint = endpoint.trim_end_matches('/');
            let value: serde_json::Value = reqwest::blocking::get(format!("{endpoint}/api/tags"))
                .context("calling Ollama /api/tags")?
                .json()
                .context("parsing Ollama model list")?;
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        LlmSubcommand::Generate {
            model,
            prompt,
            prompt_file,
            stream,
            endpoint,
        } => {
            let prompt = read_prompt(prompt, prompt_file)?;
            let endpoint = endpoint.trim_end_matches('/');
            let client = reqwest::blocking::Client::builder().build()?;
            let response = client
                .post(format!("{endpoint}/api/generate"))
                .json(&serde_json::json!({
                    "model": model,
                    "prompt": prompt,
                    "stream": stream,
                    "options": {
                        "temperature": 0.1,
                        "top_p": 0.9
                    }
                }))
                .send()
                .context("calling Ollama /api/generate")?;

            if !response.status().is_success() {
                anyhow::bail!("Ollama generate failed with {}", response.status());
            }

            if stream {
                let reader = std::io::BufReader::new(response);
                for line in reader.lines() {
                    let line = line?;
                    if line.trim().is_empty() {
                        continue;
                    }
                    let value: serde_json::Value =
                        serde_json::from_str(&line).context("parsing Ollama stream chunk")?;
                    if let Some(chunk) = value.get("response").and_then(|value| value.as_str()) {
                        print!("{chunk}");
                    }
                    if value.get("done").and_then(|value| value.as_bool()) == Some(true) {
                        println!();
                        break;
                    }
                }
            } else {
                let value: serde_json::Value = response
                    .json()
                    .context("parsing Ollama generate response")?;
                if let Some(text) = value.get("response").and_then(|value| value.as_str()) {
                    println!("{}", text.trim());
                } else {
                    println!("{}", serde_json::to_string_pretty(&value)?);
                }
            }
            Ok(())
        }
    }
}

fn read_prompt(prompt: Option<String>, prompt_file: Option<PathBuf>) -> Result<String> {
    if let Some(prompt) = prompt {
        return Ok(prompt);
    }
    if let Some(path) = prompt_file {
        return std::fs::read_to_string(&path)
            .with_context(|| format!("reading prompt file {}", path.display()));
    }
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    if buffer.trim().is_empty() {
        anyhow::bail!("Prompt is required through --prompt, --prompt-file, or stdin");
    }
    Ok(buffer)
}

fn run_legacy(args: Vec<OsString>) -> Result<()> {
    let args = args
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| anyhow::anyhow!("legacy argument was not valid UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;
    if args.len() < 2 {
        anyhow::bail!("Usage: ion <agent> <case> [args]...");
    }
    let agent = &args[0];
    let case = &args[1];
    let rest = args[2..].to_vec();

    match agent.as_str() {
        "hermes-transcribe" => minios::agents::hermes::run_transcription_command(case, &rest),
        "psyche-tui" => minios::agents::psyche::run_tui(case),
        "evidence-review" => minios::evidence_review::run(case),
        "run-all" => minios::run_all::run(case),
        "desktop-serve" => minios::desktop_server::serve(case, &rest),
        _ => minios::agents::dispatch(agent, case),
    }
}

fn prompt_password(no_prompt: bool) -> Result<Option<String>> {
    if no_prompt || !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let password = rpassword::prompt_password("Encrypted backup password: ")?;
    Ok((!password.is_empty()).then_some(password))
}

#[allow(dead_code)]
fn default_cases_root() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("backups")
        .join("cases")
}
