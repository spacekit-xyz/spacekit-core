use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use parking_lot::RwLock;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::auth::{sign_coordinator_ticket, CoordinatorTicketBody, TICKET_TTL_S};
use crate::manifest_sig::verify_manifest;
use crate::crypto::{b64_encode, hex32_from_bytes};
use crate::payments::PaymentsState;
use crate::pq_crypto::signer_generate;
use crate::registry::RegistryState;
use crate::storage::StorageGateway;
use crate::types::{
    AuditRecord, CoverageStatus, EntitlementStatus, GuardianInfo, Hex32, Manifest, Placement,
    PlacementGrade, ShardHealth, SlaQuote, SlaTier, StartRecoveryResponse, SubjectIdentity,
};

pub struct CoordinatorState {
    pub coordinator_pk_b64: String,
    coordinator_sk: Zeroizing<Vec<u8>>,
    manifests: RwLock<HashMap<Hex32, Manifest>>,
    subjects: RwLock<HashMap<Hex32, SubjectIdentity>>,
    retired: RwLock<HashSet<Hex32>>,
    last_recovery: RwLock<HashMap<Hex32, i64>>,
    objects: RwLock<HashMap<Hex32, Vec<u8>>>,
    audit: RwLock<Vec<AuditRecord>>,
    payments: PaymentsState,
    registry: RegistryState,
    storage: StorageGateway,
    node_pool: Vec<(String, String)>,
}

impl CoordinatorState {
    pub fn new(storage_base: String, registry: RegistryState) -> Result<Self> {
        let (coordinator_sk, coordinator_pk) = signer_generate()?;
        let node_pool = vec![
            ("did:sk:node:aurora".into(), "op-1".into()),
            ("did:sk:node:basalt".into(), "op-2".into()),
            ("did:sk:node:cinder".into(), "op-3".into()),
            ("did:sk:node:delta".into(), "op-4".into()),
            ("did:sk:node:ember".into(), "op-5".into()),
            ("did:sk:node:fjord".into(), "op-6".into()),
        ];
        Ok(Self {
            coordinator_pk_b64: B64.encode(&coordinator_pk),
            coordinator_sk: coordinator_sk.clone(),
            manifests: RwLock::new(HashMap::new()),
            subjects: RwLock::new(HashMap::new()),
            retired: RwLock::new(HashSet::new()),
            last_recovery: RwLock::new(HashMap::new()),
            objects: RwLock::new(HashMap::new()),
            audit: RwLock::new(Vec::new()),
            payments: PaymentsState::new(coordinator_sk),
            registry,
            storage: StorageGateway::new(storage_base),
            node_pool,
        })
    }

    pub fn register_subject(&self, identity: SubjectIdentity) {
        self.subjects.write().insert(identity.subject.clone(), identity);
    }

    pub async fn put_manifest(&self, manifest: Manifest) -> Result<()> {
        self.put_manifest_verified(manifest).await
    }

    pub async fn put_manifest_raw(&self, raw: &str) -> Result<()> {
        let manifest: Manifest =
            serde_json::from_str(raw).map_err(|e| anyhow!("invalid manifest json: {e}"))?;
        self.put_manifest_verified(manifest).await
    }

    async fn put_manifest_verified(&self, manifest: Manifest) -> Result<()> {
        let prev = self.manifests.read().get(&manifest.subject).cloned();
        if let Some(p) = prev {
            if p.keystore_id != manifest.keystore_id {
                self.retired.write().insert(p.keystore_id);
            }
        }
        let identity = self
            .subjects
            .read()
            .get(&manifest.subject)
            .cloned()
            .ok_or_else(|| anyhow!("subject not registered"))?;
        verify_manifest(&manifest, &identity.signer_pk_b64)?;

        for g in self.registry.list() {
            g.register_subject(
                manifest.subject.clone(),
                identity.signer_pk_b64.clone(),
                manifest.keystore_id.clone(),
                manifest.policy.cooldown_s,
            );
        }
        for g in self.registry.list_info() {
            let _ = reqwest::Client::new()
                .post(format!("{}/v1/guardian/admin/enroll", g.endpoint))
                .json(&serde_json::json!({
                    "subject": manifest.subject,
                    "signer_pk_b64": identity.signer_pk_b64,
                    "keystore_id": manifest.keystore_id,
                    "cooldown_s": manifest.policy.cooldown_s,
                }))
                .send()
                .await;
        }

        self.manifests.write().insert(manifest.subject.clone(), manifest);
        Ok(())
    }

    pub fn get_manifest(&self, subject: &str) -> Option<Manifest> {
        self.manifests.read().get(subject).cloned()
    }

    pub fn request_placements(&self, count: usize) -> Result<Vec<Vec<Placement>>> {
        if count > self.node_pool.len() {
            return Err(anyhow!("placement constraints unsatisfiable"));
        }
        let mut picks = Vec::new();
        let mut used = HashSet::new();
        for i in 0..count {
            let (node_did, _op) = &self.node_pool[i];
            let primary = node_did.clone();
            let replica_idx = (i + 1) % self.node_pool.len();
            let replica = self.node_pool[replica_idx].0.clone();
            used.insert(primary.clone());
            used.insert(replica.clone());
            let mut buf = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut buf);
            let oid1 = hex32_from_bytes(&buf);
            rand::thread_rng().fill_bytes(&mut buf);
            let oid2 = hex32_from_bytes(&buf);
            picks.push(vec![
                Placement { node_did: primary, object_id: oid1 },
                Placement { node_did: replica, object_id: oid2 },
            ]);
        }
        Ok(picks)
    }

    pub async fn put_object(&self, bytes: &[u8], placements: &[Placement]) -> Result<()> {
        for p in placements {
            self.objects.write().insert(p.object_id.clone(), bytes.to_vec());
            if let Err(e) = self.storage.put(p, bytes).await {
                tracing::warn!("storage gateway put failed (local fallback active): {e}");
            }
        }
        Ok(())
    }

    pub async fn get_object(&self, placements: &[Placement]) -> Result<Vec<u8>> {
        for p in placements {
            if let Some(b) = self.objects.read().get(&p.object_id) {
                return Ok(b.clone());
            }
            if let Ok(b) = self.storage.get(p).await {
                return Ok(b);
            }
        }
        Err(anyhow!("object unavailable on all placements"))
    }

    pub async fn delete_object(&self, placements: &[Placement]) -> Result<()> {
        for p in placements {
            self.objects.write().remove(&p.object_id);
            let _ = self.storage.delete(p).await;
        }
        Ok(())
    }

    pub fn start_recovery(&self, subject: &str, keystore_id: &str) -> Result<StartRecoveryResponse> {
        let manifest = self
            .manifests
            .read()
            .get(subject)
            .cloned()
            .ok_or_else(|| anyhow!("not enrolled"))?;
        if manifest.keystore_id != keystore_id {
            return Err(anyhow!("keystore generation mismatch"));
        }
        if self.retired.read().contains(keystore_id) {
            return Err(anyhow!("generation retired"));
        }

        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        let session_id = hex32_from_bytes(&buf);
        let now = unix_now();
        let mut tickets = HashMap::new();
        for g in self.registry.list_info() {
            let mut nonce = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut nonce);
            let body = CoordinatorTicketBody {
                v: 1,
                session_id: session_id.clone(),
                subject: subject.to_string(),
                guardian_kid: g.kid.clone(),
                keystore_id: keystore_id.to_string(),
                issued_at: now,
                expires_at: now + TICKET_TTL_S,
                nonce: b64_encode(&nonce),
            };
            tickets.insert(
                g.kid.clone(),
                sign_coordinator_ticket(&body, &self.coordinator_sk)?,
            );
        }
        Ok(StartRecoveryResponse { session_id, tickets })
    }

    pub fn coverage(&self, subject: &str) -> CoverageStatus {
        let manifest = self.manifests.read().get(subject).cloned();
        let ent = self.payments.status(subject);
        if manifest.is_none() {
            return CoverageStatus {
                covered: false,
                placement_grade: None,
                shards: vec![],
                blob_replicas_ok: 0,
                blob_replicas_total: 0,
                enrolled: false,
            };
        }
        let m = manifest.unwrap();
        let objects = self.objects.read();
        let shard_health: Vec<ShardHealth> = m
            .shards
            .iter()
            .map(|sh| {
                let ok = sh
                    .placements
                    .iter()
                    .filter(|p| objects.contains_key(&p.object_id))
                    .count() as u32;
                ShardHealth {
                    index: sh.index,
                    verified: ok > 0,
                    last_checked: Some(unix_now()),
                    placements_ok: ok,
                    placements_total: sh.placements.len() as u32,
                }
            })
            .collect();
        let blob_ok = m
            .blob_ref
            .placements
            .iter()
            .filter(|p| objects.contains_key(&p.object_id))
            .count() as u32;
        let verified = shard_health.iter().filter(|s| s.verified).count() as u32;
        CoverageStatus {
            covered: ent.active
                && m.placement_grade == PlacementGrade::Full
                && verified >= 4
                && blob_ok >= 1,
            placement_grade: Some(m.placement_grade.clone()),
            shards: shard_health,
            blob_replicas_ok: blob_ok,
            blob_replicas_total: m.blob_ref.placements.len() as u32,
            enrolled: true,
        }
    }

    pub fn audit_log(&self) -> Vec<AuditRecord> {
        self.audit.read().clone()
    }

    pub fn retire_generation(&self, subject: &str, keystore_id: &str) -> Result<()> {
        self.retired.write().insert(keystore_id.to_string());
        self.last_recovery.write().insert(subject.to_string(), unix_now());
        for g in self.registry.list() {
            g.retire_generation(keystore_id.to_string());
        }
        for g in self.registry.list_info() {
            let _ = reqwest::Client::new()
                .post(format!("{}/v1/guardian/admin/retire", g.endpoint))
                .json(&serde_json::json!({ "keystore_id": keystore_id }))
                .send();
        }
        if self
            .manifests
            .read()
            .get(subject)
            .is_some_and(|m| m.keystore_id == keystore_id)
        {
            self.manifests.write().remove(subject);
        }
        Ok(())
    }

    pub fn destroy(&self, subject: &str) -> Result<()> {
        if let Some(m) = self.manifests.write().remove(subject) {
            self.retired.write().insert(m.keystore_id.clone());
            for g in self.registry.list() {
                g.retire_generation(m.keystore_id.clone());
            }
            for g in self.registry.list_info() {
                let _ = reqwest::Client::new()
                    .post(format!("{}/v1/guardian/admin/retire", g.endpoint))
                    .json(&serde_json::json!({ "keystore_id": m.keystore_id }))
                    .send();
            }
        }
        Ok(())
    }

    pub fn payments(&self) -> &PaymentsState {
        &self.payments
    }

    pub fn coordinator_pk_b64(&self) -> &str {
        &self.coordinator_pk_b64
    }

    pub fn register_guardian(&self, info: GuardianInfo) {
        self.registry.register_info(info);
    }

    pub fn list_guardians(&self) -> Vec<GuardianInfo> {
        self.registry.list_info()
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
