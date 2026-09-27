//! `spacekit network test --suite poa`: a proof-of-authority devnet, end to end.
//!
//! Four genesis authorities and one observer run as real compute sidecars
//! (`spacekit network devnet`). The gates cover what a testnet needs before
//! it takes outside operators:
//!
//! 1. authorities produce sealed blocks in turn and every node agrees on the head;
//! 2. balances come from the genesis allocation and the faucet is off;
//! 3. two different signers see the same contract storage;
//! 4. a node started later catches up on blocks, seals and governance, and
//!    cannot produce blocks while it is not an authority;
//! 5. `lift_poa` is refused below `min_validators_to_lift`;
//! 6. an `add_authority` proposal admits the observer, which then produces;
//! 7. service rewards settle as system transactions, locked during PoA
//!    (skipped without an AstraRewards WASM);
//! 8. `lift_poa` passes, every node switches to proof of stake, blocks
//!    keep coming, and AstraRewards leaves its PoA phase.

use super::*;
use crate::network_devnet::{self, DevnetLayout, DevnetNode, DevnetOptions};
use std::collections::BTreeSet;

const AUTHORITIES: usize = 4;
const MIN_TO_LIFT: usize = 5;
const FUNDS: u128 = 10_000_000_000;
const EPOCH_SECS: u64 = 15;
/// On-demand production, tuned so the suite runs in minutes.
const BLOCK_TIME_MS: u64 = 1_000;
const BATCH_WINDOW_MS: u64 = 300;
const HEARTBEAT_SECS: u64 = 3;

struct Signer {
    key: k256::ecdsa::SigningKey,
    address: String,
    nonce: u64,
}

impl Signer {
    fn random() -> Self {
        use k256::elliptic_curve::sec1::ToEncodedPoint;
        use sha3::{Digest as _, Keccak256};
        let key = k256::ecdsa::SigningKey::random(&mut rand::thread_rng());
        let point = key.verifying_key().to_encoded_point(false);
        let digest: [u8; 32] = Keccak256::digest(&point.as_bytes()[1..]).into();
        Self {
            key,
            address: format!("0x{}", hex::encode(&digest[12..])),
            nonce: 0,
        }
    }
}

fn astra_rewards_wasm() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SPACEKIT_ASTRA_REWARDS_WASM") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    [
        "sdks/spacekit-standard-library/target/wasm32-unknown-unknown/release/astra_rewards.wasm",
        "target/wasm32-unknown-unknown/release/astra_rewards.wasm",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

pub(super) async fn run_poa(context: &Context) -> SuiteReport {
    let mut gates = Vec::new();
    let client = &context.client;
    let mut alice = Signer::random();
    let mut bob = Signer::random();
    let wasm = astra_rewards_wasm();

    let started = Instant::now();
    let layout = (|| -> Result<DevnetLayout, String> {
        let ports = free_ports((AUTHORITIES + 1) * 12)?;
        network_devnet::create(&DevnetOptions {
            dir: context.root.join("devnet"),
            authorities: AUTHORITIES,
            observers: 1,
            min_validators_to_lift: Some(MIN_TO_LIFT),
            block_time_ms: BLOCK_TIME_MS,
            production: "on_demand".into(),
            batch_window_ms: BATCH_WINDOW_MS,
            heartbeat_secs: HEARTBEAT_SECS,
            // Tiny stake and no grace period, so the lift gate can see
            // validators take over (or the authority fallback without them).
            min_stake_astra: 1,
            unbonding_days: 1,
            pos_grace_days: 0,
            ports,
            base_port: 0,
            fund: vec![
                (alice.address.clone(), FUNDS),
                (bob.address.clone(), FUNDS),
            ],
            rewards: wasm.is_some(),
            reward_epoch_secs: EPOCH_SECS,
            astra_rewards_wasm: wasm.clone(),
            // Alice's gas credits go to her address; marking it affiliated
            // makes them PoA-locked, like an authority's.
            affiliated: vec![format!(
                "did:spacekit:devnet:{}",
                alice.address.trim_start_matches("0x")
            )],
            force: true,
        })
    })();
    let _ = std::fs::copy(
        context.root.join("devnet/devnet.json"),
        context.artifacts.join("poa-devnet.json"),
    );
    gates.push(gate(
        "poa devnet config",
        started,
        layout.as_ref().map(|l| {
            format!(
                "{} authorities + 1 observer, SPHINCS+ wallets, genesis, alloc, manifest; rewards {}",
                AUTHORITIES,
                if wasm.is_some() { "on" } else { "off (no AstraRewards WASM)" }
            )
        }).map_err(Clone::clone),
    ));
    let Ok(layout) = layout else {
        return SuiteReport { suite: "poa".into(), gates };
    };
    let authorities: Vec<&DevnetNode> = layout.nodes.iter().filter(|n| n.authority).collect();
    let observer = layout.nodes.iter().find(|n| !n.authority).expect("observer");
    let auth_urls: Vec<String> = authorities.iter().map(|n| n.compute_url.clone()).collect();
    let all_urls: Vec<String> = layout.nodes.iter().map(|n| n.compute_url.clone()).collect();
    let _stop = StopDevnet(context, &layout);

    let started = Instant::now();
    let live = start_nodes(context, &authorities).await;
    gates.push(gate("four authorities start", started, live.clone().map(|_| {
        "four compute sidecars healthy with PoA genesis and authority keys".into()
    })));
    if live.is_err() {
        return SuiteReport { suite: "poa".into(), gates };
    }

    let started = Instant::now();
    gates.push(gate(
        "sealed round-robin production",
        started,
        round_robin(client, &auth_urls, &authorities, Duration::from_secs(90)).await,
    ));

    let started = Instant::now();
    gates.push(gate(
        "idle chain makes only heartbeat blocks",
        started,
        idle_heartbeats(client, &auth_urls[0]).await,
    ));

    let started = Instant::now();
    gates.push(gate(
        "genesis alloc and faucet off",
        started,
        genesis_funds(client, &auth_urls, &alice.address).await,
    ));

    let started = Instant::now();
    let contract = shared_contract_state(client, &auth_urls, &mut alice, &mut bob).await;
    gates.push(gate(
        "contract storage shared across signers",
        started,
        contract.as_ref().map(|(detail, _)| detail.clone()).map_err(Clone::clone),
    ));
    let contract = contract.ok().map(|(_, c)| c);

    let started = Instant::now();
    let late = late_joiner(context, client, observer, &auth_urls, &mut alice, contract.as_deref()).await;
    gates.push(gate("late joiner catches up, cannot produce", started, late.clone()));

    let started = Instant::now();
    gates.push(gate(
        "lift refused below minimum",
        started,
        lift_refused(context, client, authorities[0]).await,
    ));

    let started = Instant::now();
    let admitted = if late.is_ok() {
        admit_observer(context, client, &authorities, observer, &all_urls).await
    } else {
        Err("observer is not running".into())
    };
    gates.push(gate("governance admits observer", started, admitted.clone()));

    let started = Instant::now();
    if wasm.is_none() {
        gates.push(skip(
            "rewards settle locked during PoA",
            "no AstraRewards WASM (build sdks/spacekit-standard-library or set SPACEKIT_ASTRA_REWARDS_WASM)",
        ));
    } else {
        gates.push(gate(
            "rewards settle locked during PoA",
            started,
            locked_rewards(client, &auth_urls, &alice.address).await,
        ));
    }

    // Validators stake what they earned producing blocks (locked PoA
    // rewards count). Without the AstraRewards WASM nobody earns anything.
    let started = Instant::now();
    let stakers: Vec<&DevnetNode> = if admitted.is_ok() {
        layout.nodes.iter().collect()
    } else {
        authorities.clone()
    };
    let staked = if wasm.is_none() {
        gates.push(skip(
            "validators stake earned ASTRA",
            "no AstraRewards WASM: validators have no earnings to stake",
        ));
        false
    } else {
        let result = stake_earnings(context, client, &stakers, &all_urls).await;
        let ok = result.is_ok();
        gates.push(gate("validators stake earned ASTRA", started, result));
        ok
    };

    let started = Instant::now();
    let voters: Vec<&DevnetNode> = if admitted.is_ok() {
        layout.nodes.iter().collect()
    } else {
        authorities.clone()
    };
    let lift_urls = if admitted.is_ok() { all_urls.clone() } else { auth_urls.clone() };
    gates.push(gate(
        "lift_poa switches to proof of stake",
        started,
        if admitted.is_ok() {
            lift_poa(context, client, &voters, &lift_urls, wasm.is_some(), staked).await
        } else {
            Err("needs the observer admitted first (min_validators_to_lift = 5)".into())
        },
    ));

    let started = Instant::now();
    if staked {
        gates.push(gate(
            "stake-weighted governance after the lift",
            started,
            pos_governance(context, client, &stakers, &lift_urls).await,
        ));
    } else {
        gates.push(skip(
            "stake-weighted governance after the lift",
            "needs staked validators (AstraRewards WASM)",
        ));
    }

    SuiteReport { suite: "poa".into(), gates }
}

struct StopDevnet<'a>(&'a Context, &'a DevnetLayout);

impl Drop for StopDevnet<'_> {
    fn drop(&mut self) {
        for node in &self.1.nodes {
            network_devnet::stop_node(&self.0.exe, node);
            // Keep the node logs, profiles and seal stores with the report,
            // but not the wallets.
            let target = self.0.artifacts.join(format!("poa-node-{}", node.index));
            let _ = archive_runtime_artifacts(&node.root, &target);
            let _ = std::fs::remove_file(target.join("wallet.json"));
            let _ = std::fs::remove_file(target.join("home/.spacekit/did_wallet.json"));
            let seals = node.root.join("compute/block_seals.jsonl");
            if seals.is_file() {
                let _ = std::fs::create_dir_all(target.join("compute"));
                let _ = std::fs::copy(&seals, target.join("compute/block_seals.jsonl"));
            }
        }
    }
}

async fn start_nodes(context: &Context, nodes: &[&DevnetNode]) -> Result<(), String> {
    for node in nodes {
        network_devnet::start_node(&context.exe, node)?;
    }
    let health: Vec<String> = nodes
        .iter()
        .map(|n| format!("{}/health", n.compute_url.trim_end_matches('/')))
        .collect();
    wait_for_json(&context.client, &health, Duration::from_secs(90)).await?;
    Ok(())
}

async fn chain_status(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    get_json(client, &format!("{}/v1/chain/status", url.trim_end_matches('/'))).await
}

async fn governance(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    get_json(client, &format!("{}/v1/governance", url.trim_end_matches('/'))).await
}

fn head(status: &Value) -> (u64, String) {
    (
        status.pointer("/head/number").and_then(Value::as_u64).unwrap_or(0),
        status.pointer("/head/hash").and_then(Value::as_str).unwrap_or_default().to_string(),
    )
}

/// Wait until every node reports the same head at or above `min_height`.
async fn converge(
    client: &reqwest::Client,
    urls: &[String],
    min_height: u64,
    timeout: Duration,
) -> Result<(u64, String), String> {
    let started = Instant::now();
    let mut last = Vec::new();
    while started.elapsed() < timeout {
        let mut heads = Vec::new();
        for url in urls {
            match chain_status(client, url).await {
                Ok(status) => heads.push(head(&status)),
                Err(e) => {
                    last = vec![(0, e)];
                    break;
                }
            }
        }
        if heads.len() == urls.len()
            && heads[0].0 >= min_height
            && heads.windows(2).all(|p| p[0] == p[1])
        {
            return Ok(heads.swap_remove(0));
        }
        if !heads.is_empty() {
            last = heads;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(format!("no common head at height >= {min_height}; last {last:?}"))
}

async fn round_robin(
    client: &reqwest::Client,
    urls: &[String],
    authorities: &[&DevnetNode],
    timeout: Duration,
) -> Result<String, String> {
    let expected: BTreeSet<String> = authorities.iter().map(|n| n.did.clone()).collect();
    // Three full rounds.
    let target = 3 * expected.len() as u64;
    let (height, hash) = converge(client, urls, target, timeout).await?;
    let mut seen = BTreeSet::new();
    let mut by_producer = std::collections::BTreeMap::<String, u64>::new();
    for number in 1..=height {
        // Ask a different node each time: every node must hold every seal.
        let url = &urls[number as usize % urls.len()];
        let seal = get_json(
            client,
            &format!("{}/v1/chain/seals/{number}", url.trim_end_matches('/')),
        )
        .await?;
        let proposer = seal["proposer_did"]
            .as_str()
            .ok_or_else(|| format!("{url} has no seal for block {number}: {seal}"))?;
        if !expected.contains(proposer) {
            return Err(format!("block {number} sealed by non-authority {proposer}"));
        }
        seen.insert(proposer.to_string());
        *by_producer.entry(proposer.rsplit(':').next().unwrap_or(proposer)[..8].to_string()).or_default() += 1;
    }
    if seen != expected {
        return Err(format!(
            "only {}/{} authorities produced blocks 1..={height}: {by_producer:?}",
            seen.len(),
            expected.len()
        ));
    }
    Ok(format!(
        "{} nodes agree on {height} {hash}; every block sealed by an authority, blocks per producer {by_producer:?}",
        urls.len()
    ))
}

async fn genesis_funds(
    client: &reqwest::Client,
    urls: &[String],
    address: &str,
) -> Result<String, String> {
    for url in urls {
        let account = get_json(
            client,
            &format!("{}/account/{}", url.trim_end_matches('/'), address.trim_start_matches("0x")),
        )
        .await?;
        let balance = match &account["balance"] {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if balance != FUNDS.to_string() {
            return Err(format!("{url}: {address} balance {balance}, expected {FUNDS}"));
        }
    }
    let response = client
        .post(format!("{}/faucet", urls[0].trim_end_matches('/')))
        .json(&json!({"did": "did:spacekit:e2e:poa-faucet", "address": address, "amount": 1}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    if body["success"] == true {
        return Err("faucet credited an account on a PoA network".into());
    }
    Ok(format!(
        "{} nodes hold the genesis balance; faucet refused: {}",
        urls.len(),
        body["error"].as_str().unwrap_or("refused")
    ))
}

/// Submit a signed transaction to `url` and wait for its receipt (the
/// node's own block producer includes it on its next turn).
async fn submit_tx(
    client: &reqwest::Client,
    url: &str,
    signer: &mut Signer,
    to: Option<&str>,
    data_hex: &str,
) -> Result<Value, String> {
    let signature = sign_swtchvm_http_tx(
        &signer.key,
        network_devnet::DEVNET_CHAIN_ID,
        &signer.address,
        to,
        0,
        signer.nonce,
        1_000_000,
        1,
        data_hex,
    )?;
    let (path, body) = match to {
        None => (
            "/contract/deploy",
            json!({
                "from": signer.address, "wasm_hex": data_hex, "gas_limit": "1000000",
                "gas_price": "1", "value": "0", "nonce": signer.nonce, "signature": signature,
            }),
        ),
        Some(contract) => (
            "/contract/call",
            json!({
                "from": signer.address, "contract": contract, "data_hex": data_hex,
                "gas_limit": "1000000", "gas_price": "1", "value": "0",
                "nonce": signer.nonce, "signature": signature,
            }),
        ),
    };
    let response = client
        .post(format!("{}{}", url.trim_end_matches('/'), path))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let reply: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{path} returned {status}: {reply}"));
    }
    signer.nonce += 1;
    let hash = reply["tx_hash"]
        .as_str()
        .ok_or_else(|| format!("{path} response missing tx_hash: {reply}"))?
        .trim_start_matches("0x")
        .to_string();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(45) {
        if let Ok(receipt) =
            get_json(client, &format!("{}/receipt/{hash}", url.trim_end_matches('/'))).await
        {
            if receipt["success"] != true {
                return Err(format!("{path} receipt failed: {receipt}"));
            }
            return Ok(receipt);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!("{path} tx {hash} was not included within 45 s"))
}

/// A counter contract: each call adds one to the value stored under "n" and
/// logs `counter=<value>`.
fn counter_wasm() -> Result<Vec<u8>, String> {
    wat::parse_str(
        r#"(module
            (import "env" "storage_read" (func $read (param i32 i32 i32 i32) (result i32)))
            (import "env" "storage_write" (func $write (param i32 i32 i32 i32) (result i32)))
            (import "env" "log" (func $log (param i32 i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "n")
            (data (i32.const 16) "counter=")
            (func (export "main") (param i32 i32) (result i32)
                (if (i32.lt_s (call $read (i32.const 0) (i32.const 1) (i32.const 24) (i32.const 1))
                              (i32.const 1))
                    (then (i32.store8 (i32.const 24) (i32.const 0))))
                (i32.store8 (i32.const 24) (i32.add (i32.load8_u (i32.const 24)) (i32.const 1)))
                (drop (call $write (i32.const 0) (i32.const 1) (i32.const 24) (i32.const 1)))
                (call $log (i32.const 16) (i32.const 9))
                (i32.const 0))
        )"#,
    )
    .map_err(|e| e.to_string())
}

fn logged_counter(receipt: &Value) -> Option<u8> {
    receipt["logs"].as_array()?.iter().find_map(|log| {
        let data: Vec<u8> = log["data"]
            .as_array()?
            .iter()
            .map(|b| b.as_u64().and_then(|v| u8::try_from(v).ok()))
            .collect::<Option<_>>()?;
        (data.len() == 9 && data.starts_with(b"counter=")).then(|| data[8])
    })
}

async fn shared_contract_state(
    client: &reqwest::Client,
    urls: &[String],
    alice: &mut Signer,
    bob: &mut Signer,
) -> Result<(String, String), String> {
    let wasm = hex::encode(counter_wasm()?);
    let deploy = submit_tx(client, &urls[0], alice, None, &wasm).await?;
    let contract = json_address(&deploy["created_address"])
        .ok_or_else(|| format!("deploy receipt missing created_address: {deploy}"))?;
    // Each call goes to a different node: transactions are relayed to
    // whichever authority's turn it is.
    let t = Instant::now();
    let first = submit_tx(client, &urls[1], alice, Some(&contract), "").await?;
    let first_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let second = submit_tx(client, &urls[2], bob, Some(&contract), "").await?;
    let second_ms = t.elapsed().as_millis();
    let (a, b) = (logged_counter(&first), logged_counter(&second));
    let (Some(a), Some(b)) = (a, b) else {
        return Err(format!("counter logs missing: alice {first}, bob {second}"));
    };
    if b != a + 1 {
        return Err(format!(
            "bob's call saw a separate copy of the contract state (alice counter={a}, bob counter={b})"
        ));
    }
    // Every node re-executed the same blocks.
    let height = second["block_number"].as_u64().unwrap_or(0);
    let (h, hash) = converge(client, urls, height, Duration::from_secs(30)).await?;
    let limit = u128::from(BLOCK_TIME_MS + BATCH_WINDOW_MS) + 3 * 3_000 + 2_000;
    if first_ms > limit || second_ms > limit {
        return Err(format!(
            "transactions took {first_ms} ms and {second_ms} ms to be included (limit {limit} ms)"
        ));
    }
    Ok((
        format!(
            "alice via node 1 counter={a} ({first_ms} ms), bob via node 2 counter={b} ({second_ms} ms) on {contract}; {} nodes agree at {h} {hash}",
            urls.len()
        ),
        contract,
    ))
}

/// With no transactions, blocks come only from heartbeats and are empty.
async fn idle_heartbeats(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let window = Duration::from_secs(HEARTBEAT_SECS * 4);
    let start = head(&chain_status(client, url).await?).0;
    tokio::time::sleep(window).await;
    let end = head(&chain_status(client, url).await?).0;
    let produced = end.saturating_sub(start);
    // At most one block per heartbeat, plus one for the boundary.
    let max = window.as_secs() / HEARTBEAT_SECS + 1;
    if produced == 0 {
        return Err(format!("no heartbeat block in {} s", window.as_secs()));
    }
    if produced > max {
        return Err(format!("{produced} blocks in {} s of idle time (max {max})", window.as_secs()));
    }
    let mut last_ts = None;
    for number in start..=end {
        let block = get_json(client, &format!("{}/block/{number}", url.trim_end_matches('/'))).await?;
        let txs = block["transactions"].as_array().map(Vec::len).unwrap_or(0);
        if number > start && txs != 0 {
            return Err(format!("idle block {number} carries {txs} transactions"));
        }
        let ts = block["timestamp"].as_u64().unwrap_or(0);
        if let Some(prev) = last_ts {
            // Timestamps are whole seconds.
            if ts + 1 < prev + HEARTBEAT_SECS {
                return Err(format!("block {number} came {} s after its parent (heartbeat {HEARTBEAT_SECS} s)", ts.saturating_sub(prev)));
            }
        }
        last_ts = Some(ts);
    }
    Ok(format!(
        "{produced} empty blocks in {} s idle (heartbeat {HEARTBEAT_SECS} s), none in between",
        window.as_secs()
    ))
}

async fn late_joiner(
    context: &Context,
    client: &reqwest::Client,
    observer: &DevnetNode,
    urls: &[String],
    alice: &mut Signer,
    contract: Option<&str>,
) -> Result<String, String> {
    start_nodes(context, &[observer]).await?;
    let mut all = urls.to_vec();
    all.push(observer.compute_url.clone());
    let reference = chain_status(client, &urls[0]).await?;
    let (target, _) = head(&reference);
    let (height, hash) = converge(client, &all, target, Duration::from_secs(90)).await?;
    let gov_ref = governance(client, &urls[0]).await?;
    let gov_obs = governance(client, &observer.compute_url).await?;
    if gov_ref["state_hash"] != gov_obs["state_hash"] {
        return Err(format!(
            "governance differs: {} vs observer {}",
            gov_ref["state_hash"], gov_obs["state_hash"]
        ));
    }
    let status = chain_status(client, &observer.compute_url).await?;
    let seals = status.pointer("/sealing/seals_stored").and_then(Value::as_u64).unwrap_or(0);
    if seals == 0 {
        return Err("observer stored no block seals".into());
    }
    let mine = client
        .post(format!("{}/mine", observer.compute_url.trim_end_matches('/')))
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if mine.status().is_success() {
        return Err("non-authority observer produced a block via POST /mine".into());
    }
    // A transaction sent to a node that cannot produce still gets in.
    let relayed = match contract {
        Some(contract) => {
            let t = Instant::now();
            let receipt = submit_tx(client, &observer.compute_url, alice, Some(contract), "").await?;
            format!(
                "; call sent to the observer included in block {} after {} ms",
                receipt["block_number"],
                t.elapsed().as_millis()
            )
        }
        None => String::new(),
    };
    Ok(format!(
        "observer synced to {height} {hash}, {seals} seals, same governance state; POST /mine refused ({}){relayed}",
        mine.status()
    ))
}

fn governance_cli(
    context: &Context,
    label: &str,
    node: &DevnetNode,
    target_url: &str,
    args: &[&str],
) -> Result<Output, String> {
    let wallet = node.wallet.display().to_string();
    let mut full = vec!["governance", "--node", target_url, "--wallet", wallet.as_str()];
    full.extend_from_slice(args);
    context.run_cli(label, &full)
}

async fn pending_proposal(client: &reqwest::Client, url: &str, kind: &str) -> Result<String, String> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        let list = get_json(
            client,
            &format!("{}/v1/governance/proposals?status=pending", url.trim_end_matches('/')),
        )
        .await?;
        if let Some(id) = list["proposals"].as_array().and_then(|ps| {
            ps.iter()
                .find(|p| p.pointer("/action/kind").and_then(Value::as_str) == Some(kind))
                .and_then(|p| p["id"].as_str().map(str::to_owned))
        }) {
            return Ok(id);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!("no pending {kind} proposal on {url}"))
}

/// Wait until every node's governance satisfies `check`, with one state hash.
async fn governance_converges(
    client: &reqwest::Client,
    urls: &[String],
    what: &str,
    check: impl Fn(&Value) -> bool,
) -> Result<String, String> {
    let started = Instant::now();
    let mut last = String::new();
    while started.elapsed() < Duration::from_secs(60) {
        let mut hashes = BTreeSet::new();
        let mut ok = true;
        for url in urls {
            match governance(client, url).await {
                Ok(g) if check(&g) => {
                    hashes.insert(g["state_hash"].as_str().unwrap_or_default().to_string());
                }
                Ok(g) => {
                    ok = false;
                    last = format!("{url}: mode {} authorities {}", g["mode"], g["authority_count"]);
                }
                Err(e) => {
                    ok = false;
                    last = e;
                }
            }
        }
        if ok && hashes.len() == 1 {
            return Ok(hashes.into_iter().next().unwrap_or_default());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Err(format!("{what} did not converge: {last}"))
}

async fn lift_refused(
    context: &Context,
    client: &reqwest::Client,
    proposer: &DevnetNode,
) -> Result<String, String> {
    let out = governance_cli(
        context,
        "poa-lift-refused",
        proposer,
        &proposer.compute_url,
        &["propose", "--title", "Lift too early", "lift-poa"],
    )?;
    if out.status.success() {
        return Err("lift_poa was accepted with 4 authorities (minimum 5)".into());
    }
    let g = governance(client, &proposer.compute_url).await?;
    if g["mode"] != "proof_of_authority" {
        return Err(format!("mode changed to {}", g["mode"]));
    }
    Ok(format!(
        "refused with {} of {} validators: {}",
        g["authority_count"],
        g["min_validators_to_lift"],
        String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("").trim()
    ))
}

async fn admit_observer(
    context: &Context,
    client: &reqwest::Client,
    authorities: &[&DevnetNode],
    observer: &DevnetNode,
    all_urls: &[String],
) -> Result<String, String> {
    let proposer = authorities[0];
    let out = governance_cli(
        context,
        "poa-propose-add",
        proposer,
        &proposer.compute_url,
        &[
            "propose", "--title", "Admit observer", "add-authority",
            "--did", &observer.did,
            "--sphincs-pk-hex", &observer.sphincs_pk_hex,
            "--name", "Devnet observer",
        ],
    )?;
    if !out.status.success() {
        return Err(format!("propose failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let id = pending_proposal(client, &proposer.compute_url, "add_authority").await?;
    // ceil(2*4/3) = 3 approvals; each vote goes to a different node and
    // reaches the others by gossip.
    for (i, voter) in authorities.iter().take(3).enumerate() {
        let out = governance_cli(
            context,
            &format!("poa-vote-add-{i}"),
            voter,
            &voter.compute_url,
            &["vote", &id, "approve"],
        )?;
        if !out.status.success() {
            return Err(format!("vote {i} failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    let hash = governance_converges(client, all_urls, "admission", |g| {
        g["authority_count"].as_u64() == Some(authorities.len() as u64 + 1)
    })
    .await?;
    // The new authority takes its turn.
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(45) {
        let status = chain_status(client, &all_urls[0]).await?;
        if status.pointer("/head/proposer_did").and_then(Value::as_str) == Some(observer.did.as_str()) {
            return Ok(format!(
                "proposal {id} executed on all {} nodes (state {hash}); observer sealed block {}",
                all_urls.len(),
                head(&status).0
            ));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err(format!("proposal {id} executed (state {hash}), but the observer never produced a block"))
}

async fn locked_rewards(
    client: &reqwest::Client,
    urls: &[String],
    alice: &str,
) -> Result<String, String> {
    let recipient = format!("{:0>64}", alice.trim_start_matches("0x").to_ascii_lowercase());
    let started = Instant::now();
    let mut last = Value::Null;
    while started.elapsed() < Duration::from_secs(EPOCH_SECS * 4 + 30) {
        let status = chain_status(client, &urls[0]).await?;
        last = status["rewards"].clone();
        let credit = last["recent"].as_array().and_then(|blocks| {
            blocks.iter().find_map(|b| {
                b["credits"].as_array()?.iter().find(|c| {
                    c["recipient"].as_str().map(|r| r.trim_start_matches("0x").to_ascii_lowercase())
                        == Some(recipient.clone())
                }).map(|c| (b["block_number"].as_u64().unwrap_or(0), c.clone()))
            })
        });
        if let Some((block, credit)) = credit {
            if credit["onchain_ok"] != true {
                return Err(format!("credit in block {block} failed on chain: {credit}"));
            }
            if credit["locked"] != true {
                return Err(format!("affiliated credit in block {block} was not locked: {credit}"));
            }
            let (h, hash) = converge(client, urls, block, Duration::from_secs(30)).await?;
            return Ok(format!(
                "block {block}: {} wei locked for alice (locked_wei {}); nodes agree at {h} {hash}",
                credit["amount_wei"], credit["locked_wei"]
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(format!("no settled credit for alice; rewards status {last}"))
}

async fn lift_poa(
    context: &Context,
    client: &reqwest::Client,
    voters: &[&DevnetNode],
    urls: &[String],
    rewards: bool,
    staked: bool,
) -> Result<String, String> {
    let proposer = voters[0];
    let out = governance_cli(
        context,
        "poa-propose-lift",
        proposer,
        &proposer.compute_url,
        &["propose", "--title", "Lift PoA", "lift-poa"],
    )?;
    if !out.status.success() {
        return Err(format!("propose lift failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let id = pending_proposal(client, &proposer.compute_url, "lift_poa").await?;
    let needed = (2 * voters.len()).div_ceil(3);
    for (i, voter) in voters.iter().take(needed).enumerate() {
        let out = governance_cli(
            context,
            &format!("poa-vote-lift-{i}"),
            voter,
            &voter.compute_url,
            &["vote", &id, "approve"],
        )?;
        if !out.status.success() {
            return Err(format!("lift vote {i} failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    let hash = governance_converges(client, urls, "lift", |g| g["mode"] == "proof_of_stake").await?;
    let before = head(&chain_status(client, &urls[0]).await?).0;
    let (after, block_hash) = converge(client, urls, before + 3, Duration::from_secs(45)).await?;
    // With no grace period, staked validators produce now; without stake
    // the authorities keep the chain going as a fallback.
    let status = chain_status(client, &urls[0]).await?;
    let weighted = status.pointer("/sealing/stake_weighted") == Some(&Value::Bool(true));
    let fallback = status.pointer("/sealing/authority_fallback") == Some(&Value::Bool(true));
    if staked && !weighted {
        return Err(format!("staked validators are not producing after the lift: {}", status["sealing"]));
    }
    if !staked && !fallback {
        return Err(format!("no stake, but the authority fallback is off: {}", status["sealing"]));
    }
    let mut detail = format!(
        "proposal {id} passed with {needed} votes; all {} nodes in proof_of_stake (state {hash}); \
         blocks continue {before} -> {after} {block_hash}, produced by {}",
        urls.len(),
        if weighted { "staked validators (stake-weighted)" } else { "the authorities (no stake yet)" }
    );
    if rewards {
        let started = Instant::now();
        loop {
            let status = chain_status(client, &urls[0]).await?;
            let phase = status.pointer("/rewards/contract/phase").and_then(Value::as_str);
            if phase == Some("proof_of_stake") {
                detail.push_str("; AstraRewards END_POA applied");
                break;
            }
            if started.elapsed() > Duration::from_secs(30) {
                return Err(format!("{detail}; AstraRewards still in phase {phase:?}"));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    Ok(detail)
}

/// Each staker bonds 1 ASTRA from what it earned producing blocks.
async fn stake_earnings(
    context: &Context,
    client: &reqwest::Client,
    stakers: &[&DevnetNode],
    urls: &[String],
) -> Result<String, String> {
    for (i, node) in stakers.iter().enumerate() {
        let started = Instant::now();
        loop {
            // Refused until the DID holds 1 ASTRA (credits settle per epoch).
            let out = governance_cli(
                context,
                &format!("poa-stake-{i}"),
                node,
                &node.compute_url,
                &["stake", "bond", "1", "--name", &format!("devnet-{}", node.index)],
            )?;
            if out.status.success() {
                break;
            }
            if started.elapsed() > Duration::from_secs(EPOCH_SECS * 6 + 30) {
                return Err(format!(
                    "node {} could not bond: {}",
                    node.index,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    let started = Instant::now();
    loop {
        let v = get_json(client, &format!("{}/v1/staking", urls[0].trim_end_matches('/'))).await?;
        let active = v["validators"]
            .as_array()
            .map(|vs| vs.iter().filter(|x| x["active"] == true).count())
            .unwrap_or(0);
        if active == stakers.len() {
            return Ok(format!(
                "{active} validators bonded 1 ASTRA each from their (locked) block rewards"
            ));
        }
        if started.elapsed() > Duration::from_secs(60) {
            return Err(format!("{active}/{} validators active: {}", stakers.len(), v["validators"]));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// After the lift a staked validator changes block production, and the
/// others' stake carries the vote.
async fn pos_governance(
    context: &Context,
    client: &reqwest::Client,
    stakers: &[&DevnetNode],
    urls: &[String],
) -> Result<String, String> {
    let proposer = stakers[0];
    let out = governance_cli(
        context,
        "pos-propose",
        proposer,
        &proposer.compute_url,
        &[
            "propose", "--title", "Longer heartbeat", "set-block-production",
            "--mode", "on-demand", "--block-time-ms", "1000",
            "--batch-window-ms", "300", "--heartbeat-secs", "4",
        ],
    )?;
    if !out.status.success() {
        return Err(format!("propose failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let id = pending_proposal(client, &proposer.compute_url, "set_block_production").await?;
    // Equal stake: ceil(2/3) of the voters carries it.
    let needed = (2 * stakers.len()).div_ceil(3);
    for (i, voter) in stakers.iter().take(needed).enumerate() {
        let out = governance_cli(
            context,
            &format!("pos-vote-{i}"),
            voter,
            &voter.compute_url,
            &["vote", &id, "approve"],
        )?;
        if !out.status.success() {
            return Err(format!("vote {i} failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
    }
    let hash = governance_converges(client, urls, "block production change", |g| {
        g["block_production"]["heartbeat_secs"] == 4
    })
    .await?;
    Ok(format!(
        "proposal {id} passed by stake weight ({needed}/{} equal stakes); every node now runs a 4 s heartbeat (state {hash})",
        stakers.len()
    ))
}
