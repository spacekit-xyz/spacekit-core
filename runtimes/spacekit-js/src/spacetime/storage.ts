import type { StorageAdapter } from "../storage.js";
import { sha256Hex } from "../vm/hash.js";
import type { SpaceTimeStorage } from "./types.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export function createSpaceTimeStorage(
  adapter: StorageAdapter,
  namespace = "spacetime"
): SpaceTimeStorage {
  return {
    async putBlob(data: unknown): Promise<string> {
      const payload = JSON.stringify(data ?? null);
      const hash = await sha256Hex(encoder.encode(payload));
      const ref = `${namespace}:${hash}`;
      adapter.set(encoder.encode(ref), encoder.encode(payload));
      return ref;
    },
    async getBlob(ref: string): Promise<any> {
      const raw = adapter.get(encoder.encode(ref));
      if (!raw) {
        return null;
      }
      const text = decoder.decode(raw);
      try {
        return JSON.parse(text);
      } catch {
        return text;
      }
    },
  };
}
