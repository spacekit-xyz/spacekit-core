/**
 * Ethereum proof-bridge adapter. Optional dependency: viem.
 * Submits state roots and bundle hashes to an L1 contract.
 */

import type {
  ProofBridgeAdapter,
  ProofBridgeChainConfig,
  ProofBridgePayload,
  ProofBridgeSubmitResult,
} from "../proof_bridge.js";

const CHAIN_ID_PREFIX = "ethereum";

function hexToBytes32(hex: string): `0x${string}` {
  const h = hex.startsWith("0x") ? hex.slice(2) : hex;
  const padded = h.padStart(64, "0").slice(0, 64);
  return `0x${padded}` as `0x${string}`;
}

function createStubAdapter(
  chainId: string,
  reason: string
): ProofBridgeAdapter {
  return {
    chainId,
    isReady: () => false,
    async submit(): Promise<ProofBridgeSubmitResult> {
      return { success: false, error: reason };
    },
  };
}

export async function createEthereumAdapter(
  config: ProofBridgeChainConfig
): Promise<ProofBridgeAdapter | null> {
  if (!config.chainId.toLowerCase().startsWith(CHAIN_ID_PREFIX) || !config.enabled) return null;
  if (!config.rpcUrl || !config.contractAddress) return null;

  let viem: Record<string, unknown>;
  let viemAccounts: Record<string, unknown>;
  try {
    viem = await import("viem") as Record<string, unknown>;
    viemAccounts = await import("viem/accounts") as Record<string, unknown>;
  } catch {
    return createStubAdapter(config.chainId, "viem not installed");
  }

  const createWalletClient = viem.createWalletClient as (opts: unknown) => unknown;
  const createPublicClient = viem.createPublicClient as (opts: unknown) => { waitForTransactionReceipt: (opts: { hash: string }) => Promise<{ status: string }> };
  const http = viem.http as (url: string) => unknown;
  const encodeFunctionData = viem.encodeFunctionData as (opts: unknown) => `0x${string}`;
  const chain = config.chainId.toLowerCase().includes("sepolia") ? viem.sepolia : viem.mainnet;
  const transport = http(config.rpcUrl);
  const publicClient = createPublicClient({ transport, chain });

  let walletClient: { sendTransaction: (opts: unknown) => Promise<string> } | null = null;
  let account: `0x${string}` | undefined;
  if (config.privateKeyHex) {
    const pk = (config.privateKeyHex.startsWith("0x") ? config.privateKeyHex : `0x${config.privateKeyHex}`) as `0x${string}`;
    const acc = (viemAccounts.privateKeyToAccount as (k: `0x${string}`) => { address: `0x${string}` })(pk);
    walletClient = createWalletClient({ account: acc, transport, chain }) as { sendTransaction: (opts: unknown) => Promise<string> };
    account = acc.address;
  }

  const contractAddress = config.contractAddress as `0x${string}`;
  const gasLimit = config.gasLimit != null ? BigInt(config.gasLimit) : 200_000n;

  const adapter: ProofBridgeAdapter = {
    chainId: config.chainId,
    isReady: () => true,
    async submit(payload: ProofBridgePayload): Promise<ProofBridgeSubmitResult> {
      try {
        if (payload.kind === "state_root") {
          const stateRootBytes = hexToBytes32(payload.stateRoot);
          const blockNum = BigInt(payload.blockHeight);
          if (!walletClient || !account)
            return { success: false, error: "no wallet configured" };
          const data = encodeFunctionData({
            abi: [
              {
                type: "function",
                name: "commitStateRoot",
                stateMutability: "nonpayable",
                inputs: [
                  { name: "blockNumber", type: "uint256" },
                  { name: "stateRoot", type: "bytes32" },
                ],
              },
            ],
            functionName: "commitStateRoot",
            args: [blockNum, stateRootBytes],
          });
          const hash = await walletClient.sendTransaction({
            to: contractAddress,
            data,
            gas: gasLimit,
            account,
          });
          const receipt = await publicClient.waitForTransactionReceipt({ hash });
          return receipt.status === "success"
            ? { success: true, id: hash }
            : { success: false, error: "reverted" };
        }
        if (payload.kind === "bundle" || payload.kind === "signed_bundle") {
          const bundleHash = payload.kind === "bundle" ? payload.bundle.bundleHash : payload.signed.bundleHash;
          const bundleHashBytes = hexToBytes32(bundleHash);
          if (!walletClient || !account)
            return { success: false, error: "no wallet configured" };
          const data = encodeFunctionData({
            abi: [
              {
                type: "function",
                name: "commitBundle",
                stateMutability: "nonpayable",
                inputs: [{ name: "bundleHash", type: "bytes32" }],
              },
            ],
            functionName: "commitBundle",
            args: [bundleHashBytes],
          });
          const hash = await walletClient.sendTransaction({
            to: contractAddress,
            data,
            gas: gasLimit,
            account,
          });
          const receipt = await publicClient.waitForTransactionReceipt({ hash });
          return receipt.status === "success"
            ? { success: true, id: hash }
            : { success: false, error: "reverted" };
        }
        return { success: false, error: "unsupported payload kind" };
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        return { success: false, error: msg };
      }
    },
  };

  return adapter;
}
