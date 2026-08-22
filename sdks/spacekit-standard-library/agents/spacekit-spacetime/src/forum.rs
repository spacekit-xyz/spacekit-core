use spacekit::prelude::*;
use alloc::{string::String, vec::Vec, collections::BTreeMap};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

pub type ThreadId = u64;
pub type PostId = u64;

#[cfg_attr(not(test), derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Thread {
    pub id: ThreadId,
    pub title: String,
    pub author_did: Did,
    pub content_ref: String,
    pub created_at: u64,
}

#[cfg_attr(not(test), derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Post {
    pub id: PostId,
    pub thread_id: ThreadId,
    pub parent_post_id: Option<PostId>,
    pub author_did: Did,
    pub content_ref: String,
    pub created_at: u64,
}

#[derive(Default)]
pub struct SpaceTimeForum {
    identity_contract: Address,
    threads: BTreeMap<ThreadId, Thread>,
    posts: BTreeMap<PostId, Post>,
    thread_counter: ThreadId,
    post_counter: PostId,
}

impl SpaceTimeForum {
    pub fn init(&mut self, identity_contract: Address) {
        self.identity_contract = identity_contract;
    }

    pub fn create_thread(&mut self, title: String, content_ref: String) -> ThreadId {
        self.require_agent();
        self.require_non_empty("title", &title);
        self.require_non_empty("content_ref", &content_ref);
        self.thread_counter += 1;

        let thread = Thread {
            id: self.thread_counter,
            title,
            author_did: env::caller(),
            content_ref,
            created_at: env::block_timestamp(),
        };

        self.threads.insert(thread.id, thread.clone());
        env::emit("ThreadCreated", &thread);
        thread.id
    }

    pub fn reply(
        &mut self,
        thread_id: ThreadId,
        parent_post_id: Option<PostId>,
        content_ref: String,
    ) -> PostId {
        self.require_agent();
        assert!(self.threads.contains_key(&thread_id), "thread not found");
        if let Some(parent_id) = parent_post_id {
            let parent = self.posts.get(&parent_id).expect("parent post not found");
            assert!(parent.thread_id == thread_id, "parent post thread mismatch");
        }
        self.require_non_empty("content_ref", &content_ref);

        self.post_counter += 1;

        let post = Post {
            id: self.post_counter,
            thread_id,
            parent_post_id,
            author_did: env::caller(),
            content_ref,
            created_at: env::block_timestamp(),
        };

        self.posts.insert(post.id, post.clone());
        env::emit("PostCreated", &post);
        post.id
    }

    pub fn get_thread(&self, thread_id: ThreadId) -> Option<Thread> {
        self.threads.get(&thread_id).cloned()
    }

    pub fn get_post(&self, post_id: PostId) -> Option<Post> {
        self.posts.get(&post_id).cloned()
    }

    pub fn list_threads(&self, offset: u64, limit: u64) -> Vec<Thread> {
        if limit == 0 {
            return Vec::new();
        }
        self.threads
            .values()
            .cloned()
            .skip(offset as usize)
            .take(limit as usize)
            .collect()
    }

    pub fn list_posts(&self, thread_id: ThreadId, offset: u64, limit: u64) -> Vec<Post> {
        if limit == 0 {
            return Vec::new();
        }
        self.posts
            .values()
            .filter(|post| post.thread_id == thread_id)
            .cloned()
            .skip(offset as usize)
            .take(limit as usize)
            .collect()
    }

    fn require_agent(&self) {
        let caller = env::caller();
        let is_agent: bool = env::call(self.identity_contract.clone(), "is_agent", &caller);
        assert!(is_agent, "caller is not a registered agent");
    }

    fn require_non_empty(&self, field: &str, value: &str) {
        assert!(!value.trim().is_empty(), "empty {field}");
    }
}
