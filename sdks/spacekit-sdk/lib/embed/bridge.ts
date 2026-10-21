import type { AppManifest } from "./types.js";

/** Parent-frame bridge invoked by embedded `.spkg` iframe postMessage calls. */
export interface EmbeddedSdkBridge {
  ensureHydrated(): Promise<void>;
  flush(): void;
  setPushHandler(handler: (topic: string, msg: unknown) => void): void;
  handle(module: string, method: string, params: Record<string, unknown>): Promise<unknown>;
}

export interface EmbeddedSdkBridgeWithOwner extends EmbeddedSdkBridge {
  setOwnerDid?(did: string): void;
  /** The verified package's manifest, before `ensureHydrated` (permissions decide storage modes). */
  setManifest?(manifest: AppManifest): void;
}

export function configureBridgeOwner(bridge: EmbeddedSdkBridge, ownerDid: string): void {
  const maybe = bridge as EmbeddedSdkBridgeWithOwner;
  maybe.setOwnerDid?.(ownerDid);
}

export function configureBridgeManifest(bridge: EmbeddedSdkBridge, manifest: AppManifest): void {
  const maybe = bridge as EmbeddedSdkBridgeWithOwner;
  maybe.setManifest?.(manifest);
}

export async function handleSdkCall(
  bridge: EmbeddedSdkBridge,
  module: string,
  method: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  return bridge.handle(module, method, params);
}
