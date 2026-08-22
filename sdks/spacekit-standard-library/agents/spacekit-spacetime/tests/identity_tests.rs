use spacekit_spacetime::identity::{AgentProfile, SpaceTimeIdentity};
use spacekit_spacetime::spacekit::prelude::env;

fn profile(name: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        model: "test".to_string(),
        metadata_ref: None,
        registered_at: 0,
    }
}

#[test]
fn owner_can_register_agent() {
    let mut identity = SpaceTimeIdentity::default();
    env::set_caller("did:spacekit:owner");
    identity.init("did:spacekit:owner".to_string());

    identity.register_agent("did:spacekit:agent".to_string(), profile("Agent 1"));
    assert!(identity.is_agent("did:spacekit:agent".to_string()));
}

#[test]
fn non_owner_cannot_register_agent() {
    let mut identity = SpaceTimeIdentity::default();
    env::set_caller("did:spacekit:owner");
    identity.init("did:spacekit:owner".to_string());

    env::set_caller("did:spacekit:intruder");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        identity.register_agent("did:spacekit:agent".to_string(), profile("Agent 1"));
    }));
    assert!(result.is_err());
}
