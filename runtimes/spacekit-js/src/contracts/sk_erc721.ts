import type { HostContext } from "../host.js";
import { callSpacekitMain } from "../runtime.js";

const OP_MINT = 1;
const OP_TRANSFER = 2;
const OP_OWNER_OF = 3;
const OP_SET_TOKEN_URI = 4;
const OP_TOKEN_URI = 5;
const OP_TOTAL_SUPPLY = 6;

function concat(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, p) => sum + p.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function encodeU16(value: number): Uint8Array {
  const buffer = new ArrayBuffer(2);
  new DataView(buffer).setUint16(0, value, true);
  return new Uint8Array(buffer);
}

function encodeU64(value: bigint): Uint8Array {
  const buffer = new ArrayBuffer(8);
  new DataView(buffer).setBigUint64(0, value, true);
  return new Uint8Array(buffer);
}

function encodeString(value: string): Uint8Array {
  const data = new TextEncoder().encode(value);
  return concat([encodeU16(data.length), data]);
}

function decodeU64(bytes: Uint8Array): bigint {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return view.getBigUint64(0, true);
}

export class SkErc721Client {
  private ctx: HostContext;
  private instance: WebAssembly.Instance;

  constructor(ctx: HostContext, instance: WebAssembly.Instance) {
    this.ctx = ctx;
    this.instance = instance;
  }

  mint(tokenId: bigint, ownerDid: string): boolean {
    const payload = concat([
      Uint8Array.of(OP_MINT),
      encodeU64(tokenId),
      encodeString(ownerDid),
    ]);
    const { status } = callSpacekitMain(this.ctx, this.instance, payload);
    return status > 0;
  }

  transfer(tokenId: bigint, fromDid: string, toDid: string): boolean {
    const payload = concat([
      Uint8Array.of(OP_TRANSFER),
      encodeU64(tokenId),
      encodeString(fromDid),
      encodeString(toDid),
    ]);
    const { status } = callSpacekitMain(this.ctx, this.instance, payload);
    return status > 0;
  }

  ownerOf(tokenId: bigint): string {
    const payload = concat([Uint8Array.of(OP_OWNER_OF), encodeU64(tokenId)]);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0) {
      return "";
    }
    return new TextDecoder().decode(result);
  }

  setTokenUri(tokenId: bigint, uri: string): boolean {
    const payload = concat([
      Uint8Array.of(OP_SET_TOKEN_URI),
      encodeU64(tokenId),
      encodeString(uri),
    ]);
    const { status } = callSpacekitMain(this.ctx, this.instance, payload);
    return status > 0;
  }

  tokenUri(tokenId: bigint): string {
    const payload = concat([Uint8Array.of(OP_TOKEN_URI), encodeU64(tokenId)]);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0) {
      return "";
    }
    return new TextDecoder().decode(result);
  }

  totalSupply(): bigint {
    const payload = Uint8Array.of(OP_TOTAL_SUPPLY);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0 || result.length < 8) {
      return 0n;
    }
    return decodeU64(result);
  }
}
