//! `spacekit-runbook` — CLI tooling for runbook scenarios and training corpus
//! generation.
//!
//! Subcommands:
//!   - `generate-corpus` — read logs + scenarios, produce JSONL training rows
//!   - `verify-corpus` — sanity-check existing JSONL files for consistency
//!   - `list-scenarios` — print loaded scenarios for debugging

mod corpus;
mod log_reader;
mod scenario;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "spacekit-runbook")]
#[command(about = "Runbook tooling for SpaceKit consensus")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate JSONL training corpus from logs and scenarios.
    GenerateCorpus {
        /// Directory containing spacekit-log JSONL files.
        #[arg(long)]
        logs: PathBuf,
        /// Directory containing runbook scenario YAML files.
        #[arg(long)]
        scenarios: PathBuf,
        /// Output directory for {domain}.jsonl files.
        #[arg(long)]
        output: PathBuf,
        /// Maximum training examples per scenario (deduplicated).
        #[arg(long, default_value = "200")]
        cap_per_scenario: usize,
        /// Fraction of rows to put in the 'test' split (rest go to 'train').
        #[arg(long, default_value = "0.10")]
        test_split: f64,
        /// Policy regime to label rows with (read from operational logs in
        /// production; defaults to 'default' for offline runs).
        #[arg(long, default_value = "default")]
        policy_regime: String,
        /// Truncate output files before writing (default: append).
        #[arg(long)]
        truncate: bool,
    },
    /// Validate an existing JSONL corpus for consistency.
    VerifyCorpus {
        /// Directory containing {domain}.jsonl files.
        #[arg(long)]
        dir: PathBuf,
    },
    /// List loaded scenarios from a directory.
    ListScenarios {
        #[arg(long)]
        scenarios: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GenerateCorpus {
            logs,
            scenarios,
            output,
            cap_per_scenario,
            test_split,
            policy_regime,
            truncate,
        } => run_generate(
            &logs,
            &scenarios,
            &output,
            cap_per_scenario,
            test_split,
            &policy_regime,
            truncate,
        ),
        Command::VerifyCorpus { dir } => run_verify(&dir),
        Command::ListScenarios { scenarios } => run_list_scenarios(&scenarios),
    }
}

fn run_generate(
    logs: &std::path::Path,
    scenarios_dir: &std::path::Path,
    output: &std::path::Path,
    cap: usize,
    test_split: f64,
    policy_regime: &str,
    truncate: bool,
) -> Result<()> {
    println!("Loading scenarios from {:?}...", scenarios_dir);
    let scenarios = scenario::load_scenarios_from_dir(scenarios_dir)?;
    println!("Loaded {} scenarios.", scenarios.len());

    // Pre-compile queries; each scenario gets its own Vec of compiled queries.
    let mut compiled: Vec<(scenario::Scenario, Vec<spacekit_log::ScenarioQuery>)> = Vec::new();
    for s in scenarios {
        let queries = s.compiled_queries()?;
        compiled.push((s, queries));
    }

    if truncate {
        println!("Truncating existing output files in {:?}...", output);
        for s in &compiled {
            let path = output.join(format!(
                "{}.jsonl",
                s.0.recommended_agent_classification.domain
            ));
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    println!("Reading logs from {:?}...", logs);
    let mut matched_rows: Vec<(String, corpus::TrainingRow)> = Vec::new();
    let mut event_count: u64 = 0;
    let mut match_count: u64 = 0;

    log_reader::iter_logs(logs, |event| {
        event_count += 1;
        // Check this event against every scenario's queries.
        for (s, queries) in &compiled {
            // ANY query matching means the scenario claims this event.
            let claimed = queries.iter().any(|q| q.matches(&event));
            if !claimed {
                continue;
            }

            // Decide split deterministically based on the event content hash
            // so re-runs produce the same split assignment.
            let event_hash = event.content_hash(keccak256_wrapper);
            let split_byte = event_hash.as_slice()[0];
            let split = if (split_byte as f64) < 256.0 * test_split {
                "test"
            } else {
                "train"
            };

            let row = corpus::build_training_row(&event, s, policy_regime, split);
            matched_rows.push((s.scenario_id.clone(), row));
            match_count += 1;
        }
        Ok(())
    })?;

    println!(
        "Processed {} events; {} matched a scenario.",
        event_count, match_count
    );

    // Dedup + cap.
    let final_rows = corpus::deduplicate_and_cap(matched_rows, cap);
    println!(
        "After dedup and cap-per-scenario ({}): {} rows.",
        cap,
        final_rows.len()
    );

    // Per-domain distribution.
    let mut per_domain: HashMap<String, usize> = HashMap::new();
    for row in &final_rows {
        *per_domain.entry(row.domain.clone()).or_insert(0) += 1;
    }
    for (domain, count) in &per_domain {
        println!("  domain {} : {} rows", domain, count);
    }

    // Sanity warning: domain dominated by one scenario.
    let total: usize = final_rows.len();
    if total > 0 {
        let mut per_scenario_per_domain: HashMap<(String, String), usize> = HashMap::new();
        for row in &final_rows {
            let scenario_prefix = row.task_id.split('_').next().unwrap_or("").to_string();
            *per_scenario_per_domain
                .entry((row.domain.clone(), scenario_prefix))
                .or_insert(0) += 1;
        }
        for ((domain, scenario_id), count) in &per_scenario_per_domain {
            let domain_total = per_domain[domain];
            let share = *count as f64 / domain_total as f64;
            if share > 0.8 && domain_total > 10 {
                eprintln!(
                    "WARNING: domain {} is {:.0}% from scenario {} — consider broader corpus",
                    domain,
                    share * 100.0,
                    scenario_id
                );
            }
        }
    }

    println!("Writing to {:?}...", output);
    let counts = corpus::write_corpus(&final_rows, output)?;
    for (d, c) in counts {
        println!("  Wrote {} rows to {}.jsonl", c, d);
    }
    Ok(())
}

fn run_verify(dir: &std::path::Path) -> Result<()> {
    use std::collections::HashSet;
    use std::io::BufRead;
    let known_domains = [
        "consensus_tuning",
        "anomaly_scoring",
        "clique_assessment",
        "fraud_classification",
        "policy_regime_recommendation",
    ];
    let mut errors = 0;

    for domain in &known_domains {
        let path = dir.join(format!("{}.jsonl", domain));
        if !path.exists() {
            eprintln!("NOTE: {} not present", path.display());
            continue;
        }
        let file = std::fs::File::open(&path)?;
        let reader = std::io::BufReader::new(file);
        let mut task_ids = HashSet::new();
        let mut row_count = 0;
        let mut split_counts: HashMap<String, usize> = HashMap::new();
        let mut intent_counts: HashMap<String, usize> = HashMap::new();
        for (lineno, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let row: corpus::TrainingRow = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!(
                        "ERROR: {}:{} malformed JSONL: {}",
                        path.display(),
                        lineno + 1,
                        e
                    );
                    errors += 1;
                    continue;
                }
            };
            row_count += 1;
            if row.domain != *domain {
                eprintln!(
                    "ERROR: {}:{} row's domain '{}' != file domain '{}'",
                    path.display(),
                    lineno + 1,
                    row.domain,
                    domain
                );
                errors += 1;
            }
            if !task_ids.insert(row.task_id.clone()) {
                eprintln!(
                    "ERROR: {}:{} duplicate task_id: {}",
                    path.display(),
                    lineno + 1,
                    row.task_id
                );
                errors += 1;
            }
            *split_counts.entry(row.split.clone()).or_insert(0) += 1;
            *intent_counts
                .entry(row.semantic_intent.clone())
                .or_insert(0) += 1;
        }
        println!("{}: {} rows", domain, row_count);
        for (split, count) in &split_counts {
            println!("  split {} : {}", split, count);
        }
        for (intent, count) in &intent_counts {
            println!("  intent {} : {}", intent, count);
        }
    }
    if errors > 0 {
        bail!("{} errors found; corpus is not valid", errors);
    }
    println!("All checks passed.");
    Ok(())
}

fn run_list_scenarios(dir: &std::path::Path) -> Result<()> {
    let scenarios = scenario::load_scenarios_from_dir(dir)?;
    let mut ids = std::collections::HashSet::new();
    for scenario in &scenarios {
        if !ids.insert(&scenario.scenario_id) {
            bail!("duplicate scenario_id: {}", scenario.scenario_id);
        }
        scenario.compiled_queries()?;
    }
    println!(
        "{:<8} {:<10} {:<55} {}",
        "ID", "DOMAIN", "SUMMARY", "INTENT"
    );
    println!("{}", "-".repeat(110));
    for s in &scenarios {
        let summary = if s.summary.len() > 55 {
            format!("{}...", &s.summary[..52])
        } else {
            s.summary.clone()
        };
        println!(
            "{:<8} {:<10} {:<55} {}",
            s.scenario_id,
            &s.recommended_agent_classification.domain
                [..s.recommended_agent_classification.domain.len().min(10)],
            summary,
            s.recommended_agent_classification.intent
        );
    }
    println!("\n{} scenarios loaded.", scenarios.len());
    Ok(())
}

fn keccak256_wrapper(b: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Keccak256};
    let mut h = Keccak256::new();
    h.update(b);
    let r = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&r);
    out
}
