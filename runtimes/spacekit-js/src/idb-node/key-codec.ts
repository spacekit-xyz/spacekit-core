/**
 * Key encoding for IndexedDB-compatible lexicographic ordering.
 * Order: number < Date < string < binary < Array
 * Used by the WAL backend for range queries and cursors (no SQL).
 */

import { Buffer } from "node:buffer";

export const KeyType = {
  NUMBER: 0x10,
  DATE: 0x20,
  STRING: 0x30,
  BINARY: 0x40,
  ARRAY: 0x50,
} as const;

export function encodeKey(key: IDBValidKey): Buffer {
  if (typeof key === "number") return encodeNumber(key);
  if (key instanceof Date) return encodeDate(key);
  if (typeof key === "string") return encodeString(key);
  if (key instanceof ArrayBuffer || ArrayBuffer.isView(key)) return encodeBinary(key);
  if (Array.isArray(key)) return encodeArray(key);
  throw new TypeError(`Invalid key type: ${typeof key}`);
}

function encodeNumber(n: number): Buffer {
  const buf = Buffer.allocUnsafe(9);
  buf[0] = KeyType.NUMBER;
  const view = new DataView(buf.buffer, buf.byteOffset + 1, 8);
  view.setFloat64(0, n, false);
  if (n < 0 || Object.is(n, -0)) {
    for (let i = 1; i <= 8; i++) buf[i] ^= 0xff;
  } else {
    buf[1] ^= 0x80;
  }
  return buf;
}

function encodeDate(d: Date): Buffer {
  const buf = Buffer.allocUnsafe(9);
  buf[0] = KeyType.DATE;
  new DataView(buf.buffer, buf.byteOffset + 1, 8).setFloat64(0, d.getTime(), false);
  buf[1] ^= 0x80;
  return buf;
}

function encodeString(s: string): Buffer {
  const encoded = Buffer.from(s, "utf8");
  const buf = Buffer.allocUnsafe(1 + encoded.length + 1);
  buf[0] = KeyType.STRING;
  encoded.copy(buf, 1);
  buf[buf.length - 1] = 0x00;
  return buf;
}

function encodeBinary(b: ArrayBuffer | ArrayBufferView): Buffer {
  const bytes = b instanceof ArrayBuffer ? new Uint8Array(b) : new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
  const buf = Buffer.allocUnsafe(1 + bytes.length + 1);
  buf[0] = KeyType.BINARY;
  buf.set(bytes, 1);
  buf[buf.length - 1] = 0x00;
  return buf;
}

function encodeArray(arr: IDBValidKey[]): Buffer {
  const parts = arr.map(encodeKey);
  const total = 1 + parts.reduce((n, p) => n + 4 + p.length, 0);
  const buf = Buffer.allocUnsafe(total);
  buf[0] = KeyType.ARRAY;
  let offset = 1;
  for (const p of parts) {
    buf.writeUInt32BE(p.length, offset);
    offset += 4;
    p.copy(buf, offset);
    offset += p.length;
  }
  return buf;
}

export function decodeKey(buf: Buffer): IDBValidKey {
  if (buf.length === 0) throw new Error("decodeKey: empty buffer");
  const type = buf[0];
  const rest = buf.subarray(1);
  switch (type) {
    case KeyType.NUMBER:
      return decodeNumber(rest);
    case KeyType.DATE:
      return decodeDate(rest);
    case KeyType.STRING:
      return rest.subarray(0, rest.length - 1).toString("utf8");
    case KeyType.BINARY: {
      const slice = rest.subarray(0, rest.length - 1);
      return slice.buffer.slice(slice.byteOffset, slice.byteOffset + slice.byteLength) as ArrayBuffer;
    }
    case KeyType.ARRAY:
      return decodeArray(rest);
    default:
      throw new Error(`Unknown key type: ${type}`);
  }
}

function decodeNumber(buf: Buffer): number {
  const b = Buffer.from(buf.subarray(0, 8));
  if (b[0]! & 0x80) {
    for (let i = 0; i < 8; i++) b[i]! ^= 0xff;
  } else {
    b[0]! ^= 0x80;
  }
  return new DataView(b.buffer, b.byteOffset, 8).getFloat64(0, false);
}

function decodeDate(buf: Buffer): Date {
  const b = Buffer.from(buf.subarray(0, 8));
  b[0]! ^= 0x80;
  const ms = new DataView(b.buffer, b.byteOffset, 8).getFloat64(0, false);
  return new Date(ms);
}

function decodeArray(buf: Buffer): IDBValidKey[] {
  const arr: IDBValidKey[] = [];
  let offset = 0;
  while (offset < buf.length) {
    const len = buf.readUInt32BE(offset);
    offset += 4;
    arr.push(decodeKey(buf.subarray(offset, offset + len)));
    offset += len;
  }
  return arr;
}

/** Compare two encoded keys (lexicographic). Returns -1, 0, or 1. */
export function compareEncodedKeys(a: Buffer, b: Buffer): number {
  const minLen = Math.min(a.length, b.length);
  for (let i = 0; i < minLen; i++) {
    if (a[i] !== b[i]) return a[i]! < b[i]! ? -1 : 1;
  }
  if (a.length !== b.length) return a.length < b.length ? -1 : 1;
  return 0;
}
