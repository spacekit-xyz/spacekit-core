//! Validator staking, kept in chain state.
//!
//! Validators stake ASTRA they hold in AstraRewards (spendable or locked;
//! locked PoA-phase earnings are stakeable). A stake operation is a message
//! signed with the validator DID's SPHINCS+ key and carried in a block as a
//! consensus transaction (see `chain_consensus`), so every node applies the
//! same operations in the same order.
//!
//! ```text
//! SPACEKIT-STAKE-v1\n{body_json}
//! body: { "version": 1, "network", "did", "sphincs_pk_hex",
//!         "action": "bond" | "unbond", "amount_wei": "…", "nonce": n, "name"? }
//! ```
//!
//! - `bond` adds to the validator's bonded stake. The total bonded plus
//!   unbonding may not exceed what the DID holds in AstraRewards.
//! - `unbond` moves stake into unbonding; it stops counting immediately and is
//!   released after `unbonding_secs`.
//! - `nonce` must equal the validator's operation count, so a signed message
//!   cannot be replayed.
//!
//! A validator's effective stake is `min(bonded, holdings − unbonding)`: if
//! the DID's AstraRewards holdings fall, its weight falls with them. A
//! validator is active when its effective stake reaches `min_stake_astra`.
//! There is no slashing yet.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const STAKE_DOMAIN: &str = "SPACEKIT-STAKE-v1";
pub const WEI_PER_ASTRA: u128 = 1_000_000_000_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StakingParams {
    /// Minimum effective stake for an active validator, in whole ASTRA.
    pub min_stake_astra: u64,
    /// How long unbonded stake stays locked.
    pub unbonding_secs: u64,
}

impl Default for StakingParams {
    fn default() -> Self {
        Self {
            min_stake_astra: 10_000,
            unbonding_secs: 21 * 86_400,
        }
    }
}

impl StakingParams {
    pub fn min_stake_wei(&self) -> u128 {
        self.min_stake_astra as u128 * WEI_PER_ASTRA
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unbonding {
    #[serde(with = "crate::serde_u128")]
    pub amount_wei: u128,
    pub release_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validator {
    pub did: String,
    pub sphincs_pk_hex: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(with = "crate::serde_u128")]
    pub bonded_wei: u128,
    #[serde(default)]
    pub unbonding: Vec<Unbonding>,
    /// Operations applied so far (the next message must carry this nonce).
    pub nonce: u64,
    pub registered_at: u64,
}

impl Validator {
    pub fn unbonding_wei(&self) -> u128 {
        self.unbonding.iter().map(|u| u.amount_wei).sum()
    }

    /// `min(bonded, holdings − unbonding)`.
    pub fn effective_wei(&self, holdings_wei: u128) -> u128 {
        self.bonded_wei
            .min(holdings_wei.saturating_sub(self.unbonding_wei()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StakingState {
    pub params: StakingParams,
    pub validators: BTreeMap<String, Validator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StakeAction {
    Bond,
    Unbond,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StakeBody {
    pub version: u32,
    pub network: String,
    pub did: String,
    pub sphincs_pk_hex: String,
    pub action: StakeAction,
    pub amount_wei: String,
    pub nonce: u64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedStake {
    pub body_json: String,
    pub signature_hex: String,
}

pub fn stake_signing_payload(body_json: &str) -> Vec<u8> {
    format!("{STAKE_DOMAIN}\n{body_json}").into_bytes()
}

impl StakingState {
    pub fn new(params: StakingParams) -> Self {
        Self {
            params,
            validators: BTreeMap::new(),
        }
    }

    /// Release unbonding stake whose period has ended.
    pub fn mature(&mut self, now: u64) -> bool {
        let mut changed = false;
        for v in self.validators.values_mut() {
            let before = v.unbonding.len();
            v.unbonding.retain(|u| u.release_at > now);
            changed |= v.unbonding.len() != before;
        }
        changed
    }

    /// Validate and apply a signed stake message. `holdings_wei` returns what
    /// a DID holds in AstraRewards (spendable plus unreleased locked).
    pub fn apply(
        &mut self,
        signed: &SignedStake,
        network: &str,
        now: u64,
        verify: &dyn Fn(&[u8], &[u8], &[u8]) -> bool,
        holdings_wei: &dyn Fn(&str) -> u128,
    ) -> Result<String, String> {
        let body = self.check(signed, network, verify)?;
        let amount: u128 = body
            .amount_wei
            .trim()
            .parse()
            .map_err(|_| "amount_wei is not a whole number".to_string())?;
        let mut v = self
            .validators
            .get(&body.did)
            .cloned()
            .unwrap_or_else(|| Validator {
                did: body.did.clone(),
                sphincs_pk_hex: body.sphincs_pk_hex.trim().to_ascii_lowercase(),
                name: body.name.clone(),
                bonded_wei: 0,
                unbonding: Vec::new(),
                nonce: 0,
                registered_at: now,
            });
        match body.action {
            StakeAction::Bond => {
                let holdings = holdings_wei(&body.did);
                let committed = v
                    .bonded_wei
                    .saturating_add(v.unbonding_wei())
                    .saturating_add(amount);
                if committed > holdings {
                    return Err(format!(
                        "cannot bond {amount} wei: {} holds {holdings} wei in AstraRewards and has \
                         {} wei bonded or unbonding",
                        body.did,
                        v.bonded_wei.saturating_add(v.unbonding_wei())
                    ));
                }
                v.bonded_wei += amount;
            }
            StakeAction::Unbond => {
                if amount > v.bonded_wei {
                    return Err(format!(
                        "cannot unbond {amount} wei: only {} wei is bonded",
                        v.bonded_wei
                    ));
                }
                v.bonded_wei -= amount;
                v.unbonding.push(Unbonding {
                    amount_wei: amount,
                    release_at: now + self.params.unbonding_secs,
                });
            }
        }
        if body.name.is_some() {
            v.name = body.name.clone();
        }
        v.nonce += 1;
        let summary = format!(
            "{} {:?} {amount} wei; bonded {} wei",
            body.did, body.action, v.bonded_wei
        );
        self.validators.insert(body.did.clone(), v);
        Ok(summary)
    }

    /// Everything that does not depend on holdings: body, network, key,
    /// signature, nonce.
    pub fn check(
        &self,
        signed: &SignedStake,
        network: &str,
        verify: &dyn Fn(&[u8], &[u8], &[u8]) -> bool,
    ) -> Result<StakeBody, String> {
        let body: StakeBody = serde_json::from_str(&signed.body_json)
            .map_err(|e| format!("body_json is not a valid stake message: {e}"))?;
        if body.version != 1 {
            return Err(format!("unsupported stake version {}", body.version));
        }
        if body.network != network {
            return Err(format!(
                "stake message is for network {:?}, this chain is {:?}",
                body.network, network
            ));
        }
        let amount: u128 = body
            .amount_wei
            .trim()
            .parse()
            .map_err(|_| "amount_wei is not a whole number".to_string())?;
        if amount == 0 {
            return Err("amount_wei must be positive".into());
        }
        if body.name.as_ref().is_some_and(|n| n.len() > 200) {
            return Err("name is too long".into());
        }
        let pk = hex::decode(body.sphincs_pk_hex.trim())
            .map_err(|_| "sphincs_pk_hex is not hex".to_string())?;
        if !crate::validator_governance::did_matches_key(&body.did, &pk) {
            return Err(format!("{} is not derived from sphincs_pk_hex", body.did));
        }
        let expected_nonce = match self.validators.get(&body.did) {
            Some(v) => {
                if !v.sphincs_pk_hex.eq_ignore_ascii_case(body.sphincs_pk_hex.trim()) {
                    return Err("sphincs_pk_hex differs from the registered key".into());
                }
                v.nonce
            }
            None => {
                if body.action == StakeAction::Unbond {
                    return Err(format!("{} has no stake", body.did));
                }
                0
            }
        };
        if body.nonce != expected_nonce {
            return Err(format!(
                "nonce {} is not the next one for {} (expected {expected_nonce})",
                body.nonce, body.did
            ));
        }
        let signature = hex::decode(signed.signature_hex.trim())
            .map_err(|_| "signature_hex is not hex".to_string())?;
        if !verify(&pk, &stake_signing_payload(&signed.body_json), &signature) {
            return Err("stake signature does not verify against the DID's key".into());
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn fake_verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        let mut h = Sha256::new();
        h.update(pk);
        h.update(msg);
        h.finalize().as_slice() == sig
    }

    fn signed(pk: &[u8], did: &str, action: &str, amount: u128, nonce: u64) -> SignedStake {
        let body_json = serde_json::json!({
            "version": 1, "network": "testnet", "did": did,
            "sphincs_pk_hex": hex::encode(pk), "action": action,
            "amount_wei": amount.to_string(), "nonce": nonce,
        })
        .to_string();
        let mut h = Sha256::new();
        h.update(pk);
        h.update(stake_signing_payload(&body_json));
        SignedStake {
            body_json,
            signature_hex: hex::encode(h.finalize()),
        }
    }

    #[test]
    fn bond_unbond_nonce_and_holdings() {
        let pk = vec![7u8; 32];
        let did = format!("did:spacekit:testnet:{}", hex::encode(&Sha256::digest(&pk)[..20]));
        let mut s = StakingState::new(StakingParams::default());
        let holdings = |_: &str| 100 * WEI_PER_ASTRA;

        // Cannot bond more than held.
        let too_much = signed(&pk, &did, "bond", 101 * WEI_PER_ASTRA, 0);
        assert!(s.apply(&too_much, "testnet", 10, &fake_verify, &holdings).is_err());

        s.apply(&signed(&pk, &did, "bond", 60 * WEI_PER_ASTRA, 0), "testnet", 10, &fake_verify, &holdings)
            .unwrap();
        // Replay is refused.
        assert!(s
            .apply(&signed(&pk, &did, "bond", 1, 0), "testnet", 11, &fake_verify, &holdings)
            .is_err());
        s.apply(&signed(&pk, &did, "unbond", 20 * WEI_PER_ASTRA, 1), "testnet", 12, &fake_verify, &holdings)
            .unwrap();
        let v = s.validators[&did].clone();
        assert_eq!(v.bonded_wei, 40 * WEI_PER_ASTRA);
        assert_eq!(v.unbonding_wei(), 20 * WEI_PER_ASTRA);
        // Unbonding stake still counts against holdings for new bonds.
        assert!(s
            .apply(&signed(&pk, &did, "bond", 41 * WEI_PER_ASTRA, 2), "testnet", 13, &fake_verify, &holdings)
            .is_err());
        // Holdings shrinking caps the effective stake.
        assert_eq!(v.effective_wei(50 * WEI_PER_ASTRA), 30 * WEI_PER_ASTRA);
        // Maturity releases unbonding stake.
        assert!(s.mature(12 + StakingParams::default().unbonding_secs));
        assert_eq!(s.validators[&did].unbonding_wei(), 0);
        // Wrong network and bad signature.
        let mut bad = signed(&pk, &did, "bond", 1, 2);
        bad.signature_hex = "00".into();
        assert!(s.apply(&bad, "testnet", 20, &fake_verify, &holdings).is_err());
        assert!(s
            .apply(&signed(&pk, &did, "bond", 1, 2), "mainnet", 20, &fake_verify, &holdings)
            .is_err());
    }
}
