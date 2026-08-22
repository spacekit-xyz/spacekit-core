import type { SpacekitVm } from "./spacekitvm.js";

export type ContractCallMode = "execute" | "submit" | "simulate";

export interface ContractCallCodec {
  encode(method: string, args: unknown[]): Uint8Array;
  decode<T>(method: string, output: Uint8Array): T;
}

export interface VmContractCallerOptions {
  callerDid: string;
  codec: ContractCallCodec;
  mode?: ContractCallMode;
  value?: bigint;
}

export function createVmContractCaller(vm: SpacekitVm, options: VmContractCallerOptions) {
  const mode: ContractCallMode = options.mode ?? "execute";
  const value = options.value ?? 0n;

  return async function callContract<T>(
    contractId: string,
    method: string,
    args: unknown[]
  ): Promise<T> {
    const input = options.codec.encode(method, args);
    if (mode === "simulate") {
      const sim = await vm.simulateCall(contractId, input, options.callerDid, value);
      if (sim.status <= 0) {
        throw new Error(`Simulated call failed: ${sim.status}`);
      }
      return options.codec.decode<T>(method, sim.result);
    }
    if (mode === "submit") {
      await vm.submitTransaction(contractId, input, options.callerDid, value);
      return options.codec.decode<T>(method, new Uint8Array());
    }
    const receipt = await vm.executeTransaction(contractId, input, options.callerDid, value);
    if (receipt.status <= 0) {
      throw new Error(`Contract call failed: ${receipt.status}`);
    }
    return options.codec.decode<T>(method, receipt.result);
  };
}

export const JsonContractCodec: ContractCallCodec = {
  encode(method: string, args: unknown[]) {
    const payload = JSON.stringify({ method, args });
    return new TextEncoder().encode(payload);
  },
  decode<T>(_method: string, output: Uint8Array) {
    if (!output || output.length === 0) {
      return undefined as T;
    }
    const text = new TextDecoder().decode(output);
    try {
      return JSON.parse(text) as T;
    } catch {
      return text as unknown as T;
    }
  },
};
