// DID wallet trait and in-memory implementation

/// Local DID with key material. Both `public_key` and `private_key` are required;
/// `sign` uses the private key and then self-verifies with the public key.
#[derive(Clone)]
pub struct LocalDid {
    pub did: String,
    pub key_id: String,
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}

pub trait DidWallet {
    fn list_dids(&self) -> anyhow::Result<Vec<String>>;
    fn get_local_did(&self, did: &str) -> anyhow::Result<LocalDid>;
    fn sign(&self, did: &str, payload: &[u8]) -> anyhow::Result<Vec<u8>>;
}

pub struct InMemoryDidWallet {
    pub keys: Vec<LocalDid>,
}

impl DidWallet for InMemoryDidWallet {
    fn list_dids(&self) -> anyhow::Result<Vec<String>> {
        Ok(self.keys.iter().map(|k| k.did.clone()).collect())
    }

    fn get_local_did(&self, did: &str) -> anyhow::Result<LocalDid> {
        self.keys
            .iter()
            .find(|k| k.did == did)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("DID not found"))
    }

    fn sign(&self, did: &str, payload: &[u8]) -> anyhow::Result<Vec<u8>> {
        let local = self.get_local_did(did)?;
        sign_with_private_key(&local.public_key, &local.private_key, payload)
    }
}

/// Signs `payload` with the SPHINCS+ private key (quantum-resistant) and verifies the result.
fn sign_with_private_key(
    public_key: &[u8],
    private_key: &[u8],
    payload: &[u8],
) -> anyhow::Result<Vec<u8>> {
    use super::quantum::SphincsPlus;
    let sig = SphincsPlus::sign(private_key, payload)
        .map_err(|_| anyhow::anyhow!("invalid SPHINCS+ private key"))?;
    if !SphincsPlus::verify(public_key, payload, &sig) {
        anyhow::bail!("signature self-verification failed");
    }
    Ok(sig)
}
