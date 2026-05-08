//! MINiOS pipeline orchestrator.
//!
//! Forensic extraction is defined declaratively in `pipeline.toml`.
//! The runner performs a topological sort on agent dependencies and executes
//! them sequentially, logging progress and handling failures.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

// Include the pipeline TOML at compile time
const PIPELINE_TOML: &str = include_str!("pipeline.toml");

#[derive(Debug, Deserialize)]
pub struct Pipeline {
    pub step: Vec<Step>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Step {
    pub agent: String,
    pub required: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub description: Option<String>,
}

/// Parsed pipeline with dependency graph resolved.
pub struct PipelineRunner {
    steps: Vec<Step>,
    // maps agent name to step index
    index: HashMap<String, usize>,
}

impl PipelineRunner {
    /// Load pipeline from embedded TOML.
    pub fn load() -> Result<Self> {
        let pipeline: Pipeline = toml::from_str(PIPELINE_TOML)
            .context("parsing pipeline.toml")?;

        let mut index = HashMap::new();
        for (i, step) in pipeline.step.iter().enumerate() {
            if index.insert(step.agent.clone(), i).is_some() {
                bail!("Duplicate agent '{}' in pipeline", step.agent);
            }
        }

        // Validate dependencies exist
        for step in &pipeline.step {
            for dep in &step.depends_on {
                if !index.contains_key(dep) {
                    bail!(
                        "Step '{}' depends on '{}' which is not defined",
                        step.agent, dep
                    );
                }
            }
        }

        // Topologically sort steps
        let sorted = topo_sort(&pipeline.step, &index)?;

        Ok(Self { steps: sorted, index })
    }

    /// Run the full pipeline against a case.
    pub fn run(&self, case_name: &str) -> Result<PipelineResult> {
        let mut completed = HashSet::new();
        let mut failed = Vec::new();
        let mut skipped = Vec::new();

        for step in &self.steps {
            // Check if all dependencies completed
            let deps_ok = step.depends_on.iter().all(|d| completed.contains(d));
            if !deps_ok {
                if step.required {
                    bail!(
                        "Required step '{}' has unmet dependencies",
                        step.agent
                    );
                } else {
                    eprintln!("[NEMESIS] Skipping '{}' — dependencies not met", step.agent);
                    skipped.push(step.agent.clone());
                    continue;
                }
            }

            print!("[NEMESIS] Running {} ... ", step.agent);
            match run_agent(&step.agent, case_name) {
                Ok(()) => {
                    println!("OK");
                    completed.insert(step.agent.clone());
                }
                Err(e) => {
                    println!("FAILED: {}", e);
                    if step.required {
                        bail!(
                            "Required agent '{}' failed: {}",
                            step.agent, e
                        );
                    }
                    failed.push((step.agent.clone(), format!("{}", e)));
                }
            }
        }

        Ok(PipelineResult {
            completed: completed.into_iter().collect(),
            failed,
            skipped,
        })
    }

    /// Run a single agent (used by UI for individual agent runs).
    pub fn run_single(&self, agent: &str, case_name: &str) -> Result<()> {
        run_agent(agent, case_name)
    }

    /// List all agents in pipeline order.
    pub fn agents(&self) -> Vec<&str> {
        self.steps.iter().map(|s| s.agent.as_str()).collect()
    }

    /// Get step info for an agent.
    pub fn step_info(&self, agent: &str) -> Option<&Step> {
        self.index.get(agent).map(|&i| &self.steps[i])
    }
}

#[derive(Debug)]
pub struct PipelineResult {
    pub completed: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub skipped: Vec<String>,
}

impl PipelineResult {
    pub fn summary(&self) -> String {
        format!(
            "Pipeline complete: {} succeeded, {} failed, {} skipped",
            self.completed.len(),
            self.failed.len(),
            self.skipped.len()
        )
    }

    pub fn all_ok(&self) -> bool {
        self.failed.is_empty() && self.skipped.is_empty()
    }
}

fn run_agent(agent: &str, case_name: &str) -> Result<()> {
    // Uses the unified minios binary
    let status = Command::new("minios")
        .arg(agent)
        .arg(case_name)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .with_context(|| format!("failed to spawn minios {}", agent))?;

    if !status.success() {
        bail!("Agent '{}' exited with code {:?}", agent, status.code());
    }
    Ok(())
}

fn topo_sort(
    steps: &[Step],
    index: &HashMap<String, usize>,
) -> Result<Vec<Step>> {
    // Kahn's algorithm
    let n = steps.len();
    let mut in_degree = vec![0; n];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

    for step in steps {
        let u = index[&step.agent];
        for dep in &step.depends_on {
            let v = index[dep]; // dep must come before step
            adj[v].push(u);
            in_degree[u] += 1;
        }
    }

    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut sorted = Vec::with_capacity(n);

    while let Some(u) = queue.pop() {
        sorted.push(steps[u].clone());
        for &v in &adj[u] {
            in_degree[v] -= 1;
            if in_degree[v] == 0 {
                queue.push(v);
            }
        }
    }

    if sorted.len() != n {
        bail!("Pipeline has circular dependencies");
    }

    Ok(sorted)
}
