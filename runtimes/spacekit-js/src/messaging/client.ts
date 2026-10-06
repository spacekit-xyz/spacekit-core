/**
 * Client for a SpaceKit messaging node's HTTP gateway (`spacekit-messaging-http`).
 *
 * The messaging node is the P2P layer: each user's node admits that user's
 * DID, and nodes exchange envelopes and group state over libp2p gossip. A
 * browser talks only to its own node:
 *
 * - send:    `POST /api/messages/envelope`
 * - receive: `GET  /api/messages/stream?did=…` (Server-Sent Events)
 * - groups:  create, list, join, leave, invite, remove, delete
 * - keys:    register and look up Kyber public keys
 *
 * Joining a group owned by another user's node is asynchronous: the join
 * returns `pending` and the result arrives as a `group` event on the stream.
 * `waitForMembership` wraps that.
 *
 * @module messaging/client
 */

import type { SpacekitMessageEnvelope } from "../spacetime/message.js";

export type GroupVisibility = "public" | "private" | "paid";

export interface GroupInfo {
  id: string;
  name: string;
  creator_did: string;
  description: string;
  visibility: GroupVisibility;
  member_dids: string[];
  created_at: string;
  /** Entitlement-ledger listing of a paid channel. */
  listing_id?: string;
  version: number;
  deleted?: boolean;
}

/** A message delivered on the stream. */
export interface MessageEvent {
  type: "message";
  message_id: string;
  conversation_id: string;
  conversation_type: "direct" | "group" | string;
  group_id?: string | null;
  sender: { did: string };
  /** Chat text, or the JSON of a `spacetime` payload. */
  content: string;
  created_at: string;
  participants: string[];
}

/** A group was created or its membership changed. */
export interface GroupEvent {
  type: "group" | "group_deleted";
  group_id: string;
  group: GroupInfo;
  participants: string[];
  created_at: string;
}

export interface DeleteEvent {
  type: "delete";
  message_id: string;
  deleted_by: string;
  group_id?: string | null;
  participants: string[];
  created_at: string;
}

export type MessagingEvent = MessageEvent | GroupEvent | DeleteEvent;

export interface SendOptions {
  recipientDid?: string;
  recipientDids?: string[];
  /** For a known group the node sends to its current members. */
  groupId?: string;
  conversationType?: "direct" | "group";
}

export interface SendResult {
  status: string;
  conversation_id: string;
  created_at: string;
  message_id: string;
}

export interface CreateGroupOptions {
  name: string;
  description?: string;
  visibility?: GroupVisibility;
  memberDids?: string[];
  /** Required for `paid` channels. */
  listingId?: string;
}

export interface JoinOptions {
  /** Paid channels: entitlement id (hex) bought for the channel's listing. */
  entitlementId?: string;
  /** Paid channels: SHA-256 of the subscriber's Kyber public key (hex). */
  buyerPkHash?: string;
}

export interface JoinResult {
  status: "joined" | "pending";
  group_id: string;
  creator_did?: string;
}

export interface SubscribeOptions {
  /** Called on stream errors; the client reconnects unless aborted. */
  onError?: (error: unknown) => void;
  /** Called each time the stream (re)connects. */
  onOpen?: () => void;
  /** Reconnect after a dropped stream (default true). */
  reconnect?: boolean;
  /** First reconnect delay in ms, doubled up to `maxBackoffMs` (default 500). */
  initialBackoffMs?: number;
  maxBackoffMs?: number;
  signal?: AbortSignal;
}

export interface MessagingClientOptions {
  /** Base URL of the user's messaging node gateway, e.g. `http://localhost:3031`. */
  baseUrl: string;
  /** The DID this node admits (the user's DID): `did:spacekit:<address>`. */
  did: string;
  /**
   * The node's API token (`SPACEKIT_MESSAGING_API_TOKEN`, or the node's
   * `<storage>/api-token`). Every route but `/health` and key lookups needs it.
   */
  token?: string;
  headers?: Record<string, string>;
  /** Override `fetch` (tests, custom transports). */
  fetch?: typeof fetch;
}

export class MessagingError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly body: string,
  ) {
    super(message);
    this.name = "MessagingError";
  }
}

/**
 * Incremental Server-Sent Events parser. Feed it text chunks; it calls
 * `onEvent` with each complete event's `data` (multi-line data joined by `\n`).
 */
export class SseParser {
  private buffer = "";
  private data: string[] = [];

  constructor(private readonly onEvent: (data: string, event?: string) => void) {}

  private eventName?: string;

  push(chunk: string): void {
    this.buffer += chunk;
    let newline: number;
    while ((newline = this.buffer.search(/\r\n|\r|\n/)) >= 0) {
      const line = this.buffer.slice(0, newline);
      const sepLen = this.buffer.startsWith("\r\n", newline) ? 2 : 1;
      this.buffer = this.buffer.slice(newline + sepLen);
      this.line(line);
    }
  }

  private line(line: string): void {
    if (line === "") {
      if (this.data.length > 0) {
        this.onEvent(this.data.join("\n"), this.eventName);
      }
      this.data = [];
      this.eventName = undefined;
      return;
    }
    if (line.startsWith(":")) return; // comment / keep-alive
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "data") this.data.push(value);
    else if (field === "event") this.eventName = value;
  }
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal?.aborted) return resolve();
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

export class MessagingClient {
  readonly baseUrl: string;
  readonly did: string;
  private readonly headers: Record<string, string>;
  private readonly fetchFn: typeof fetch;

  constructor(options: MessagingClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, "");
    this.did = options.did;
    this.headers = {
      ...(options.token ? { Authorization: `Bearer ${options.token}` } : {}),
      ...(options.headers ?? {}),
    };
    this.fetchFn = options.fetch ?? ((...args) => fetch(...args));
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      ...init,
      headers: {
        ...(init.body !== undefined ? { "Content-Type": "application/json" } : {}),
        ...this.headers,
        ...(init.headers as Record<string, string> | undefined),
      },
    });
    const text = await res.text();
    if (!res.ok) {
      throw new MessagingError(
        `${init.method ?? "GET"} ${path} failed (${res.status}): ${text}`,
        res.status,
        text,
      );
    }
    return (text ? JSON.parse(text) : {}) as T;
  }

  private post<T>(path: string, body: unknown): Promise<T> {
    return this.request<T>(path, { method: "POST", body: JSON.stringify(body) });
  }

  // ───────────── Sending ─────────────

  /** Send an envelope (any kind the node accepts: `chat` or `spacetime`). */
  send(message: SpacekitMessageEnvelope, options: SendOptions = {}): Promise<SendResult> {
    if (message.context.did !== this.did) {
      throw new Error("envelope sender must be this client's DID");
    }
    return this.post<SendResult>("/api/messages/envelope", {
      message,
      conversation_type: options.conversationType,
      recipient_did: options.recipientDid,
      recipient_dids: options.recipientDids ?? [],
      group_id: options.groupId,
    });
  }

  /** Send chat text (encrypt it first for private conversations). */
  sendChat(text: string, options: SendOptions = {}): Promise<SendResult> {
    return this.send(
      { kind: "chat", payload: text, context: { did: this.did, timestamp: Date.now() } },
      options,
    );
  }

  /** Send a structured payload (delivered as JSON in `content`). */
  sendSpacetime(payload: unknown, options: SendOptions = {}): Promise<SendResult> {
    return this.send(
      { kind: "spacetime", payload, context: { did: this.did, timestamp: Date.now() } },
      options,
    );
  }

  // ───────────── Receiving ─────────────

  /**
   * Stream events for this DID (messages, group changes, deletes). Reconnects
   * with backoff until the returned function is called or `signal` aborts.
   */
  subscribe(
    handler: (event: MessagingEvent) => void,
    options: SubscribeOptions = {},
  ): () => void {
    const controller = new AbortController();
    const outer = options.signal;
    if (outer) {
      if (outer.aborted) controller.abort();
      else outer.addEventListener("abort", () => controller.abort(), { once: true });
    }
    const reconnect = options.reconnect ?? true;
    const initial = options.initialBackoffMs ?? 500;
    const max = options.maxBackoffMs ?? 30_000;
    const url = `${this.baseUrl}/api/messages/stream?did=${encodeURIComponent(this.did)}`;

    const run = async () => {
      let backoff = initial;
      while (!controller.signal.aborted) {
        try {
          const res = await this.fetchFn(url, {
            headers: { Accept: "text/event-stream", ...this.headers },
            signal: controller.signal,
          });
          if (!res.ok || !res.body) {
            throw new MessagingError(`stream failed (${res.status})`, res.status, "");
          }
          options.onOpen?.();
          backoff = initial;
          const parser = new SseParser((data) => {
            let event: MessagingEvent;
            try {
              event = JSON.parse(data) as MessagingEvent;
            } catch {
              return;
            }
            handler(event);
          });
          const reader = res.body.getReader();
          const decoder = new TextDecoder();
          for (;;) {
            const { value, done } = await reader.read();
            if (done) break;
            parser.push(decoder.decode(value, { stream: true }));
          }
        } catch (error) {
          if (controller.signal.aborted) return;
          options.onError?.(error);
        }
        if (!reconnect || controller.signal.aborted) return;
        await sleep(backoff, controller.signal);
        backoff = Math.min(backoff * 2, max);
      }
    };
    void run();
    return () => controller.abort();
  }

  /**
   * Recent events for this DID (bounded; persisted by the node, including
   * messages it caught up on after being offline). Uses the client's token
   * unless another is given.
   */
  async history(token?: string): Promise<MessagingEvent[]> {
    const res = await this.request<{ messages: MessagingEvent[] }>(
      `/api/messages/history?did=${encodeURIComponent(this.did)}`,
      token ? { headers: { Authorization: `Bearer ${token}` } } : {},
    );
    return res.messages;
  }

  // ───────────── Keys ─────────────

  registerKey(publicKeyHex: string, algorithm = "kyber1024"): Promise<unknown> {
    return this.post("/api/messages/register-key", {
      did: this.did,
      publicKey: publicKeyHex,
      algorithm,
    });
  }

  getKey(did: string): Promise<{ did: string; publicKey: string; algorithm: string }> {
    return this.request(`/api/messages/keys/${encodeURIComponent(did)}`);
  }

  // ───────────── Groups and channels ─────────────

  createGroup(options: CreateGroupOptions): Promise<GroupInfo> {
    return this.post<GroupInfo>("/api/messages/groups", {
      name: options.name,
      creator_did: this.did,
      description: options.description ?? "",
      visibility: options.visibility ?? "public",
      member_dids: options.memberDids ?? [],
      listing_id: options.listingId,
    });
  }

  /** Public and paid groups, plus private groups this DID belongs to. */
  async listGroups(): Promise<GroupInfo[]> {
    const res = await this.request<{ groups: GroupInfo[] }>(
      `/api/messages/groups?did=${encodeURIComponent(this.did)}`,
    );
    return res.groups;
  }

  getGroup(groupId: string): Promise<GroupInfo> {
    return this.request(`/api/messages/groups/${encodeURIComponent(groupId)}`);
  }

  joinGroup(groupId: string, options: JoinOptions = {}): Promise<JoinResult> {
    return this.post(`/api/messages/groups/${encodeURIComponent(groupId)}/join`, {
      did: this.did,
      entitlement_id: options.entitlementId,
      buyer_pk_hash: options.buyerPkHash,
    });
  }

  leaveGroup(groupId: string): Promise<JoinResult> {
    return this.post(`/api/messages/groups/${encodeURIComponent(groupId)}/leave`, {
      did: this.did,
    });
  }

  /** Creator only. In a paid channel this comps the member. */
  inviteToGroup(groupId: string, did: string): Promise<unknown> {
    return this.post(`/api/messages/groups/${encodeURIComponent(groupId)}/invite`, { did });
  }

  /** Creator only. */
  removeMember(groupId: string, did: string): Promise<GroupInfo> {
    return this.post(`/api/messages/groups/${encodeURIComponent(groupId)}/remove`, { did });
  }

  /** Creator only. */
  deleteGroup(groupId: string): Promise<unknown> {
    return this.post(`/api/messages/groups/${encodeURIComponent(groupId)}/delete`, {});
  }

  /**
   * Resolve once the group lists this DID as a member (or reject on timeout,
   * or if the group is deleted). Checks the replica first, then the stream.
   */
  waitForMembership(groupId: string, timeoutMs = 30_000): Promise<GroupInfo> {
    return new Promise((resolve, reject) => {
      let settled = false;
      let stop: () => void = () => {};
      const finish = (fn: () => void) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        stop();
        fn();
      };
      const timer = setTimeout(
        () => finish(() => reject(new Error(`not admitted to ${groupId} within ${timeoutMs} ms`))),
        timeoutMs,
      );
      stop = this.subscribe(
        (event) => {
          if (event.type === "group_deleted" && event.group_id === groupId) {
            finish(() => reject(new Error(`group ${groupId} was deleted`)));
          } else if (
            event.type === "group" &&
            event.group_id === groupId &&
            event.group.member_dids.includes(this.did)
          ) {
            finish(() => resolve(event.group));
          }
        },
        {
          onOpen: () => {
            this.getGroup(groupId)
              .then((group) => {
                if (group.member_dids.includes(this.did)) finish(() => resolve(group));
              })
              .catch(() => {});
          },
        },
      );
    });
  }

  health(): Promise<Record<string, unknown>> {
    return this.request("/health");
  }
}
