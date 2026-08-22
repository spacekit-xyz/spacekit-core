import type { MerkleStep } from "./merkle.js";
import type { QuantumStateProof } from "./spacekitvm.js";
import { verifyQuantumVerkleProof, type QuantumVerkleOptions } from "./quantum_verkle.js";
import { verifyMerkleProof, verifyMerkleProofFromHash } from "./merkle.js";

export interface ComputeNodeBlockHeader {
  blockHash: string;
  txRoot: string;
  receiptRoot: string;
  stateRoot: string;
  quantumStateRoot?: string;
  height: number;
}

export interface ComputeNodeTxProof {
  txHash: string;
  txRoot: string;
  index: number;
  blockHash: string;
  blockHeight: number;
  proof: MerkleStep[];
}

export interface ComputeNodeReceiptProof {
  txHash: string;
  receiptHash: string;
  receiptRoot: string;
  index: number;
  blockHash: string;
  blockHeight: number;
  proof: MerkleStep[];
}

export interface ComputeNodeStateProof {
  keyHex: string;
  valueHex: string | null;
  stateRoot: string;
  proofHash: string;
  proof: MerkleStep[];
}

export async function verifyComputeNodeTxProof(
  proof: ComputeNodeTxProof,
  header?: ComputeNodeBlockHeader
): Promise<boolean> {
  const leafHash = strip0x(proof.txHash);
  const root = strip0x(proof.txRoot);
  if (header && strip0x(header.txRoot) !== root) {
    return false;
  }
  return verifyMerkleProofFromHash(leafHash, proof.proof, root);
}

export async function verifyComputeNodeReceiptProof(
  proof: ComputeNodeReceiptProof,
  header?: ComputeNodeBlockHeader
): Promise<boolean> {
  const leafHash = strip0x(proof.receiptHash);
  const root = strip0x(proof.receiptRoot);
  if (header && strip0x(header.receiptRoot) !== root) {
    return false;
  }
  return verifyMerkleProofFromHash(leafHash, proof.proof, root);
}

export async function verifyComputeNodeStateProof(
  proof: ComputeNodeStateProof,
  header?: ComputeNodeBlockHeader
): Promise<boolean> {
  if (!proof.valueHex) {
    return false;
  }
  const leaf = `${strip0x(proof.keyHex)}:${strip0x(proof.valueHex)}`;
  const root = strip0x(proof.stateRoot);
  if (header && strip0x(header.stateRoot) !== root) {
    return false;
  }
  return verifyMerkleProof(leaf, proof.proof, root);
}

export async function verifyComputeNodeQuantumStateProof(
  proof: QuantumStateProof,
  header?: ComputeNodeBlockHeader,
  options: QuantumVerkleOptions = {}
): Promise<boolean> {
  if (header?.quantumStateRoot && header.quantumStateRoot !== proof.stateRoot) {
    return false;
  }
  if (!proof.valueHex) {
    return false;
  }
  return verifyQuantumVerkleProof(proof, options);
}

function strip0x(value: string): string {
  if (!value) {
    return value;
  }
  return value.startsWith("0x") ? value.slice(2) : value;
}
