/// <reference path="../node_modules/assemblyscript/std/assembly/index.d.ts" />
// SpaceKit Micro-GPT Agent Contract (AssemblyScript)
// Uses spacekit_microgpt host primitive for next-token prediction.
// ABI: OP_NEXT_TOKEN (1) / OP_CHAT_STEP (2): [op][token_id:u32 LE][pos_id:u32 LE] -> [next_token_id:u8]

import {
  Contract,
  ContractError,
  Result,
  contract,
  microgpt_forward,
  MICROGPT_VOCAB_SIZE,
  MICROGPT_LOGITS_BYTES,
} from "../../../../spacekit-assembly-sdk/assembly/spacekit-as-sdk/index";

const OP_NEXT_TOKEN: u8 = 1;
const OP_CHAT_STEP: u8 = 2;

@contract
class MicroGptAgent extends Contract {

  init(): void {
    // no state
  }

  handle(input: Uint8Array): Result<Uint8Array> {
    if (input.length < 9) {
      return Result.err<Uint8Array>(ContractError.InvalidInput);
    }

    const op = input[0];
    if (op != OP_NEXT_TOKEN && op != OP_CHAT_STEP) {
      return Result.err<Uint8Array>(ContractError.InvalidInput);
    }

    const tokenId = readU32(input, 1);
    const posId = readU32(input, 5);

    const logitsBuf = new Uint8Array(MICROGPT_LOGITS_BYTES);
    microgpt_forward(tokenId, posId, changetype<usize>(logitsBuf.buffer));

    const logits = float32View(logitsBuf);
    softmax(logits);
    const nextId = argmax(logits);

    const out = new Uint8Array(1);
    out[0] = <u8>nextId;
    return Result.ok(out);
  }
}

function float32View(bytes: Uint8Array): Float32Array {
  return Float32Array.wrap(bytes.buffer, 0, MICROGPT_VOCAB_SIZE);
}

function softmax(logits: Float32Array): void {
  let max: f32 = -3.4e38;
  for (let i = 0; i < MICROGPT_VOCAB_SIZE; i++) {
    const v = logits[i];
    if (v > max) max = v;
  }
  let sum: f32 = 0.0;
  for (let i = 0; i < MICROGPT_VOCAB_SIZE; i++) {
    const e = Mathf.exp(logits[i] - max);
    logits[i] = e;
    sum += e;
  }
  if (sum > 0.0) {
    for (let i = 0; i < MICROGPT_VOCAB_SIZE; i++) {
      logits[i] = logits[i] / sum;
    }
  }
}

function argmax(logits: Float32Array): i32 {
  let bestIdx = 0;
  let bestVal = logits[0];
  for (let i = 1; i < MICROGPT_VOCAB_SIZE; i++) {
    if (logits[i] > bestVal) {
      bestVal = logits[i];
      bestIdx = i;
    }
  }
  return bestIdx;
}

function readU32(input: Uint8Array, offset: i32): u32 {
  if (offset + 4 > input.length) return 0;
  return (
    (input[offset] as u32) |
    ((input[offset + 1] as u32) << 8) |
    ((input[offset + 2] as u32) << 16) |
    ((input[offset + 3] as u32) << 24)
  );
}

// Singleton and result buffer – same pattern as Rust spacekit_contract! / astra-access-control
let contractInstance: MicroGptAgent | null = null;
const resultBuf = new Uint8Array(4096);
let resultLen: i32 = 0;

function ensureInit(): void {
  if (contractInstance == null) {
    contractInstance = new MicroGptAgent();
    contractInstance!.init();
  }
}

export function main(inputPtr: i32, inputLen: i32): i32 {
  ensureInit();
  const inst = contractInstance!;
  const input = new Uint8Array(inputLen);
  memory.copy(changetype<usize>(input.buffer), inputPtr as usize, inputLen);
  const res = inst.handle(input);
  if (!res.isOk()) return res.code();
  const data = res.value;
  resultLen = data.length;
  memory.copy(
    changetype<usize>(resultBuf.buffer),
    changetype<usize>(data.buffer),
    resultLen
  );
  return resultLen;
}

export function get_result(destPtr: i32, maxLen: i32): i32 {
  const len = resultLen < maxLen ? resultLen : maxLen;
  if (len <= 0) return 0;
  memory.copy(destPtr as usize, changetype<usize>(resultBuf.buffer), len);
  return len;
}
