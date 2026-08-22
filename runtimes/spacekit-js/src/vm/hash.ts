const encoder = new TextEncoder();

export async function sha256Hex(data: Uint8Array): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  if (subtle) {
    const digest = await subtle.digest("SHA-256", data.slice().buffer);
    return toHex(new Uint8Array(digest));
  }

  const { sha256 } = await import("@noble/hashes/sha2");
  return toHex(sha256(data));
}

export function hashString(value: string): Uint8Array {
  return encoder.encode(value);
}

function toHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) {
    out += b.toString(16).padStart(2, "0");
  }
  return out;
}
