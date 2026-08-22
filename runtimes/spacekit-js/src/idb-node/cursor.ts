/**
 * IDBCursor / IDBCursorWithValue: powered by backend.scan().
 */

import { encodeKey } from "./key-codec.js";
import type { WalBackend } from "./wal-backend.js";
import type { ScanRange } from "./wal-backend.js";
import type { FakeIDBRequest } from "./request.js";

export type IDBCursorDirection = "next" | "nextunique" | "prev" | "prevunique";

export interface CursorSource {
  readonly name: string;
  readonly keyPath: string | string[] | null;
  readonly autoIncrement: boolean;
}

export class FakeIDBCursor {
  protected results: Array<{ key: IDBValidKey; value: unknown }>;
  protected index: number;
  private _source: CursorSource;
  private _storeName: string;
  private _request: FakeIDBRequest<IDBCursorWithValue | null> | null = null;

  constructor(
    storeName: string,
    source: CursorSource,
    backend: WalBackend,
    range: ScanRange | null,
    direction: IDBCursorDirection,
    request?: FakeIDBRequest<IDBCursorWithValue | null>
  ) {
    this._storeName = storeName;
    this._source = source;
    this._request = request ?? null;
    const dir = direction === "prev" || direction === "prevunique" ? "prev" : "next";
    this.results = backend.scan(storeName, range ?? {}, dir);
    this.index = 0;
  }

  get key(): IDBValidKey {
    return this.results[this.index]?.key as IDBValidKey;
  }

  get primaryKey(): IDBValidKey {
    return this.results[this.index]?.key as IDBValidKey;
  }

  get value(): unknown {
    return this.results[this.index]?.value;
  }

  get source(): CursorSource {
    return this._source;
  }

  get storeName(): string {
    return this._storeName;
  }

  continue(key?: IDBValidKey): void {
    if (key !== undefined) {
      const keyHex = encodeKey(key).toString("hex");
      const nextIdx = this.results.findIndex(
        (r, i) => i > this.index && encodeKey(r.key).toString("hex") >= keyHex
      );
      this.index = nextIdx === -1 ? this.results.length : nextIdx;
    } else {
      this.index++;
    }
    this._fireRequest();
  }

  advance(count: number): void {
    this.index += count;
    this._fireRequest();
  }

  private _fireRequest(): void {
    if (!this._request) return;
    const req = this._request;
    const done = this.finished;
    const cursor = done ? null : (this as unknown as IDBCursorWithValue);
    queueMicrotask(() => req._resolve(cursor as never));
  }

  get finished(): boolean {
    return this.index >= this.results.length;
  }
}

export class FakeIDBCursorWithValue extends FakeIDBCursor {
  override get value(): unknown {
    return super.value;
  }
}
