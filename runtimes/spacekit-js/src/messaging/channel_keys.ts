/**
 * End-to-end encryption for group and channel messages.
 *
 * Messaging nodes relay content they cannot read: each group has a symmetric
 * channel key (AES-256-GCM), numbered by epoch. The creator's client
 * distributes the key to members, wrapped to each member's Kyber public key
 * (the KEM is injected, as in `envelope.ts`). When anyone leaves, is removed
 * or lets a subscription lapse, the creator rotates to a new epoch that only
 * the remaining members receive, so former members cannot read new posts.
 * New members receive the current key.
 *
 * Wire formats:
 * - sealed text: `skc1.<epoch>.<base64url(nonce ‖ ciphertext)>`, with
 *   `"<group id>|<epoch>"` as associated data;
 * - key distribution (sent as a `spacetime` envelope to the group):
 *   `{ type: "channel_key", group_id, epoch, keys: { <did>: <wrapped key> } }`.
 *
 * @module messaging/channel_keys
 */

import { gcm } from "@noble/ciphers/aes";
import type { GroupInfo, MessagingClient, MessagingEvent } from "./client.js";

const PREFIX = "skc1";

/** Wraps a channel key to a member's public key, and unwraps our own. */
export interface KeyWrapper {
  wrap(publicKeyHex: string, key: Uint8Array): Promise<unknown>;
  unwrap(wrapped: unknown): Promise<Uint8Array>;
}

export interface KeyRecipient {
  did: string;
  publicKeyHex: string;
}

export interface ChannelKeyMessage {
  type: "channel_key";
  group_id: string;
  epoch: number;
  keys: Record<string, unknown>;
}

function randomBytes(n: number): Uint8Array {
  const out = new Uint8Array(n);
  globalThis.crypto.getRandomValues(out);
  return out;
}

function toBase64Url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(text: string): Uint8Array {
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const bin = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function aad(groupId: string, epoch: number): Uint8Array {
  return new TextEncoder().encode(`${groupId}|${epoch}`);
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function unhex(text: string): Uint8Array {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(text.slice(2 * i, 2 * i + 2), 16);
  return out;
}

/** Channel keys this client holds, per group and epoch. */
export class ChannelKeyring {
  private keys = new Map<string, Map<number, Uint8Array>>();
  private current = new Map<string, number>();

  static isSealed(text: string): boolean {
    return text.startsWith(`${PREFIX}.`);
  }

  setKey(groupId: string, epoch: number, key: Uint8Array): void {
    if (key.length !== 32) throw new Error("channel keys are 32 bytes");
    let epochs = this.keys.get(groupId);
    if (!epochs) {
      epochs = new Map();
      this.keys.set(groupId, epochs);
    }
    epochs.set(epoch, key);
    if ((this.current.get(groupId) ?? -1) < epoch) this.current.set(groupId, epoch);
  }

  /** The newest epoch held for the group, or `undefined`. */
  currentEpoch(groupId: string): number | undefined {
    return this.current.get(groupId);
  }

  hasKey(groupId: string): boolean {
    return this.current.has(groupId);
  }

  /** Encrypt with the group's current key. */
  seal(groupId: string, plaintext: string): string {
    const epoch = this.current.get(groupId);
    if (epoch === undefined) throw new Error(`no channel key for ${groupId}`);
    const key = this.keys.get(groupId)!.get(epoch)!;
    const nonce = randomBytes(12);
    const ct = gcm(key, nonce, aad(groupId, epoch)).encrypt(new TextEncoder().encode(plaintext));
    const body = new Uint8Array(nonce.length + ct.length);
    body.set(nonce);
    body.set(ct, nonce.length);
    return `${PREFIX}.${epoch}.${toBase64Url(body)}`;
  }

  /** Decrypt text sealed for the group (any epoch this keyring holds). */
  open(groupId: string, sealed: string): string {
    const [prefix, epochText, body] = sealed.split(".");
    if (prefix !== PREFIX || body === undefined) throw new Error("not a sealed channel message");
    const epoch = Number(epochText);
    const key = this.keys.get(groupId)?.get(epoch);
    if (!key) throw new Error(`no key for ${groupId} epoch ${epoch}`);
    const bytes = fromBase64Url(body);
    const pt = gcm(key, bytes.subarray(0, 12), aad(groupId, epoch)).decrypt(bytes.subarray(12));
    return new TextDecoder().decode(pt);
  }

  /** Creator: start a new epoch and wrap its key for each recipient. */
  async rotate(
    groupId: string,
    recipients: KeyRecipient[],
    wrapper: KeyWrapper,
  ): Promise<ChannelKeyMessage> {
    const epoch = (this.current.get(groupId) ?? 0) + 1;
    const key = randomBytes(32);
    this.setKey(groupId, epoch, key);
    return this.wrapFor(groupId, epoch, key, recipients, wrapper);
  }

  /** Creator: wrap the current key for new members. */
  async share(
    groupId: string,
    recipients: KeyRecipient[],
    wrapper: KeyWrapper,
  ): Promise<ChannelKeyMessage> {
    const epoch = this.current.get(groupId);
    if (epoch === undefined) return this.rotate(groupId, recipients, wrapper);
    const key = this.keys.get(groupId)!.get(epoch)!;
    return this.wrapFor(groupId, epoch, key, recipients, wrapper);
  }

  private async wrapFor(
    groupId: string,
    epoch: number,
    key: Uint8Array,
    recipients: KeyRecipient[],
    wrapper: KeyWrapper,
  ): Promise<ChannelKeyMessage> {
    const keys: Record<string, unknown> = {};
    for (const r of recipients) keys[r.did] = await wrapper.wrap(r.publicKeyHex, key);
    return { type: "channel_key", group_id: groupId, epoch, keys };
  }

  /** Member: take our key from a distribution. False if none was for us. */
  async accept(message: ChannelKeyMessage, myDid: string, wrapper: KeyWrapper): Promise<boolean> {
    const wrapped = message.keys[myDid];
    if (wrapped === undefined) return false;
    this.setKey(message.group_id, message.epoch, await wrapper.unwrap(wrapped));
    return true;
  }

  /** Serializable copy (hex keys) for the app to persist. */
  export(): Record<string, Record<number, string>> {
    const out: Record<string, Record<number, string>> = {};
    for (const [groupId, epochs] of this.keys) {
      out[groupId] = {};
      for (const [epoch, key] of epochs) out[groupId][epoch] = hex(key);
    }
    return out;
  }

  static import(data: Record<string, Record<number, string>>): ChannelKeyring {
    const ring = new ChannelKeyring();
    for (const [groupId, epochs] of Object.entries(data)) {
      for (const [epoch, key] of Object.entries(epochs)) ring.setKey(groupId, Number(epoch), unhex(key));
    }
    return ring;
  }
}

/** The Kyber functions of `kyber_wasm` (spacekit-js `extension/wasm`). */
export interface KyberModule {
  kyber_encrypt(algorithm: string, publicKeyBase64: string, plaintextBase64: string): unknown;
  kyber_decrypt(
    algorithm: string,
    secretKeyBase64: string,
    kemCiphertextBase64: string,
    nonceBase64: string,
    ciphertextBase64: string,
  ): unknown;
}

const b64 = (bytes: Uint8Array): string => {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
};

const unb64 = (text: string): Uint8Array => {
  const bin = atob(text);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
};

/**
 * The default `KeyWrapper`: Kyber (ML-KEM) via `kyber_wasm`, AES-256-GCM
 * under the shared secret. Public keys are hex (as registered with the
 * messaging node); the secret key is this member's, base64.
 */
export function kyberKeyWrapper(
  kyber: KyberModule,
  options: { secretKeyBase64: string; algorithm?: "kyber512" | "kyber768" | "kyber1024" },
): KeyWrapper {
  const algorithm = options.algorithm ?? "kyber1024";
  return {
    async wrap(publicKeyHex, key) {
      const out = kyber.kyber_encrypt(algorithm, b64(unhex(publicKeyHex.replace(/^0x/, ""))), b64(key)) as
        | { kemCiphertextBase64: string; nonceBase64: string; ciphertextBase64: string }
        | null;
      if (!out) throw new Error("kyber_encrypt failed");
      return { alg: algorithm, kem: out.kemCiphertextBase64, nonce: out.nonceBase64, ct: out.ciphertextBase64 };
    },
    async unwrap(wrapped) {
      const w = wrapped as { alg?: string; kem: string; nonce: string; ct: string };
      const plain = kyber.kyber_decrypt(w.alg ?? algorithm, options.secretKeyBase64, w.kem, w.nonce, w.ct);
      if (typeof plain !== "string") throw new Error("kyber_decrypt failed");
      const key = unb64(plain);
      if (key.length !== 32) throw new Error("unwrapped key is not 32 bytes");
      return key;
    },
  };
}

/**
 * Persist a keyring encrypted at rest (AES-256-GCM under `storageKey`, 32
 * bytes the app keeps in a platform keystore or derives from the user's
 * secret). `ChannelKeyring.openSealed` reverses it.
 */
export function sealKeyring(ring: ChannelKeyring, storageKey: Uint8Array): string {
  if (storageKey.length !== 32) throw new Error("storageKey must be 32 bytes");
  const nonce = randomBytes(12);
  const ct = gcm(storageKey, nonce, new TextEncoder().encode("SPACEKIT-KEYRING-v1")).encrypt(
    new TextEncoder().encode(JSON.stringify(ring.export())),
  );
  const out = new Uint8Array(12 + ct.length);
  out.set(nonce);
  out.set(ct, 12);
  return toBase64Url(out);
}

export function openSealedKeyring(sealed: string, storageKey: Uint8Array): ChannelKeyring {
  const bytes = fromBase64Url(sealed);
  const pt = gcm(storageKey, bytes.subarray(0, 12), new TextEncoder().encode("SPACEKIT-KEYRING-v1")).decrypt(
    bytes.subarray(12),
  );
  return ChannelKeyring.import(JSON.parse(new TextDecoder().decode(pt)));
}

/** Who joined and who left between two member lists. */
export function membershipChange(
  before: string[],
  after: string[],
): { added: string[]; removed: string[] } {
  const b = new Set(before);
  const a = new Set(after);
  return {
    added: after.filter((d) => !b.has(d)),
    removed: before.filter((d) => !a.has(d)),
  };
}

/** Parse a channel key distribution out of a stream event, if it is one. */
export function channelKeyMessageOf(event: MessagingEvent): ChannelKeyMessage | undefined {
  if (event.type !== "message" || !event.group_id) return undefined;
  try {
    const parsed = JSON.parse(event.content) as Partial<ChannelKeyMessage>;
    if (
      parsed?.type === "channel_key" &&
      parsed.group_id === event.group_id &&
      typeof parsed.epoch === "number" &&
      parsed.keys &&
      typeof parsed.keys === "object"
    ) {
      return parsed as ChannelKeyMessage;
    }
  } catch {
    // not JSON: an ordinary message
  }
  return undefined;
}

export interface ChannelKeyManagerOptions {
  client: MessagingClient;
  keyring: ChannelKeyring;
  wrapper: KeyWrapper;
  /** A member's Kyber public key (default: the node's key registry). */
  resolvePublicKey?: (did: string) => Promise<string>;
  /** Called after the keyring changed (persist it here). */
  onKeysChanged?: (groupId: string, epoch: number) => void;
  onError?: (error: unknown) => void;
}

/**
 * Keep channel keys current, on the stream:
 *
 * - for groups this client created: start a new epoch for a new group and on
 *   first sight after (re)start (members may have left while offline), give
 *   new members the current key, rotate when anyone leaves;
 * - for every group: accept key distributions addressed to this DID.
 *
 * Only distributions sent by the group's creator are accepted. Returns a
 * function that stops the manager.
 */
export function runChannelKeyManager(options: ChannelKeyManagerOptions): () => void {
  const { client, keyring, wrapper } = options;
  const resolve =
    options.resolvePublicKey ?? (async (did: string) => (await client.getKey(did)).publicKey);
  const known = new Map<string, GroupInfo>();
  let queue: Promise<void> = Promise.resolve();
  const enqueue = (task: () => Promise<void>) => {
    queue = queue.then(task).catch((e) => options.onError?.(e));
  };

  const recipients = async (dids: string[]): Promise<KeyRecipient[]> => {
    const out: KeyRecipient[] = [];
    for (const did of dids) {
      if (did === client.did) continue;
      try {
        out.push({ did, publicKeyHex: await resolve(did) });
      } catch (e) {
        options.onError?.(e);
      }
    }
    return out;
  };

  const distribute = async (message: ChannelKeyMessage) => {
    await client.sendSpacetime(message, { groupId: message.group_id });
    options.onKeysChanged?.(message.group_id, message.epoch);
  };

  const onGroup = async (group: GroupInfo) => {
    const previous = known.get(group.id);
    known.set(group.id, group);
    if (group.deleted || group.creator_did !== client.did) return;
    // A new group, or the first sight since this client started: members may
    // have left while it was offline, so start a fresh epoch.
    if (!keyring.hasKey(group.id) || !previous) {
      await distribute(await keyring.rotate(group.id, await recipients(group.member_dids), wrapper));
      return;
    }
    const { added, removed } = membershipChange(previous.member_dids, group.member_dids);
    if (removed.length > 0) {
      await distribute(await keyring.rotate(group.id, await recipients(group.member_dids), wrapper));
    } else if (added.length > 0) {
      await distribute(await keyring.share(group.id, await recipients(added), wrapper));
    }
  };

  const stop = client.subscribe(
    (event) => {
      if (event.type === "group" || event.type === "group_deleted") {
        enqueue(() => onGroup(event.group));
        return;
      }
      const message = channelKeyMessageOf(event);
      if (!message || event.type !== "message") return;
      enqueue(async () => {
        const group = known.get(message.group_id) ?? (await client.getGroup(message.group_id));
        known.set(group.id, group);
        if (event.sender.did !== group.creator_did) return;
        if (await keyring.accept(message, client.did, wrapper)) {
          options.onKeysChanged?.(message.group_id, message.epoch);
        }
      });
    },
    {
      onError: options.onError,
      onOpen: () =>
        enqueue(async () => {
          for (const group of await client.listGroups()) {
            if (!group.member_dids.includes(client.did)) continue;
            await onGroup(group);
          }
        }),
    },
  );
  return stop;
}
