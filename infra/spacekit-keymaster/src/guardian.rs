use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::audit::{append_audit, AuditLog};
use crate::auth::{
    user_sig_message, verify_coordinator_ticket, verify_user_sig, CoordinatorTicketBody,
};
use crate::crypto::{
    aes_gcm_decrypt, aes_gcm_encrypt, b64_decode, b64_encode, derive_session_key, derive_shard_key,
    envelope_aad, hex32_from_bytes,
};
use crate::pq_crypto::{kem_decapsulate, kem_encapsulate};
use crate::rate_limit::RateLimiter;
use crate::types::{AuditDecision, DecryptRequest, DecryptResponse, GuardianInfo, Hex32};

pub struct GuardianState {
    pub info: GuardianInfo,
    kem_decap_sk: Zeroizing<Vec<u8>>,
    coordinator_pk: Vec<u8>,
    rate: RateLimiter,
    retired: RwLock<HashSet<Hex32>>,
    subject_keys: RwLock<HashMap<Hex32, String>>,
    current_keystore: RwLock<HashMap<Hex32, Hex32>>,
    last_recovery: RwLock<HashMap<Hex32, i64>>,
    cooldown_s: RwLock<HashMap<Hex32, u64>>,
    audit: AuditLog,
    audit_sk: Zeroizing<Vec<u8>>,
}

impl GuardianState {
    pub fn new(
        operator: String,
        endpoint: String,
        kem_decap_sk: Zeroizing<Vec<u8>>,
        kem_pk: Vec<u8>,
        coordinator_pk: Vec<u8>,
        audit_sk: Zeroizing<Vec<u8>>,
    ) -> Self {
        let kid = hex32_from_bytes(&Sha256::digest(&kem_pk));
        let info = GuardianInfo {
            kid,
            mlkem_pk: b64_encode(&kem_pk),
            endpoint,
            operator,
            online: true,
        };
        Self {
            info,
            kem_decap_sk,
            coordinator_pk,
            rate: RateLimiter::from_env(),
            retired: RwLock::new(HashSet::new()),
            subject_keys: RwLock::new(HashMap::new()),
            current_keystore: RwLock::new(HashMap::new()),
            last_recovery: RwLock::new(HashMap::new()),
            cooldown_s: RwLock::new(HashMap::new()),
            audit: AuditLog::new(),
            audit_sk,
        }
    }

    pub fn register_subject(&self, subject: Hex32, signer_pk_b64: String, keystore_id: Hex32, cooldown: u64) {
        self.subject_keys.write().insert(subject.clone(), signer_pk_b64);
        self.current_keystore.write().insert(subject.clone(), keystore_id);
        self.cooldown_s.write().insert(subject, cooldown);
    }

    pub fn retire_generation(&self, keystore_id: Hex32) {
        self.retired.write().insert(keystore_id);
    }

    pub fn enroll_subject(
        &self,
        subject: Hex32,
        signer_pk_b64: String,
        keystore_id: Hex32,
        cooldown: u64,
    ) {
        self.register_subject(subject, signer_pk_b64, keystore_id, cooldown);
    }

    pub fn decrypt(&self, req: DecryptRequest) -> Result<DecryptResponse> {
        let now = unix_now();
        let break_glass = req.break_glass;

        let deny = |reason: &str| -> Result<DecryptResponse> {
            append_audit(
                &self.audit,
                &self.audit_sk,
                &req.session_id,
                &self.info.kid,
                req.shard_index,
                AuditDecision::Denied,
                Some(reason),
            );
            Err(anyhow!("guardian {}: {reason}", self.info.operator))
        };

        if self.retired.read().contains(&req.keystore_id) {
            return deny("shard generation retired");
        }

        if let Some(current) = self.current_keystore.read().get(&req.subject) {
            if current != &req.keystore_id {
                return deny("keystore generation not current");
            }
        }

        if !break_glass {
            if let Some(cooldown) = self.cooldown_s.read().get(&req.subject) {
                if let Some(last) = self.last_recovery.read().get(&req.subject) {
                    if now - last < *cooldown as i64 {
                        return deny("recovery cooldown active");
                    }
                }
            }
        }

        if req.envelope.guardian_kid != self.info.kid {
            return deny("envelope not addressed to this guardian");
        }

        let signer_pk = self
            .subject_keys
            .read()
            .get(&req.subject)
            .cloned()
            .ok_or_else(|| anyhow!("subject not enrolled"))?;

        let sig_msg = user_sig_message(
            &req.session_id,
            &req.subject,
            &req.keystore_id,
            req.shard_index,
            &req.client_eph_pk,
        );
        if verify_user_sig(&signer_pk, &sig_msg, &req.auth.user_sig).is_err() {
            return deny("invalid user signature");
        }

        if !break_glass {
            if req.auth.coordinator_ticket.is_empty() {
                return deny("401 missing coordinator ticket");
            }
            let body = CoordinatorTicketBody {
                v: 1,
                session_id: req.session_id.clone(),
                subject: req.subject.clone(),
                guardian_kid: self.info.kid.clone(),
                keystore_id: req.keystore_id.clone(),
                issued_at: 0,
                expires_at: 0,
                nonce: String::new(),
            };
            if verify_coordinator_ticket(
                &req.auth.coordinator_ticket,
                &self.coordinator_pk,
                &body,
                now,
            )
            .is_err()
            {
                return deny("invalid coordinator ticket");
            }
        }

        if !self.rate.allow(&req.subject, now) {
            return deny("429 rate limited");
        }

        let kem_ct = b64_decode(&req.envelope.kem_ct)?;
        let ss = kem_decapsulate(self.kem_decap_sk.as_ref(), &kem_ct)?;
        let k_shard = derive_shard_key(&ss, &req.subject, &req.keystore_id, req.shard_index)?;
        let nonce = b64_decode(&req.envelope.nonce)?;
        let ct = b64_decode(&req.envelope.ct)?;
        let aad = envelope_aad(&req.envelope);
        let shard_bytes = aes_gcm_decrypt(&*k_shard, &nonce, &ct, &aad)
            .map_err(|_| anyhow!("guardian {}: envelope authentication failed", self.info.operator))?;

        let client_pk = b64_decode(&req.client_eph_pk)?;
        let (re_ct, re_ss) = kem_encapsulate(&client_pk)?;
        let k_session = derive_session_key(&re_ss, &req.session_id, req.shard_index)?;
        let session_aad = req.session_id.as_bytes();
        let (re_nonce, re_enc) = aes_gcm_encrypt(&*k_session, &shard_bytes, session_aad)?;

        append_audit(
            &self.audit,
            &self.audit_sk,
            &req.session_id,
            &self.info.kid,
            req.shard_index,
            AuditDecision::Granted,
            None,
        );

        Ok(DecryptResponse {
            reshard_kem_ct: b64_encode(&re_ct),
            reshard_nonce: b64_encode(&re_nonce),
            reshard_ct: b64_encode(&re_enc),
        })
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
