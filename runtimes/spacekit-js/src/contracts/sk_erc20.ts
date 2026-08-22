import type { HostContext } from "../host.js";
import { callSpacekitMain } from "../runtime.js";

const OP_MINT = 1;
const OP_TRANSFER = 2;
const OP_BALANCE = 3;
const OP_TOTAL_SUPPLY = 4;
const OP_METADATA = 5;

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

function decodeString(bytes: Uint8Array, offset: number): { value: string; next: number } {
  const view = new DataView(bytes.buffer, bytes.byteOffset + offset, bytes.byteLength - offset);
  const len = view.getUint16(0, true);
  const start = offset + 2;
  const end = start + len;
  const slice = bytes.slice(start, end);
  const value = new TextDecoder().decode(slice);
  return { value, next: end };
}

export interface SkErc20Metadata {
  version: number;
  name: string;
  symbol: string;
  decimals: number;
}

export class SkErc20Client {
  private ctx: HostContext;
  private instance: WebAssembly.Instance;

  constructor(ctx: HostContext, instance: WebAssembly.Instance) {
    this.ctx = ctx;
    this.instance = instance;
  }

  mint(toDid: string, amount: bigint): boolean {
    const payload = concat([
      Uint8Array.of(OP_MINT),
      encodeString(toDid),
      encodeU64(amount),
    ]);
    const { status } = callSpacekitMain(this.ctx, this.instance, payload);
    return status > 0;
  }

  transfer(fromDid: string, toDid: string, amount: bigint): boolean {
    const payload = concat([
      Uint8Array.of(OP_TRANSFER),
      encodeString(fromDid),
      encodeString(toDid),
      encodeU64(amount),
    ]);
    const { status } = callSpacekitMain(this.ctx, this.instance, payload);
    return status > 0;
  }

  balanceOf(did: string): bigint {
    const payload = concat([Uint8Array.of(OP_BALANCE), encodeString(did)]);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0 || result.length < 8) {
      return 0n;
    }
    return decodeU64(result);
  }

  totalSupply(): bigint {
    const payload = Uint8Array.of(OP_TOTAL_SUPPLY);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0 || result.length < 8) {
      return 0n;
    }
    return decodeU64(result);
  }

  metadata(): SkErc20Metadata {
    const payload = Uint8Array.of(OP_METADATA);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0 || result.length < 1) {
      return { version: 0, name: "", symbol: "", decimals: 0 };
    }
    let offset = 0;
    const version = result[offset];
    offset += 1;
    const nameDecoded = decodeString(result, offset);
    offset = nameDecoded.next;
    const symbolDecoded = decodeString(result, offset);
    offset = symbolDecoded.next;
    const decimals = result[offset] ?? 0;
    return {
      version,
      name: nameDecoded.value,
      symbol: symbolDecoded.value,
      decimals,
    };
  }
}
