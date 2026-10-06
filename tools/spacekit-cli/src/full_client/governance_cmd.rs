//! `spacekit governance` — proof-of-authority validator governance.
//!
//! Authorities sign proposals and votes with the SPHINCS+ key in their DID
//! wallet (`~/.spacekit/did_wallet.json`, written by `spacekit did create
//! --save`). Signed messages go to any node, which gossips them to the rest.
//! The payload formats match `validator_governance.rs` in the compute node:
//!
//! ```text
//! SPACEKIT-GOVERNANCE-PROPOSAL-v1\n{body_json}
//! SPACEKIT-GOVERNANCE-VOTE-v1\n{network}\n{proposal_id}\n{approve|reject}
//! ```

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const PROPOSAL_DOMAIN: &str = "SPACEKIT-GOVERNANCE-PROPOSAL-v1";
const VOTE_DOMAIN: &str = "SPACEKIT-GOVERNANCE-VOTE-v1";

type CmdResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Args, Debug)]
pub struct GovernanceArgs {
    /// Compute node API (default: $SPACEKIT_COMPUTE_URL or http://127.0.0.1:8080)
    #[arg(long, global = true)]
    pub node: Option<String>,
    /// DID wallet holding the authority's SPHINCS+ key
    /// (default: ~/.spacekit/did_wallet.json)
    #[arg(long, global = true)]
    pub wallet: Option<PathBuf>,
    #[command(subcommand)]
    pub command: GovernanceCommands,
}

#[derive(Subcommand, Debug)]
pub enum GovernanceCommands {
    /// Consensus mode, authorities, and thresholds
    Status,
    /// Staking rules, validators and their stake, and the block producers
    Staking,
    /// Bond or unbond this wallet's ASTRA as validator stake (signed, applied in the next block)
    Stake {
        #[arg(value_enum)]
        action: StakeActionArg,
        /// Amount in ASTRA (decimal, e.g. 15000 or 0.5)
        amount: String,
        /// Validator name shown by explorers
        #[arg(long)]
        name: Option<String>,
    },
    /// List proposals
    Proposals {
        /// Filter: pending, executed, rejected, expired, stale, failed
        #[arg(long)]
        status: Option<String>,
    },
    /// Show one proposal, including the exact text to sign for a vote
    Show { id: String },
    /// Create, sign, and submit a proposal
    Propose {
        #[command(subcommand)]
        action: ProposeAction,
        /// Short title shown in the governance UI
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
        /// Voting period in days (1 hour minimum, 30 days maximum)
        #[arg(long, default_value_t = 7.0)]
        voting_days: f64,
        /// Print the signed proposal instead of submitting it
        #[arg(long)]
        print_only: bool,
    },
    /// Sign and submit a vote
    Vote {
        proposal_id: String,
        choice: Choice,
    },
    /// Sign a proposal body produced elsewhere (e.g. on spacekit.xyz) and print the signature
    SignProposal {
        /// The body JSON exactly as shown, or @path to read it from a file
        #[arg(long)]
        body_json: String,
    },
    /// Sign a vote without submitting it and print the signature
    SignVote {
        proposal_id: String,
        choice: Choice,
        /// Network name (default: read from the node)
        #[arg(long)]
        network: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProposeAction {
    /// Admit an operator as an authority (validator without stake)
    AddAuthority {
        #[arg(long)]
        did: String,
        #[arg(long)]
        sphincs_pk_hex: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove an authority
    RemoveAuthority {
        #[arg(long)]
        did: String,
    },
    /// End proof of authority and switch to proof of stake (needs >= 10 validators)
    LiftPoa,
    /// Change when blocks are produced (every node follows the decided setting)
    SetBlockProduction {
        /// on-demand: blocks only for transactions, due rewards, and heartbeats; interval: every block time
        #[arg(long, value_enum)]
        mode: ProductionMode,
        /// Minimum gap between blocks (on-demand) or the block time (interval)
        #[arg(long, default_value_t = 2000)]
        block_time_ms: u64,
        /// On-demand: wait this long after the first pending transaction
        #[arg(long, default_value_t = 500)]
        batch_window_ms: u64,
        /// On-demand: an empty block after this much idle time (0 = never)
        #[arg(long, default_value_t = 300)]
        heartbeat_secs: u64,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ProductionMode {
    OnDemand,
    Interval,
}

impl ProductionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ProductionMode::OnDemand => "on_demand",
            ProductionMode::Interval => "interval",
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum StakeActionArg {
    /// Add stake (from the DID's AstraRewards holdings, locked or not)
    Bond,
    /// Start unbonding (stops counting now; released after the unbonding period)
    Unbond,
}

/// Parse a decimal ASTRA amount into wei (18 decimals).
fn astra_to_wei(amount: &str) -> Result<u128, Box<dyn std::error::Error>> {
    let amount = amount.trim();
    let (whole, frac) = amount.split_once('.').unwrap_or((amount, ""));
    if frac.len() > 18 || whole.is_empty() && frac.is_empty() {
        return Err(format!("invalid ASTRA amount {amount:?}").into());
    }
    let whole: u128 = if whole.is_empty() { 0 } else { whole.parse()? };
    let frac: u128 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<18}").parse()?
    };
    Ok(whole
        .checked_mul(1_000_000_000_000_000_000)
        .and_then(|w| w.checked_add(frac))
        .ok_or("amount too large")?)
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Choice {
    Approve,
    Reject,
}

impl Choice {
    fn as_str(self) -> &'static str {
        match self {
            Choice::Approve => "approve",
            Choice::Reject => "reject",
        }
    }
}

struct Wallet {
    did: String,
    pk_hex: String,
    secret_key: Vec<u8>,
}

fn load_wallet(path: Option<&PathBuf>) -> Result<Wallet, Box<dyn std::error::Error>> {
    let path = match path {
        Some(p) => p.clone(),
        None => dirs::home_dir()
            .ok_or("home directory not found")?
            .join(".spacekit")
            .join("did_wallet.json"),
    };
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("reading DID wallet {}: {e}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&raw)?;
    let did = json["did"].as_str().ok_or("wallet has no did")?.to_string();
    let sk_hex = json["sphincs_sk_hex"]
        .as_str()
        .ok_or("wallet has no sphincs_sk_hex")?;
    let pk_hex = json["sphincs_pk_hex"].as_str().unwrap_or_default();
    if let Ok(pk) = hex::decode(pk_hex) {
        let address = hex::encode(&Sha256::digest(&pk)[..20]);
        if !did.ends_with(&address) {
            return Err(format!("wallet DID {did} does not match its public key").into());
        }
    }
    Ok(Wallet {
        did,
        pk_hex: pk_hex.to_ascii_lowercase(),
        secret_key: hex::decode(sk_hex)?,
    })
}

fn sign(wallet: &Wallet, message: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let sig = spacekit_did::sphincs::SphincsPlus::sign(&wallet.secret_key, message)
        .map_err(|_| "invalid SPHINCS+ secret key in wallet")?;
    Ok(hex::encode(sig))
}

fn node_url(args: &GovernanceArgs) -> String {
    args.node
        .clone()
        .or_else(|| std::env::var("SPACEKIT_COMPUTE_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string())
        .trim_end_matches('/')
        .to_string()
}

async fn get_json(url: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let res = reqwest::get(url).await?;
    let status = res.status();
    let body: serde_json::Value = res.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{url}: {status} {}", body["error"].as_str().unwrap_or("")).into());
    }
    Ok(body)
}

async fn post_json(
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let res = reqwest::Client::new().post(url).json(body).send().await?;
    let status = res.status();
    let json: serde_json::Value = res.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{status}: {}", json["error"].as_str().unwrap_or("request failed")).into());
    }
    Ok(json)
}

fn short(s: &str) -> String {
    if s.len() > 20 {
        format!("{}…{}", &s[..12], &s[s.len() - 6..])
    } else {
        s.to_string()
    }
}

fn print_proposal_line(p: &serde_json::Value) {
    let tally = &p["tally"];
    println!(
        "  {}  {:<9} {:<40} {}/{} approve, {} reject",
        short(p["id"].as_str().unwrap_or("")).cyan(),
        p["status"].as_str().unwrap_or(""),
        p["title"].as_str().unwrap_or(""),
        tally["approve"],
        tally["needed"],
        tally["reject"],
    );
}

pub async fn handle_governance_command(args: &GovernanceArgs) -> CmdResult {
    let node = node_url(args);
    match &args.command {
        GovernanceCommands::Status => {
            let g = get_json(&format!("{node}/v1/governance")).await?;
            println!("Network:        {}", g["network"].as_str().unwrap_or("").bold());
            println!("Mode:           {}", g["mode"].as_str().unwrap_or("").green());
            println!(
                "Authorities:    {} (approvals needed {}, tolerates {} faulty)",
                g["authority_count"], g["approvals_needed"], g["fault_tolerance"]
            );
            println!(
                "Lift PoA:       needs {} validators ({})",
                g["min_validators_to_lift"],
                if g["can_lift_poa"].as_bool().unwrap_or(false) {
                    "can be proposed now"
                } else {
                    "not yet"
                }
            );
            let bp = &g["block_production"];
            if bp.is_object() {
                println!(
                    "Blocks:         {} (block time {} ms, batch {} ms, heartbeat {} s)",
                    bp["mode"].as_str().unwrap_or("?").replace('_', "-"),
                    bp["block_time_ms"],
                    bp["batch_window_ms"],
                    bp["heartbeat_secs"]
                );
            }
            println!("Electorate:     {}", g["electorate_hash"].as_str().unwrap_or(""));
            println!("State hash:     {}", g["state_hash"].as_str().unwrap_or(""));
            if let Some(list) = g["authorities"].as_array() {
                println!("\nAuthorities:");
                for a in list {
                    println!(
                        "  {}  {}",
                        a["did"].as_str().unwrap_or(""),
                        a["name"].as_str().unwrap_or("")
                    );
                }
            }
        }
        GovernanceCommands::Proposals { status } => {
            let mut url = format!("{node}/v1/governance/proposals");
            if let Some(s) = status {
                url.push_str(&format!("?status={s}"));
            }
            let list = get_json(&url).await?;
            let proposals = list["proposals"].as_array().cloned().unwrap_or_default();
            if proposals.is_empty() {
                println!("No proposals.");
            }
            for p in &proposals {
                print_proposal_line(p);
            }
        }
        GovernanceCommands::Show { id } => {
            let p = get_json(&format!("{node}/v1/governance/proposals/{id}")).await?;
            println!("{}", serde_json::to_string_pretty(&p)?);
        }
        GovernanceCommands::Propose {
            action,
            title,
            description,
            voting_days,
            print_only,
        } => {
            let wallet = load_wallet(args.wallet.as_ref())?;
            let g = get_json(&format!("{node}/v1/governance")).await?;
            let network = g["network"].as_str().ok_or("node did not report its network")?;
            let electorate = g["electorate_hash"]
                .as_str()
                .ok_or("node did not report its electorate")?;
            let action = match action {
                ProposeAction::AddAuthority {
                    did,
                    sphincs_pk_hex,
                    name,
                } => serde_json::json!({
                    "kind": "add_authority",
                    "did": did,
                    "sphincs_pk_hex": sphincs_pk_hex,
                    "name": name,
                }),
                ProposeAction::RemoveAuthority { did } => {
                    serde_json::json!({ "kind": "remove_authority", "did": did })
                }
                ProposeAction::LiftPoa => serde_json::json!({ "kind": "lift_poa" }),
                ProposeAction::SetBlockProduction {
                    mode,
                    block_time_ms,
                    batch_window_ms,
                    heartbeat_secs,
                } => serde_json::json!({
                    "kind": "set_block_production",
                    "config": {
                        "mode": mode.as_str(),
                        "block_time_ms": block_time_ms,
                        "batch_window_ms": batch_window_ms,
                        "heartbeat_secs": heartbeat_secs,
                    },
                }),
            };
            let now = chrono::Utc::now().timestamp();
            let period = (voting_days * 86_400.0).round() as i64;
            let body = serde_json::json!({
                "version": 1,
                "network": network,
                "action": action,
                "title": title,
                "description": description,
                "proposer_did": wallet.did,
                "electorate_hash": electorate,
                "created_at": now,
                "expires_at": now + period,
            });
            let body_json = serde_json::to_string(&body)?;
            let signature_hex = sign(
                &wallet,
                format!("{PROPOSAL_DOMAIN}\n{body_json}").as_bytes(),
            )?;
            let signed = serde_json::json!({ "body_json": body_json, "signature_hex": signature_hex });
            if *print_only {
                println!("{}", serde_json::to_string_pretty(&signed)?);
                return Ok(());
            }
            let res = post_json(&format!("{node}/v1/governance/proposals"), &signed).await?;
            let id = res["id"].as_str().unwrap_or("");
            println!("✅ Proposal {} ({})", id.cyan(), res["status"].as_str().unwrap_or(""));
            println!(
                "   Vote with: {}",
                format!("spacekit governance vote {id} approve").yellow()
            );
        }
        GovernanceCommands::Vote { proposal_id, choice } => {
            let wallet = load_wallet(args.wallet.as_ref())?;
            let g = get_json(&format!("{node}/v1/governance")).await?;
            let network = g["network"].as_str().ok_or("node did not report its network")?;
            let signature_hex = sign(
                &wallet,
                format!("{VOTE_DOMAIN}\n{network}\n{proposal_id}\n{}", choice.as_str()).as_bytes(),
            )?;
            let res = post_json(
                &format!("{node}/v1/governance/votes"),
                &serde_json::json!({
                    "proposal_id": proposal_id,
                    "voter_did": wallet.did,
                    "choice": choice.as_str(),
                    "signature_hex": signature_hex,
                }),
            )
            .await?;
            println!("✅ Vote {}", res["status"].as_str().unwrap_or("recorded"));
            if res["proposal"].is_object() {
                print_proposal_line(&res["proposal"]);
            }
        }
        GovernanceCommands::SignProposal { body_json } => {
            let wallet = load_wallet(args.wallet.as_ref())?;
            let body_json = match body_json.strip_prefix('@') {
                Some(path) => std::fs::read_to_string(path)?,
                None => body_json.clone(),
            };
            // Sign exactly what the website will submit: no trailing newline.
            let body_json = body_json.trim_end_matches(['\n', '\r']);
            let parsed: serde_json::Value = serde_json::from_str(body_json)
                .map_err(|e| format!("body is not JSON: {e}"))?;
            if parsed["proposer_did"].as_str() != Some(wallet.did.as_str()) {
                return Err(format!(
                    "proposer_did is {}, but this wallet is {}",
                    parsed["proposer_did"], wallet.did
                )
                .into());
            }
            let sig = sign(&wallet, format!("{PROPOSAL_DOMAIN}\n{body_json}").as_bytes())?;
            println!("{sig}");
        }
        GovernanceCommands::Staking => {
            let v = get_json(&format!("{node}/v1/staking")).await?;
            if v["enabled"] != true {
                println!("This network has no on-chain staking (no PoA genesis).");
                return Ok(());
            }
            println!("Mode:           {}", v["mode"].as_str().unwrap_or("").green());
            println!(
                "Minimum stake:  {} ASTRA, unbonding {} days",
                v["params"]["min_stake_astra"],
                v["params"]["unbonding_secs"].as_u64().unwrap_or(0) / 86_400
            );
            let producers = &v["producers"];
            println!(
                "Producers:      {} ({})",
                producers["members"].as_array().map(Vec::len).unwrap_or(0),
                if producers["weighted"] == true {
                    "stake-weighted"
                } else if producers["fallback"] == true {
                    "authorities, no validator has the minimum stake yet"
                } else {
                    "authorities, round robin"
                }
            );
            println!("\nValidators:");
            for val in v["validators"].as_array().cloned().unwrap_or_default() {
                let wei = |k: &str| {
                    val[k].as_str().and_then(|s| s.parse::<u128>().ok()).unwrap_or(0)
                        / 1_000_000_000_000_000_000
                };
                println!(
                    "   {} {}  bonded {} ASTRA, effective {} ASTRA, holds {} ASTRA{}",
                    if val["active"] == true { "●".green() } else { "○".yellow() },
                    val["did"].as_str().unwrap_or(""),
                    wei("bonded_wei"),
                    wei("effective_wei"),
                    wei("holdings_wei"),
                    val["name"].as_str().map(|n| format!(" ({n})")).unwrap_or_default()
                );
            }
        }
        GovernanceCommands::Stake {
            action,
            amount,
            name,
        } => {
            let wallet = load_wallet(args.wallet.as_ref())?;
            let g = get_json(&format!("{node}/v1/governance")).await?;
            let network = g["network"].as_str().ok_or("node did not report its network")?;
            let staking = get_json(&format!("{node}/v1/staking")).await?;
            let nonce = staking["validators"]
                .as_array()
                .and_then(|vs| vs.iter().find(|v| v["did"].as_str() == Some(wallet.did.as_str())))
                .and_then(|v| v["next_nonce"].as_u64())
                .unwrap_or(0);
            let amount_wei = astra_to_wei(amount)?;
            let mut body = serde_json::json!({
                "version": 1,
                "network": network,
                "did": wallet.did,
                "sphincs_pk_hex": wallet.pk_hex,
                "action": match action {
                    StakeActionArg::Bond => "bond",
                    StakeActionArg::Unbond => "unbond",
                },
                "amount_wei": amount_wei.to_string(),
                "nonce": nonce,
            });
            if let Some(name) = name {
                body["name"] = serde_json::Value::String(name.clone());
            }
            let body_json = serde_json::to_string(&body)?;
            let signature_hex = sign(&wallet, format!("SPACEKIT-STAKE-v1\n{body_json}").as_bytes())?;
            let res = post_json(
                &format!("{node}/v1/staking"),
                &serde_json::json!({ "body_json": body_json, "signature_hex": signature_hex }),
            )
            .await?;
            println!(
                "✅ Stake message {} ({}); it takes effect in the next block",
                res["status"].as_str().unwrap_or(""),
                res["tx_hash"].as_str().unwrap_or("")
            );
        }
        GovernanceCommands::SignVote {
            proposal_id,
            choice,
            network,
        } => {
            let wallet = load_wallet(args.wallet.as_ref())?;
            let network = match network {
                Some(n) => n.clone(),
                None => get_json(&format!("{node}/v1/governance")).await?["network"]
                    .as_str()
                    .ok_or("node did not report its network")?
                    .to_string(),
            };
            let sig = sign(
                &wallet,
                format!("{VOTE_DOMAIN}\n{network}\n{proposal_id}\n{}", choice.as_str()).as_bytes(),
            )?;
            eprintln!("voter_did: {}", wallet.did);
            println!("{sig}");
        }
    }
    Ok(())
}
