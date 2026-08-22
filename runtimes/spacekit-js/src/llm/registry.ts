import type { LlmAdapter } from "../host.js";

type RegistryOptions = {
  makeActive?: boolean;
  overwrite?: boolean;
};

const adapterRegistry = new Map<string, LlmAdapter>();
let activeAdapterId: string | null = null;

export function registerLlmAdapter(id: string, adapter: LlmAdapter, options: RegistryOptions = {}): boolean {
  if (!options.overwrite && adapterRegistry.has(id)) {
    return false;
  }
  adapterRegistry.set(id, adapter);
  if (options.makeActive) {
    activeAdapterId = id;
  }
  return true;
}

export function unregisterLlmAdapter(id: string): boolean {
  const existed = adapterRegistry.delete(id);
  if (activeAdapterId === id) {
    activeAdapterId = null;
  }
  return existed;
}

export function listLlmAdapters(): string[] {
  return Array.from(adapterRegistry.keys());
}

export function setActiveLlmAdapter(id: string): LlmAdapter | null {
  if (!adapterRegistry.has(id)) {
    return null;
  }
  activeAdapterId = id;
  return adapterRegistry.get(id) ?? null;
}

export function getActiveLlmAdapter(): LlmAdapter | null {
  if (!activeAdapterId) {
    return null;
  }
  return adapterRegistry.get(activeAdapterId) ?? null;
}
