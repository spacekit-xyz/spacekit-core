use anyhow::{anyhow, Result};
use ml_dsa::{
    Generate, KeyExport, Keypair, MlDsa65, Signer, Signature, Verifier, VerifyingKey, SigningKey,
};
use ml_kem::{
    kem::{Decapsulate, Encapsulate, Kem, KeyExport as KemKeyExport, TryKeyInit},
    Ciphertext, DecapsulationKey, EncapsulationKey, MlKem768, Seed,
};
use zeroize::Zeroizing;

pub const KEM_PK_SIZE: usize = 1184;
pub const KEM_SK_SEED_SIZE: usize = 64;
pub const KEM_CT_SIZE: usize = 1088;
pub const MLDSA_SEED_SIZE: usize = 32;

pub fn kem_generate() -> Result<(Zeroizing<Vec<u8>>, Vec<u8>)> {
    let (dk, ek) = MlKem768::generate_keypair();
    Ok((Zeroizing::new(dk.to_bytes().to_vec()), ek.to_bytes().to_vec()))
}

pub fn kem_encapsulate(pk: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if pk.len() != KEM_PK_SIZE {
        return Err(anyhow!("bad kem pk len: {}", pk.len()));
    }
    let key: ml_kem::kem::Key<EncapsulationKey<MlKem768>> =
        pk.try_into().map_err(|_| anyhow!("bad kem pk"))?;
    let ek = EncapsulationKey::<MlKem768>::new(&key).map_err(|e| anyhow!("bad kem pk: {e:?}"))?;
    let (ct, ss) = ek.encapsulate();
    Ok((ct.as_slice().to_vec(), ss.as_slice().to_vec()))
}

pub fn kem_decapsulate(sk: &[u8], ct: &[u8]) -> Result<Vec<u8>> {
    if sk.len() != KEM_SK_SEED_SIZE {
        return Err(anyhow!("bad kem sk len: {}", sk.len()));
    }
    let seed: Seed = sk.try_into().map_err(|_| anyhow!("bad kem sk len"))?;
    let dk = DecapsulationKey::<MlKem768>::from_seed(seed);
    let ct_arr: Ciphertext<MlKem768> = ct.try_into().map_err(|_| anyhow!("bad kem ct len"))?;
    Ok(dk.decapsulate(&ct_arr).as_slice().to_vec())
}

pub fn signer_generate() -> Result<(Zeroizing<Vec<u8>>, Vec<u8>)> {
    let sk = SigningKey::<MlDsa65>::generate();
    let seed = sk.to_bytes();
    let pk = sk.verifying_key().to_bytes().to_vec();
    Ok((Zeroizing::new(seed.to_vec()), pk))
}

pub fn sign(sk: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
    let seed: ml_dsa::Seed = sk.try_into().map_err(|_| anyhow!("bad sk len"))?;
    let sk = SigningKey::<MlDsa65>::from_seed(&seed);
    Ok(sk.sign(msg).encode().to_vec())
}

pub fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> Result<()> {
    let key: ml_dsa::EncodedVerifyingKey<MlDsa65> =
        pk.try_into().map_err(|_| anyhow!("bad pk len"))?;
    let vk = VerifyingKey::<MlDsa65>::decode(&key);
    let sig_enc: ml_dsa::EncodedSignature<MlDsa65> =
        sig.try_into().map_err(|_| anyhow!("bad sig len"))?;
    let sig = Signature::<MlDsa65>::decode(&sig_enc).ok_or_else(|| anyhow!("bad sig"))?;
    vk.verify(msg, &sig)
        .map_err(|_| anyhow!("signature verify failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_sign_roundtrip() {
        let seed = [7u8; 32];
        let sk = SigningKey::<MlDsa65>::from_seed(&seed.into());
        let pk = sk.verifying_key().to_bytes().to_vec();
        let msg = b"hello manifest";
        let sig = sign(&seed, msg).unwrap();
        verify(&pk, msg, &sig).expect("rust sign should verify in rust");
    }
}
