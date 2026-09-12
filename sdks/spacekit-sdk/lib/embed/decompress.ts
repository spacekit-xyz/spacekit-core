/**
 * Content-ref decompression for the web-package loader.
 *
 * Storage may hold (and transfer) an app's assets compressed — either as a plain
 * codec named on `ContentRef.compression`, or wrapped in the `spacekit-compressor`
 * `SKADCMPR` adaptive envelope (used when content is compressed *before* being
 * encrypted, where HTTP `Content-Encoding` can't help). The loader hashes and
 * runs the *decompressed* bytes, so this decodes a ref's fetched bytes back to
 * their original form before integrity-checking — matching how the `.spkg` path
 * already hashes over inflated entries.
 *
 * Everything here is browser-native: gzip/deflate go through `DecompressionStream`
 * with zero dependencies. The `SKADCMPR` `Stored` and `Binary` (gzip) inner
 * methods decode natively; its Pattern/Hybrid methods and codecs like zstd/brotli
 * need a WASM decoder and throw a clear error until one is wired in.
 *
 * Backward-compatible: empty / "none" / "stored" compression passes straight
 * through, so uncompressed content behaves exactly as before.
 */

/** "SKADCMPR" — the spacekit-compressor adaptive envelope magic. */
const ADAPTIVE_MAGIC = [0x53, 0x4b, 0x41, 0x44, 0x43, 0x4d, 0x50, 0x52];
const ADAPTIVE_HEADER_LEN = 8 + 1 + 1 + 8 + 8; // magic + version + tag + originalLen + payloadLen

type NativeFormat = "gzip" | "deflate" | "deflate-raw";

function codecName(compression: unknown): string {
  if (!compression) return "";
  if (typeof compression === "string") return compression.trim().toLowerCase();
  if (typeof compression === "object") {
    const o = compression as Record<string, unknown>;
    const v = o.type ?? o.codec ?? o.algorithm ?? o.method ?? o.name;
    if (typeof v === "string") return v.trim().toLowerCase();
  }
  return "";
}

function hasAdaptiveMagic(bytes: Uint8Array): boolean {
  if (bytes.length < ADAPTIVE_HEADER_LEN) return false;
  for (let i = 0; i < ADAPTIVE_MAGIC.length; i++) {
    if (bytes[i] !== ADAPTIVE_MAGIC[i]) return false;
  }
  return true;
}

async function inflate(bytes: Uint8Array, format: NativeFormat): Promise<Uint8Array> {
  if (typeof DecompressionStream === "undefined") {
    throw new Error(`${format} decompression unavailable (DecompressionStream missing)`);
  }
  const stream = new Blob([new Uint8Array(bytes)])
    .stream()
    .pipeThrough(new DecompressionStream(format));
  const buf = await new Response(stream).arrayBuffer();
  return new Uint8Array(buf);
}

/** Map an SKADCMPR (version, tag) to its inner method name. */
function adaptiveMethod(version: number, tag: number): string {
  if (tag === 0) return "stored";
  if (tag === 2) return "binary"; // gzip, both v1 and v2
  if (version === 1 && tag === 1) return "pattern-v1";
  if (version === 1 && tag === 3) return "hybrid-v1";
  if (version === 2 && tag === 4) return "pattern-v2";
  if (version === 2 && tag === 5) return "hybrid-v2";
  throw new Error(`SKADCMPR: unknown method tag ${tag} for envelope version ${version}`);
}

async function decodeAdaptiveEnvelope(bytes: Uint8Array): Promise<Uint8Array> {
  const version = bytes[8];
  if (version !== 1 && version !== 2) {
    throw new Error(`SKADCMPR: unsupported envelope version ${version}`);
  }
  const tag = bytes[9];
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const originalLen = Number(view.getBigUint64(10, true));
  const payloadLen = Number(view.getBigUint64(18, true));
  if (ADAPTIVE_HEADER_LEN + payloadLen !== bytes.length) {
    throw new Error(
      `SKADCMPR: envelope length mismatch (declared ${ADAPTIVE_HEADER_LEN + payloadLen}, got ${bytes.length})`,
    );
  }
  const payload = bytes.subarray(ADAPTIVE_HEADER_LEN);

  const method = adaptiveMethod(version, tag);
  let out: Uint8Array;
  if (method === "stored") {
    out = payload.slice();
  } else if (method === "binary") {
    out = await inflate(payload, "gzip");
  } else {
    throw new Error(
      `SKADCMPR: method '${method}' needs the pattern decoder, which is not available in the browser yet`,
    );
  }
  if (out.length !== originalLen) {
    throw new Error(`SKADCMPR: decoded length ${out.length} does not match declared ${originalLen}`);
  }
  return out;
}

/**
 * Decode a content ref's fetched bytes to their original form.
 *
 * Auto-detects the `SKADCMPR` envelope regardless of the declared codec; otherwise
 * dispatches on `ContentRef.compression`. Unsupported codecs throw a clear error
 * rather than silently serving compressed bytes.
 */
export async function decodeContentRef(bytes: Uint8Array, compression: unknown): Promise<Uint8Array> {
  if (bytes.length === 0) return bytes;
  if (hasAdaptiveMagic(bytes)) return decodeAdaptiveEnvelope(bytes);

  const codec = codecName(compression);
  switch (codec) {
    case "":
    case "none":
    case "stored":
    case "identity":
    case "raw":
      return bytes;
    case "gzip":
    case "gz":
      return inflate(bytes, "gzip");
    case "deflate":
      return inflate(bytes, "deflate");
    case "deflate-raw":
    case "raw-deflate":
      return inflate(bytes, "deflate-raw");
    case "adaptive":
    case "skadcmpr":
      throw new Error("compression 'adaptive' was declared but no SKADCMPR envelope was found");
    case "zstd":
    case "zstandard":
    case "brotli":
    case "br":
    case "lz4":
    case "lzma":
    case "xz":
    case "pattern":
    case "hybrid":
      throw new Error(
        `compression codec '${codec}' is not supported in the browser yet (needs a WASM decoder)`,
      );
    default:
      throw new Error(`unknown compression codec '${codec}'`);
  }
}

/** True when this ref's declared codec (or SKADCMPR magic) can be decoded here. */
export function isDecodableCompression(compression: unknown): boolean {
  const codec = codecName(compression);
  return (
    codec === "" ||
    codec === "none" ||
    codec === "stored" ||
    codec === "identity" ||
    codec === "raw" ||
    codec === "gzip" ||
    codec === "gz" ||
    codec === "deflate" ||
    codec === "deflate-raw" ||
    codec === "raw-deflate" ||
    codec === "adaptive" ||
    codec === "skadcmpr"
  );
}
