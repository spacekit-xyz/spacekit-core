import { SpacekitVm } from "./spacekitvm.js";
import { sha256Hex, hashString } from "./hash.js";
import { hexToBytes } from "../storage.js";
import * as ed from "@noble/ed25519";
import { HOST_ABI_VERSION, HOST_IMPORT_MODULES } from "./abi.js";
import { verifyMerkleProof } from "./merkle.js";

export interface JsonRpcRequest {
  jsonrpc?: "2.0";
  id?: string | number | null;
  method: string;
  params?: Record<string, unknown>;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: string | number | null;
  result?: unknown;
  error?: { code: number; message: string };
}

function ok(id: string | number | null, result: unknown): JsonRpcResponse {
  return { jsonrpc: "2.0", id, result };
}

function err(id: string | number | null, code: number, message: string): JsonRpcResponse {
  return { jsonrpc: "2.0", id, error: { code, message } };
}

function decodeBase64(value: string): Uint8Array {
  if (typeof atob !== "undefined") {
    const raw = atob(value);
    const out = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i += 1) {
      out[i] = raw.charCodeAt(i);
    }
    return out;
  }
  return Uint8Array.from(Buffer.from(value, "base64"));
}

function encodeUtf8(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

function parseValue(value: unknown): bigint {
  if (typeof value === "number" && Number.isFinite(value)) {
    return BigInt(Math.floor(value));
  }
  if (typeof value === "string" && value.trim().length > 0) {
    return BigInt(value);
  }
  return 0n;
}

export function createJsonRpcHandler(vm: SpacekitVm) {
  return async function handle(request: JsonRpcRequest | string): Promise<JsonRpcResponse> {
    const payload: JsonRpcRequest =
      typeof request === "string" ? JSON.parse(request) : request;
    const id = payload.id ?? null;

    try {
      switch (payload.method) {
        case "vm_deploy": {
          const wasmBase64 = payload.params?.wasmBase64;
          const wasmUrl = payload.params?.wasmUrl;
          const contractId = payload.params?.contractId as string | undefined;
          let wasmBytes: Uint8Array;
          if (typeof wasmBase64 === "string") {
            wasmBytes = decodeBase64(wasmBase64);
          } else if (typeof wasmUrl === "string") {
            const res = await fetch(wasmUrl);
            const buf = await res.arrayBuffer();
            wasmBytes = new Uint8Array(buf);
          } else {
            return err(id, -32602, "Missing wasmBase64 or wasmUrl");
          }
          const contract = await vm.deployContract(wasmBytes, contractId);
          return ok(id, { contractId: contract.id, wasmHash: contract.wasmHash });
        }
        case "vm_deployBatch": {
          const deployments = payload.params?.deployments as Array<Record<string, unknown>> | undefined;
          if (!deployments || !Array.isArray(deployments)) {
            return err(id, -32602, "Missing deployments array");
          }
          const results = [];
          for (const item of deployments) {
            const wasmBase64 = item.wasmBase64;
            const wasmUrl = item.wasmUrl;
            const contractId = item.contractId as string | undefined;
            let wasmBytes: Uint8Array;
            if (typeof wasmBase64 === "string") {
              wasmBytes = decodeBase64(wasmBase64);
            } else if (typeof wasmUrl === "string") {
              const res = await fetch(wasmUrl);
              const buf = await res.arrayBuffer();
              wasmBytes = new Uint8Array(buf);
            } else {
              results.push({ error: "Missing wasmBase64 or wasmUrl" });
              continue;
            }
            const contract = await vm.deployContract(wasmBytes, contractId);
            results.push({ contractId: contract.id, wasmHash: contract.wasmHash });
          }
          return ok(id, results);
        }
        case "vm_submit": {
          const contractId = payload.params?.contractId as string;
          const callerDid = payload.params?.callerDid as string;
          const inputBase64 = payload.params?.inputBase64 as string;
          if (!contractId || !callerDid || !inputBase64) {
            return err(id, -32602, "Missing contractId/callerDid/inputBase64");
          }
          const input = decodeBase64(inputBase64);
          const value = parseValue(payload.params?.value);
          const tx = await vm.submitTransaction(contractId, input, callerDid, value);
          return ok(id, { txId: tx.id });
        }
        case "vm_getNonce": {
          const callerDid = payload.params?.callerDid as string;
          if (!callerDid) {
            return err(id, -32602, "Missing callerDid");
          }
          return ok(id, { nonce: vm.getNonce(callerDid) });
        }
        case "vm_submitSigned": {
          const contractId = payload.params?.contractId as string;
          const callerDid = payload.params?.callerDid as string;
          const inputBase64 = payload.params?.inputBase64 as string;
          const nonce = payload.params?.nonce as number;
          const timestamp = payload.params?.timestamp as number;
          const publicKeyHex = payload.params?.publicKeyHex as string;
          const signatureBase64 = payload.params?.signatureBase64 as string;
          const pqPublicKeyHex = payload.params?.pqPublicKeyHex as string | undefined;
          const pqSignatureBase64 = payload.params?.pqSignatureBase64 as string | undefined;
          const pqAlgorithm = payload.params?.pqAlgorithm as string | undefined;
          if (
            !contractId ||
            !callerDid ||
            !inputBase64 ||
            typeof nonce !== "number" ||
            typeof timestamp !== "number" ||
            !publicKeyHex ||
            !signatureBase64
          ) {
            return err(id, -32602, "Missing signed tx fields");
          }
          const expectedNonce = vm.getNonce(callerDid);
          if (nonce !== expectedNonce) {
            return err(id, -32602, `Invalid nonce; expected ${expectedNonce}`);
          }
          const value = parseValue(payload.params?.value);
          const payloadToSign = JSON.stringify({
            contractId,
            callerDid,
            inputBase64,
            nonce,
            timestamp,
            value: value.toString(),
          });
          const digestHex = await sha256Hex(hashString(payloadToSign));
          const signature = decodeBase64(signatureBase64);
          const okSig = await ed.verify(signature, hexToBytes(digestHex), hexToBytes(publicKeyHex));
          if (!okSig) {
            return err(id, -32602, "Invalid signature");
          }
          if (pqSignatureBase64 || pqPublicKeyHex || vm.isPqSignatureRequired()) {
            if (!pqSignatureBase64 || !pqPublicKeyHex) {
              return err(id, -32602, "Missing PQ signature fields");
            }
            const okPq = await vm.verifyPqSignature(
              digestHex,
              pqSignatureBase64,
              pqPublicKeyHex,
              pqAlgorithm
            );
            if (!okPq) {
              return err(id, -32602, "Invalid PQ signature");
            }
          }
          const input = decodeBase64(inputBase64);
          const tx = await vm.submitTransaction(contractId, input, callerDid, value);
          vm.bumpNonce(callerDid);
          return ok(id, { txId: tx.id });
        }
        case "vm_submitSignedBatch": {
          const transactions = payload.params?.transactions as Array<Record<string, unknown>> | undefined;
          if (!transactions || !Array.isArray(transactions)) {
            return err(id, -32602, "Missing transactions array");
          }
          const results = [];
          for (const item of transactions) {
            const contractId = item.contractId as string;
            const callerDid = item.callerDid as string;
            const inputBase64 = item.inputBase64 as string;
            const nonce = item.nonce as number;
            const timestamp = item.timestamp as number;
            const publicKeyHex = item.publicKeyHex as string;
            const signatureBase64 = item.signatureBase64 as string;
            const pqPublicKeyHex = item.pqPublicKeyHex as string | undefined;
            const pqSignatureBase64 = item.pqSignatureBase64 as string | undefined;
            const pqAlgorithm = item.pqAlgorithm as string | undefined;
            if (
              !contractId ||
              !callerDid ||
              !inputBase64 ||
              typeof nonce !== "number" ||
              typeof timestamp !== "number" ||
              !publicKeyHex ||
              !signatureBase64
            ) {
              results.push({ error: "Missing signed tx fields" });
              continue;
            }
            const expectedNonce = vm.getNonce(callerDid);
            if (nonce !== expectedNonce) {
              results.push({ error: `Invalid nonce; expected ${expectedNonce}` });
              continue;
            }
            const value = parseValue(item.value);
            const payloadToSign = JSON.stringify({
              contractId,
              callerDid,
              inputBase64,
              nonce,
              timestamp,
              value: value.toString(),
            });
            const digestHex = await sha256Hex(hashString(payloadToSign));
            const signature = decodeBase64(signatureBase64);
            const okSig = await ed.verify(signature, hexToBytes(digestHex), hexToBytes(publicKeyHex));
            if (!okSig) {
              results.push({ error: "Invalid signature" });
              continue;
            }
            if (pqSignatureBase64 || pqPublicKeyHex || vm.isPqSignatureRequired()) {
              if (!pqSignatureBase64 || !pqPublicKeyHex) {
                results.push({ error: "Missing PQ signature fields" });
                continue;
              }
              const okPq = await vm.verifyPqSignature(
                digestHex,
                pqSignatureBase64,
                pqPublicKeyHex,
                pqAlgorithm
              );
              if (!okPq) {
                results.push({ error: "Invalid PQ signature" });
                continue;
              }
            }
            const input = decodeBase64(inputBase64);
            const tx = await vm.submitTransaction(contractId, input, callerDid, value);
            vm.bumpNonce(callerDid);
            results.push({ txId: tx.id });
          }
          return ok(id, results);
        }
        case "vm_submitBatch": {
          const transactions = payload.params?.transactions as Array<Record<string, unknown>> | undefined;
          if (!transactions || !Array.isArray(transactions)) {
            return err(id, -32602, "Missing transactions array");
          }
          const results = [];
          for (const item of transactions) {
            const contractId = item.contractId as string;
            const callerDid = item.callerDid as string;
            const inputBase64 = item.inputBase64 as string;
            if (!contractId || !callerDid || !inputBase64) {
              results.push({ error: "Missing contractId/callerDid/inputBase64" });
              continue;
            }
            const input = decodeBase64(inputBase64);
            const value = parseValue(item.value);
            const tx = await vm.submitTransaction(contractId, input, callerDid, value);
            results.push({ txId: tx.id });
          }
          return ok(id, results);
        }
        case "vm_simulate": {
          const contractId = payload.params?.contractId as string;
          const callerDid = payload.params?.callerDid as string;
          const inputBase64 = payload.params?.inputBase64 as string;
          if (!contractId || !callerDid || !inputBase64) {
            return err(id, -32602, "Missing contractId/callerDid/inputBase64");
          }
          const input = decodeBase64(inputBase64);
          const value = parseValue(payload.params?.value);
          const sim = await vm.simulateCall(contractId, input, callerDid, value);
          return ok(id, {
            status: sim.status,
            resultBase64: Buffer.from(sim.result).toString("base64"),
            events: sim.events.map((event) => ({
              type: event.type,
              dataBase64: Buffer.from(event.data).toString("base64"),
            })),
            gasUsed: sim.gasUsed,
          });
        }
        case "vm_execute": {
          const contractId = payload.params?.contractId as string;
          const callerDid = payload.params?.callerDid as string;
          const inputBase64 = payload.params?.inputBase64 as string;
          if (!contractId || !callerDid || !inputBase64) {
            return err(id, -32602, "Missing contractId/callerDid/inputBase64");
          }
          const input = decodeBase64(inputBase64);
          const value = parseValue(payload.params?.value);
          const receipt = await vm.executeTransaction(contractId, input, callerDid, value);
          return ok(id, {
            status: receipt.status,
            resultBase64: Buffer.from(receipt.result).toString("base64"),
            events: receipt.events.map((event) => ({
              type: event.type,
              dataBase64: Buffer.from(event.data).toString("base64"),
            })),
            receiptHash: receipt.receiptHash,
            gasUsed: receipt.gasUsed ?? 0,
          });
        }
        case "vm_mine": {
          const block = await vm.mineBlock();
          return ok(id, block);
        }
        case "vm_blocks": {
          return ok(id, vm.getBlocks());
        }
        case "vm_chainId": {
          return ok(id, { chainId: vm.getChainId() });
        }
        case "vm_tx": {
          const txId = payload.params?.txId as string;
          if (!txId) {
            return err(id, -32602, "Missing txId");
          }
          return ok(id, vm.getTransaction(txId) ?? null);
        }
        case "vm_receipt": {
          const txId = payload.params?.txId as string;
          if (!txId) {
            return err(id, -32602, "Missing txId");
          }
          return ok(id, vm.getReceipt(txId) ?? null);
        }
        case "vm_stateProof": {
          const keyHex = payload.params?.keyHex as string;
          if (!keyHex) {
            return err(id, -32602, "Missing keyHex");
          }
          const proof = await vm.getStateProof(keyHex);
          return ok(id, proof);
        }
        case "vm_quantumStateRoot": {
          const root = await vm.computeQuantumStateRoot();
          return ok(id, { root });
        }
        case "vm_quantumStateProof": {
          const keyHex = payload.params?.keyHex as string;
          if (!keyHex) {
            return err(id, -32602, "Missing keyHex");
          }
          const proof = await vm.getQuantumStateProof(keyHex);
          return ok(id, proof);
        }
        case "vm_blockHeader": {
          const height = payload.params?.height as number;
          if (typeof height !== "number") {
            return err(id, -32602, "Missing height");
          }
          return ok(id, vm.getBlockHeader(height));
        }
        case "vm_feePolicy": {
          return ok(id, vm.getFeePolicy());
        }
        case "vm_feeEstimate": {
          const bytes = payload.params?.bytes as number;
          if (typeof bytes !== "number") {
            return err(id, -32602, "Missing bytes");
          }
          return ok(id, { fee: vm.estimateFee(bytes).toString() });
        }
        case "vm_gasPolicy": {
          return ok(id, vm.getGasPolicy());
        }
        case "vm_gasEstimate": {
          const bytes = payload.params?.bytes as number;
          if (typeof bytes !== "number") {
            return err(id, -32602, "Missing bytes");
          }
          return ok(id, { gas: vm.estimateGas(bytes) });
        }
        case "vm_snapshot": {
          const snapshot = vm.exportStateSnapshot();
          return ok(id, snapshot);
        }
        case "vm_storageGet": {
          const keyHex = payload.params?.keyHex as string;
          if (!keyHex) {
            return err(id, -32602, "Missing keyHex");
          }
          const value = vm.getStorageValue(keyHex);
          return ok(id, { valueHex: value ? Buffer.from(value).toString("hex") : null });
        }
        case "vm_astraBalance": {
          const did = payload.params?.did as string;
          if (!did) {
            return err(id, -32602, "Missing did");
          }
          const key = `astra:erc20:balance:${did}`;
          const keyHex = Buffer.from(key, "utf8").toString("hex");
          const value = vm.getStorageValue(keyHex);
          if (!value || value.length < 8) {
            return ok(id, { balance: "0" });
          }
          const view = new DataView(value.buffer, value.byteOffset, value.byteLength);
          const balance = view.getBigUint64(0, true);
          return ok(id, { balance: balance.toString() });
        }
        case "vm_txProof": {
          const txId = payload.params?.txId as string;
          if (!txId) {
            return err(id, -32602, "Missing txId");
          }
          const proof = await vm.getTxProof(txId);
          return ok(id, proof);
        }
        case "vm_receiptProof": {
          const txId = payload.params?.txId as string;
          if (!txId) {
            return err(id, -32602, "Missing txId");
          }
          const proof = await vm.getReceiptProof(txId);
          return ok(id, proof);
        }
        case "vm_verifyProof": {
          const leaf = payload.params?.leaf as string;
          const root = payload.params?.root as string;
          const proof = payload.params?.proof as Array<{ sibling: string; position: "left" | "right" }>;
          if (!leaf || !root || !Array.isArray(proof)) {
            return err(id, -32602, "Missing leaf/root/proof");
          }
          const okProof = await verifyMerkleProof(leaf, proof, root);
          return ok(id, { valid: okProof });
        }
        case "vm_sealed": {
          return ok(id, vm.getSealedArchives());
        }
        case "vm_hostAbi": {
          return ok(id, { version: HOST_ABI_VERSION, modules: HOST_IMPORT_MODULES });
        }
        default:
          return err(id, -32601, `Unknown method: ${payload.method}`);
      }
    } catch (error) {
      return err(id, -32000, error instanceof Error ? error.message : "Unknown error");
    }
  };
}
