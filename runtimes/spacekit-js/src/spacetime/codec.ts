import type { ContractCallCodec } from "../vm/contract_caller.js";

export type SpaceTimeMethod =
  | "is_agent"
  | "get_profile"
  | "create_thread"
  | "reply"
  | "get_thread"
  | "get_post"
  | "list_threads"
  | "list_posts"
  | "flag_post"
  | "hide_post"
  | "unhide_post"
  | "is_hidden"
  | "get_flags";

export const SpaceTimeJsonCodec: ContractCallCodec = {
  encode(method: string, args: unknown[]) {
    const payload = JSON.stringify({ method, args });
    return new TextEncoder().encode(payload);
  },
  decode<T>(_method: string, output: Uint8Array) {
    if (!output || output.length === 0) {
      return undefined as T;
    }
    const text = new TextDecoder().decode(output);
    try {
      return JSON.parse(text) as T;
    } catch {
      return text as unknown as T;
    }
  },
};
