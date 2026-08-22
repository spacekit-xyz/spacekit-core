use spacekit::prelude::*;
use alloc::{string::String, vec::Vec, collections::BTreeMap, collections::BTreeSet};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

pub type PostId = u64;

#[cfg_attr(not(test), derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct Flag {
    pub post_id: PostId,
    pub flagger_did: Did,
    pub reason: String,
    pub created_at: u64,
}

#[derive(Default)]
pub struct SpaceTimeModeration {
    forum_contract: Address,
    owner: Did,
    flags: BTreeMap<PostId, Vec<Flag>>,
    hidden_posts: BTreeSet<PostId>,
}

impl SpaceTimeModeration {
    pub fn init(&mut self, forum_contract: Address, owner: Did) {
        self.forum_contract = forum_contract;
        self.owner = owner;
    }

    pub fn flag_post(&mut self, post_id: PostId, reason: String) {
        let caller = env::caller();
        let flag = Flag {
            post_id,
            flagger_did: caller,
            reason,
            created_at: env::block_timestamp(),
        };

        self.flags.entry(post_id).or_default().push(flag.clone());
        env::emit("PostFlagged", &flag);
    }

    pub fn hide_post(&mut self, post_id: PostId) {
        self.require_owner();
        self.hidden_posts.insert(post_id);
        env::emit("PostHidden", &post_id);
    }

    pub fn unhide_post(&mut self, post_id: PostId) {
        self.require_owner();
        if self.hidden_posts.remove(&post_id) {
            env::emit("PostUnhidden", &post_id);
        }
    }

    pub fn is_hidden(&self, post_id: PostId) -> bool {
        self.hidden_posts.contains(&post_id)
    }

    pub fn get_flags(&self, post_id: PostId) -> Vec<Flag> {
        self.flags.get(&post_id).cloned().unwrap_or_default()
    }

    fn require_owner(&self) {
        assert!(env::caller() == self.owner, "not owner");
    }
}
