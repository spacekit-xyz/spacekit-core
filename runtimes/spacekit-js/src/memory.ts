const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

export class MemoryView {
  private memory: WebAssembly.Memory;

  constructor(memory: WebAssembly.Memory) {
    this.memory = memory;
  }

  setMemory(memory: WebAssembly.Memory) {
    this.memory = memory;
  }

  readBytes(ptr: number, len: number): Uint8Array {
    const view = new Uint8Array(this.memory.buffer);
    return view.slice(ptr, ptr + len);
  }

  writeBytes(ptr: number, data: Uint8Array) {
    const view = new Uint8Array(this.memory.buffer);
    view.set(data, ptr);
  }

  readString(ptr: number, len: number): string {
    if (len <= 0) {
      return "";
    }
    return textDecoder.decode(this.readBytes(ptr, len));
  }

  writeString(ptr: number, value: string): number {
    const data = textEncoder.encode(value);
    this.writeBytes(ptr, data);
    return data.length;
  }
}

export class LinearMemoryAllocator {
  private memory: WebAssembly.Memory;
  private offset = 0;

  constructor(memory: WebAssembly.Memory) {
    this.memory = memory;
    this.offset = memory.buffer.byteLength;
  }

  setMemory(memory: WebAssembly.Memory) {
    this.memory = memory;
    this.offset = memory.buffer.byteLength;
  }

  alloc(size: number): number {
    const aligned = (size + 7) & ~7;
    const ptr = this.offset;
    const next = ptr + aligned;
    if (next > this.memory.buffer.byteLength) {
      const pagesNeeded = Math.ceil((next - this.memory.buffer.byteLength) / 65536);
      this.memory.grow(pagesNeeded);
    }
    this.offset = next;
    return ptr;
  }
}
