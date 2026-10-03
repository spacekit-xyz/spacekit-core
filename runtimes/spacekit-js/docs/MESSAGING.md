# Messaging, groups and paid channels

How browsers running spacekit-js talk to each other through SpaceKit messaging nodes.

## Topology

```
browser A ──HTTP/SSE──▶ messaging node A ◀──libp2p gossip──▶ messaging node B ◀──HTTP/SSE── browser B
                              │                                     │
                              └──── read-only ledger checks ──▶ compute node (chain)
```

- Each user has a home messaging node, which may run on their own machine or be hosted.
- The browser talks only to that node. It sends with `POST /api/messages/envelope` and receives over Server-Sent Events at `GET /api/messages/stream?did=…`.
- Nodes exchange envelopes, group announcements, join requests and backfill over libp2p gossipsub.

## Identity and trust

A node's DID is `did:spacekit:<address>` of its k256 key: the `private_key` in its config. This is the same key and address the user has on the chain.

- **Signing.** The node signs every message it gossips.
- **Checking.** Every node checks that a message is signed by the DID it claims. A message with no signature, or with someone else's, is dropped. Nodes that predate signatures are accepted only with `SPACEKIT_MESSAGING_ALLOW_UNSIGNED=1`.
- **The HTTP API acts as the node's DID.** So every route except `/health` and key lookups needs `Authorization: Bearer <token>` (or `?token=` on the stream). The token comes from `SPACEKIT_MESSAGING_API_TOKEN`, or the node generates one into `<storage>/api-token` (mode 0600). The node listens on `127.0.0.1` by default.
- **Rate limits.** Messages from each remote DID are limited to a sustained 20 per second, with bursts of 100.
- **Replays.** Join, leave, add and remove requests carry a nonce that must increase. A replayed or duplicate message is dropped.

## Client

```ts
import { MessagingClient } from "@spacekit/spacekit-js/messaging";

const client = new MessagingClient({ baseUrl: "http://127.0.0.1:3031", did: myDid, token });
const stop = client.subscribe((event) => { /* message | group | group_deleted | delete */ });
await client.sendChat(sealedText, { groupId });
```

## Groups

| Visibility | Who can join | Listed to |
|---|---|---|
| `public` | anyone | everyone |
| `private` | added by the creator or an admin | members |
| `paid` | holders of an entitlement for the group's listing | everyone |

**Canonical membership.** The creator's node announces the group, and its version number only goes up. Other nodes keep replicas.

**Effective membership.** Messages go to, and are accepted from, the effective members. That is the creator's list, plus anyone this node admitted itself, minus anyone banned. A node admits without waiting for the creator:

- a **paid** joiner, after checking the entitlement on the chain itself;
- a **public** joiner, as is;
- anyone an **admin** adds.

The creator's node folds these members into the canonical list when it sees them, so admission works while the creator is offline.

**Admins.** The creator names admins (`POST /groups/:id/admins`). An admin's signed add or remove applies on every node at once. A removal is also a ban, so the member cannot rejoin by asking again.

## Paid channels

`createPaidChannel` creates the group, then the entitlement-ledger listing for it on the chain. `subscribeToChannel` buys the entitlement on the chain and joins:

```ts
const publisherLedger = chainContractCaller(chain, publisher, ledgerAddress);
const { group } = await createPaidChannel({ client, submitContractCall: publisherLedger,
  name: "Studio notes", price: parseAstra("5"), periodSecs: 30n * 86_400n });

await subscribeToChannel({ client: fan, submitContractCall: chainContractCaller(chain, fanAccount, ledgerAddress),
  group, price: parseAstra("5"), publicKeyHex: fanKyberPublicKeyHex });
```

**Payment.** The payment goes from the subscriber to the publisher's address on the chain. The ledger keeps nothing.

**Admission checks.** A node admits a subscriber only if both checks pass, because anyone can create a listing under any id for any group:

1. `OP_VERIFY_LISTING` says the entitlement is valid for the group's listing: it belongs to this buyer and key, is not expired and is not revoked.
2. `OP_GET_LISTING` shows the listing is the group creator's, made for this group.

**Re-checks.** Every node re-checks the subscribers it admitted every `SPACEKIT_MESSAGING_GROUP_REFRESH_SECS` (default 300) and drops lapsed ones.

**Renewal.** `OP_RENEW` extends the same entitlement by one period.

**Configuration.** Set `SPACEKIT_COMPUTE_NODE_URL` and `SPACEKIT_ENTITLEMENT_CONTRACT_ID` on the nodes.

## Offline members: backfill

A node persists its receive history, along with each sender's signed original. Every refresh interval it asks peers for messages newer than the last one it saw. A peer answers with signed originals the requester may see:

- direct messages to the requester;
- messages in groups the requester belongs to.

The requester checks each original's signature before storing it, so a member who was offline catches up without trusting whoever relayed the messages.

## End-to-end encryption

`ChannelKeyring` holds an AES-256-GCM channel key per group, per epoch. `runChannelKeyManager` keeps the keys current:

- **On the creator's client.** It starts a new epoch for a new group, and again after a restart. It rotates when anyone leaves, and gives new members the current key.
- **Distribution.** Each key is wrapped to the member's Kyber public key and sent as a `channel_key` message.
- **On members' clients.** They accept keys only from the creator.

`kyberKeyWrapper(kyber_wasm, { secretKeyBase64 })` is the default wrapper (ML-KEM-1024 plus AES-GCM). `sealKeyring` and `openSealedKeyring` keep the keyring encrypted at rest.
