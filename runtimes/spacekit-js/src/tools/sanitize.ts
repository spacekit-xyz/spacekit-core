/**
 * SKTCS v0.1 — Result sanitizers.
 *
 * Applied to tool results before they are written back to guest memory.
 */

/**
 * Strip control characters (0x00-0x08, 0x0B-0x0C, 0x0E-0x1F) from a string.
 * Preserves newline (0x0A), carriage return (0x0D), and tab (0x09).
 */
export function stripControlChars(input: string): string {
  // eslint-disable-next-line no-control-regex
  return input.replace(/[\x00-\x08\x0B\x0C\x0E-\x1F]/g, "");
}

/**
 * Wrap externally-sourced content in deterministic fence tokens so the LLM
 * can distinguish host-returned data from injected instructions.
 *
 * Format: `<<<SPACEKIT_DATA_FENCE_{prefix}>>>\n{content}\n<<<END_SPACEKIT_DATA_FENCE_{prefix}>>>`
 *
 * `hashPrefix` should be the first 8 hex chars of the current block hash
 * or a caller-supplied nonce, making the fence unpredictable to the content author.
 */
export function promptFence(content: string, hashPrefix: string): string {
  const tag = hashPrefix.slice(0, 8).padEnd(8, "0");
  return `<<<SPACEKIT_DATA_FENCE_${tag}>>>\n${content}\n<<<END_SPACEKIT_DATA_FENCE_${tag}>>>`;
}

/**
 * Apply the sanitize mode declared in a param definition to a result string.
 */
export function applySanitize(
  value: string,
  mode: string | undefined,
  hashPrefix: string,
): string {
  switch (mode) {
    case "strip_control_chars":
      return stripControlChars(value);
    case "prompt_fence":
      return promptFence(stripControlChars(value), hashPrefix);
    default:
      return value;
  }
}
