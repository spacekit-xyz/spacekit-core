import { SpacekitVm, type StateSnapshot } from "./spacekitvm.js";
import type { IndexedDbStorageAdapter } from "../storage.js";

const snapshotKey = new TextEncoder().encode("__spacekitvm_snapshot__");

export interface AutoSyncOptions {
  storage: IndexedDbStorageAdapter;
  snapshotIntervalMs?: number;
  syncIntervalMs?: number;
  onSnapshot?: (snapshot: StateSnapshot) => void;
}

export class VmAutoSync {
  private vm: SpacekitVm;
  private storage: IndexedDbStorageAdapter;
  private snapshotIntervalMs: number;
  private syncIntervalMs: number;
  private onSnapshot?: (snapshot: StateSnapshot) => void;
  private snapshotTimer: ReturnType<typeof setInterval> | null = null;
  private syncTimer: ReturnType<typeof setInterval> | null = null;

  constructor(vm: SpacekitVm, options: AutoSyncOptions) {
    this.vm = vm;
    this.storage = options.storage;
    this.snapshotIntervalMs = options.snapshotIntervalMs ?? 10_000;
    this.syncIntervalMs = options.syncIntervalMs ?? 5_000;
    this.onSnapshot = options.onSnapshot;
  }

  async initFromSnapshot(): Promise<StateSnapshot | null> {
    await this.storage.init();
    const snapshotBytes = this.storage.get(snapshotKey);
    if (!snapshotBytes) {
      return null;
    }
    const json = new TextDecoder().decode(snapshotBytes);
    const snapshot = JSON.parse(json) as StateSnapshot;
    this.vm.restoreSnapshot(snapshot);
    return snapshot;
  }

  start() {
    if (this.syncTimer !== null || this.snapshotTimer !== null) {
      return;
    }
    this.syncTimer = setInterval(() => {
      void this.storage.syncAll();
    }, this.syncIntervalMs);
    this.snapshotTimer = setInterval(async () => {
      const snapshot = await this.vm.createSnapshot();
      const json = JSON.stringify(snapshot);
      this.storage.set(snapshotKey, new TextEncoder().encode(json));
      void this.storage.syncAll();
      if (this.onSnapshot) {
        this.onSnapshot(snapshot);
      }
    }, this.snapshotIntervalMs);
  }

  stop() {
    if (this.syncTimer !== null) {
      clearInterval(this.syncTimer);
      this.syncTimer = null;
    }
    if (this.snapshotTimer !== null) {
      clearInterval(this.snapshotTimer);
      this.snapshotTimer = null;
    }
  }
}
