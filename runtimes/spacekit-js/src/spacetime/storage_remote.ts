import { bytesToHex, hexToBytes } from "../storage.js";
import { sha256Hex } from "../vm/hash.js";
import type { SpaceTimeStorage } from "./types.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export interface SpaceTimeStorageNodeOptions {
  baseUrl: string;
  did: string;
  collection?: string;
  namespace?: string;
}

export interface SpaceTimeStorageNodeFallbackOptions
  extends Omit<SpaceTimeStorageNodeOptions, "baseUrl"> {
  baseUrls: string[];
  probePath?: string;
}

export function createSpaceTimeStorageNode(
  options: SpaceTimeStorageNodeOptions
): SpaceTimeStorage {
  // SpaceTime JSON blobs (stringify → hash → ref). Prefer `createStorageNodeRemoteBlobAdapter` for
  // WASM `remote_storage_put`/`get` opaque bytes against the same `/api/documents` HTTP API.
  const baseUrl = options.baseUrl.replace(/\/$/, "");
  const collection = options.collection ?? "spacetime";
  const namespace = options.namespace ?? "spacetime";
  const headers = {
    Authorization: `DID ${options.did}`,
    "Content-Type": "application/json",
  };

  return {
    async putBlob(data: unknown): Promise<string> {
      const payload = JSON.stringify(data ?? null);
      const hash = await sha256Hex(encoder.encode(payload));
      const ref = `${namespace}:${hash}`;
      const url = `${baseUrl}/api/documents/${encodeURIComponent(
        collection
      )}/${encodeURIComponent(ref)}`;
      const res = await fetch(url, {
        method: "PUT",
        headers,
        body: JSON.stringify({
          value_hex: bytesToHex(encoder.encode(payload)),
          updated_at: Date.now(),
        }),
      });
      if (!res.ok) {
        throw new Error(`Storage node write failed: ${res.status}`);
      }
      return ref;
    },
    async getBlob(ref: string): Promise<any> {
      const url = `${baseUrl}/api/documents/${encodeURIComponent(
        collection
      )}/${encodeURIComponent(ref)}`;
      const res = await fetch(url, { headers: { Authorization: headers.Authorization } });
      if (!res.ok) {
        return null;
      }
      const json = await res.json();
      const valueHex = json?.document?.data?.value_hex;
      if (typeof valueHex !== "string") {
        return null;
      }
      const payload = decoder.decode(hexToBytes(valueHex));
      try {
        return JSON.parse(payload);
      } catch {
        return payload;
      }
    },
  };
}

export async function createSpaceTimeStorageNodeWithFallback(
  options: SpaceTimeStorageNodeFallbackOptions
): Promise<SpaceTimeStorage> {
  const collection = options.collection ?? "spacetime";
  const probePath =
    options.probePath ?? `/api/documents/${encodeURIComponent(collection)}`;
  const headers = { Authorization: `DID ${options.did}` };

  for (const baseUrlRaw of options.baseUrls) {
    const baseUrl = baseUrlRaw.replace(/\/$/, "");
    try {
      const res = await fetch(`${baseUrl}${probePath}`, { headers });
      if (res.ok) {
        return createSpaceTimeStorageNode({
          baseUrl,
          did: options.did,
          collection,
          namespace: options.namespace,
        });
      }
    } catch {
      // Try next endpoint.
    }
  }

  const fallback = options.baseUrls[0];
  if (!fallback) {
    throw new Error("No storage node endpoints provided");
  }
  return createSpaceTimeStorageNode({
    baseUrl: fallback,
    did: options.did,
    collection,
    namespace: options.namespace,
  });
}
