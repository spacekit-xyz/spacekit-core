/**
 * SpaceKit React Hooks
 * 
 * Provides React-specific utilities for SpaceKit integration.
 * These hooks handle identity changes, storage isolation, and
 * component lifecycle management automatically.
 */

import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import { SpacekitClient } from "./SpacekitClient";
import type { ClientEvent } from "./SpacekitClient";

/**
 * Hook that provides the current identity DID with automatic updates.
 * 
 * This hook:
 * - Initializes SpacekitClient on first use
 * - Returns the current DID (never null - defaults to alice)
 * - Automatically updates when identity changes (via BroadcastChannel or localStorage)
 * - Triggers re-renders when identity changes
 * 
 * @returns The current identity DID (guaranteed non-null)
 * 
 * @example
 * const did = useIdentity();
 * // did is always a valid string like "did:spacekit:demo:alice"
 */
export function useIdentity(): string {
  // Initialize client
  useEffect(() => {
    SpacekitClient.init();
  }, []);

  const [did, setDid] = useState<string>(() => {
    SpacekitClient.init();
    return SpacekitClient.requireDid();
  });

  // Listen for identity changes via SpacekitClient events
  useEffect(() => {
    const unsubscribe = SpacekitClient.on("identity-change", (event: ClientEvent) => {
      if (event.did) {
        setDid(event.did);
      }
    });

    return unsubscribe;
  }, []);

  // Also listen for localStorage changes from other tabs/iframes
  useEffect(() => {
    const handleStorage = (e: StorageEvent) => {
      if (e.key === "spacekit:identityDid" && e.newValue) {
        setDid(e.newValue);
      }
    };

    window.addEventListener("storage", handleStorage);
    return () => window.removeEventListener("storage", handleStorage);
  }, []);

  return did;
}

/**
 * Hook that provides identity-scoped database name.
 * Automatically updates when identity changes.
 * 
 * @param prefix - The storage prefix (e.g., "video", "files", "llm-chat")
 * @returns A tuple of [dbName, did] that updates when identity changes
 * 
 * @example
 * const [dbName, did] = useScopedDb("video");
 * // dbName = "spacekitvm-video-did-spacekit-demo-alice"
 */
export function useScopedDb(prefix: string): [string, string] {
  const did = useIdentity();
  const dbName = useMemo(() => SpacekitClient.getScopedDbName(prefix, did), [prefix, did]);
  return [dbName, did];
}

/**
 * Hook that tracks if the identity has changed since the component mounted.
 * Useful for invalidating caches and reinitializing storage.
 * 
 * @returns Object with:
 *   - did: Current DID
 *   - hasChanged: Whether DID changed since mount
 *   - previousDid: The previous DID (null on first render)
 *   - resetChangeFlag: Function to reset hasChanged to false
 * 
 * @example
 * const { did, hasChanged, previousDid, resetChangeFlag } = useIdentityChange();
 * useEffect(() => {
 *   if (hasChanged) {
 *     // Clear caches, reinitialize storage, etc.
 *     clearStorageRefs();
 *     resetChangeFlag();
 *   }
 * }, [hasChanged]);
 */
export function useIdentityChange(): {
  did: string;
  hasChanged: boolean;
  previousDid: string | null;
  resetChangeFlag: () => void;
} {
  const did = useIdentity();
  const previousDidRef = useRef<string | null>(null);
  const [hasChanged, setHasChanged] = useState(false);

  useEffect(() => {
    if (previousDidRef.current !== null && previousDidRef.current !== did) {
      setHasChanged(true);
    }
    previousDidRef.current = did;
  }, [did]);

  const resetChangeFlag = useCallback(() => {
    setHasChanged(false);
  }, []);

  return {
    did,
    hasChanged,
    previousDid: previousDidRef.current,
    resetChangeFlag,
  };
}

/**
 * Hook that provides a stable key for React component remounting.
 * When identity changes, the key changes, forcing React to unmount and remount.
 * 
 * @param prefix - Optional prefix for the key
 * @returns A key string that changes when identity changes
 * 
 * @example
 * const key = useIdentityKey("video");
 * return <VideoComponent key={key} />;
 * // Component will remount when identity changes
 */
export function useIdentityKey(prefix = "component"): string {
  const did = useIdentity();
  const [mountId, setMountId] = useState(0);
  const previousDidRef = useRef<string | null>(null);

  useEffect(() => {
    if (previousDidRef.current !== null && previousDidRef.current !== did) {
      setMountId((m) => m + 1);
    }
    previousDidRef.current = did;
  }, [did]);

  return `${prefix}-${did}-${mountId}`;
}

/**
 * Hook that manages identity-scoped storage initialization.
 * Automatically clears and reinitializes storage when identity changes.
 * 
 * @param options Configuration options
 * @returns Object with storage state and utilities
 * 
 * @example
 * const { did, dbName, isStale, markFresh, generation } = useIdentityStorage({
 *   prefix: "video",
 *   onIdentityChange: async (newDid, oldDid) => {
 *     // Clear old storage refs, reinitialize for new user
 *     storageRef.current = null;
 *     await initStorage();
 *   }
 * });
 */
export function useIdentityStorage(options: {
  prefix: string;
  onIdentityChange?: (newDid: string, oldDid: string | null) => void | Promise<void>;
}): {
  did: string;
  dbName: string;
  isStale: boolean;
  markFresh: () => void;
  generation: number;
} {
  const { prefix, onIdentityChange } = options;
  const did = useIdentity();
  const dbName = useMemo(() => SpacekitClient.getScopedDbName(prefix, did), [prefix, did]);
  
  const previousDidRef = useRef<string | null>(null);
  const [isStale, setIsStale] = useState(false);
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    const oldDid = previousDidRef.current;
    if (oldDid !== null && oldDid !== did) {
      setIsStale(true);
      setGeneration((g) => g + 1);
      
      if (onIdentityChange) {
        void Promise.resolve(onIdentityChange(did, oldDid));
      }
    }
    previousDidRef.current = did;
  }, [did, onIdentityChange]);

  const markFresh = useCallback(() => {
    setIsStale(false);
  }, []);

  return { did, dbName, isStale, markFresh, generation };
}
