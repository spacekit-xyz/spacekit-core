/** Parent-frame bridge invoked by embedded `.spkg` iframe postMessage calls. */
export interface EmbeddedSdkBridge {
  ensureHydrated(): Promise<void>;
  flush(): void;
  setPushHandler(handler: (topic: string, msg: unknown) => void): void;
  handle(module: string, method: string, params: Record<string, unknown>): Promise<unknown>;
}

export interface EmbeddedSdkBridgeWithOwner extends EmbeddedSdkBridge {
  setOwnerDid?(did: string): void;
}

export function configureBridgeOwner(bridge: EmbeddedSdkBridge, ownerDid: string): void {
  const maybe = bridge as EmbeddedSdkBridgeWithOwner;
  maybe.setOwnerDid?.(ownerDid);
}

export async function handleSdkCall(
  bridge: EmbeddedSdkBridge,
  module: string,
  method: string,
  params: Record<string, unknown>,
): Promise<unknown> {
  return bridge.handle(module, method, params);
}
