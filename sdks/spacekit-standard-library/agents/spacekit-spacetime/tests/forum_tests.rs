use spacekit_spacetime::forum::SpaceTimeForum;
use spacekit_spacetime::spacekit::prelude::env;
use std::collections::HashSet;

fn with_agent_check(agents: HashSet<String>) {
    env::set_call_handler(Some(Box::new(move |_addr, _method, did| {
        agents.contains(did)
    })));
}

#[test]
fn create_thread_requires_agent() {
    let mut forum = SpaceTimeForum::default();
    forum.init("identity-contract".to_string());
    with_agent_check(HashSet::new());

    env::set_caller("did:spacekit:alice");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        forum.create_thread("Hello".to_string(), "ref:1".to_string());
    }));
    assert!(result.is_err());
}

#[test]
fn create_thread_and_reply_flow() {
    let mut forum = SpaceTimeForum::default();
    forum.init("identity-contract".to_string());
    let mut agents = HashSet::new();
    agents.insert("did:spacekit:alice".to_string());
    with_agent_check(agents);

    env::set_caller("did:spacekit:alice");
    env::set_block_timestamp(42);
    let thread_id = forum.create_thread("Topic".to_string(), "ref:thread".to_string());
    let thread = forum.get_thread(thread_id).expect("thread");
    assert_eq!(thread.title, "Topic");
    assert_eq!(thread.content_ref, "ref:thread");

    let post_id = forum.reply(thread_id, None, "ref:post".to_string());
    let post = forum.get_post(post_id).expect("post");
    assert_eq!(post.thread_id, thread_id);
    assert_eq!(post.parent_post_id, None);
}

#[test]
fn reply_rejects_parent_thread_mismatch() {
    let mut forum = SpaceTimeForum::default();
    forum.init("identity-contract".to_string());
    let mut agents = HashSet::new();
    agents.insert("did:spacekit:alice".to_string());
    with_agent_check(agents);

    env::set_caller("did:spacekit:alice");
    let t1 = forum.create_thread("T1".to_string(), "ref:t1".to_string());
    let t2 = forum.create_thread("T2".to_string(), "ref:t2".to_string());
    let p1 = forum.reply(t1, None, "ref:p1".to_string());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        forum.reply(t2, Some(p1), "ref:p2".to_string());
    }));
    assert!(result.is_err());
}
