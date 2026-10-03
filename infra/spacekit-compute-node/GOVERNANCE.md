# Consensus, governance and staking

How a SpaceKit network produces blocks, decides who may produce them, and changes its own rules. Implemented in the compute node:

| File | |
|---|---|
| `src/chain_consensus.rs` | Governance and staking kept in the chain; consensus transactions; the producer set |
| `src/validator_governance.rs` | The governance state machine (proposals, votes, tallies) and its HTTP API |
| `src/staking.rs` | Validator stake: bond, unbond, effective stake |
| `src/block_production.rs` | When blocks are due (on demand or interval) |
| `src/block_seal.rs` | Producer keys and block seals |
| `src/spacekitvm/state_commitment.rs` | The state root and per-block undo |
| `src/service_reward_accumulator.rs` | Service rewards, settled in blocks |

The CLI side is `spacekit governance` (`tools/spacekit-cli/src/full_client/governance_cmd.rs`) and `spacekit network devnet`.

## Lifecycle

1. **Genesis (proof of authority).** The network starts with the authorities named in its genesis file. Authorities produce blocks without stake.
2. **Growing the set.** Authorities admit operators with `add_authority` proposals and drop them with `remove_authority`. One vote per authority; a proposal passes with `ceil(2n/3)` approvals.
3. **Staking.** Any operator can bond ASTRA it holds (its native balance plus locked rewards not yet released) as validator stake, before or after the lift. Authorities earn the consensus share of emission for every block they produce, locked during PoA, and locked ASTRA is stakeable.
4. **Lifting PoA.** Once there are at least `min_validators_to_lift` authorities (default 10), an authority can propose `lift_poa`. Nothing switches automatically.
5. **Proof of stake.** After the lift, validators whose effective stake reaches the minimum produce blocks, chosen in proportion to stake. Authorities keep producing unstaked for the grace period (`pos_grace_days`, default 30). Governance continues, weighted by stake, for protocol settings. If no validator has the minimum stake once the grace period ends, the authorities keep producing so the chain does not halt.

With 3 authorities the network tolerates no faulty producer (`floor((n-1)/3)`), so start with at least 4.

## Everything consensus-relevant is in the chain

Governance proposals, votes and stake operations are **consensus transactions**. A node that receives one over HTTP checks its signature and validity against the head block, puts it in its transaction pool, and relays it to its peers like any other transaction. It takes effect when a block includes it, at that block's time, in that block's order. Every node applies the same messages in the same order, and a node that joins later gets the same state by replaying the chain.

The governance and staking state lives in contract storage under the consensus address `0x0000000000000000000000000000000000000005`, so the block's state root covers it and a reorganization rolls it back.

A consensus transaction is a transaction from and to the consensus address, with value 0, nonce 0 and a zero signature, whose `data` is JSON:

```json
{ "kind": "governance_proposal", "proposal": { "body_json": "…", "signature_hex": "…" } }
{ "kind": "governance_vote", "vote": { "proposal_id": "…", "voter_did": "…", "choice": "approve", "signature_hex": "…" } }
{ "kind": "stake", "stake": { "body_json": "…", "signature_hex": "…" } }
```

It pays no gas. Its receipt fails if the message is invalid when the block applies it; `return_data` says why. A block carries at most 256.

Each block starts with deterministic transitions at its timestamp: pending proposals past their deadline expire, matured unbonding is released, and after the lift the stake electorate is refreshed.

## Transactions

A user transaction's secp256k1 signature covers SHA-256 of:

```text
SPACEKIT-TX-v2
{chain_id}
{from}
{to}
{value}
{nonce}
{gas_limit}
{gas_price}
{data}
```

Lines are joined with `\n`. Addresses and data are lowercase hex without `0x`, `to` is empty for a deployment, numbers are decimal, and `chain_id` is the network's numeric chain id. Binding gas and the chain id means a relaying node cannot change the fee, and a signature for one network is invalid on every other. Nodes reject unsigned or wrongly signed transactions when they are submitted (HTTP 400). `SPACEKIT_LEGACY_TX_SIGNATURES=1` also accepts the old `{from}|{to}|{value}|{nonce}|{data}` payload, for test networks with old clients only.

Transactions are relayed between nodes (`SwtchvmTransaction` P2P messages), so one sent to any node reaches whichever producer's turn it is. Nodes check signatures and nonces before accepting or relaying, ignore duplicates, drop pending transactions that a block included or made stale, and hold at most 50,000.

## State root

A block's `state_root` commits to every account (balance, nonce, code, storage, compute used), every contract storage entry, and every storage slot. It is a lattice hash (LtHash16): each entry maps to 1,024 16-bit lanes by BLAKE3, the lanes of all entries are summed with wrap-around, and the root is BLAKE3 over the sum. The sum does not depend on order and is updated incrementally: when an entry changes, its old lanes are subtracted and its new ones added, so a block costs work in proportion to what it touched.

Every change is journaled with the entry's previous value. The journal of a block is its undo record: importing a block that fails validation, or rolling one back for fork choice, restores the previous state exactly.

## Block production

When blocks are produced is a network setting, `block_production`, in the genesis file, changeable by a `set_block_production` proposal.

| Field | Default | |
|---|---|---|
| `mode` | `on_demand` | `on_demand`: a block only when there is work. `interval`: a block every `block_time_ms`, empty or not. |
| `block_time_ms` | `2000` | Minimum gap between blocks (`on_demand`) or the block time (`interval`). 250 ms to 10 minutes. |
| `batch_window_ms` | `500` | `on_demand`: after the first pending transaction arrives, wait this long so a burst lands in one block. |
| `heartbeat_secs` | `300` | `on_demand`: an empty block after this much idle time. `0` turns heartbeats off. |

With `on_demand`, a block becomes due at the earliest of: the oldest pending transaction plus `batch_window_ms`; the moment a system transaction is due (reward settlement, `END_POA`); the parent's time plus `heartbeat_secs`. It is never due sooner than `block_time_ms` after the parent. Heartbeats keep block timestamps moving (vesting of locked rewards reads them) and let explorers tell an idle chain from a stalled one. Keep `heartbeat_secs` at 300 or less on mainnet: it bounds how late a reward settlement can be on an idle chain.

**Who produces.** The producer set comes from the chain:

- **PoA**: the authorities, sorted by DID, in turn: `authorities[(height + offset) mod n]`.
- **PoS**: validators whose effective stake is at least `min_stake_astra`, weighted by stake (whole ASTRA), plus authorities still in the grace period (weighted as the minimum stake). The producer for `(height, offset)` is picked by `SHA-256("SPACEKIT-PRODUCER-v1" ‖ height ‖ offset)` modulo the total weight. The seed depends only on height and offset, so a producer cannot steer it.

`offset` starts at 0 and goes up by one for every grace period (three block times, at least 3 seconds) in which the block did not arrive, so a missing producer is skipped. A fallback producer stands down while a peer reports a longer chain. A node whose key is not in the set refuses to produce (`POST /mine` fails too).

**Seals.** The producer writes its DID in the block (`proposer_did`, covered by the block hash) and seals the block with its SPHINCS+ key:

```text
SPACEKIT-BLOCK-SEAL-v1\n{chain_id}\n{number}\n{hex block hash}
```

The seal travels with the block over P2P. A node imports a block only if it is sealed by the block's `proposer_did` and that DID is in the producer set of the parent state. Nodes keep the seals they accepted (`SPACEKIT_SEAL_STORE_PATH`) and serve them to nodes catching up. `GET /v1/chain/seals/{number}` returns one block's seal.

## Fork choice

Two producers can make different blocks at the same height (a slow producer and its fallback). Nodes then keep both branches and pick one:

- A block produced by the scheduled producer on its first turn weighs 2, any other block 1.
- The branch with more weight wins; on equal weight the longer branch; then the lower tip hash.
- A node switches by rolling back to the fork point and importing the other branch. It keeps undo records for the last 64 blocks; blocks deeper than that are final. If the other branch has an invalid block, the node restores its own.
- Transactions from rolled-back blocks go back to the pool.
- When a peer announces a different head, the node fetches the recent blocks of that branch to compare.

Blocks sealed by a producer that should have waited are therefore temporary: the in-turn branch outweighs them.

## Governance

| | PoA | PoS (after the lift) |
|---|---|---|
| Who proposes and votes | Current authorities | Validators in the stake electorate |
| Weight | 1 per authority | Effective stake, whole ASTRA |
| To pass | `ceil(2n/3)` approvals | Two thirds of the weight |
| Counted against | The current authorities | The electorate when the proposal was included |
| Actions | `add_authority`, `remove_authority`, `lift_poa`, `set_block_production` | `set_block_production` |
| When the set changes | Other pending proposals become `stale` | Nothing: each proposal keeps its snapshot |

Voting period: 7 days by default, 1 hour to 30 days. Votes cannot be changed. The last authority cannot be removed. Statuses: `pending`, `executed`, `rejected`, `expired`, `stale`, `failed`.

A vote for a proposal is accepted only once the proposal is in a block, so vote after the proposal shows up in `GET /v1/governance/proposals`.

Signatures are SPHINCS+-SHAKE-256s-simple (NIST level 5; 29,792-byte signatures) with the DID's key. A `did:spacekit:*` DID must end with `hex(sha256(pk)[..20])`.

```text
SPACEKIT-GOVERNANCE-PROPOSAL-v1\n{body_json}
SPACEKIT-GOVERNANCE-VOTE-v1\n{network}\n{proposal_id}\n{approve|reject}
```

- `body_json` is signed exactly as submitted. The proposal id is `hex(sha256(body_json))`.
- Body fields: `version` (1), `network`, `action`, `title`, `description`, `proposer_did`, `electorate_hash`, `created_at`, `expires_at`.
- Actions: `{"kind":"add_authority","did","sphincs_pk_hex","name"}`, `{"kind":"remove_authority","did"}`, `{"kind":"lift_poa"}`, `{"kind":"set_block_production","config":{"mode","block_time_ms","batch_window_ms","heartbeat_secs"}}`.
- In PoA, `electorate_hash` must equal `GET /v1/governance` → `electorate_hash` (the sorted authority DIDs). In PoS it is informational.

Block production is an operational setting, so the authorities may change it during PoA; the economic parameters (emission, shares, fees, slashing) stay at their genesis values until stake-weighted governance (Tokenomics §1.7).

## Staking

Staking is native. Validators stake ASTRA they hold: the native balance of the DID's address plus locked PoA-phase earnings not yet released. There is no staking receipt token. A stake operation is signed with the validator DID's key:

```text
SPACEKIT-STAKE-v1\n{body_json}
body: { "version": 1, "network", "did", "sphincs_pk_hex", "action": "bond" | "unbond", "amount_wei": "…", "nonce": n, "name"? }
```

- `bond` adds to the bonded stake. Bonded plus unbonding may not exceed the DID's holdings.
- `unbond` stops the amount counting immediately and releases it after `unbonding_secs`.
- `nonce` is the validator's operation count (`GET /v1/staking` → `next_nonce`), so a message cannot be replayed.
- **Effective stake** is `min(bonded, holdings − unbonding)`. If holdings fall, the weight falls with them.

Genesis staking rules: `"staking": { "min_stake_astra": 10000, "unbonding_secs": 1814400 }` (defaults) and `"pos_grace_days": 30`. `POST /v1/consensus/register-validator` answers 403 during PoA and 410 afterwards on a network with a genesis: stake goes through `/v1/staking`. On a network without a genesis, `register-validator` takes `stake_wei`, which must be backed by the DID's native holdings and be at least `SPACEKIT_MIN_VALIDATOR_STAKE_WEI` (default 10,000 ASTRA).

There is no slashing yet, so stake is not at risk. A SPHINCS+ DID holds its ASTRA at its own address and spends it with SPHINCS+-signed transfers (`POST /v1/transfer`, see [`ASTRA_LEDGER.md`](ASTRA_LEDGER.md)).

## Service rewards

Rewards are settled once per epoch as system transactions to the rewards system address `0x…0003` at the start of the first block after the epoch ends. The node executes them natively (`native_rewards.rs`); there is no rewards contract ledger. `INIT` (once) mints the genesis treasury allocation, 350,000,000 ASTRA, to the treasury contract `0x…0004`; `END_POA` after the lift; then one `CREDIT` per recipient, minted straight into the recipient address's native balance. During PoA, credits to authorities and affiliated operators are `CREDIT_LOCKED` (12-month cliff from genesis, vesting to month 36, stakeable, not transferable). Every node derives the same list from the chain (including the lock list, which comes from the on-chain governance state) and rejects a block whose leading system transactions differ. Total emission, locked amounts included, is capped at 2,000,000,000 ASTRA.

**Treasury.** The treasury contract `0x…0004` holds native ASTRA and pays M-of-N approved spends from its own balance with `transfer_u128`. Its signers and threshold come from the genesis `treasury` section (`{ "threshold": 2, "signer_dids": [...] }`). Anyone can deposit ASTRA to it (`DEPOSIT`, op 0x20).

Events counted: service logs and gas of user transactions, and **one consensus unit per block to its `proposer_did`**. The consensus unit is how block producers earn the consensus share of emission, and so how they come to hold stake.

## Endpoints

Reads are public. Writes carry their own signatures and are queued for the next block (HTTP 202 with `tx_hash`).

| Route | |
|---|---|
| `GET /v1/chain/status` | Head (with `proposer_did`), producer set, block production, rewards, consensus summary |
| `GET /v1/chain/seals/{number}` | One block's seal |
| `GET /v1/governance` | Mode, authorities, stake electorate, thresholds, `electorate_hash`, `state_hash`, block production |
| `GET /v1/governance/proposals?status=` | Proposals with tallies |
| `GET /v1/governance/proposals/{id}` | Full proposal, votes, the exact vote text to sign |
| `POST /v1/governance/proposals` | `{ body_json, signature_hex }` |
| `POST /v1/governance/votes` | `{ proposal_id, voter_did, choice, signature_hex }` |
| `GET /v1/governance/export` | Every signed proposal and vote |
| `GET /v1/staking` | Staking rules, validators (bonded, holdings, effective, active, next nonce), producer set |
| `POST /v1/staking` | `{ body_json, signature_hex }` |

## Configuration

| Variable | Default | |
|---|---|---|
| `SPACEKIT_POA_GENESIS_FILE` | unset | The genesis file. Installed into the chain at height 0. Without it the node runs a plain chain with no governance. |
| `SPACEKIT_AUTHORITY_WALLET` | unset | This node's producer wallet (`{ did, sphincs_pk_hex, sphincs_sk_hex }`). Without it the node follows the chain but does not produce. |
| `SPACEKIT_SEAL_STORE_PATH` | `temp_blockchain_storage/block_seals.jsonl` | Block seals |
| `SPACEKIT_GENESIS_ALLOC_FILE` | unset | Genesis balances, `{ "accounts": [{ "address": "0x…", "balance": "…" }] }`, applied at height 0 |
| `SPACEKIT_BLOCK_PRODUCER`, `SPACEKIT_BLOCK_PRODUCTION`, `SPACEKIT_BLOCK_TIME_MS`, `SPACEKIT_BLOCK_BATCH_WINDOW_MS`, `SPACEKIT_BLOCK_HEARTBEAT_SECS` | unset, `on_demand`, 2000, 500, 300 | A node without a genesis produces alone only with `SPACEKIT_BLOCK_PRODUCER=solo`. Ignored on a governed network. |
| `SPACEKIT_SRA_EPOCH_SECS` | `86400` | Reward epoch length |
| `SPACEKIT_SRA_GENESIS_TS` | node default | Epoch origin; must be the same on every node |
| `SPACEKIT_AFFILIATED_OPERATOR_DIDS` | unset | DIDs whose PoA credits are locked like the authorities' |
| `SPACEKIT_LEGACY_TX_SIGNATURES` | unset | `1` also accepts pre-v2 transaction signatures (test networks) |

Removed: `SPACEKIT_GOVERNANCE_STATE_PATH` and `SPACEKIT_GOVERNANCE_SYNC_URLS` (governance is in the chain), `SPACEKIT_ASTRA_REWARDS_WASM` (rewards are native; there is no AstraRewards contract) and `SPACEKIT_POS_GRACE_DAYS` (now `pos_grace_days` in the genesis file, since every node must agree on it).

The CLI sets these from `[blockchain]` and `[blockchain.poa]` in the network profile (`spacekit network init --poa-genesis … --authority-wallet …`).

Genesis file (identical on every node):

```json
{
  "network": "testnet",
  "min_validators_to_lift": 10,
  "block_production": { "mode": "on_demand", "block_time_ms": 2000, "batch_window_ms": 500, "heartbeat_secs": 300 },
  "staking": { "min_stake_astra": 10000, "unbonding_secs": 1814400 },
  "pos_grace_days": 30,
  "authorities": [
    { "did": "did:spacekit:testnet:<address>", "sphincs_pk_hex": "<hex>", "name": "Operator 1" }
  ]
}
```

`network` must match `network.name` in the node config.

## Off-chain writes are off on governed networks

The faucet, rollup bundle settlement (`/rollup/*`) and contract reads that fall back to the storage node all touch state outside blocks, so the node that used them would compute a different state root from its peers. They are refused on a network with a genesis file. Fund test accounts with `SPACEKIT_GENESIS_ALLOC_FILE` or transfers. Rollup settlement needs to become a consensus transaction (with the sequencer keys in governance) before browser rollups can settle on a governed network.

## Operator workflow

```sh
# Once per operator: the producer / governance key (~/.spacekit/did_wallet.json)
spacekit did create --save

spacekit governance --node http://127.0.0.1:8080 status
spacekit governance propose --title "Admit Operator 5" \
  add-authority --did did:spacekit:testnet:… --sphincs-pk-hex … --name "Operator 5"
spacekit governance proposals --status pending      # once the proposal is in a block
spacekit governance vote <proposal-id> approve

# Stake (from ASTRA this DID holds, locked or not)
spacekit governance staking
spacekit governance stake bond 15000 --name "Operator 5"
spacekit governance stake unbond 5000

# After the lift: change block production by stake vote
spacekit governance propose --title "Blocks every 2 s" \
  set-block-production --mode interval --block-time-ms 2000

# Signing for spacekit.xyz/governance (the key stays on your machine)
spacekit governance sign-proposal --body-json @proposal.json > proposal.sig
spacekit governance sign-vote <proposal-id> approve --network testnet > vote.sig
```

Keep `did_wallet.json` on an operator machine. It is the producer's sealing key and its voting key.

## Local devnet and end-to-end test

```sh
# Four authorities and one observer on this machine (ports from 39000)
spacekit network devnet init --authorities 4 --observers 1 \
  --fund 0xYOUR_ADDRESS=10000000000 --rewards
spacekit network devnet up
spacekit network devnet status
spacekit governance --node http://127.0.0.1:39002 --wallet ~/.spacekit/devnet/node-0/wallet.json status
spacekit network devnet down

# The gate suite (its own devnet; writes a report plus node logs)
spacekit network test --suite poa --report poa-e2e.xml
```

Devnet flags: `--production on-demand|interval`, `--block-time-ms`, `--batch-window-ms`, `--heartbeat-secs`, `--min-stake-astra`, `--unbonding-days`, `--pos-grace-days`.

The `poa` suite checks: sealed round-robin production and head agreement; an idle chain makes only heartbeat blocks; genesis balances and the disabled faucet; shared contract storage across two signers, with transactions sent to different nodes; a late joiner (blocks, seals, governance; cannot produce; a transaction sent to it is included); `lift_poa` refused below the minimum; admission by `add_authority` (the new authority then produces); locked reward settlement; validators staking their earnings; the lift (staked validators take over, or the authorities as fallback); and a stake-weighted `set_block_production` vote. `.github/workflows/poa-e2e.yml` runs the suite nightly.

## Upgrading

This release changes block contents (`proposer_did`), the state root, transaction signatures and where governance lives. **Start every network from fresh state and upgrade every node and client together.** A node refuses to start on a chain that has blocks but no on-chain governance state.

## Known limits

- **No slashing.** Stake carries weight but is not at risk yet.
- **Rollup settlement** is off on governed networks until it is a consensus transaction.
- **Reorganizations** go back at most 64 blocks, and competing blocks are checked against the current producer set (exact for PoA; a close approximation in PoS).
- **Seal size.** Each block carries a 29,792-byte SPHINCS+ seal, which dominates block size for small blocks (about 30 KB per block, 8.6 MB a day at the default heartbeat, more under load). Nodes keep the latest 2,048 seals in memory and index the rest on disk. A smaller post-quantum signature (ML-DSA/Dilithium, about 2.4 KB) for seals would cut this by an order of magnitude; the governance and staking signatures could stay SPHINCS+.
- **State snapshots** are written in full after every block, and every block is kept in memory; large chains will need periodic checkpoints and an on-disk block store.
- **Governance JSON** is rewritten whole on every change; proposals are never pruned.
- **Votes for a proposal** are only accepted once the proposal is in a block.
