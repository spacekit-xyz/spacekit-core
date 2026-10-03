# ASTRA: one currency, one ledger

ASTRA exists in exactly one place: the native account balance on the SpaceKit chain (`SwtchvmAccount::balance`). Balances are in wei (18 decimals), keyed by 20-byte address, and covered by the block's state root. Every node computes the same balances by executing the same blocks.

Nothing else is a currency:

- **The browser.** spacekit-js keeps no balance (see `runtimes/spacekit-js/docs/CURRENCY.md`).
- **Rewards.** There is no rewards contract ledger. Rewards mint straight into native balances.
- **The treasury.** It holds native ASTRA at its own address.
- **The compute worker's sandbox VM.** It holds no value outside `SPACEKIT_DEV_MODE`.

## Addresses and DIDs

An address is `keccak256(uncompressed k256 public key)[12..]` (Ethereum style). Its DID is `did:spacekit:<address hex>`.

A SPHINCS+ DID `did:spacekit:<sha256(pk)[..20] hex>` designates that 20-byte address. No k256 key exists for it, so its holder spends with SPHINCS+-signed transfers (below).

## How ASTRA is created

ASTRA is created only inside blocks, by the rewards system calls the Service Reward Accumulator puts at the start of a block. Every importer re-derives and checks those calls. The node executes them itself, in `native_rewards.rs`:

| Call | Effect |
|---|---|
| `INIT` | Mints the genesis treasury allocation (350,000,000 ASTRA) to the treasury contract's address `0x…0004`. Starts proof of authority. |
| `CREDIT` | Mints to the recipient's address. A recipient key that is not an address is refused, and nothing is minted. |
| `CREDIT_LOCKED` | During PoA, for authorities and affiliated operators. Records a locked amount that vests from the genesis block time: nothing before 365 days, then linearly until 1,095 days. Every block releases what has vested into the address's balance. |
| `END_POA` | Stops locking new credits. |

Total emission, locked amounts included, is capped at 2,000,000,000 ASTRA (`rewards.total_emitted`).

Genesis balances can also come from `SPACEKIT_GENESIS_ALLOC_FILE` (height 0 only).

## How ASTRA moves

The value of a transaction moves only if the transaction succeeds:

- **Gas.** The nonce and `gas_limit × gas_price` are always consumed. Unused gas is refunded, and used gas is burned.
- **Atomic effects.** Everything else is the transaction's effect, recorded in its own journal: value, contract storage and transfers. If the transaction fails, all of it is undone.
- **Plain transfers.** A transaction to an address without code and with no data is a plain transfer. It costs 21,000 gas.
- **Calls with value.** The value moves to the contract before it runs, so the contract sees it in its balance.

### Contract host functions

| Function | Meaning |
|---|---|
| `msg_value_u128(out)` | The value attached to this call (16 bytes, little-endian). |
| `get_balance_u128(addr, out)` | The native balance of an address. |
| `transfer_u128(to, amount)` | Pays from the **executing contract's own** balance. A contract can never spend its caller's balance. |
| `msg_value()`, `get_balance(addr)`, `transfer(to, i64)` | The i64 forms. Values above `i64::MAX` are capped (saturated) at `i64::MAX`. |

Time and randomness come from the block:

- `get_timestamp()` and WASI `clock_time_get` return the **block's** timestamp.
- WASI `random_get` is deterministic: SHA-256 over the block, the caller, the contract and the remaining fuel. It is not suitable for secrets.

A nested `contract_call` carries no value.

### SPHINCS+ holders

The holder signs a transfer message and `POST`s it to `/v1/transfer`. The body is `{ body_json, signature_hex }`, where `body_json` is:

```json
{ "version": 1, "network": "…", "did": "…", "sphincs_pk_hex": "…",
  "to": "0x…", "amount_wei": "…", "nonce": 0 }
```

- The signature covers `SPACEKIT-PQ-TRANSFER-v1\n{body_json}`.
- The nonce is the sending address's account nonce.
- A fee of 0.001 ASTRA is burned.

## Staking and the treasury

- **Staking.** A validator's holdings are the native balance of its DID's address, plus any locked rewards not yet released. Effective stake is `min(bonded, holdings − unbonding)`.
- **Treasury.** The treasury contract `0x…0004` pays M-of-N approved spends from its own native balance with `transfer_u128`. Its signers and threshold come from the PoA genesis `treasury` section: `{ "threshold": 2, "signer_dids": [...] }`. Anyone can send ASTRA to it with `DEPOSIT` (0x20).

## What writes outside blocks

Nothing does on a consensus (PoA/PoS) network. The faucet, rollup settlement, PoTW awards and treasury disbursement through the host bridge are all refused there. On a single-node development chain, the faucet still works.

## HTTP

| Route | Purpose |
|---|---|
| `GET /v1/balance/{address}` | `balance_wei` (a decimal string), `locked_wei`, `nonce`. |
| `POST /transaction` | Submit a signed transaction (payload `SPACEKIT-TX-v2`, see `transaction_signing_payload`). |
| `POST /api/contracts/{address}/call` | Read-only call: raw call data in, raw return data out. Nothing is charged or kept. |
| `POST /v1/transfer` | SPHINCS+ transfer. |

## Payments in USD (open question)

`spacekit-payments` can turn USDC/aUSD receipts into ASTRA credits.

- A credit now names its payer (`Credit::payer_did`): the treasury for USD payments, the sender for ASTRA payments.
- An applier must apply a credit as a transfer on the chain, never as a mint.
- No applier is wired up today: the node only logs credits.

Whether USD-denominated balances (aUSD, `AusdVault`) should exist at all is a product decision. They are not ASTRA and are not on the chain.
