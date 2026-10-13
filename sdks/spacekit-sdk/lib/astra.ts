/**
 * ASTRA amounts.
 *
 * ASTRA is SpaceKit's only currency. Amounts travel as wei: an integer with 18
 * decimals, written as a decimal string so it survives JSON and JavaScript
 * numbers. These helpers convert between wei and the "1.5 ASTRA" form people
 * read and type.
 */

export const ASTRA_DECIMALS = 18;
export const WEI_PER_ASTRA = 10n ** 18n;

/**
 * Parse a wei amount: a non-negative integer as a bigint, a safe integer
 * number, or a decimal string. Returns null for anything else.
 */
export function parseWei(value: unknown): bigint | null {
  if (typeof value === "bigint") return value >= 0n ? value : null;
  if (typeof value === "number") {
    return Number.isSafeInteger(value) && value >= 0 ? BigInt(value) : null;
  }
  if (typeof value === "string" && /^\d{1,78}$/.test(value.trim())) return BigInt(value.trim());
  return null;
}

/** Parse "1.5" (ASTRA) into wei. Returns null for malformed input or more than 18 decimals. */
export function parseAstra(value: unknown): bigint | null {
  const text = typeof value === "number" ? String(value) : typeof value === "string" ? value.trim() : "";
  const m = /^(\d{1,60})(?:\.(\d{1,18}))?$/.exec(text);
  if (!m) return null;
  const whole = BigInt(m[1]);
  const frac = BigInt((m[2] ?? "").padEnd(ASTRA_DECIMALS, "0"));
  return whole * WEI_PER_ASTRA + frac;
}

/** Format wei as ASTRA, trimming trailing zeros: `1500000000000000000n` → `"1.5"`. */
export function formatAstra(wei: bigint | string, maxDecimals = ASTRA_DECIMALS): string {
  const value = typeof wei === "bigint" ? wei : parseWei(wei) ?? 0n;
  const whole = value / WEI_PER_ASTRA;
  let frac = (value % WEI_PER_ASTRA).toString().padStart(ASTRA_DECIMALS, "0").slice(0, maxDecimals);
  frac = frac.replace(/0+$/, "");
  if (!frac && value > 0n && whole === 0n) return `<0.${"0".repeat(Math.max(0, maxDecimals - 1))}1`;
  return frac ? `${whole}.${frac}` : whole.toString();
}

/**
 * Read a price from call parameters: `amountWei` (wei) or `amountAstra`
 * ("1.5"). USD fields are refused with a message that says why.
 */
export function priceFromParams(params: Record<string, unknown>, call: string): bigint {
  if (params.amountCents !== undefined || params.amountUsd !== undefined) {
    throw new Error(`${call}: prices are in ASTRA. Pass amountAstra ("1.5") or amountWei, not USD`);
  }
  const wei =
    params.amountWei !== undefined
      ? parseWei(params.amountWei)
      : params.amountAstra !== undefined
        ? parseAstra(params.amountAstra)
        : null;
  if (wei === null || wei <= 0n) {
    throw new Error(`${call}: invalid amount. Pass amountAstra ("1.5") or amountWei (a positive integer string)`);
  }
  return wei;
}
