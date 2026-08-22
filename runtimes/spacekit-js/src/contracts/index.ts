/**
 * SpaceKit Contract Clients
 *
 * Opt-in contract client library. Import from "@spacekit/spacekit-js/contracts"
 * to access typed clients for standard SpaceKit contracts without bloating
 * the core VM entry point.
 *
 * @example
 * ```ts
 * import { SkErc20Client } from "@spacekit/spacekit-js/contracts";
 * ```
 */

export type { SkErc20Metadata } from "./sk_erc20.js";
export { SkErc20Client } from "./sk_erc20.js";
export { SkErc721Client } from "./sk_erc721.js";
export type { IntentClassifierResult } from "./spacekit_intent_classifier.js";
export { SpacekitIntentClassifierClient } from "./spacekit_intent_classifier.js";
