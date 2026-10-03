//! Emission into the one ASTRA ledger: the native account balance.
//!
//! ASTRA exists in exactly one place: `SwtchvmAccount::balance`, in wei
//! (18 decimals), covered by the state root. Rewards are not a second ledger:
//! the SRA's leading system transactions (INIT, CREDIT, CREDIT_LOCKED,
//! END_POA, addressed to `0x…0003`) are executed here, by the node, and mint
//! into native balances. Minting happens nowhere else after genesis, and only
//! inside blocks, so every node computes the same supply.
//!
//! - **INIT** mints the genesis treasury allocation (350M ASTRA) to the
//!   treasury contract's address and starts proof of authority.
//! - **CREDIT** mints to the recipient's address. Recipients are keyed by
//!   address (`[0; 12] ‖ address`); for `did:spacekit:<40 hex>` DIDs that is
//!   the address in the DID. A credit to any other key is refused (nothing is
//!   minted), so no value can land somewhere nobody can spend it.
//! - **CREDIT_LOCKED** (authorities and affiliated operators during PoA)
//!   records a locked amount for the address. It vests from the genesis block
//!   time: nothing before 365 days, linearly to 1,095 days. Every block
//!   releases what has vested into the address's balance.
//! - **END_POA** stops new credits from being locked.
//! - Total emission is capped at 2B ASTRA (locked amounts included).
//!
//! Holders of SPHINCS+ DIDs, whose address has no k256 key, spend with
//! SPHINCS+-signed transfer messages (see `chain_consensus`).

use crate::spacekitvm::genesis_node::system_contracts;
use crate::spacekitvm::{SwtchvmAddress, SwtchvmState};

pub const WEI_PER_ASTRA: u128 = 1_000_000_000_000_000_000;
/// Total emission cap: 2,000,000,000 ASTRA.
pub const HARD_CAP_WEI: u128 = 2_000_000_000 * WEI_PER_ASTRA;
/// Genesis treasury allocation minted by INIT: 350,000,000 ASTRA.
pub const GENESIS_TREASURY_WEI: u128 = 350_000_000 * WEI_PER_ASTRA;
pub const LOCK_CLIFF_SECS: u64 = 365 * 86_400;
pub const LOCK_END_SECS: u64 = 1_095 * 86_400;

const OP_INIT: u8 = 0x01;
const OP_CREDIT: u8 = 0x10;
const OP_CREDIT_LOCKED: u8 = 0x11;
const OP_END_POA: u8 = 0x41;

const KEY_INITIALIZED: &[u8] = b"rewards.initialized";
const KEY_PHASE: &[u8] = b"rewards.phase";
const KEY_GENESIS_TS: &[u8] = b"rewards.genesis_ts";
const KEY_TOTAL_EMITTED: &[u8] = b"rewards.total_emitted";
const KEY_LOCKED_INDEX: &[u8] = b"rewards.locked_index";
const LOCKED_PREFIX: &str = "rewards.locked.";

pub const PHASE_POA: u8 = 0;
pub const PHASE_POS: u8 = 1;

pub fn rewards_address() -> SwtchvmAddress {
    SwtchvmAddress::from_hex(system_contracts::ASTRA_REWARDS).expect("rewards address")
}

pub fn treasury_address() -> SwtchvmAddress {
    SwtchvmAddress::from_hex(system_contracts::TREASURY).expect("treasury address")
}

/// The address a reward key designates: `[0; 12] ‖ address`.
pub fn key_address(key: &[u8; 32]) -> Option<SwtchvmAddress> {
    if key[..12].iter().any(|b| *b != 0) || key[12..].iter().all(|b| *b == 0) {
        return None;
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&key[12..]);
    Some(SwtchvmAddress::new(a))
}

/// The address in `did:spacekit:<40 hex>` (also `0x`-prefixed).
pub fn did_address(did: &str) -> Option<SwtchvmAddress> {
    let suffix = did.trim().rsplit(':').next()?;
    let hex = suffix.strip_prefix("0x").unwrap_or(suffix);
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    SwtchvmAddress::from_hex(hex).ok()
}

/// Reward key for a DID: its address, padded to 32 bytes.
pub fn did_reward_key(did: &str) -> Option<[u8; 32]> {
    did_address(did).map(|a| {
        let mut k = [0u8; 32];
        k[12..].copy_from_slice(a.as_bytes());
        k
    })
}

fn read_u128(state: &SwtchvmState, key: &[u8]) -> u128 {
    state
        .kv_get(&rewards_address(), key)
        .and_then(|v| v.get(..16))
        .and_then(|b| <[u8; 16]>::try_from(b).ok())
        .map(u128::from_le_bytes)
        .unwrap_or(0)
}

fn write(state: &mut SwtchvmState, key: &[u8], value: Vec<u8>) {
    state.kv_insert(rewards_address(), key.to_vec(), value);
}

pub fn is_initialized(state: &SwtchvmState) -> bool {
    state.kv_get(&rewards_address(), KEY_INITIALIZED).is_some()
}

/// Proof of authority until END_POA (and before INIT).
pub fn phase(state: &SwtchvmState) -> u8 {
    state
        .kv_get(&rewards_address(), KEY_PHASE)
        .and_then(|v| v.first().copied())
        .unwrap_or(PHASE_POA)
}

pub fn total_emitted(state: &SwtchvmState) -> u128 {
    read_u128(state, KEY_TOTAL_EMITTED)
}

fn genesis_ts(state: &SwtchvmState) -> u64 {
    state
        .kv_get(&rewards_address(), KEY_GENESIS_TS)
        .and_then(|v| <[u8; 8]>::try_from(v.as_slice()).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

fn locked_key(address: &SwtchvmAddress) -> Vec<u8> {
    format!("{LOCKED_PREFIX}{}", hex::encode(address.as_bytes())).into_bytes()
}

/// `(locked total, released so far)` for an address.
pub fn lock_position(state: &SwtchvmState, address: &SwtchvmAddress) -> (u128, u128) {
    match state.kv_get(&rewards_address(), &locked_key(address)) {
        Some(v) if v.len() == 32 => (
            u128::from_le_bytes(v[..16].try_into().unwrap()),
            u128::from_le_bytes(v[16..].try_into().unwrap()),
        ),
        _ => (0, 0),
    }
}

fn locked_index(state: &SwtchvmState) -> Vec<SwtchvmAddress> {
    state
        .kv_get(&rewards_address(), KEY_LOCKED_INDEX)
        .map(|v| {
            v.chunks_exact(20)
                .map(|c| SwtchvmAddress::new(c.try_into().unwrap()))
                .collect()
        })
        .unwrap_or_default()
}

/// Vested part of `locked` at `now`: 0 before the cliff, linear to the end.
pub fn vested_amount(locked: u128, genesis: u64, now: u64) -> u128 {
    let elapsed = now.saturating_sub(genesis);
    if elapsed < LOCK_CLIFF_SECS {
        return 0;
    }
    if elapsed >= LOCK_END_SECS {
        return locked;
    }
    let since_cliff = (elapsed - LOCK_CLIFF_SECS) as u128;
    let span = (LOCK_END_SECS - LOCK_CLIFF_SECS) as u128;
    locked.saturating_mul(since_cliff) / span
}

/// What an address holds: spendable balance plus locked, not yet released.
pub fn holdings_wei(state: &SwtchvmState, address: &SwtchvmAddress) -> u128 {
    let balance = state.get_account(address).map(|a| a.balance).unwrap_or(0);
    let (locked, released) = lock_position(state, address);
    balance.saturating_add(locked.saturating_sub(released))
}

fn mint(state: &mut SwtchvmState, to: &SwtchvmAddress, amount: u128) -> Result<(), String> {
    let total = total_emitted(state);
    let new_total = total
        .checked_add(amount)
        .filter(|t| *t <= HARD_CAP_WEI)
        .ok_or_else(|| "emission cap reached".to_string())?;
    let account = state.get_account_mut(to);
    account.balance = account.balance.saturating_add(amount);
    write(state, KEY_TOTAL_EMITTED, new_total.to_le_bytes().to_vec());
    Ok(())
}

/// Execute one SRA system call. On error nothing has changed.
pub fn execute(state: &mut SwtchvmState, data: &[u8], block_timestamp: u64) -> Result<(), String> {
    let (&op, rest) = data.split_first().ok_or("empty rewards call")?;
    match op {
        OP_INIT => {
            if is_initialized(state) {
                return Err("already initialized".into());
            }
            mint(state, &treasury_address(), GENESIS_TREASURY_WEI)?;
            write(state, KEY_INITIALIZED, vec![1]);
            write(state, KEY_PHASE, vec![PHASE_POA]);
            write(state, KEY_GENESIS_TS, block_timestamp.to_le_bytes().to_vec());
            Ok(())
        }
        OP_CREDIT | OP_CREDIT_LOCKED => {
            if !is_initialized(state) {
                return Err("not initialized".into());
            }
            if rest.len() < 32 + 16 {
                return Err("malformed credit".into());
            }
            let key: [u8; 32] = rest[..32].try_into().unwrap();
            let amount = u128::from_le_bytes(rest[32..48].try_into().unwrap());
            if amount == 0 {
                return Ok(());
            }
            let address = key_address(&key)
                .ok_or("recipient is not an address; nothing minted".to_string())?;
            if op == OP_CREDIT || phase(state) != PHASE_POA {
                return mint(state, &address, amount);
            }
            let total = total_emitted(state);
            let new_total = total
                .checked_add(amount)
                .filter(|t| *t <= HARD_CAP_WEI)
                .ok_or_else(|| "emission cap reached".to_string())?;
            let (locked, released) = lock_position(state, &address);
            let mut record = locked.saturating_add(amount).to_le_bytes().to_vec();
            record.extend_from_slice(&released.to_le_bytes());
            write(state, &locked_key(&address), record);
            let mut index = locked_index(state);
            if !index.contains(&address) {
                index.push(address);
                write(
                    state,
                    KEY_LOCKED_INDEX,
                    index.iter().flat_map(|a| a.as_bytes().to_vec()).collect(),
                );
            }
            write(state, KEY_TOTAL_EMITTED, new_total.to_le_bytes().to_vec());
            Ok(())
        }
        OP_END_POA => {
            if !is_initialized(state) {
                return Err("not initialized".into());
            }
            if phase(state) == PHASE_POS {
                return Err("proof of authority already ended".into());
            }
            write(state, KEY_PHASE, vec![PHASE_POS]);
            Ok(())
        }
        other => Err(format!("unknown rewards operation 0x{other:02x}")),
    }
}

/// Start of every block: move vested locked amounts into balances.
pub fn release_vested(state: &mut SwtchvmState, now: u64) {
    let index = locked_index(state);
    if index.is_empty() {
        return;
    }
    let genesis = genesis_ts(state);
    for address in index {
        let (locked, released) = lock_position(state, &address);
        let releasable = vested_amount(locked, genesis, now).saturating_sub(released);
        if releasable == 0 {
            continue;
        }
        let account = state.get_account_mut(&address);
        account.balance = account.balance.saturating_add(releasable);
        let mut record = locked.to_le_bytes().to_vec();
        record.extend_from_slice(&(released + releasable).to_le_bytes());
        write(state, &locked_key(&address), record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(a: &SwtchvmAddress) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[12..].copy_from_slice(a.as_bytes());
        k
    }

    fn credit(op: u8, k: [u8; 32], amount: u128) -> Vec<u8> {
        let mut d = vec![op];
        d.extend_from_slice(&k);
        d.extend_from_slice(&amount.to_le_bytes());
        d.extend_from_slice(&[0u8; 32]);
        d
    }

    #[test]
    fn credits_mint_into_native_balances_with_locks_and_a_cap() {
        let mut state = SwtchvmState::new();
        let genesis = 1_000_000;
        let alice = SwtchvmAddress::new([0xa1; 20]);
        let auth = SwtchvmAddress::new([0xb2; 20]);
        assert!(execute(&mut state, &credit(OP_CREDIT, key(&alice), 1), genesis).is_err());
        execute(&mut state, &[OP_INIT, 0], genesis).unwrap();
        assert!(execute(&mut state, &[OP_INIT, 0], genesis).is_err());
        assert_eq!(state.get_account(&treasury_address()).unwrap().balance, GENESIS_TREASURY_WEI);

        execute(&mut state, &credit(OP_CREDIT, key(&alice), 5 * WEI_PER_ASTRA), genesis).unwrap();
        assert_eq!(state.get_account(&alice).unwrap().balance, 5 * WEI_PER_ASTRA);

        // Not an address: refused, nothing minted.
        let before = total_emitted(&state);
        assert!(execute(&mut state, &credit(OP_CREDIT, [7u8; 32], WEI_PER_ASTRA), genesis).is_err());
        assert_eq!(total_emitted(&state), before);

        // Locked during PoA: counted, not spendable, vests after the cliff.
        execute(&mut state, &credit(OP_CREDIT_LOCKED, key(&auth), 100 * WEI_PER_ASTRA), genesis).unwrap();
        assert!(state.get_account(&auth).is_none());
        assert_eq!(holdings_wei(&state, &auth), 100 * WEI_PER_ASTRA);
        release_vested(&mut state, genesis + LOCK_CLIFF_SECS - 1);
        assert!(state.get_account(&auth).is_none());
        let halfway = genesis + (LOCK_CLIFF_SECS + LOCK_END_SECS) / 2;
        release_vested(&mut state, halfway);
        assert_eq!(state.get_account(&auth).unwrap().balance, 50 * WEI_PER_ASTRA);
        release_vested(&mut state, halfway);
        assert_eq!(state.get_account(&auth).unwrap().balance, 50 * WEI_PER_ASTRA);
        release_vested(&mut state, genesis + LOCK_END_SECS);
        assert_eq!(state.get_account(&auth).unwrap().balance, 100 * WEI_PER_ASTRA);
        assert_eq!(holdings_wei(&state, &auth), 100 * WEI_PER_ASTRA);

        // After END_POA, CREDIT_LOCKED mints spendable.
        execute(&mut state, &[OP_END_POA], genesis).unwrap();
        execute(&mut state, &credit(OP_CREDIT_LOCKED, key(&alice), WEI_PER_ASTRA), genesis).unwrap();
        assert_eq!(state.get_account(&alice).unwrap().balance, 6 * WEI_PER_ASTRA);

        // The cap.
        let room = HARD_CAP_WEI - total_emitted(&state);
        assert!(execute(&mut state, &credit(OP_CREDIT, key(&alice), room + 1), genesis).is_err());
        execute(&mut state, &credit(OP_CREDIT, key(&alice), room), genesis).unwrap();
        assert_eq!(total_emitted(&state), HARD_CAP_WEI);

        // Supply equals emission: nothing else exists.
        let supply = state.total_supply();
        assert_eq!(supply, HARD_CAP_WEI);
    }

    #[test]
    fn did_addresses() {
        let a = did_address("did:spacekit:0x00112233445566778899aabbccddeeff00112233").unwrap();
        assert_eq!(a.as_bytes()[1], 0x11);
        assert!(did_address("did:spacekit:user:alice").is_none());
        assert_eq!(key_address(&did_reward_key("did:spacekit:00112233445566778899aabbccddeeff00112233").unwrap()), Some(a));
    }
}
