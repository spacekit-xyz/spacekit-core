import type { HostContext } from "../host.js";
import { callSpacekitMain } from "../runtime.js";

const OP_CLASSIFY = 1;
const OP_STATUS = 2;

function concat(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function encodeU16(value: number): Uint8Array {
  const buf = new ArrayBuffer(2);
  new DataView(buf).setUint16(0, value, true);
  return new Uint8Array(buf);
}

function encodeString(value: string): Uint8Array {
  const data = new TextEncoder().encode(value);
  return concat([encodeU16(data.length), data]);
}

function decodeString(bytes: Uint8Array): string {
  return new TextDecoder().decode(bytes);
}

export interface IntentClassifierResult {
  intent: string;
  confidence: number;
}

export class SpacekitIntentClassifierClient {
  private ctx: HostContext;
  private instance: WebAssembly.Instance;

  constructor(ctx: HostContext, instance: WebAssembly.Instance) {
    this.ctx = ctx;
    this.instance = instance;
  }

  classify(message: string): IntentClassifierResult {
    const payload = concat([Uint8Array.of(OP_CLASSIFY), encodeString(message)]);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0) {
      return { intent: "ask_unknown", confidence: 0 };
    }
    const text = decodeString(result);
    try {
      const parsed = JSON.parse(text) as IntentClassifierResult;
      return parsed;
    } catch {
      return { intent: "ask_unknown", confidence: 0 };
    }
  }

  status(): string {
    const payload = Uint8Array.of(OP_STATUS);
    const { status, result } = callSpacekitMain(this.ctx, this.instance, payload);
    if (status <= 0) {
      return "unknown";
    }
    return decodeString(result);
  }
}
