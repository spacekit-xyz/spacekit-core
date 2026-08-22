use core::marker::PhantomData;
use sha3::{Digest, Sha3_256};

pub trait NistLatticeParameterSet {
    const NAME: &'static str;
    const N: usize;
    const Q: u16;
    const ETA: u16;
}

pub enum Kyber512Params {}
pub enum Kyber768Params {}
pub enum Kyber1024Params {}

impl NistLatticeParameterSet for Kyber512Params {
    const NAME: &'static str = "Kyber512";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 2;
}

impl NistLatticeParameterSet for Kyber768Params {
    const NAME: &'static str = "Kyber768";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 2;
}

impl NistLatticeParameterSet for Kyber1024Params {
    const NAME: &'static str = "Kyber1024";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 3;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuantumLatticeCommitment(pub Vec<u16>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuantumLatticeProof {
    pub s: Vec<u16>,
    pub e: Vec<u16>,
}

pub struct QuantumLatticeVc<P: NistLatticeParameterSet>(PhantomData<P>);

impl<P: NistLatticeParameterSet> QuantumLatticeVc<P> {
    pub fn commit(value: &[u8]) -> QuantumLatticeCommitment {
        let (s, e) = derive_opening::<P>(value);
        QuantumLatticeCommitment(commit_with_vectors::<P>(&s, &e))
    }

    pub fn open(value: &[u8]) -> QuantumLatticeProof {
        let (s, e) = derive_opening::<P>(value);
        QuantumLatticeProof { s, e }
    }

    pub fn verify(
        commitment: &QuantumLatticeCommitment,
        value: &[u8],
        proof: &QuantumLatticeProof,
    ) -> bool {
        let (s, e) = derive_opening::<P>(value);
        if s != proof.s || e != proof.e {
            return false;
        }
        commitment.0 == commit_with_vectors::<P>(&s, &e)
    }

    pub fn commitment_bytes(commitment: &QuantumLatticeCommitment) -> Vec<u8> {
        let mut out = Vec::with_capacity(commitment.0.len() * 2);
        for value in &commitment.0 {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    pub fn commitment_from_bytes(bytes: &[u8]) -> Result<QuantumLatticeCommitment, &'static str> {
        if bytes.len() % 2 != 0 {
            return Err("invalid commitment length");
        }
        let mut out = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks(2) {
            out.push(u16::from_be_bytes([chunk[0], chunk[1]]));
        }
        Ok(QuantumLatticeCommitment(out))
    }

    pub fn proof_bytes(proof: &QuantumLatticeProof) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + proof.s.len() * 2 + 4 + proof.e.len() * 2);
        out.extend_from_slice(&(proof.s.len() as u32).to_be_bytes());
        for value in &proof.s {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&(proof.e.len() as u32).to_be_bytes());
        for value in &proof.e {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    pub fn proof_from_bytes(bytes: &[u8]) -> Result<QuantumLatticeProof, &'static str> {
        let mut cursor = 0usize;
        let s_len = read_u32(bytes, &mut cursor)?;
        let s = read_u16_vec(bytes, &mut cursor, s_len)?;
        let e_len = read_u32(bytes, &mut cursor)?;
        let e = read_u16_vec(bytes, &mut cursor, e_len)?;
        Ok(QuantumLatticeProof { s, e })
    }
}

pub type NistLatticeVc = QuantumLatticeVc<Kyber1024Params>;

const LATTICE_SEED: &[u8] = b"nist-lattice-vc-v1";

fn derive_opening<P: NistLatticeParameterSet>(value: &[u8]) -> (Vec<u16>, Vec<u16>) {
    let s = hash_to_u16s::<P>(b"lattice-s", value, P::N, P::Q);
    let e = hash_to_small_u16s::<P>(b"lattice-e", value, P::N, P::ETA);
    (s, e)
}

fn commit_with_vectors<P: NistLatticeParameterSet>(s: &[u16], e: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(P::N);
    for row in 0..P::N {
        let mut acc: u32 = 0;
        for col in 0..P::N {
            let a = matrix_entry::<P>(row, col) as u32;
            let sv = s[col] as u32;
            acc = acc.wrapping_add(a.wrapping_mul(sv));
        }
        acc = acc.wrapping_add(e[row] as u32);
        out.push((acc % (P::Q as u32)) as u16);
    }
    out
}

fn matrix_entry<P: NistLatticeParameterSet>(row: usize, col: usize) -> u16 {
    let mut hasher = Sha3_256::new();
    hasher.update(LATTICE_SEED);
    hasher.update(P::NAME.as_bytes());
    hasher.update(&(row as u32).to_be_bytes());
    hasher.update(&(col as u32).to_be_bytes());
    let digest = hasher.finalize();
    u16::from_be_bytes([digest[0], digest[1]]) % P::Q
}

fn hash_to_u16s<P: NistLatticeParameterSet>(
    domain: &[u8],
    value: &[u8],
    count: usize,
    modulus: u16,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut counter: u32 = 0;
    while out.len() < count {
        let mut hasher = Sha3_256::new();
        hasher.update(domain);
        hasher.update(P::NAME.as_bytes());
        hasher.update(value);
        hasher.update(&counter.to_be_bytes());
        let digest = hasher.finalize();
        for chunk in digest.chunks(2) {
            if out.len() == count {
                break;
            }
            out.push(u16::from_be_bytes([chunk[0], chunk[1]]) % modulus);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn hash_to_small_u16s<P: NistLatticeParameterSet>(
    domain: &[u8],
    value: &[u8],
    count: usize,
    modulus: u16,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut counter: u32 = 0;
    while out.len() < count {
        let mut hasher = Sha3_256::new();
        hasher.update(domain);
        hasher.update(P::NAME.as_bytes());
        hasher.update(value);
        hasher.update(&counter.to_be_bytes());
        let digest = hasher.finalize();
        for byte in digest.iter() {
            if out.len() == count {
                break;
            }
            out.push((*byte as u16) % modulus);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<usize, &'static str> {
    if *cursor + 4 > bytes.len() {
        return Err("invalid proof length");
    }
    let value = u32::from_be_bytes([
        bytes[*cursor],
        bytes[*cursor + 1],
        bytes[*cursor + 2],
        bytes[*cursor + 3],
    ]);
    *cursor += 4;
    Ok(value as usize)
}

fn read_u16_vec(bytes: &[u8], cursor: &mut usize, count: usize) -> Result<Vec<u16>, &'static str> {
    let byte_len = count.checked_mul(2).ok_or("invalid proof length")?;
    if *cursor + byte_len > bytes.len() {
        return Err("invalid proof length");
    }
    let mut out = Vec::with_capacity(count);
    for chunk in bytes[*cursor..*cursor + byte_len].chunks(2) {
        out.push(u16::from_be_bytes([chunk[0], chunk[1]]));
    }
    *cursor += byte_len;
    Ok(out)
}
