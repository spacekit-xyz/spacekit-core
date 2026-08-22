// VPN access policy and PolicyDecision (no external crate refs)
pub struct VpnPolicy {
    pub trusted_issuers: Vec<String>,
    pub allowed_plans: Vec<String>,
}

pub enum PolicyDecision {
    Allow,
    Deny(String),
}

pub trait AccessPolicy {
    fn evaluate(&self, issuer_did: &str, subject_did: &str, plan: &str) -> PolicyDecision;
}

impl AccessPolicy for VpnPolicy {
    fn evaluate(&self, issuer_did: &str, _subject_did: &str, plan: &str) -> PolicyDecision {
        if !self.trusted_issuers.contains(&issuer_did.to_string()) {
            return PolicyDecision::Deny("untrusted issuer".into());
        }
        if !self.allowed_plans.contains(&plan.to_string()) {
            return PolicyDecision::Deny("plan not allowed".into());
        }
        PolicyDecision::Allow
    }
}
