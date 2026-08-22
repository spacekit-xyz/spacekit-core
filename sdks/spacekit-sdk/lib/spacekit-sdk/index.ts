/**
 * SpaceKit SDK - React integration for SpaceKit-JS
 * 
 * A complete SDK for building decentralized applications with SpaceKit.
 * 
 * @example
 * ```tsx
 * import { SpacekitProvider, useSpacekit } from './lib/spacekit-sdk';
 * 
 * function App() {
 *   return (
 *     <SpacekitProvider>
 *       <MyApp />
 *     </SpacekitProvider>
 *   );
 * }
 * 
 * function MyApp() {
 *   const { identity, balance, vm, explorer } = useSpacekit();
 *   
 *   return (
 *     <div>
 *       <p>Hello, {identity.name}!</p>
 *       <p>Balance: {balance.formatted} ASTRA</p>
 *       <p>Blocks: {explorer.blockCount}</p>
 *     </div>
 *   );
 * }
 * ```
 */

// Core exports
export { SpacekitProvider, useSpacekit, useSpacekitOptional } from './SpacekitProvider';
export type { SpacekitContextValue, SpacekitProviderProps } from './SpacekitProvider';

// Hook exports (Provider-based)
export { useIdentity } from './hooks/useIdentity';
export { useBalance } from './hooks/useBalance';
export { useExplorer } from './hooks/useExplorer';
export { useVm } from './hooks/useVm';
export { useKeys } from './hooks/useKeys';

// Standalone hooks (no Provider required) - for simpler use cases
export {
  useIdentity as useStandaloneIdentity,
  useScopedDb,
  useIdentityChange,
  useIdentityKey,
  useIdentityStorage,
} from '../react';

// Component exports
export { SpacekitWallet } from './components/SpacekitWallet';
export { SpacekitExplorer } from './components/SpacekitExplorer';
export { SpacekitIdentityCard } from './components/SpacekitIdentityCard';

// Client utilities (React consumers)
export { SpacekitClient } from '../SpacekitClient';

// Type exports
export type {
  LlmAdapter,
  LlmStatus,
  LlmChatEngine,
  LlmChatMessage,
  Identity,
  Balance,
  Block,
  Transaction,
  Receipt,
  ExplorerState,
  VmState,
  KeysState,
} from './types';

// LLM adapter utilities (SDK convenience)
export {
  LLM_STATUS,
  WebLlmAdapter,
  registerLlmAdapter,
  unregisterLlmAdapter,
  listLlmAdapters,
  setActiveLlmAdapter,
  getActiveLlmAdapter,
} from "@spacekit/spacekit-js";

// Crypto utilities
export {
  safeUUID,
} from '../crypto';