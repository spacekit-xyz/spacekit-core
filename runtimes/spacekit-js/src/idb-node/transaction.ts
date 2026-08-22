/**
 * Transaction: buffer writes and apply on commit; support abort.
 * Tracks requests so we can resolve them after commit.
 */

import type { WalBackend } from "./wal-backend.js";
import type { FakeIDBRequest } from "./request.js";

export type TransactionMode = "readonly" | "readwrite" | "versionchange";

export interface QueuedOp {
  store: string;
  op: "put" | "del" | "clear";
  key?: IDBValidKey;
  value?: unknown;
}

interface PendingRequest {
  request: FakeIDBRequest;
  resolveWith: IDBValidKey | undefined;
}

export class IDBTransactionImpl {
  private ops: QueuedOp[] = [];
  private pendingRequests: PendingRequest[] = [];
  private aborted = false;

  constructor(
    public readonly mode: TransactionMode,
    private backend: WalBackend,
    private onCommit: (ops: QueuedOp[]) => void
  ) {}

  enqueuePut(store: string, key: IDBValidKey, value: unknown): void {
    if (this.aborted) throw new DOMException("Transaction aborted", "AbortError");
    if (this.mode === "readonly") throw new DOMException("Read-only transaction", "ReadOnlyError");
    this.ops.push({ store, op: "put", key, value });
  }

  enqueueDelete(store: string, key: IDBValidKey): void {
    if (this.aborted) throw new DOMException("Transaction aborted", "AbortError");
    if (this.mode === "readonly") throw new DOMException("Read-only transaction", "ReadOnlyError");
    this.ops.push({ store, op: "del", key });
  }

  enqueueClear(store: string): void {
    if (this.aborted) throw new DOMException("Transaction aborted", "AbortError");
    if (this.mode === "readonly") throw new DOMException("Read-only transaction", "ReadOnlyError");
    this.ops.push({ store, op: "clear" });
  }

  addRequest(request: FakeIDBRequest, resolveWith: IDBValidKey | undefined): void {
    this.pendingRequests.push({ request, resolveWith });
  }

  commit(): void {
    if (this.aborted) return;
    this.onCommit(this.ops);
    for (const { request, resolveWith } of this.pendingRequests) {
      request._resolve(resolveWith as never);
    }
    this.pendingRequests = [];
    this.ops = [];
  }

  abort(): void {
    this.aborted = true;
    for (const { request } of this.pendingRequests) {
      request._reject(new DOMException("Transaction aborted", "AbortError"));
    }
    this.pendingRequests = [];
    this.ops = [];
  }

  get abortedFlag(): boolean {
    return this.aborted;
  }
}
