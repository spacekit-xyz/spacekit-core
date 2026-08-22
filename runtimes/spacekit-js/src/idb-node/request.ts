/**
 * Event-based IDBRequest simulation (async via queueMicrotask).
 */

export class FakeIDBRequest<T = unknown> extends EventTarget {
  result: T | undefined = undefined;
  error: DOMException | null = null;
  readyState: "pending" | "done" = "pending";

  onsuccess: ((ev: Event) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;

  _resolve(value: T): void {
    this.result = value;
    this.readyState = "done";
    queueMicrotask(() => {
      const event = new Event("success");
      if (this.onsuccess) this.onsuccess(event);
      this.dispatchEvent(event);
    });
  }

  _reject(err: Error): void {
    this.error = err instanceof DOMException ? err : new DOMException(err.message, "UnknownError");
    this.readyState = "done";
    queueMicrotask(() => {
      const event = new Event("error");
      if (this.onerror) this.onerror(event);
      this.dispatchEvent(event);
    });
  }

  toPromise(): Promise<T> {
    return new Promise((resolve, reject) => {
      this.onsuccess = () => resolve(this.result as T);
      this.onerror = () => reject(this.error);
    });
  }
}

export class FakeIDBOpenDBRequest extends FakeIDBRequest<IDBDatabase> {
  onblocked: ((ev: Event) => void) | null = null;
  onupgradeneeded: ((ev: IDBVersionChangeEvent) => void) | null = null;
}
