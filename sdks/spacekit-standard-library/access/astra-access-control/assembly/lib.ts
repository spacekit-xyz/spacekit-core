/// <reference path="../node_modules/assemblyscript/std/assembly/index.d.ts" />
// SpaceKit Access Control Contract (AssemblyScript)
// -------------------------------------------------
// ABI (short):
// OP_GRANT_ROLE (1) – [op][role:string][account:string] -> [1]
// OP_REVOKE_ROLE (2) – [op][role:string][account:string] -> [1]
// OP_HAS_ROLE (3) – [op][role:string][account:string] -> [1][has:u8]
// OP_SET_ADMIN (4) – [op][role:string][admin:string] -> [1]
// OP_GET_ADMIN (5) – [op][role:string] -> [1][admin:string]

import {
  Contract,
  ContractError,
  Result,
  contract,
} from "spacekit-as-sdk";

// ─────────────────────────────────────────────────────────────
// Host storage imports (spacekit_storage)
// ─────────────────────────────────────────────────────────────

@external("spacekit_storage", "storage_save")
declare function storage_save(
  key_ptr: usize,
  key_len: usize,
  data_ptr: usize,
  data_len: usize
): i32;

@external("spacekit_storage", "storage_load")
declare function storage_load(
  key_ptr: usize,
  key_len: usize,
  dest_ptr: usize,
  max_len: usize
): usize;

// ─────────────────────────────────────────────────────────────
// Opcodes
// ─────────────────────────────────────────────────────────────

const OP_GRANT_ROLE: u8 = 1;
const OP_REVOKE_ROLE: u8 = 2;
const OP_HAS_ROLE: u8 = 3;
const OP_SET_ADMIN: u8 = 4;
const OP_GET_ADMIN: u8 = 5;

// ─────────────────────────────────────────────────────────────
// Contract (internal; WASM exports functions, not classes)
// ─────────────────────────────────────────────────────────────

@contract
class SpaceKitAccessControl extends Contract {

  init(): void {
    // no state to initialize
  }

  handle(input: Uint8Array): Result<Uint8Array> {
    let cursor = 0;

    const op = readU8(input, cursor);
    cursor += 1;

    // [op][role:string][account:string]
    if (op == OP_GRANT_ROLE) {
      const role = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(role);

      const account = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(account);

      const key = roleMemberKey(role, account);
      const ok = storageSaveBytes(key, bytes1(1));
      if (!ok) return Result.err<Uint8Array>(ContractError.StorageError);

      return Result.ok(bytes1(1));

    // [op][role:string][account:string]
    } else if (op == OP_REVOKE_ROLE) {
      const role = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(role);

      const account = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(account);

      const key = roleMemberKey(role, account);
      const ok = storageSaveBytes(key, bytes1(0));
      if (!ok) return Result.err<Uint8Array>(ContractError.StorageError);

      return Result.ok(bytes1(1));

    // [op][role:string][account:string] -> [1][has:u8]
    } else if (op == OP_HAS_ROLE) {
      const role = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(role);

      const account = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(account);

      const key = roleMemberKey(role, account);
      const loaded = storageLoadBytes(key, 1);
      let has: u8 = 0;
      if (loaded != null && loaded.length > 0) {
        has = loaded[0];
      }

      const out = new Uint8Array(2);
      out[0] = 1;
      out[1] = has;
      return Result.ok(out);

    // [op][role:string][admin:string]
    } else if (op == OP_SET_ADMIN) {
      const role = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(role);

      const admin = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(admin);

      const key = roleAdminKey(role);
      const adminBytes = Uint8Array.wrap(String.UTF8.encode(admin, true));
      const ok = storageSaveBytes(key, adminBytes);
      if (!ok) return Result.err<Uint8Array>(ContractError.StorageError);

      return Result.ok(bytes1(1));

    // [op][role:string] -> [1][admin:string]
    } else if (op == OP_GET_ADMIN) {
      const role = readString(input, cursor);
      cursor += 2 + String.UTF8.byteLength(role);

      const key = roleAdminKey(role);
      const data = storageLoadBytes(key, 256);
      if (data == null || data.length == 0) {
        return Result.err<Uint8Array>(ContractError.StorageError);
      }

      const admin = String.UTF8.decode(data.buffer);
      const adminBytes = Uint8Array.wrap(String.UTF8.encode(admin, true));

      const out = new Uint8Array(1 + adminBytes.length);
      out[0] = 1;
      memory.copy(
        changetype<usize>(out.buffer) + 1,
        changetype<usize>(adminBytes.buffer),
        adminBytes.length
      );
      return Result.ok(out);
    }

    return Result.err<Uint8Array>(ContractError.InvalidInput);
  }
}

// Singleton and result buffer – same pattern as Rust spacekit_contract! macro
let contractInstance: SpaceKitAccessControl | null = null;
const resultBuf = new Uint8Array(4096);
let resultLen: i32 = 0;

/** Lazy init (called from main), no separate export needed for VM. */
function ensureInit(): void {
  if (contractInstance == null) {
    contractInstance = new SpaceKitAccessControl();
    contractInstance!.init();
  }
}

/**
 * Main entry (Rust macro parity).
 * Signature: main(input_ptr: i32, input_len: i32) -> i32
 * Returns: result length on success, or negative ContractError code on failure.
 */
export function main(inputPtr: i32, inputLen: i32): i32 {
  ensureInit();
  const inst = contractInstance!;
  const input = new Uint8Array(inputLen);
  memory.copy(changetype<usize>(input.buffer), inputPtr as usize, inputLen);
  const res = inst.handle(input);
  if (!res.isOk()) return res.code();
  const data = res.value;
  resultLen = data.length;
  memory.copy(changetype<usize>(resultBuf.buffer), changetype<usize>(data.buffer), resultLen);
  return resultLen;
}

/**
 * Get result (Rust macro parity).
 * Signature: get_result(dest_ptr: i32, max_len: i32) -> i32
 * Copies last result into dest; returns number of bytes copied.
 */
export function get_result(destPtr: i32, maxLen: i32): i32 {
  const len = resultLen < maxLen ? resultLen : maxLen;
  if (len <= 0) return 0;
  memory.copy(destPtr as usize, changetype<usize>(resultBuf.buffer), len);
  return len;
}

// ─────────────────────────────────────────────────────────────
// Storage helpers
// ─────────────────────────────────────────────────────────────

function storageSaveBytes(key: string, data: Uint8Array): bool {
  const keyBytes = Uint8Array.wrap(String.UTF8.encode(key, true));
  const r = storage_save(
    changetype<usize>(keyBytes.buffer),
    keyBytes.length,
    changetype<usize>(data.buffer),
    data.length
  );
  return r >= 0;
}

function storageLoadBytes(key: string, maxLen: i32): Uint8Array | null {
  const keyBytes = Uint8Array.wrap(String.UTF8.encode(key, true));
  const buf = new Uint8Array(maxLen);
  const n = storage_load(
    changetype<usize>(keyBytes.buffer),
    keyBytes.length,
    changetype<usize>(buf.buffer),
    maxLen
  );
  if (n == 0) return null;
  const n32 = i32(n);
  if (n32 < maxLen) {
    return buf.subarray(0, n32);
  }
  return buf;
}

// ─────────────────────────────────────────────────────────────
// Key helpers
// ─────────────────────────────────────────────────────────────

function roleMemberKey(role: string, account: string): string {
  return "access:role:" + role + ":" + account;
}

function roleAdminKey(role: string): string {
  return "access:admin:" + role;
}

// ─────────────────────────────────────────────────────────────
// IO helpers
// ─────────────────────────────────────────────────────────────

/** Creates a 1-byte Uint8Array (AssemblyScript has no Uint8Array.from). */
function bytes1(b0: u8): Uint8Array {
  const a = new Uint8Array(1);
  a[0] = b0;
  return a;
}

function readU8(input: Uint8Array, offset: i32): u8 {
  if (offset >= input.length) {
    return 0;
  }
  return input[offset];
}

function readU16(input: Uint8Array, offset: i32): u16 {
  if (offset + 1 >= input.length) {
    return 0;
  }
  return (input[offset] | (input[offset + 1] << 8)) as u16;
}

function readString(input: Uint8Array, offset: i32): string {
  const len = readU16(input, offset);
  const start = offset + 2;
  const end = start + len;
  if (end > input.length) {
    return "";
  }
  const slice = input.subarray(start, end);
  return String.UTF8.decode(slice.buffer);
}
