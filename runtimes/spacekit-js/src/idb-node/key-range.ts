/**
 * IDBKeyRange: static helpers only. Range is represented as lower/upper/open flags.
 */

export function bound(
  lower: IDBValidKey,
  upper: IDBValidKey,
  lowerOpen?: boolean,
  upperOpen?: boolean
): { lower: IDBValidKey; upper: IDBValidKey; lowerOpen: boolean; upperOpen: boolean } {
  return {
    lower,
    upper,
    lowerOpen: lowerOpen ?? false,
    upperOpen: upperOpen ?? false,
  };
}

export function only(value: IDBValidKey): { lower: IDBValidKey; upper: IDBValidKey; lowerOpen: boolean; upperOpen: boolean } {
  return { lower: value, upper: value, lowerOpen: false, upperOpen: false };
}

export function lowerBound(lower: IDBValidKey, open?: boolean): { lower: IDBValidKey; upper?: undefined; lowerOpen: boolean; upperOpen: boolean } {
  return { lower, lowerOpen: open ?? false, upperOpen: false };
}

export function upperBound(upper: IDBValidKey, open?: boolean): { lower?: undefined; upper: IDBValidKey; lowerOpen: boolean; upperOpen: boolean } {
  return { upper, lowerOpen: false, upperOpen: open ?? false };
}
