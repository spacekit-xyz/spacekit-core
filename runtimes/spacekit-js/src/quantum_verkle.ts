export type QuantumVerkleWasmModule = {
  default: (wasmUrl: string | URL | Uint8Array | ArrayBuffer) => Promise<void>;
  QuantumVerkleWasm: new () => {
    root_hex(): string;
    set(addressHex: string, keyHex: string, valueHex: string, auxHex?: string | null): void;
    get(addressHex: string, keyHex: string): string | null;
    create_proof(addressHex: string, keyHex: string): Uint8Array;
    verify_proof(proofBytes: Uint8Array, addressHex: string, keyHex: string, valueHex: string): boolean;
    create_multi_proof(addresses: string[], keys: string[]): Uint8Array;
    verify_multi_proof(proofBytes: Uint8Array, addresses: string[], keys: string[], values: string[]): boolean;
  };
  setup_params: (level: number, profile: number) => { paramsId: string; paramsBlobHex: string };
};

export type QuantumVerkleWasmLoaderOptions = {
  wasmUrl?: string;
  moduleUrl?: string;
};

let cachedModule: Promise<QuantumVerkleWasmModule> | null = null;

export async function loadQuantumVerkleWasm(options: QuantumVerkleWasmLoaderOptions = {}): Promise<QuantumVerkleWasmModule> {
  if (cachedModule) {
    return cachedModule;
  }
  cachedModule = (async () => {
    const isNode = typeof process !== "undefined" && Boolean(process.versions?.node);
    const resolveUrl = (input: string | undefined, fallbackRelative: string) => {
      if (!input) {
        return new URL(fallbackRelative, import.meta.url).toString();
      }
      if (input.startsWith("http://") || input.startsWith("https://") || input.startsWith("file:")) {
        return input;
      }
      if (input.startsWith("/")) {
        if (typeof window !== "undefined" && window.location?.origin) {
          return new URL(input, window.location.origin).toString();
        }
        return input;
      }
      return input;
    };
    const moduleUrl = resolveUrl(options.moduleUrl, "../wasm/quantum_verkle_wasm.js");
    const wasmUrl = resolveUrl(options.wasmUrl, "../wasm/quantum_verkle_wasm_bg.wasm");
    const isBrowser = !isNode && typeof window !== "undefined";
    const loadModule = async (url: string): Promise<QuantumVerkleWasmModule> => {
      if (!isBrowser || url.startsWith("file:")) {
        return (await import(/* @vite-ignore */ url)) as QuantumVerkleWasmModule;
      }
      if (url.startsWith("/") || url.startsWith("http://") || url.startsWith("https://")) {
        const response = await fetch(url);
        if (!response.ok) {
          throw new Error(`Failed to fetch wasm module: ${response.status} ${response.statusText}`);
        }
        const code = await response.text();
        const blobUrl = URL.createObjectURL(new Blob([code], { type: "text/javascript" }));
        try {
          return (await import(/* @vite-ignore */ blobUrl)) as QuantumVerkleWasmModule;
        } finally {
          URL.revokeObjectURL(blobUrl);
        }
      }
      return (await import(/* @vite-ignore */ url)) as QuantumVerkleWasmModule;
    };
    const mod = await loadModule(moduleUrl);
    if (isNode) {
      const { readFile } = await import("node:fs/promises");
      const { fileURLToPath } = await import("node:url");
      const wasmPath = wasmUrl.startsWith("file:") ? fileURLToPath(wasmUrl) : wasmUrl;
      const wasmBytes = await readFile(wasmPath);
      await mod.default(wasmBytes);
    } else {
      if (wasmUrl.startsWith("/") || wasmUrl.startsWith("http://") || wasmUrl.startsWith("https://")) {
        const response = await fetch(wasmUrl);
        if (!response.ok) {
          throw new Error(`Failed to fetch wasm bytes: ${response.status} ${response.statusText}`);
        }
        const wasmBytes = await response.arrayBuffer();
        await mod.default(wasmBytes);
      } else {
        await mod.default(wasmUrl);
      }
    }
    return mod;
  })();
  return cachedModule;
}
