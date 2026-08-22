import { sha256 } from "@noble/hashes/sha2";
import { loadQuantumVerkleWasm, type QuantumVerkleWasmModule, type QuantumVerkleWasmLoaderOptions } from "../quantum_verkle.js";
import { bytesToHex, hexToBytes, type StorageAdapter } from "../storage.js";
import { sha256Hex, hashString } from "./hash.js";

export interface QuantumVerkleEntry {
  keyHex: string;
  valueHex: string;
  auxHex?: string | null;
}

export interface QuantumVerkleProof {
  keyHex: string;
  valueHex: string | null;
  stateRoot: string;
  verkleProofHex: string;
}

export interface QuantumVerkleOptions extends QuantumVerkleWasmLoaderOptions {
  profile?: "binding" | "hiding";
}

export class QuantumVerkleBridge {
  private module: QuantumVerkleWasmModule;

  private constructor(module: QuantumVerkleWasmModule) {
    this.module = module;
  }

  static async create(options: QuantumVerkleOptions = {}): Promise<QuantumVerkleBridge> {
    const mod = await loadQuantumVerkleWasm(options);
    return new QuantumVerkleBridge(mod);
  }

  async computeRoot(entries: QuantumVerkleEntry[]): Promise<string> {
    if (entries.length === 0) {
      return "verkle:empty";
    }
    const tree = new this.module.QuantumVerkleWasm();
    for (const entry of entries) {
      const { addressHex, keyHex } = await deriveQuantumKey(entry.keyHex);
      const valueHex = normalizeU256Hex(entry.valueHex);
      tree.set(addressHex, keyHex, valueHex, entry.auxHex ?? null);
    }
    return `verkle:${tree.root_hex()}`;
  }

  async computeProof(entries: QuantumVerkleEntry[], keyHex: string): Promise<QuantumVerkleProof> {
    const tree = new this.module.QuantumVerkleWasm();
    for (const entry of entries) {
      const { addressHex, keyHex } = await deriveQuantumKey(entry.keyHex);
      const valueHex = normalizeU256Hex(entry.valueHex);
      tree.set(addressHex, keyHex, valueHex, entry.auxHex ?? null);
    }
    const { addressHex, keyHex: verkleKey } = await deriveQuantumKey(keyHex);
    const valueHex = entries.find((entry) => entry.keyHex === keyHex)?.valueHex ?? null;
    const proofBytes = tree.create_proof(addressHex, verkleKey);
    return {
      keyHex,
      valueHex,
      stateRoot: `verkle:${tree.root_hex()}`,
      verkleProofHex: bytesToHex(proofBytes),
    };
  }

  async verifyProof(proof: QuantumVerkleProof): Promise<boolean> {
    if (!proof.valueHex) {
      return false;
    }
    const tree = new this.module.QuantumVerkleWasm();
    const { addressHex, keyHex } = await deriveQuantumKey(proof.keyHex);
    const valueHex = normalizeU256Hex(proof.valueHex);
    const proofBytes = hexToBytes(strip0x(proof.verkleProofHex));
    return tree.verify_proof(proofBytes, addressHex, keyHex, valueHex);
  }
}

/** U256 slot hex for quantum tree — matches `VerkleStateManager` slot encoding (sync). */
function quantumSlotHexFromValue(value: Uint8Array): string {
  if (value.length <= 32) {
    return normalizeU256Hex(bytesToHex(value));
  }
  return normalizeU256Hex(bytesToHex(sha256(value)));
}

export function buildQuantumEntries(storage: StorageAdapter): QuantumVerkleEntry[] {
  if (!storage.entries) {
    return [];
  }
  return storage.entries().map((entry) => {
    const keyHex = bytesToHex(entry.key);
    const valueHex = quantumSlotHexFromValue(entry.value);
    const auxHex = storage.getAux ? storage.getAux(entry.key) : undefined;
    return {
      keyHex,
      valueHex,
      auxHex: auxHex ? bytesToHex(auxHex) : null,
    };
  });
}

export async function verifyQuantumVerkleProof(
  proof: QuantumVerkleProof,
  options: QuantumVerkleOptions = {}
): Promise<boolean> {
  const bridge = await QuantumVerkleBridge.create(options);
  return bridge.verifyProof(proof);
}

async function deriveQuantumKey(keyHex: string): Promise<{ addressHex: string; keyHex: string }> {
  const keyBytes = hexToBytes(strip0x(keyHex));
  const hashHex = await sha256Hex(keyBytes);
  const addressHex = hashHex.slice(0, 40);
  const fullKeyHex = hashHex.slice(0, 64);
  return { addressHex, keyHex: fullKeyHex };
}

function normalizeU256Hex(valueHex: string): string {
  const stripped = strip0x(valueHex).padStart(64, "0");
  return stripped;
}

function strip0x(value: string): string {
  return value.startsWith("0x") ? value.slice(2) : value;
}
