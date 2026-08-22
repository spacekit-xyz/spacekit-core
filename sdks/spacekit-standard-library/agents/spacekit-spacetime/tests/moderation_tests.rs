use spacekit_spacetime::moderation::SpaceTimeModeration;
use spacekit_spacetime::spacekit::prelude::env;

#[test]
fn flag_and_hide_post() {
    let mut moderation = SpaceTimeModeration::default();
    env::set_caller("did:spacekit:owner");
    moderation.init("forum-contract".to_string(), "did:spacekit:owner".to_string());

    env::set_caller("did:spacekit:agent");
    moderation.flag_post(7, "spam".to_string());
    let flags = moderation.get_flags(7);
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].reason, "spam");

    env::set_caller("did:spacekit:owner");
    moderation.hide_post(7);
    assert!(moderation.is_hidden(7));

    moderation.unhide_post(7);
    assert!(!moderation.is_hidden(7));
}
