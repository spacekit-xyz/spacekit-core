/** Stub declarations so adapters compile without optional deps installed. */
declare module "viem" {
  export const createWalletClient: (...args: unknown[]) => unknown;
  export const createPublicClient: (...args: unknown[]) => unknown;
  export const http: (...args: unknown[]) => unknown;
  export const encodeFunctionData: (...args: unknown[]) => unknown;
  export const mainnet: { id: number; name: string };
  export const sepolia: { id: number; name: string };
}
declare module "viem/accounts" {
  export function privateKeyToAccount(key: `0x${string}`): { address: `0x${string}` };
}
declare module "@solana/web3.js" {
  export class Connection {
    constructor(endpoint: string, opts?: { commitment?: string });
    sendTransaction(tx: unknown, signers: unknown[], opts?: unknown): Promise<string>;
    getLatestBlockhash(commitment?: string): Promise<{ blockhash: string; lastValidBlockHeight: number }>;
    confirmTransaction(args: unknown): Promise<void>;
  }
  export class Keypair {
    static fromSecretKey(bytes: Uint8Array): Keypair;
  }
  export class PublicKey {
    constructor(value: string);
  }
  export class Transaction {
    add(ix: unknown): Transaction;
  }
  export class TransactionInstruction {
    constructor(opts: { keys: unknown[]; programId: PublicKey; data: Buffer });
  }
}
