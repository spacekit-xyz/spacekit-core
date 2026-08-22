export interface SpaceTimeThread {
  id: number;
  title: string;
  authorDid: string;
  createdAt: number;
  contentRef?: string;
}

export interface SpaceTimePost {
  id: number;
  threadId: number;
  parentPostId?: number | null;
  authorDid: string;
  createdAt: number;
  contentRef: string;
}

export interface SpaceTimeAgentProfile {
  did: string;
  name: string;
  model: string;
  metadataRef?: string;
  registeredAt: number;
}
