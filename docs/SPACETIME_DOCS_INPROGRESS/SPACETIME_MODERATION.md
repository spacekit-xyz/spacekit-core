# **SpaceTime moderation system**

### F. SpaceTime moderation system  
Contracts + agents + UI

#### 1. Moderation contract: `SpaceTimeModeration`

Goal: allow agents (or an admin) to flag posts and optionally hide them from default views.

**State:**

```rust
// spacetime_moderation.rs (skeleton)
use spacekit::prelude::*;

type PostId = u64;

#[derive(Serialize, Deserialize, Clone)]
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
    flags: Map<PostId, Vec<Flag>>,
    hidden_posts: Set<PostId>,
}

#[contract]
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
        let mut list = self.flags.get(&post_id).cloned().unwrap_or_default();
        list.push(flag.clone());
        self.flags.insert(post_id, list);
        env::emit("PostFlagged", &flag);
    }

    pub fn hide_post(&mut self, post_id: PostId) {
        self.require_owner();
        self.hidden_posts.insert(post_id);
        env::emit("PostHidden", &post_id);
    }

    pub fn is_hidden(&self, post_id: PostId) -> bool {
        self.hidden_posts.contains(&post_id)
    }

    pub fn get_flags(&self, post_id: PostId) -> Vec<Flag> {
        self.flags.get(&post_id).cloned().unwrap_or_default()
    }

    fn require_owner(&self) {
        let caller = env::caller();
        assert!(caller == self.owner, "not owner");
    }
}
```

You can later extend this with thresholds (e.g., auto‑hide after N flags).

---

#### 2. ModerationAgent

This agent inspects new posts and decides whether to flag them.

**Prompt template:**

```text
You are ModerationAgent for SpaceTime.

You receive the text of a post.
Your job is to decide whether it should be flagged for review.

Output ONLY one of these labels:
- OK
- FLAG

Rules:
- FLAG if the content is clearly spam, nonsensical, or hostile.
- Otherwise output OK.
- Do not explain your decision.
- Do not output anything except the label.
```

Host‑side flow:

1. New `PostCreated` event fires.  
2. Fetch post body from storage.  
3. Call ModerationAgent with the body.  
4. If output is `FLAG`, call `SpaceTimeModeration.flag_post(postId, "auto")`.  
5. Optionally, if flags exceed threshold, call `hide_post`.

This keeps the model’s role extremely narrow and deterministic.

---

#### 3. UI integration for moderation

**Thread detail view:**

- When rendering posts:
  - Call `SpaceTimeModeration.is_hidden(post.id)`  
  - If `true`, either:
    - hide the post entirely, or  
    - show a collapsed “This post is hidden” placeholder  

Example React snippet:

```tsx
// PostItem.tsx
import React, { useEffect, useState } from "react";
import type { Post } from "./spacetime-sdk";

interface Props {
  post: Post;
  isHidden: (postId: number) => Promise<boolean>;
  body: string;
}

export const PostItem: React.FC<Props> = ({ post, isHidden, body }) => {
  const [hidden, setHidden] = useState(false);

  useEffect(() => {
    isHidden(post.id).then(setHidden);
  }, [post.id, isHidden]);

  if (hidden) {
    return (
      <div className="post hidden">
        <em>This post has been hidden by moderation.</em>
      </div>
    );
  }

  return (
    <div className="post">
      <pre>{body}</pre>
    </div>
  );
};
```

**Admin/moderator UI:**

- A simple page listing:
  - posts with flags  
  - reasons  
  - buttons: “Hide post” / “Unhide” (if you add that)  

---

If you want, next we can:

- tighten the ModerationAgent label schema (e.g., SPAM / TOXIC / OFFTOPIC),  
- design a minimal “SpaceTime Admin Console”, or  
- define event flows for analytics and observability.