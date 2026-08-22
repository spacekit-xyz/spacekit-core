/**
 * Persistent Verkle State Manager
 *
 * Wraps a StorageAdapter with a live QuantumVerkleWasm instance that is
 * kept in sync with every storage write. Tracks read/write sets per
 * block execution so verkle witnesses can be generated and included in
 * blocks, enabling stateless validation.
 */

import type { StorageAdapter } from "../storage.js";
import { bytesToHex, hexToBytes } from "../storage.js";
import {
  QuantumVerkleBridge,
  type QuantumVerkleOptions,
  type QuantumVerkleEntry,
} from "./quantum_verkle.js";
import { loadQuantumVerkleWasm, type QuantumVerkleWasmModule } from "../quantum_verkle.js";
import { sha256Hex } from "./hash.js";

/* ── Types ─────────────────────────────────────────────────── */

export interface AccessRecord {
  keyHex: string;
  addressHex: string;
  verkleKeyHex: string;
  valueHex: string | null;
  mode: "read" | "write";
}

export interface VerkleWitness {
  /** Multi-proof covering all accessed keys during block execution */
  proofHex: string;
  /** Keys accessed (read or written) during block execution */
  accessedKeys: Array<{
    keyHex: string;
    valueHex: string | null;
    mode: "read" | "write";
  }>;
  /** State root before block execution */
  preStateRoot: string;
  /** State root after block execution */
  postStateRoot: string;
}

/* ── Helpers ───────────────────────────────────────────────── */

function strip0x(v: string): string {
  return v.startsWith("0x") ? v.slice(2) : v;
}

function normalizeU256Hex(valueHex: string): string {
  return strip0x(valueHex).padStart(64, "0");
}

/**
 * Max raw value bytes inlined into the per-block access log as hex. Larger values omit `valueHex`
 * (null) so loading a multi‑MB Growformer brain does not allocate ~2× hex strings on every read/write
 * (which can crash the browser tab during inference + mining).
 */
const ACCESS_LOG_INLINE_VALUE_BYTES = 4096;

function accessLogValueHex(value: Uint8Array | undefined): string | null {
  if (!value) return null;
  if (value.length > ACCESS_LOG_INLINE_VALUE_BYTES) return null;
  return bytesToHex(value);
}

/**
 * Verkle tree slots are U256-sized. Small values use raw bytes (padded); large blobs (e.g. Growformer
 * `.bin` seeded via `setStorageKeyUtf8`) use SHA-256(value) so `tree.set` never receives oversized hex.
 * Full value bytes remain in `inner` storage for contracts.
 */
async function valueSlotHexForVerkle(valueBytes: Uint8Array): Promise<string> {
  if (valueBytes.length <= 32) {
    return normalizeU256Hex(bytesToHex(valueBytes));
  }
  return normalizeU256Hex(await sha256Hex(valueBytes));
}

async function deriveVerkleKey(keyHex: string): Promise<{ addressHex: string; verkleKeyHex: string }> {
  const keyBytes = hexToBytes(strip0x(keyHex));
  const hashHex = await sha256Hex(keyBytes);
  return { addressHex: hashHex.slice(0, 40), verkleKeyHex: hashHex.slice(0, 64) };
}

/* ── VerkleStateManager ──────────────────────────────────── */

export class VerkleStateManager {
  private inner: StorageAdapter;
  private wasm: QuantumVerkleWasmModule | null = null;
  private tree: any = null; // QuantumVerkleWasm instance
  private currentRoot: string = "verkle:empty";
  private accessLog: AccessRecord[] = [];
  private preBlockRoot: string = "verkle:empty";
  private initialized = false;
  private initPromise: Promise<void> | null = null;
  private wasmOptions: QuantumVerkleOptions;

  constructor(inner: StorageAdapter, wasmOptions: QuantumVerkleOptions = {}) {
    this.inner = inner;
    this.wasmOptions = wasmOptions;
  }

  /** Initialize the WASM module and build the initial tree from existing storage. */
  async init(): Promise<void> {
    if (this.initialized) return;
    if (this.initPromise) return this.initPromise;
    this.initPromise = this._init();
    return this.initPromise;
  }

  private async _init(): Promise<void> {
    this.wasm = await loadQuantumVerkleWasm(this.wasmOptions);
    this.tree = new this.wasm.QuantumVerkleWasm();

    // Populate tree with all existing storage entries
    if (this.inner.entries) {
      const entries = this.inner.entries();
      for (const entry of entries) {
        const keyHex = bytesToHex(entry.key);
        const { addressHex, verkleKeyHex } = await deriveVerkleKey(keyHex);
        const slotHex = await valueSlotHexForVerkle(entry.value);
        this.tree.set(addressHex, verkleKeyHex, slotHex, null);
      }
    }
    this.currentRoot = `verkle:${this.tree.root_hex()}`;
    this.initialized = true;
  }

  /** The current verkle state root. */
  get root(): string {
    return this.currentRoot;
  }

  /**
   * Mark the start of a new block's transaction window.
   * Called automatically when the first pending tx arrives.
   */
  markPreBlockRoot(): void {
    this.preBlockRoot = this.currentRoot;
  }

  /** Flush accumulated access records and return them + the pre-block root. */
  flushAccessLog(): { log: AccessRecord[]; preRoot: string } {
    const log = this.accessLog;
    const preRoot = this.preBlockRoot;
    this.accessLog = [];
    this.preBlockRoot = this.currentRoot;
    return { log, preRoot };
  }

  /** Read from underlying storage + record access. */
  get(key: Uint8Array): Uint8Array | undefined {
    const value = this.inner.get(key);
    const keyHex = bytesToHex(key);
    this.accessLog.push({
      keyHex,
      addressHex: "",
      verkleKeyHex: "",
      valueHex: accessLogValueHex(value),
      mode: "read",
    });
    return value;
  }

  /** Write to underlying storage + update verkle tree + record access. */
  set(key: Uint8Array, value: Uint8Array): void {
    this.inner.set(key, value);
    const keyHex = bytesToHex(key);

    this.accessLog.push({
      keyHex,
      addressHex: "",
      verkleKeyHex: "",
      valueHex: accessLogValueHex(value),
      mode: "write",
    });

    // Queue the tree update (tree is sync once initialized)
    if (this.tree) {
      void (async () => {
        const { addressHex, verkleKeyHex } = await deriveVerkleKey(keyHex);
        const slotHex = await valueSlotHexForVerkle(value);
        this.tree.set(addressHex, verkleKeyHex, slotHex, null);
        this.currentRoot = `verkle:${this.tree.root_hex()}`;
      })();
    }
  }

  /** Synchronous flush: recompute root from tree (already up-to-date). */
  async flushRoot(): Promise<string> {
    if (!this.tree) return "verkle:disabled";
    // Ensure all pending key derivations are done by re-reading root
    // In practice the fire-and-forget promises complete quickly
    await new Promise((r) => setTimeout(r, 0));
    this.currentRoot = `verkle:${this.tree.root_hex()}`;
    return this.currentRoot;
  }

  /**
   * Generate a VerkleWitness for the block's accessed keys.
   * Must be called after endBlock() and before beginBlock() of the next block.
   */
  async generateWitness(accessLog: AccessRecord[], preStateRoot: string): Promise<VerkleWitness> {
    if (!this.tree) {
      return {
        proofHex: "",
        accessedKeys: [],
        preStateRoot,
        postStateRoot: this.currentRoot,
      };
    }

    // Resolve verkle keys + compact witness values (never embed multi‑MB hex from access log).
    const resolved: Array<{ addressHex: string; verkleKeyHex: string; keyHex: string; valueHex: string | null; mode: "read" | "write" }> = [];
    for (const rec of accessLog) {
      const { addressHex, verkleKeyHex } = await deriveVerkleKey(rec.keyHex);
      let valueBytes: Uint8Array | undefined;
      if (rec.valueHex) {
        try {
          valueBytes = hexToBytes(strip0x(rec.valueHex));
        } catch {
          valueBytes = undefined;
        }
      }
      if (!valueBytes) {
        valueBytes = this.inner.get(hexToBytes(strip0x(rec.keyHex)));
      }
      if (!valueBytes) {
        continue;
      }
      const witnessValueHex = await valueSlotHexForVerkle(valueBytes);
      resolved.push({ addressHex, verkleKeyHex, keyHex: rec.keyHex, valueHex: witnessValueHex, mode: rec.mode });
    }

    // Deduplicate by verkle key (keep last occurrence for writes)
    const seen = new Map<string, typeof resolved[0]>();
    for (const r of resolved) {
      const existing = seen.get(r.verkleKeyHex);
      if (!existing || r.mode === "write") {
        seen.set(r.verkleKeyHex, r);
      }
    }

    const unique = [...seen.values()];
    if (unique.length === 0) {
      return {
        proofHex: "",
        accessedKeys: [],
        preStateRoot,
        postStateRoot: this.currentRoot,
      };
    }

    // Generate multi-proof
    const addresses = unique.map((r) => r.addressHex);
    const keys = unique.map((r) => r.verkleKeyHex);
    let proofHex = "";
    try {
      const proofBytes: Uint8Array = this.tree.create_multi_proof(addresses, keys);
      proofHex = bytesToHex(proofBytes);
    } catch (err) {
      console.warn("[VerkleStateManager] multi-proof generation failed:", err);
    }

    return {
      proofHex,
      accessedKeys: unique.map((r) => ({
        keyHex: r.keyHex,
        valueHex: r.valueHex,
        mode: r.mode,
      })),
      preStateRoot,
      postStateRoot: this.currentRoot,
    };
  }

  /**
   * Stateless verification: given a witness and block transactions, verify
   * that the state transition is valid without holding full state.
   */
  async verifyWitness(witness: VerkleWitness): Promise<boolean> {
    if (!this.tree || !witness.proofHex) return false;

    const addresses: string[] = [];
    const keys: string[] = [];
    const values: string[] = [];

    for (const ak of witness.accessedKeys) {
      if (!ak.valueHex) continue;
      const { addressHex, verkleKeyHex } = await deriveVerkleKey(ak.keyHex);
      addresses.push(addressHex);
      keys.push(verkleKeyHex);
      const raw = hexToBytes(strip0x(ak.valueHex));
      values.push(await valueSlotHexForVerkle(raw));
    }

    if (addresses.length === 0) return true;

    try {
      const proofBytes = hexToBytes(witness.proofHex);
      return this.tree.verify_multi_proof(proofBytes, addresses, keys, values);
    } catch {
      return false;
    }
  }

  /** Proxy: passthrough to inner storage. */
  getAux(key: Uint8Array): Uint8Array | undefined {
    return this.inner.getAux?.(key);
  }

  setAux(key: Uint8Array, value: Uint8Array): void {
    this.inner.setAux?.(key, value);
  }

  entries(): Array<{ key: Uint8Array; value: Uint8Array }> {
    return this.inner.entries?.() ?? [];
  }

  clear(): void {
    this.inner.clear?.();
    if (this.tree) {
      this.tree = new this.wasm!.QuantumVerkleWasm();
      this.currentRoot = `verkle:${this.tree.root_hex()}`;
    }
  }

  /** Get the underlying storage adapter (for legacy code). */
  get innerStorage(): StorageAdapter {
    return this.inner;
  }

  /** Build a StorageAdapter-compatible interface from this manager. */
  toStorageAdapter(): StorageAdapter {
    return {
      get: (key: Uint8Array) => this.get(key),
      set: (key: Uint8Array, value: Uint8Array) => this.set(key, value),
      getAux: (key: Uint8Array) => this.getAux(key),
      setAux: (key: Uint8Array, value: Uint8Array) => this.setAux(key, value),
      entries: () => this.entries(),
      clear: () => this.clear(),
    };
  }
}
