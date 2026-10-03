/**
 * Keep a second currency out of local storage.
 *
 * In `currency: "chain"` mode (the default) the VM holds no ASTRA. Its
 * storage is wrapped so that keys of the old local ledgers can be neither
 * read nor written, synced or restored:
 *
 * - `native:` (the VM's former native balances),
 * - `astra:erc20:` (an ASTRA ERC-20 kept in local contract storage).
 *
 * Writes throw `LocalCurrencyError`; reads see nothing; listings, snapshots
 * and storage-node merges skip them. Data a browser stored under these keys
 * before is left in place but ignored.
 *
 * @module chain/currency_guard
 */

import type { StorageAdapter } from "../storage.js";
import type { TokenAdapter } from "../host.js";

export const LOCAL_CURRENCY_PREFIXES = ["native:", "astra:erc20:"] as const;

const PREFIX_BYTES = LOCAL_CURRENCY_PREFIXES.map((p) => new TextEncoder().encode(p));

export class LocalCurrencyError extends Error {
  constructor(detail = "ASTRA balances live on the chain; use ChainClient") {
    super(detail);
    this.name = "LocalCurrencyError";
  }
}

export function isLocalCurrencyKey(key: Uint8Array): boolean {
  return PREFIX_BYTES.some((p) => key.length >= p.length && p.every((b, i) => key[i] === b));
}

/** Wrap a storage adapter so local-currency keys are unreachable. */
export function guardLocalCurrency(storage: StorageAdapter): StorageAdapter {
  const deny = (key: Uint8Array) => {
    if (isLocalCurrencyKey(key)) {
      throw new LocalCurrencyError(
        `refusing to write ${new TextDecoder().decode(key.slice(0, 40))}…: ` +
          "ASTRA balances live on the chain",
      );
    }
  };
  return new Proxy(storage, {
    get(target, prop, receiver) {
      switch (prop) {
        case "get":
          return (key: Uint8Array) => (isLocalCurrencyKey(key) ? undefined : target.get(key));
        case "getAux":
          return target.getAux
            ? (key: Uint8Array) => (isLocalCurrencyKey(key) ? undefined : target.getAux!(key))
            : undefined;
        case "set":
          return (key: Uint8Array, value: Uint8Array) => {
            deny(key);
            target.set(key, value);
          };
        case "setAux":
          return target.setAux
            ? (key: Uint8Array, value: Uint8Array) => {
                deny(key);
                target.setAux!(key, value);
              }
            : undefined;
        case "setWithVersion":
          return target.setWithVersion
            ? (key: Uint8Array, value: Uint8Array, version: number) => {
                deny(key);
                target.setWithVersion!(key, value, version);
              }
            : undefined;
        case "entries":
          return target.entries
            ? () => target.entries!().filter((e) => !isLocalCurrencyKey(e.key))
            : undefined;
        case "entriesWithVersion":
          return target.entriesWithVersion
            ? () => target.entriesWithVersion!().filter((e) => !isLocalCurrencyKey(e.key))
            : undefined;
        case "mergeFromRemote":
          return target.mergeFromRemote
            ? (entries: Parameters<NonNullable<StorageAdapter["mergeFromRemote"]>>[0], strategy?: "lww" | "vector") =>
                target.mergeFromRemote!(entries.filter((e) => !isLocalCurrencyKey(e.key)), strategy)
            : undefined;
        default: {
          const value = Reflect.get(target, prop, receiver);
          return typeof value === "function" ? value.bind(target) : value;
        }
      }
    },
  });
}

/**
 * The host token adapter in chain mode: no local balances, no local
 * transfers. Contracts that pay do so on the chain.
 */
export class NoLocalCurrencyTokenAdapter implements TokenAdapter {
  balanceOf(_did: string): bigint {
    return 0n;
  }
  transfer(_from: string, _to: string, _amount: bigint): boolean {
    return false;
  }
  totalSupply(): bigint {
    return 0n;
  }
}
