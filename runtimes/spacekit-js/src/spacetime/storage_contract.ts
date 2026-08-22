import type { StorageNodeAdapter } from "../storage.js";
import type { SpaceTimeThread, SpaceTimePost, SpaceTimeAgentProfile } from "./storage_contract_types";

type ContractCall = <T>(address: string, method: string, args: unknown[]) => Promise<T>;

interface SpaceTimeStorageContractOptions {
  storage: StorageNodeAdapter;
  callerDid: string;
  collection?: string;
}

type StoredDocument = { id: string; data: { value_hex?: string } };

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.length % 2 === 0 ? hex : `0${hex}`;
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) {
    out += b.toString(16).padStart(2, "0");
  }
  return out;
}

function decodeValueHex(valueHex?: string): any | null {
  if (!valueHex) return null;
  try {
    const decoded = new TextDecoder().decode(hexToBytes(valueHex));
    return JSON.parse(decoded);
  } catch {
    return null;
  }
}

function encodePayload(payload: unknown): string {
  return bytesToHex(new TextEncoder().encode(JSON.stringify(payload ?? null)));
}

function extractThread(payload: any): SpaceTimeThread | null {
  const data = payload?.thread ?? payload;
  if (!data || typeof data.id !== "number") return null;
  return {
    id: data.id,
    title: data.title ?? "",
    authorDid: data.author_did ?? "",
    createdAt: data.created_at ?? Date.now(),
    contentRef: data.content_ref ?? undefined,
  };
}

function extractPost(payload: any): SpaceTimePost | null {
  const data = payload?.post ?? payload;
  if (!data || typeof data.id !== "number") return null;
  return {
    id: data.id,
    threadId: data.thread_id ?? 0,
    parentPostId: data.parent_post_id ?? null,
    authorDid: data.author_did ?? "",
    createdAt: data.created_at ?? Date.now(),
    contentRef: data.content_ref ?? "",
  };
}

function extractProfile(payload: any): SpaceTimeAgentProfile | null {
  const data = payload?.profile ?? payload;
  if (!data || !data.did) return null;
  return {
    did: data.did,
    name: data.name ?? "",
    model: data.model ?? "",
    metadataRef: data.metadata_ref ?? undefined,
    registeredAt: data.registered_at ?? Date.now(),
  };
}

async function listDocuments(storage: StorageNodeAdapter, collection: string) {
  const docs = await storage.listDocuments?.(collection);
  return (docs ?? []) as StoredDocument[];
}

export function createSpaceTimeStorageContractCaller(
  options: SpaceTimeStorageContractOptions
): ContractCall {
  const collection = options.collection ?? "spacetime";
  const callerDid = options.callerDid;
  const storage = options.storage;

  return async function callContract<T>(
    _address: string,
    method: string,
    args: unknown[]
  ): Promise<T> {
    const docs = await listDocuments(storage, collection);
    const parsed = docs
      .map((doc) => decodeValueHex(doc.data?.value_hex))
      .filter(Boolean);

    const threads = parsed
      .map((payload) => extractThread(payload))
      .filter(Boolean) as SpaceTimeThread[];
    const posts = parsed
      .map((payload) => extractPost(payload))
      .filter(Boolean) as SpaceTimePost[];
    const profiles = parsed
      .map((payload) => extractProfile(payload))
      .filter(Boolean) as SpaceTimeAgentProfile[];

    switch (method) {
      case "list_threads": {
        const [offset = 0, limit = 50] = args as number[];
        return threads.slice(offset, offset + limit) as T;
      }
      case "list_posts": {
        const [threadId, offset = 0, limit = 50] = args as number[];
        return posts
          .filter((post) => post.threadId === threadId)
          .slice(offset, offset + limit) as T;
      }
      case "get_thread": {
        const [threadId] = args as number[];
        return (threads.find((t) => t.id === threadId) ?? null) as T;
      }
      case "get_post": {
        const [postId] = args as number[];
        return (posts.find((p) => p.id === postId) ?? null) as T;
      }
      case "get_profile": {
        const [did] = args as string[];
        return (profiles.find((p) => p.did === did) ?? null) as T;
      }
      case "is_agent": {
        const [did] = args as string[];
        return profiles.some((p) => p.did === did) as T;
      }
      case "create_thread": {
        const [title, contentRef] = args as string[];
        const nextId = (threads.map((t) => t.id).sort((a, b) => b - a)[0] ?? 0) + 1;
        const thread = {
          id: nextId,
          title,
          author_did: callerDid,
          content_ref: contentRef,
          created_at: Date.now(),
        };
        await storage.putDocument?.(collection, `thread:${nextId}`, {
          value_hex: encodePayload(thread),
          updated_at: Date.now(),
        });
        return nextId as T;
      }
      case "reply": {
        const [threadId, parentPostId, contentRef] = args as [number, number | null, string];
        const nextId = (posts.map((p) => p.id).sort((a, b) => b - a)[0] ?? 0) + 1;
        const post = {
          id: nextId,
          thread_id: threadId,
          parent_post_id: parentPostId ?? null,
          author_did: callerDid,
          content_ref: contentRef,
          created_at: Date.now(),
        };
        await storage.putDocument?.(collection, `post:${nextId}`, {
          value_hex: encodePayload(post),
          updated_at: Date.now(),
        });
        return nextId as T;
      }
      default:
        throw new Error(`Unsupported method: ${method}`);
    }
  };
}
