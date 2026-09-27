/**
 * Typed access to the SpaceKit bridge from inside an app.
 *
 * The host injects `window.spacekit` into every app it runs. Import this module
 * in your app for types and a safe accessor:
 *
 * ```ts
 * import { getSpacekit } from "@spacekit/sdk/guest";
 * const sk = getSpacekit();
 * await sk.storage.set("score", 42);
 * const res = await fetch("https://api.example.com/items"); // proxied by the host
 * ```
 *
 * Calls outside the baseline (other websites under a "manifest" network
 * policy, messaging, profile writes) need a matching `permissions` entry in the
 * app manifest; see SPACEKIT-EMBED-PROTOCOL.md.
 */

export const SPACEKIT_GUEST_PROTOCOL_VERSION = 1;

export interface SpacekitIdentityState {
  myDid: string;
  displayName: string;
  publicOptIn: boolean;
  didRegistered: boolean;
}

export interface SpacekitSubscriptionStatus {
  active: boolean;
  expiresAt: number | null;
  viewerDid: string | null;
  amountCents?: number;
  reason?: string;
}

export interface SpacekitGuestApi {
  readonly appId: string;
  /** Low-level call; prefer the typed modules below. */
  call<T = unknown>(module: string, method: string, params?: Record<string, unknown>): Promise<T>;
  http: {
    fetch(url: string, init?: RequestInit): Promise<Response>;
    sseSubscribe(url: string): Promise<string>;
    sseClose(id: string): Promise<boolean>;
  };
  /** App-scoped key/value storage, persisted by the host. */
  storage: {
    get<T = unknown>(key: string): Promise<T | null>;
    set(key: string, value: unknown): Promise<boolean>;
    list(prefix?: string): Promise<string[]>;
    delete(key: string): Promise<boolean>;
    ready(): Promise<{ ready: boolean; appId: string }>;
  };
  messaging: {
    publish(topic: string, msg: unknown): Promise<boolean>;
    subscribe(topic: string, cb: (msg: unknown) => void): () => void;
    /** Requires the "messaging" permission. */
    send(to: string, content: unknown): Promise<unknown>;
    /** Requires the "messaging" permission. */
    list(): Promise<unknown>;
  };
  payments: {
    status(): Promise<SpacekitSubscriptionStatus>;
    subscribe(options: { amountCents: number; periodDays?: number }): Promise<SpacekitSubscriptionStatus>;
    config(): Promise<{ publisherDid: string | null; currency: string; model: string }>;
  };
  identity: {
    did(): Promise<string | null>;
    getState(): Promise<SpacekitIdentityState>;
    /** Requires "identity:write". The signed-in DID cannot be changed by apps. */
    setState(state: Partial<Omit<SpacekitIdentityState, "myDid">>): Promise<SpacekitIdentityState>;
  };
  app: {
    ready(): Promise<{ ready: boolean; appId: string }>;
    isOwner(): Promise<boolean>;
    ownerDid(): Promise<string | null>;
  };
  /** App-scoped documents on the storage node. `list`/`delete` are owner-only. */
  documents: {
    get<T = unknown>(collection: string, id: string): Promise<T | null>;
    put(collection: string, id: string, data: unknown): Promise<{ id: string }>;
    list<T = unknown>(collection: string): Promise<Array<{ id: string; data: T; updatedAt?: string }>>;
    delete(collection: string, id: string): Promise<boolean>;
  };
}

declare global {
  interface Window {
    spacekit?: SpacekitGuestApi;
  }
}

/** True when this code runs inside a SpaceKit host frame. */
export function isSpacekitHosted(): boolean {
  return typeof window !== "undefined" && !!window.spacekit;
}

/** The injected bridge. Throws when the app is opened outside a SpaceKit host. */
export function getSpacekit(): SpacekitGuestApi {
  if (typeof window === "undefined" || !window.spacekit) {
    throw new Error("window.spacekit is missing: this app is not running inside a SpaceKit host");
  }
  return window.spacekit;
}
