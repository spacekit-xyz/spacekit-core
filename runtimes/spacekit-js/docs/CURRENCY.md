# ASTRA in spacekit-js: the chain is the only ledger

spacekit-js keeps no balance of its own. ASTRA exists only as native balances on the SpaceKit chain (compute nodes), in wei (18 decimals). The browser reads those balances and moves value by signing chain transactions.

## Reading and paying

```ts
import { ChainAccount, ChainClient, parseAstra, formatAstra } from "@spacekit/spacekit-js/chain";

const chain = new ChainClient({ url: "https://node.example" });
const me = ChainAccount.fromPrivateKey(secretHex);       // did:spacekit:<address>

const { balanceWei } = await chain.getBalance(me.address);
console.log(formatAstra(balanceWei), "ASTRA");

await chain.transfer(me, "0x…", parseAstra("2.5"));        // plain transfer
const out = await chain.view(contract, input);             // read-only call
await chain.send(me, contract, input, { value: parseAstra("5") }); // call with value
```

`chainContractCaller(chain, account, contract)` gives the `submitContractCall` / `submitLedgerCall` function that paid channels and entitlements take. With it:

- purchases, renewals and grants run on the chain;
- the publisher is paid directly;
- the ledger contract keeps nothing.

## The local VM

`SpacekitVm` has a `currency` option:

| Setting | Behaviour |
|---|---|
| `"chain"` (default) | No genesis treasury, no local fees, no local balances. A transaction or call carrying value throws `LocalCurrencyError`. Local execution is for contract state only. |
| `"local-dev"` | The old self-contained local ledger, for offline development and tests. Never use it for anything users pay with. |

In `"chain"` mode the VM's storage is wrapped by `guardLocalCurrency`. Keys under `native:` and `astra:erc20:` cannot be:

- read (old values in a browser profile are ignored);
- written (a write throws);
- listed, snapshotted, restored, or merged from a storage node.

The host token adapter reports no balance and refuses transfers.

The Node and Bun entry points default to `"chain"`. Pass `--currency local-dev` (or set `SPACEKIT_CURRENCY=local-dev`) for an offline dev chain.

## Contract payments and sponsorship

- **`payment_transfer`.** A contract run in the browser can ask to pay ASTRA (wei). The request is handed to the configured `PaymentAdapter` after execution, which must settle it as a chain transaction with the payer's consent. Any asset other than ASTRA is refused (`-22`). `NoopPaymentAdapter` moves nothing and reports failure.
- **No vault charges, no USD.** `payment_vault_charge` (the former aUSD) is refused. Intents carry ASTRA wei only (`value_astra`, `max_fee_astra`, `max_value_wei`).
- **Sponsorship** is the `spacekit-paymaster` contract on the chain: a sponsor deposits ASTRA into it and permitted callers draw on it. The host imports `spacekit_paymaster.*` kept budgets off chain and now always refuse.
- **Fees charged by contracts** are ASTRA attached to the call (`collect_fee` in the contract SDK). In `"chain"` mode the local VM refuses value, so paid operations run against a compute node.
