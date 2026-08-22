import type { HostContext } from "./host.js";

export interface InstantiatedContract {
  instance: WebAssembly.Instance;
  module: WebAssembly.Module;
}

export async function instantiateWasm(
  wasm: ArrayBuffer | Uint8Array | Response,
  imports: WebAssembly.Imports
): Promise<InstantiatedContract> {
  const bytes = wasm instanceof Response ? await wasm.arrayBuffer() : wasm;
  const buffer = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  const arrayBuffer = buffer.slice().buffer;
  const module = await WebAssembly.compile(arrayBuffer);
  const instance = await WebAssembly.instantiate(module, imports);
  return { instance, module };
}

export function callSpacekitMain(
  ctx: HostContext,
  instance: WebAssembly.Instance,
  input: Uint8Array
): { result: Uint8Array; status: number } {
  const main = instance.exports.main as ((ptr: number, len: number) => number) | undefined;
  const getResult = instance.exports.get_result as ((ptr: number, len: number) => number) | undefined;
  if (!main || !getResult) {
    throw new Error("Contract does not export main/get_result");
  }

  const inputPtr = ctx.alloc(input.length);
  ctx.writeBytes(inputPtr, input);
  const status = main(inputPtr, input.length);

  if (status <= 0) {
    return { result: new Uint8Array(), status };
  }

  const outputPtr = ctx.alloc(status);
  const copied = getResult(outputPtr, status);
  const result = ctx.readBytes(outputPtr, copied);
  return { result, status };
}
