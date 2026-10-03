export {
  ASTRA_DECIMALS,
  ChainAccount,
  ChainClient,
  ChainError,
  DEFAULT_GAS_LIMIT,
  PLAIN_TRANSFER_GAS,
  WEI_PER_ASTRA,
  addressOfDid,
  chainContractCaller,
  formatAstra,
  normalizeAddress,
  parseAstra,
  transactionSigningPayload,
} from "./client.js";
export type {
  ChainBalance,
  ChainClientOptions,
  ChainReceipt,
  SendOptions as ChainSendOptions,
  TransactionSignature as ChainTransactionSignature,
  UnsignedTransaction,
} from "./client.js";
export {
  LOCAL_CURRENCY_PREFIXES,
  LocalCurrencyError,
  NoLocalCurrencyTokenAdapter,
  guardLocalCurrency,
  isLocalCurrencyKey,
} from "./currency_guard.js";
