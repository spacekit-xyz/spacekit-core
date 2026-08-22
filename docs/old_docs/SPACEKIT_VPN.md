# SpaceKit VPN

You’re upgrading “Mullvad‑style VPN” into “DID‑native, PQ‑secure access network.” Love this direction.

Below is a **concrete spec + Rust skeletons** for a DID‑based VPN, assuming:

- You have **PQ crypto** (KEM + signatures) already.
- You have a **SpaceKit DID method** like `did:spacekit:testnet:alice`.
- You don’t need UI code—just engine, control plane, and node.

---

## 1. High‑level architecture with DID

**Identity & access model:**

- **Identity:** `did:spacekit:testnet:alice`
- **Auth:** DID keypair signs an access token request.
- **Entitlement:** A **Verifiable Credential (VC)** or on‑chain fact says:  
  “This DID is allowed to use VPN until `expires_at`.”
- **Tunnel keys:** Ephemeral PQ keypair per device/session, bound to the DID.

**Flow:**

1. Client holds DID keys locally (SpaceKit wallet/agent).
2. Client requests **VPN access token** from control plane:
   - Signs request with DID key.
   - Optionally presents VC or lets control plane resolve it.
3. Control plane verifies DID + entitlement, returns:
   - Short‑lived **access token** (JWT‑ish or custom).
   - Server list + public keys.
4. Client picks server, runs **PQ handshake**:
   - Presents access token.
   - Performs KEM to derive session keys.
5. Server verifies token, establishes tunnel.

---

## 2. Core Rust crates/modules

### Crate layout

- `vpn-core/`
  - `tun/` — TUN integration
  - `transport/` — UDP, NAT keepalive
  - `crypto/` — PQ KEM + AEAD (you already have)
  - `protocol/` — handshake + data frames
  - `engine/` — tunnel engine
- `vpn-identity/`
  - `did/` — DID resolution + key extraction
  - `vc/` — entitlement verification
  - `auth/` — access token issuance/validation
- `vpn-control-plane/`
  - HTTP API for tokens, server list
- `vpn-node/`
  - Server daemon (per VPN node)
- `vpn-client-cli/`
  - CLI wrapper around `vpn-core` + `vpn-identity`

---

## 3. DID & entitlement spec

### 3.1 DID document expectation

Assume a SpaceKit DID resolves to something like:

```json
{
  "id": "did:spacekit:testnet:alice",
  "verificationMethod": [
    {
      "id": "did:spacekit:testnet:alice#sig-1",
      "type": "Ed25519VerificationKey2020",
      "controller": "did:spacekit:testnet:alice",
      "publicKeyMultibase": "z..."
    }
  ],
  "authentication": [
    "did:spacekit:testnet:alice#sig-1"
  ]
}
```

### 3.2 Verifiable Credential for VPN access

Example VC (could live on SpaceKit or off‑chain):

```json
{
  "@context": ["https://www.w3.org/2018/credentials/v1"],
  "id": "urn:uuid:...",
  "type": ["VerifiableCredential", "VpnAccessCredential"],
  "issuer": "did:spacekit:testnet:spacekit-vpn-issuer",
  "credentialSubject": {
    "id": "did:spacekit:testnet:alice",
    "plan": "mullvad-style-flat",
    "expiresAt": "2026-12-31T23:59:59Z"
  },
  "issuanceDate": "2026-01-01T00:00:00Z",
  "proof": {
    "type": "Ed25519Signature2020",
    "created": "...",
    "verificationMethod": "did:spacekit:testnet:spacekit-vpn-issuer#key-1",
    "proofPurpose": "assertionMethod",
    "jws": "..."
  }
}
```

---

## 4. Rust: DID + VC + access token

### 4.1 DID resolution trait

```rust
pub struct DidDocument {
    pub id: String,
    pub verification_methods: Vec<VerificationMethod>,
    pub authentication: Vec<String>,
}

pub struct VerificationMethod {
    pub id: String,
    pub public_key: Vec<u8>, // decoded
    pub type_: String,
}

pub trait DidResolver {
    fn resolve(&self, did: &str) -> anyhow::Result<DidDocument>;
}
```

### 4.2 VC verification

```rust
pub struct VpnAccessClaim {
    pub subject_did: String,
    pub plan: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

pub trait VcVerifier {
    fn verify_vpn_access_vc(&self, vc_jwt_or_ld: &str) -> anyhow::Result<VpnAccessClaim>;
}
```

### 4.3 Access token format

Keep it simple: signed token binding DID + expiry.

```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct VpnAccessToken {
    pub subject_did: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub allowed_regions: Vec<String>,
}

pub trait AccessTokenSigner {
    fn sign(&self, token: &VpnAccessToken) -> anyhow::Result<String>; // compact string
}

pub trait AccessTokenVerifier {
    fn verify(&self, token_str: &str) -> anyhow::Result<VpnAccessToken>;
}
```

---

## 5. Control plane API (Rust spec)

### 5.1 Endpoint: request access token

`POST /v1/access-token`

**Request:**

```json
{
  "did": "did:spacekit:testnet:alice",
  "vc": "<optional_vc>",
  "nonce": "random-string",
  "signature": "base64(signature_over_did+nonce)"
}
```

**Server steps:**

1. Resolve DID → get auth key.
2. Verify `signature` over `did || nonce`.
3. If `vc` provided:
   - Verify VC → `VpnAccessClaim`.
4. Else:
   - Resolve entitlement from SpaceKit/on‑chain.
5. Check `expires_at > now`.
6. Issue `VpnAccessToken` signed by control plane.

**Response:**

```json
{
  "access_token": "<signed_token>",
  "servers": [
    {
      "id": "us-west-1",
      "host": "usw1.vpn.spacekit.xyz",
      "port": 51820,
      "public_key": "base64(pq_server_pubkey)",
      "region": "us-west"
    }
  ]
}
```

### 5.2 Rust handler skeleton (Axum‑style)

```rust
#[derive(serde::Deserialize)]
struct AccessTokenRequest {
    did: String,
    vc: Option<String>,
    nonce: String,
    signature: String,
}

#[derive(serde::Serialize)]
struct AccessTokenResponse {
    access_token: String,
    servers: Vec<ServerInfo>,
}

async fn issue_access_token(
    Json(req): Json<AccessTokenRequest>,
    State(deps): State<Deps>,
) -> Result<Json<AccessTokenResponse>, StatusCode> {
    let did_doc = deps.did_resolver.resolve(&req.did)?;
    deps.sig_verifier.verify_did_auth(&did_doc, &req.nonce, &req.signature)?;

    let claim = if let Some(vc) = req.vc {
        deps.vc_verifier.verify_vpn_access_vc(&vc)?
    } else {
        deps.entitlement_store.lookup(&req.did)?
    };

    if claim.expires_at <= chrono::Utc::now() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let token = VpnAccessToken {
        subject_did: req.did.clone(),
        expires_at: claim.expires_at,
        allowed_regions: vec!["us-west".into(), "eu-central".into()],
    };

    let token_str = deps.token_signer.sign(&token)?;

    let servers = deps.server_registry.list_for_regions(&token.allowed_regions);

    Ok(Json(AccessTokenResponse {
        access_token: token_str,
        servers,
    }))
}
```

---

## 6. Tunnel protocol with DID‑bound token

### 6.1 Handshake message

Client → Server:

```rust
pub struct ClientHello {
    pub version: u8,
    pub access_token: String,      // signed VpnAccessToken
    pub kem_public_key: Vec<u8>,   // PQ KEM public key
    pub nonce: [u8; 32],
}
```

Server → Client:

```rust
pub struct ServerHello {
    pub version: u8,
    pub kem_ciphertext: Vec<u8>,   // encapsulated shared secret
    pub nonce: [u8; 32],
}
```

### 6.2 Server handshake logic

```rust
fn handle_client_hello(
    hello: ClientHello,
    deps: &Deps,
) -> anyhow::Result<ServerHello> {
    // 1. Verify access token
    let token = deps.token_verifier.verify(&hello.access_token)?;
    if token.expires_at <= chrono::Utc::now() {
        anyhow::bail!("token expired");
    }

    // 2. Derive shared secret via KEM
    let (ciphertext, shared_secret) = deps.kem.encapsulate(&hello.kem_public_key)?;

    // 3. Derive session keys
    let session_keys = deps.kdf.derive(&shared_secret, b"vpn-session");

    // 4. Install session in engine (indexed by client addr, nonce, etc.)
    deps.session_store.insert(token.subject_did.clone(), session_keys);

    // 5. Respond
    Ok(ServerHello {
        version: 1,
        kem_ciphertext: ciphertext,
        nonce: rand::random(),
    })
}
```

---

## 7. Client engine wiring

### 7.1 Client: obtain token + connect

```rust
pub struct VpnClient<C: ControlPlaneApi, E: Engine> {
    control: C,
    engine: E,
}

impl<C: ControlPlaneApi, E: Engine> VpnClient<C, E> {
    pub async fn connect_with_did(
        &self,
        did: &str,
        vc: Option<String>,
        region: &str,
    ) -> anyhow::Result<()> {
        // 1. Get access token + servers
        let token_resp = self.control.request_access_token(did, vc).await?;
        let server = token_resp
            .servers
            .into_iter()
            .find(|s| s.region == region)
            .ok_or_else(|| anyhow::anyhow!("no server in region"))?;

        // 2. Run handshake + start tunnel
        self.engine
            .connect(server, token_resp.access_token)
            .await
    }
}
```

### 7.2 Engine: handshake + TUN loop

```rust
#[async_trait::async_trait]
pub trait Engine {
    async fn connect(&self, server: ServerInfo, access_token: String) -> anyhow::Result<()>;
}

pub struct PqEngine {
    // tun, udp, crypto, etc.
}

#[async_trait::async_trait]
impl Engine for PqEngine {
    async fn connect(&self, server: ServerInfo, access_token: String) -> anyhow::Result<()> {
        // 1. Open UDP socket to server.host:server.port
        // 2. Generate KEM keypair
        // 3. Send ClientHello { access_token, kem_public_key, nonce }
        // 4. Receive ServerHello, decapsulate, derive session keys
        // 5. Start TUN <-> UDP encrypted forwarding loop
        Ok(())
    }
}
```

---

## 8. Privacy notes vs Mullvad

- **No email/PII**: identity is the DID; how the DID was created is up to the user/SpaceKit.
- **Minimal logs**: nodes only store:
  - Session ID
  - DID (or even a hash of DID)
  - Start/stop timestamps (or none, if you’re hardcore).
- **Entitlement off‑path**: VC/on‑chain facts can be verified without centralizing user data.

---

We can do the next steps:

- Tighten the **on‑chain/SpaceKit side** of the VC/entitlement story, or  
- Go deeper into the **TUN + UDP + PQ AEAD** engine with more concrete Rust types and error handling.