# Proof-of-authority bootstrap and validator governance

Implemented in `src/validator_governance.rs`. The CLI side is `spacekit governance` (`tools/spacekit-cli/src/full_client/governance_cmd.rs`).

## Lifecycle

1. **Genesis (proof of authority).** The network starts with the authorities named in a genesis file. Authorities validate without stake. `POST /v1/consensus/register-validator` is refused (403) while the network is in PoA.
2. **Growing the set.** Authorities admit operators with `add_authority` proposals and drop them with `remove_authority`. Each authority has one vote, and a proposal passes with `ceil(2n/3)` approvals.
3. **Lifting PoA.** Once there are at least 10 validators (`min_validators_to_lift`), an authority can propose `lift_poa`. Below 10 the proposal is refused. Nothing switches automatically: PoA ends only when the lift proposal passes.
4. **Proof of stake.** After the lift, validators register with stake through `register-validator`. Authorities keep validating unstaked for a grace period (`SPACEKIT_POS_GRACE_DAYS`, default 30). After that, the node drops any authority that has not registered stake. Validator-set governance is then closed; stake-weighted governance takes over.

With 3 authorities the network tolerates no faulty validator (`floor((n-1)/3)`), so start with at least 4 if you can.

## Rules

| | |
|---|---|
| Who can propose or vote | Current authorities only |
| To pass | `ceil(2n/3)` approvals of the current authorities |
| To fail | Enough rejections that passing is impossible (`reject > n - needed`) |
| Voting period | 7 days by default, 1 hour to 30 days |
| Changing a vote | Not allowed |
| When the set changes | Other pending proposals become `stale` and must be proposed again |
| Last authority | Cannot be removed |

Statuses: `pending`, `executed`, `rejected`, `expired`, `stale`, `failed` (passed, but the action was no longer valid when it ran).

## What is signed

Signatures are SPHINCS+ (SHAKE-256-128s-simple) with the authority's DID key. A `did:spacekit:*` DID must end with `hex(sha256(pk)[..20])`.

```text
SPACEKIT-GOVERNANCE-PROPOSAL-v1\n{body_json}
SPACEKIT-GOVERNANCE-VOTE-v1\n{network}\n{proposal_id}\n{approve|reject}
```

- `body_json` is signed exactly as submitted, with no re-canonicalization. The proposal id is `hex(sha256(body_json))`.
- The body fields are `version` (1), `network`, `action`, `title`, `description`, `proposer_did`, `electorate_hash`, `created_at` and `expires_at`.
- The action is one of:
  - `{"kind":"add_authority","did","sphincs_pk_hex","name"}`
  - `{"kind":"remove_authority","did"}`
  - `{"kind":"lift_poa"}`
- `electorate_hash` is `sha256` of the sorted authority DIDs joined by `\n`. It is shown by `GET /v1/governance`.
- The network name is part of both payloads, so a testnet signature is not valid on mainnet.

## Endpoints

The read endpoints are public. The write endpoints carry their own signatures, so they need no API request signing, and relays such as the website API can forward them.

| Route | |
|---|---|
| `GET /v1/chain/status` | Head block, consensus mode, validator set hash, validators, peers |
| `GET /v1/governance` | Mode, authorities, thresholds, `electorate_hash`, `state_hash`, `can_lift_poa` |
| `GET /v1/governance/proposals?status=` | Proposal list with tallies |
| `GET /v1/governance/proposals/{id}` | Full proposal: `body_json`, votes, and the exact vote text to sign |
| `POST /v1/governance/proposals` | `{ body_json, signature_hex }` |
| `POST /v1/governance/votes` | `{ proposal_id, voter_did, choice, signature_hex }` |
| `GET /v1/governance/export` | Every signed proposal and vote, for replay |

## Replication

Proposals and votes are gossiped over P2P (`GovernanceProposal` and `GovernanceVote` messages). They are applied idempotently and re-broadcast when new. Votes that arrive before their proposal are held until it does. The website API also sends every submission to all configured nodes.

Governance is not yet ordered by the chain. Two membership changes decided at the same moment on different nodes could be applied in different orders. During bootstrap, run one membership change at a time.

Every node reports `consensus.state_hash`, a hash of the mode and the authority set. spacekit.xyz/network shows whether the nodes agree. A node that missed a decision can be caught up by replaying `GET /v1/governance/export` from a healthy node.

A node that joins later must start from the same genesis file. It then needs the decided proposals replayed from export, because votes are only accepted from the authorities current on that node.

## Configuration

| Variable | Default | |
|---|---|---|
| `SPACEKIT_POA_GENESIS_FILE` | unset | Genesis authorities. Read only when no state file exists yet. |
| `SPACEKIT_GOVERNANCE_STATE_PATH` | `temp_blockchain_storage/governance.json` | Persisted governance state (authorities, proposals, votes) |
| `SPACEKIT_POS_GRACE_DAYS` | `30` | How long authorities may validate unstaked after PoS activates |

Without a genesis file or a state file, the node runs plain proof of stake, as before, and refuses governance proposals.

Genesis file (every node on the network uses the same one):

```json
{
  "network": "testnet",
  "min_validators_to_lift": 10,
  "authorities": [
    { "did": "did:spacekit:testnet:<address>", "sphincs_pk_hex": "<hex>", "name": "Operator 1" }
  ]
}
```

`network` must match `network.name` in the node config.

## Operator workflow

```sh
# Once per operator: create the authority key (~/.spacekit/did_wallet.json)
spacekit did create --save

spacekit governance --node http://127.0.0.1:8080 status
spacekit governance propose --title "Admit Operator 5" \
  add-authority --did did:spacekit:testnet:… --sphincs-pk-hex … --name "Operator 5"
spacekit governance proposals --status pending
spacekit governance vote <proposal-id> approve

# Signing for spacekit.xyz/governance (the key stays on your machine)
spacekit governance sign-proposal --body-json @proposal.json > proposal.sig
spacekit governance sign-vote <proposal-id> approve --network testnet > vote.sig
```

Keep `did_wallet.json` offline or on an operator machine, not on the node host if you can avoid it. It is the authority's voting key.

## Scope and follow-ups

- Block finality is unchanged: it still counts 2/3 of the validators registered in `ConsensusCoordinator`. Genesis and admitted authorities are registered there with their SPHINCS+ keys (`ValidatorAdmission::Authority`), so their block votes verify. The node's own runtime identity is also registered at start as a key-less `local_bootstrap` entry, as it was before this change.
- Authority rewards: authorities earn normal emission, but PoA-phase credits to authority and SWTCH Labs–affiliated DIDs are locked in AstraRewards (12-month cliff from genesis, linear to month 36; stakeable, not transferable). The node keeps the SRA's lock list in step with this governance state every 30 seconds, and the SRA refuses to credit a block until the marks are on chain. Affiliated operators: `affiliated_operator_dids` in `[compute.sra_config]` or `SPACEKIT_AFFILIATED_OPERATOR_DIDS`. See `economics/spacekit-tokenomics/SpaceKit_Tokenomics.md` §1.11 and `ASTRA_EMISSION.md` §8a.
- No stake pool, lending or liquidity seeding: after the lift, validators stake only ASTRA they earned (authorities can stake their locked balance).
- Next step: put governance messages on chain, so their order is part of consensus rather than gossip.
