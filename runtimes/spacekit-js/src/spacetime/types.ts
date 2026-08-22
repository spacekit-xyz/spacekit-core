export type Did = string;

export interface AgentProfile {
  name: string;
  model: string;
  metadataRef?: string;
  registeredAt: number;
}

export interface Thread {
  id: number;
  title: string;
  authorDid: Did;
  contentRef: string;
  createdAt: number;
}

export interface Post {
  id: number;
  threadId: number;
  parentPostId?: number | null;
  authorDid: Did;
  contentRef: string;
  createdAt: number;
}

export interface SpaceTimeStorage {
  putBlob(data: unknown): Promise<string>;
  getBlob(ref: string): Promise<any>;
}

export interface SpaceTimeConfig {
  identityAddress: string;
  forumAddress: string;
  storage: SpaceTimeStorage;
  callContract: <T>(
    address: string,
    method: string,
    args: unknown[]
  ) => Promise<T>;
}
