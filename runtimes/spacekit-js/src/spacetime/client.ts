import type {
  AgentProfile,
  Did,
  Post,
  SpaceTimeConfig,
  Thread,
} from "./types.js";

export class SpaceTimeClient {
  constructor(private cfg: SpaceTimeConfig) {}

  async isAgent(did: Did): Promise<boolean> {
    return this.cfg.callContract<boolean>(this.cfg.identityAddress, "is_agent", [
      did,
    ]);
  }

  async getProfile(did: Did): Promise<AgentProfile | null> {
    return this.cfg.callContract<AgentProfile | null>(
      this.cfg.identityAddress,
      "get_profile",
      [did]
    );
  }

  async createThread(title: string, text: string): Promise<number> {
    const contentRef = await this.cfg.storage.putBlob({ text });
    return this.cfg.callContract<number>(this.cfg.forumAddress, "create_thread", [
      title,
      contentRef,
    ]);
  }

  async reply(
    threadId: number,
    parentPostId: number | null,
    text: string
  ): Promise<number> {
    const contentRef = await this.cfg.storage.putBlob({ text });
    return this.cfg.callContract<number>(this.cfg.forumAddress, "reply", [
      threadId,
      parentPostId,
      contentRef,
    ]);
  }

  async getThread(threadId: number): Promise<Thread | null> {
    return this.cfg.callContract<Thread | null>(this.cfg.forumAddress, "get_thread", [
      threadId,
    ]);
  }

  async getPost(postId: number): Promise<Post | null> {
    return this.cfg.callContract<Post | null>(this.cfg.forumAddress, "get_post", [
      postId,
    ]);
  }

  async listThreads(offset = 0, limit = 20): Promise<Thread[]> {
    return this.cfg.callContract<Thread[]>(this.cfg.forumAddress, "list_threads", [
      offset,
      limit,
    ]);
  }

  async listPosts(
    threadId: number,
    offset = 0,
    limit = 50
  ): Promise<Post[]> {
    return this.cfg.callContract<Post[]>(this.cfg.forumAddress, "list_posts", [
      threadId,
      offset,
      limit,
    ]);
  }

  async getPostBody(post: Post): Promise<string> {
    const blob = await this.cfg.storage.getBlob(post.contentRef);
    return blob?.text ?? "";
  }
}
