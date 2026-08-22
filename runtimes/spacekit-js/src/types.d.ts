declare module "@noble/ed25519";
declare module "wasm-metering" {
  export type MeteringOptions = {
    meterType?: "i32" | "i64" | "f32" | "f64";
    costTable?: Record<string, unknown>;
    moduleStr?: string;
    fieldStr?: string;
  };
  export function meterWASM(
    wasm: Uint8Array | ArrayBuffer,
    options?: MeteringOptions
  ): Uint8Array;
  const metering: { meterWASM: typeof meterWASM };
  export default metering;
}
