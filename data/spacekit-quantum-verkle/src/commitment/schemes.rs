use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloy_primitives::{utils::Keccak256, B256};
use core::marker::PhantomData;
/// A generic commitment scheme interface for PQ commitment schemes.
pub trait CommitmentScheme {
    type Commitment;
    type Proof;
    type MultiProof;

    const PARAMS_ID: &'static str;
    const VERSION: u16 = 1;

    fn commit(values: &[Vec<u8>]) -> Self::Commitment;
    fn commit_with_aux(
        values: &[Vec<u8>],
        _aux: Option<&[u8]>,
    ) -> (Self::Commitment, Option<Vec<u8>>) {
        (Self::commit(values), None)
    }
    fn open(values: &[Vec<u8>], index: usize) -> Result<Self::Proof, String>;
    fn open_with_aux(
        values: &[Vec<u8>],
        index: usize,
        _aux: Option<&[u8]>,
    ) -> Result<Self::Proof, String> {
        Self::open(values, index)
    }
    fn verify(
        commitment: &Self::Commitment,
        index: usize,
        value: &[u8],
        proof: &Self::Proof,
    ) -> bool;
    fn open_multi(
        values: &[Vec<u8>],
        indices: &[usize],
        aux: Option<&[u8]>,
    ) -> Result<Self::MultiProof, String>;
    fn verify_multi(
        commitment: &Self::Commitment,
        indices: &[usize],
        values: &[Vec<u8>],
        proof: &Self::MultiProof,
    ) -> bool;
    fn commitment_bytes(commitment: &Self::Commitment) -> Vec<u8>;
    fn params_blob() -> Vec<u8>;
    fn proof_bytes(proof: &Self::Proof) -> Vec<u8>;
    fn proof_from_bytes(bytes: &[u8]) -> Result<Self::Proof, String>;
    fn multi_proof_bytes(proof: &Self::MultiProof) -> Vec<u8>;
    fn multi_proof_from_bytes(bytes: &[u8]) -> Result<Self::MultiProof, String>;
}

/// Hash-based commitment scheme (PQ-friendly, binding only).
/// Commitment is H(v0||v1||...); openings are H(v_i). Use `verify_multi` with the full
/// value list to check openings against the commitment; `verify` only checks proof == H(value).
pub struct HashCommitmentScheme;

impl CommitmentScheme for HashCommitmentScheme {
    type Commitment = B256;
    type Proof = B256;
    type MultiProof = Vec<B256>;
    const PARAMS_ID: &'static str = "VC_HASH_V1";

    fn commit(values: &[Vec<u8>]) -> Self::Commitment {
        let mut hasher = Keccak256::new();
        for value in values {
            hasher.update(value);
        }
        hasher.finalize().into()
    }

    fn open(values: &[Vec<u8>], index: usize) -> Result<Self::Proof, String> {
        if index >= values.len() {
            return Err("index out of bounds".to_string());
        }
        let mut hasher = Keccak256::new();
        hasher.update(&values[index]);
        Ok(hasher.finalize().into())
    }

    fn verify(
        commitment: &Self::Commitment,
        _index: usize,
        value: &[u8],
        proof: &Self::Proof,
    ) -> bool {
        let mut hasher = Keccak256::new();
        hasher.update(value);
        let expected: B256 = hasher.finalize().into();
        // Proof must equal H(value). For single-value use, caller should also assert commitment == proof.
        &expected == proof
    }

    fn open_multi(
        values: &[Vec<u8>],
        indices: &[usize],
        _aux: Option<&[u8]>,
    ) -> Result<Self::MultiProof, String> {
        let mut proofs = Vec::with_capacity(indices.len());
        for &index in indices {
            if index >= values.len() {
                return Err("index out of bounds".to_string());
            }
            let mut hasher = Keccak256::new();
            hasher.update(&values[index]);
            proofs.push(hasher.finalize().into());
        }
        Ok(proofs)
    }

    fn verify_multi(
        commitment: &Self::Commitment,
        indices: &[usize],
        values: &[Vec<u8>],
        proof: &Self::MultiProof,
    ) -> bool {
        if indices.len() != proof.len() {
            return false;
        }
        if &Self::commit(values) != commitment {
            return false;
        }
        for (pos, &index) in indices.iter().enumerate() {
            if index >= values.len() {
                return false;
            }
            let mut hasher = Keccak256::new();
            hasher.update(&values[index]);
            let expected: B256 = hasher.finalize().into();
            if &expected != &proof[pos] {
                return false;
            }
        }
        // For single-value usage, commitment should match the first index proof if present.
        true
    }

    fn commitment_bytes(commitment: &Self::Commitment) -> Vec<u8> {
        commitment.as_slice().to_vec()
    }

    fn params_blob() -> Vec<u8> {
        params_blob_from_id(Self::PARAMS_ID, 32, 0, 0, false)
    }

    fn proof_bytes(proof: &Self::Proof) -> Vec<u8> {
        proof.as_slice().to_vec()
    }

    fn proof_from_bytes(bytes: &[u8]) -> Result<Self::Proof, String> {
        if bytes.len() != 32 {
            return Err("invalid proof length".to_string());
        }
        Ok(B256::from_slice(bytes))
    }

    fn multi_proof_bytes(proof: &Self::MultiProof) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + proof.len() * 32);
        out.extend_from_slice(&(proof.len() as u32).to_be_bytes());
        for item in proof {
            out.extend_from_slice(item.as_slice());
        }
        out
    }

    fn multi_proof_from_bytes(bytes: &[u8]) -> Result<Self::MultiProof, String> {
        let mut cursor = 0usize;
        let count = read_u32(bytes, &mut cursor)?;
        let mut proofs = Vec::with_capacity(count);
        for _ in 0..count {
            if cursor + 32 > bytes.len() {
                return Err("invalid proof length".to_string());
            }
            proofs.push(B256::from_slice(&bytes[cursor..cursor + 32]));
            cursor += 32;
        }
        Ok(proofs)
    }
}

pub trait LatticeParameterSet {
    const NAME: &'static str;
    const N: usize;
    const Q: u16;
    const ETA: u16;
}

pub enum Kyber512Params {}
pub enum Kyber768Params {}
pub enum Kyber1024Params {}

impl LatticeParameterSet for Kyber512Params {
    const NAME: &'static str = "Kyber512";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 2;
}

impl LatticeParameterSet for Kyber768Params {
    const NAME: &'static str = "Kyber768";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 2;
}

impl LatticeParameterSet for Kyber1024Params {
    const NAME: &'static str = "Kyber1024";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 3;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatticeOpening {
    s: Vec<u16>,
    e: Vec<u16>,
}

/// Deterministic lattice vector commitment scheme with NIST parameter sets.
/// NOTE: Single-value use only; binding but not hiding.
pub struct LatticeCommitmentScheme<P: LatticeParameterSet>(PhantomData<P>);

impl<P: LatticeParameterSet> CommitmentScheme for LatticeCommitmentScheme<P> {
    type Commitment = Vec<u16>;
    type Proof = LatticeOpening;
    type MultiProof = Vec<LatticeOpening>;
    const PARAMS_ID: &'static str = "VC_NIST_LATTICE_V1";

    fn commit(values: &[Vec<u8>]) -> Self::Commitment {
        let value = values.get(0).map(|v| v.as_slice()).unwrap_or(&[]);
        let (s, e) = derive_opening::<P>(value);
        commit_with_vectors::<P>(&s, &e)
    }

    fn open(values: &[Vec<u8>], index: usize) -> Result<Self::Proof, String> {
        if index != 0 {
            return Err("index out of bounds".to_string());
        }
        let value = values
            .get(0)
            .ok_or_else(|| "no values provided".to_string())?;
        let (s, e) = derive_opening::<P>(value);
        Ok(LatticeOpening { s, e })
    }

    fn verify(
        commitment: &Self::Commitment,
        _index: usize,
        value: &[u8],
        proof: &Self::Proof,
    ) -> bool {
        let (s, e) = derive_opening::<P>(value);
        if s != proof.s || e != proof.e {
            return false;
        }
        &commit_with_vectors::<P>(&s, &e) == commitment
    }

    fn open_multi(
        values: &[Vec<u8>],
        indices: &[usize],
        _aux: Option<&[u8]>,
    ) -> Result<Self::MultiProof, String> {
        let mut proofs = Vec::with_capacity(indices.len());
        for &index in indices {
            if index >= values.len() {
                return Err("index out of bounds".to_string());
            }
            let (s, e) = derive_opening::<P>(&values[index]);
            proofs.push(LatticeOpening { s, e });
        }
        Ok(proofs)
    }

    fn verify_multi(
        commitment: &Self::Commitment,
        indices: &[usize],
        values: &[Vec<u8>],
        proof: &Self::MultiProof,
    ) -> bool {
        if indices.len() != proof.len() {
            return false;
        }
        if &Self::commit(values) != commitment {
            return false;
        }
        for (pos, &index) in indices.iter().enumerate() {
            if index >= values.len() {
                return false;
            }
            if !Self::verify(commitment, index, &values[index], &proof[pos]) {
                return false;
            }
        }
        true
    }

    fn commitment_bytes(commitment: &Self::Commitment) -> Vec<u8> {
        let mut out = Vec::with_capacity(commitment.len() * 2);
        for value in commitment {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    fn params_blob() -> Vec<u8> {
        params_blob_from_id(Self::PARAMS_ID, P::N as u32, P::Q, P::ETA, false)
    }

    fn proof_bytes(proof: &Self::Proof) -> Vec<u8> {
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

    fn proof_from_bytes(bytes: &[u8]) -> Result<Self::Proof, String> {
        let mut cursor = 0usize;
        let s_len = read_u32(bytes, &mut cursor)?;
        let s = read_u16_vec(bytes, &mut cursor, s_len)?;
        let e_len = read_u32(bytes, &mut cursor)?;
        let e = read_u16_vec(bytes, &mut cursor, e_len)?;
        Ok(LatticeOpening { s, e })
    }

    fn multi_proof_bytes(proof: &Self::MultiProof) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(proof.len() as u32).to_be_bytes());
        for opening in proof {
            let bytes = Self::proof_bytes(opening);
            out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(&bytes);
        }
        out
    }

    fn multi_proof_from_bytes(bytes: &[u8]) -> Result<Self::MultiProof, String> {
        let mut cursor = 0usize;
        let count = read_u32(bytes, &mut cursor)?;
        let mut proofs = Vec::with_capacity(count);
        for _ in 0..count {
            let len = read_u32(bytes, &mut cursor)?;
            let chunk = read_bytes(bytes, &mut cursor, len)?;
            proofs.push(Self::proof_from_bytes(&chunk)?);
        }
        Ok(proofs)
    }
}

/// Default lattice commitment profile (Kyber-*-style parameters n, q, η). Matches ML-KEM-1024 naming / NIST Level 5.
/// Browser WASM uses `NistSisScheme` instead; this alias is for lattice-mode trees.
pub type NistLatticeScheme = LatticeCommitmentScheme<Kyber1024Params>;

const LATTICE_SEED: &[u8] = b"nist-lattice-vc-v1";

fn derive_opening<P: LatticeParameterSet>(value: &[u8]) -> (Vec<u16>, Vec<u16>) {
    let s = hash_to_u16s::<P>(b"lattice-s", value, P::N, P::Q);
    let e = hash_to_small_u16s::<P>(b"lattice-e", value, P::N, P::ETA);
    (s, e)
}

fn commit_with_vectors<P: LatticeParameterSet>(s: &[u16], e: &[u16]) -> Vec<u16> {
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

fn matrix_entry<P: LatticeParameterSet>(row: usize, col: usize) -> u16 {
    let mut hasher = Keccak256::new();
    hasher.update(LATTICE_SEED);
    hasher.update(P::NAME.as_bytes());
    hasher.update(&(row as u32).to_be_bytes());
    hasher.update(&(col as u32).to_be_bytes());
    let digest: B256 = hasher.finalize().into();
    u16::from_be_bytes([digest.as_slice()[0], digest.as_slice()[1]]) % P::Q
}

fn hash_to_u16s<P: LatticeParameterSet>(
    domain: &[u8],
    value: &[u8],
    count: usize,
    modulus: u16,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut counter: u32 = 0;
    while out.len() < count {
        let mut hasher = Keccak256::new();
        hasher.update(domain);
        hasher.update(P::NAME.as_bytes());
        hasher.update(value);
        hasher.update(&counter.to_be_bytes());
        let digest: B256 = hasher.finalize().into();
        for chunk in digest.as_slice().chunks(2) {
            if out.len() == count {
                break;
            }
            let pair = [chunk[0], chunk[1]];
            out.push(u16::from_be_bytes(pair) % modulus);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn hash_to_small_u16s<P: LatticeParameterSet>(
    domain: &[u8],
    value: &[u8],
    count: usize,
    modulus: u16,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut counter: u32 = 0;
    while out.len() < count {
        let mut hasher = Keccak256::new();
        hasher.update(domain);
        hasher.update(P::NAME.as_bytes());
        hasher.update(value);
        hasher.update(&counter.to_be_bytes());
        let digest: B256 = hasher.finalize().into();
        for byte in digest.as_slice() {
            if out.len() == count {
                break;
            }
            out.push((*byte as u16) % modulus);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<usize, String> {
    if *cursor + 4 > bytes.len() {
        return Err("invalid proof length".to_string());
    }
    let value = u32::from_be_bytes(
        bytes[*cursor..*cursor + 4]
            .try_into()
            .map_err(|_| "invalid proof length".to_string())?,
    );
    *cursor += 4;
    Ok(value as usize)
}

fn read_u16_vec(bytes: &[u8], cursor: &mut usize, count: usize) -> Result<Vec<u16>, String> {
    let byte_len = count
        .checked_mul(2)
        .ok_or_else(|| "invalid proof length".to_string())?;
    if *cursor + byte_len > bytes.len() {
        return Err("invalid proof length".to_string());
    }
    let mut out = Vec::with_capacity(count);
    for chunk in bytes[*cursor..*cursor + byte_len].chunks(2) {
        out.push(u16::from_be_bytes([chunk[0], chunk[1]]));
    }
    *cursor += byte_len;
    Ok(out)
}

fn read_bytes(bytes: &[u8], cursor: &mut usize, count: usize) -> Result<Vec<u8>, String> {
    if *cursor + count > bytes.len() {
        return Err("invalid proof length".to_string());
    }
    let out = bytes[*cursor..*cursor + count].to_vec();
    *cursor += count;
    Ok(out)
}

pub trait WeeWuSisParams {
    const PARAMS_ID: &'static str;
    const N: usize;
    const Q: u16;
    const ETA: u16;
    const HIDE: bool;
}

pub enum Sis128B {}
pub enum Sis128HB {}
pub enum Sis192B {}
pub enum Sis192HB {}

impl WeeWuSisParams for Sis128B {
    const PARAMS_ID: &'static str = "VC_SIS_128B_v1";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 2;
    const HIDE: bool = false;
}

impl WeeWuSisParams for Sis128HB {
    const PARAMS_ID: &'static str = "VC_SIS_128HB_v1";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 2;
    const HIDE: bool = true;
}

impl WeeWuSisParams for Sis192B {
    const PARAMS_ID: &'static str = "VC_SIS_192B_v1";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 3;
    const HIDE: bool = false;
}

impl WeeWuSisParams for Sis192HB {
    const PARAMS_ID: &'static str = "VC_SIS_192HB_v1";
    const N: usize = 256;
    const Q: u16 = 3329;
    const ETA: u16 = 3;
    const HIDE: bool = true;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SisOpening {
    s: Vec<u16>,
    e: Vec<u16>,
    aux: Option<Vec<u8>>,
}

pub struct WeeWuSisCommitmentScheme<P: WeeWuSisParams>(PhantomData<P>);

impl<P: WeeWuSisParams> CommitmentScheme for WeeWuSisCommitmentScheme<P> {
    type Commitment = Vec<u16>;
    type Proof = SisOpening;
    type MultiProof = Vec<SisOpening>;
    const PARAMS_ID: &'static str = P::PARAMS_ID;

    fn commit(values: &[Vec<u8>]) -> Self::Commitment {
        let (commitment, _) = Self::commit_with_aux(values, None);
        commitment
    }

    fn commit_with_aux(
        values: &[Vec<u8>],
        aux: Option<&[u8]>,
    ) -> (Self::Commitment, Option<Vec<u8>>) {
        let value = values.get(0).map(|v| v.as_slice()).unwrap_or(&[]);
        let aux_bytes = if P::HIDE {
            match aux {
                Some(bytes) => Some(bytes.to_vec()),
                None => Some(derive_aux(value)),
            }
        } else {
            None
        };
        let commitment = if values.len() > 1 {
            sis_commit_vector::<P>(values, aux_bytes.as_deref())
        } else {
            let (s, e) = sis_opening::<P>(value, aux_bytes.as_deref());
            sis_commit::<P>(&s, &e)
        };
        (commitment, aux_bytes)
    }

    fn open(values: &[Vec<u8>], index: usize) -> Result<Self::Proof, String> {
        if index != 0 {
            return Err("index out of bounds".to_string());
        }
        let value = values
            .get(0)
            .ok_or_else(|| "no values provided".to_string())?;
        let (s, e) = sis_opening::<P>(value, None);
        Ok(SisOpening { s, e, aux: None })
    }

    fn open_with_aux(
        values: &[Vec<u8>],
        index: usize,
        aux: Option<&[u8]>,
    ) -> Result<Self::Proof, String> {
        if index != 0 {
            return Err("index out of bounds".to_string());
        }
        let value = values
            .get(0)
            .ok_or_else(|| "no values provided".to_string())?;
        if P::HIDE && aux.is_none() {
            return Err("missing aux for hiding profile".to_string());
        }
        let (s, e) = sis_opening::<P>(value, aux);
        Ok(SisOpening {
            s,
            e,
            aux: aux.map(|bytes| bytes.to_vec()),
        })
    }

    fn verify(
        commitment: &Self::Commitment,
        _index: usize,
        value: &[u8],
        proof: &Self::Proof,
    ) -> bool {
        if P::HIDE && proof.aux.is_none() {
            return false;
        }
        let (s, e) = sis_opening::<P>(value, proof.aux.as_deref());
        if s != proof.s || e != proof.e {
            return false;
        }
        &sis_commit::<P>(&s, &e) == commitment
    }

    fn open_multi(
        values: &[Vec<u8>],
        indices: &[usize],
        aux: Option<&[u8]>,
    ) -> Result<Self::MultiProof, String> {
        let context = sis_vector_context(values);
        let mut proofs = Vec::with_capacity(indices.len());
        for &index in indices {
            if index >= values.len() {
                return Err("index out of bounds".to_string());
            }
            let opening_value = sis_opening_material(&values[index], &context, index);
            let (s, e) = sis_opening::<P>(&opening_value, aux);
            proofs.push(SisOpening {
                s,
                e,
                aux: aux.map(|bytes| bytes.to_vec()),
            });
        }
        Ok(proofs)
    }

    fn verify_multi(
        commitment: &Self::Commitment,
        indices: &[usize],
        values: &[Vec<u8>],
        proof: &Self::MultiProof,
    ) -> bool {
        if indices.len() != proof.len() {
            return false;
        }
        let aux = proof.first().and_then(|opening| opening.aux.as_deref());
        if proof.iter().any(|opening| opening.aux.as_deref() != aux) {
            return false;
        }
        // `commit_with_aux` uses a single-value commitment when len == 1, but `open_multi` always
        // uses vector-style opening material. Match the commitment branch so `create_multi_proof` /
        // `verify_multi_proof` agree for batch size 1.
        let expected_commitment = if values.len() == 1 {
            let value = values[0].as_slice();
            let (s, e) = sis_opening::<P>(value, aux);
            sis_commit::<P>(&s, &e)
        } else {
            sis_commit_vector::<P>(values, aux)
        };
        if &expected_commitment != commitment {
            return false;
        }
        let context = sis_vector_context(values);
        for (pos, &index) in indices.iter().enumerate() {
            if index >= values.len() {
                return false;
            }
            let opening_value = sis_opening_material(&values[index], &context, index);
            let (s, e) = sis_opening::<P>(&opening_value, aux);
            if s != proof[pos].s || e != proof[pos].e {
                return false;
            }
        }
        true
    }

    fn commitment_bytes(commitment: &Self::Commitment) -> Vec<u8> {
        let mut out = Vec::with_capacity(commitment.len() * 2);
        for value in commitment {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    fn params_blob() -> Vec<u8> {
        params_blob_from_id(Self::PARAMS_ID, P::N as u32, P::Q, P::ETA, P::HIDE)
    }

    fn proof_bytes(proof: &Self::Proof) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + proof.s.len() * 2 + proof.e.len() * 2);
        out.extend_from_slice(&(proof.s.len() as u32).to_be_bytes());
        for value in &proof.s {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&(proof.e.len() as u32).to_be_bytes());
        for value in &proof.e {
            out.extend_from_slice(&value.to_be_bytes());
        }
        let aux_len = proof.aux.as_ref().map(|bytes| bytes.len()).unwrap_or(0);
        out.extend_from_slice(&(aux_len as u32).to_be_bytes());
        if let Some(aux) = &proof.aux {
            out.extend_from_slice(aux);
        }
        out
    }

    fn proof_from_bytes(bytes: &[u8]) -> Result<Self::Proof, String> {
        let mut cursor = 0usize;
        let s_len = read_u32(bytes, &mut cursor)?;
        let s = read_u16_vec(bytes, &mut cursor, s_len)?;
        let e_len = read_u32(bytes, &mut cursor)?;
        let e = read_u16_vec(bytes, &mut cursor, e_len)?;
        let aux_len = read_u32(bytes, &mut cursor)?;
        let aux = if aux_len > 0 {
            let aux_bytes = read_bytes(bytes, &mut cursor, aux_len)?;
            Some(aux_bytes)
        } else {
            None
        };
        Ok(SisOpening { s, e, aux })
    }

    fn multi_proof_bytes(proof: &Self::MultiProof) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(proof.len() as u32).to_be_bytes());
        for opening in proof {
            let bytes = Self::proof_bytes(opening);
            out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(&bytes);
        }
        out
    }

    fn multi_proof_from_bytes(bytes: &[u8]) -> Result<Self::MultiProof, String> {
        let mut cursor = 0usize;
        let count = read_u32(bytes, &mut cursor)?;
        let mut proofs = Vec::with_capacity(count);
        for _ in 0..count {
            let len = read_u32(bytes, &mut cursor)?;
            let chunk = read_bytes(bytes, &mut cursor, len)?;
            proofs.push(Self::proof_from_bytes(&chunk)?);
        }
        Ok(proofs)
    }
}

pub type NistSisScheme = WeeWuSisCommitmentScheme<Sis128B>;

const SIS_SEED: &[u8] = b"weewuwu-sis-vc-v1";

fn sis_vector_context(values: &[Vec<u8>]) -> Vec<u8> {
    let mut hasher = Keccak256::new();
    hasher.update(b"sis-vector-ctx");
    for value in values {
        hasher.update(value);
    }
    let digest: B256 = hasher.finalize().into();
    digest.as_slice().to_vec()
}

fn sis_opening_material(value: &[u8], context: &[u8], index: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() + context.len() + 4);
    out.extend_from_slice(value);
    out.extend_from_slice(context);
    out.extend_from_slice(&(index as u32).to_be_bytes());
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SisSecurityLevel {
    L1,
    L3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SisProfile {
    Binding,
    Hiding,
}

pub fn setup_sis_params(level: SisSecurityLevel, profile: SisProfile) -> (String, Vec<u8>) {
    match (level, profile) {
        (SisSecurityLevel::L1, SisProfile::Binding) => (
            Sis128B::PARAMS_ID.to_string(),
            params_blob_from_id(
                Sis128B::PARAMS_ID,
                Sis128B::N as u32,
                Sis128B::Q,
                Sis128B::ETA,
                Sis128B::HIDE,
            ),
        ),
        (SisSecurityLevel::L1, SisProfile::Hiding) => (
            Sis128HB::PARAMS_ID.to_string(),
            params_blob_from_id(
                Sis128HB::PARAMS_ID,
                Sis128HB::N as u32,
                Sis128HB::Q,
                Sis128HB::ETA,
                Sis128HB::HIDE,
            ),
        ),
        (SisSecurityLevel::L3, SisProfile::Binding) => (
            Sis192B::PARAMS_ID.to_string(),
            params_blob_from_id(
                Sis192B::PARAMS_ID,
                Sis192B::N as u32,
                Sis192B::Q,
                Sis192B::ETA,
                Sis192B::HIDE,
            ),
        ),
        (SisSecurityLevel::L3, SisProfile::Hiding) => (
            Sis192HB::PARAMS_ID.to_string(),
            params_blob_from_id(
                Sis192HB::PARAMS_ID,
                Sis192HB::N as u32,
                Sis192HB::Q,
                Sis192HB::ETA,
                Sis192HB::HIDE,
            ),
        ),
    }
}

fn sis_opening<P: WeeWuSisParams>(value: &[u8], aux: Option<&[u8]>) -> (Vec<u16>, Vec<u16>) {
    let s = sis_hash_to_u16s::<P>(b"sis-s", value, aux, P::N, P::Q);
    let e = sis_hash_to_small::<P>(b"sis-e", value, aux, P::N, P::ETA);
    (s, e)
}

fn sis_commit<P: WeeWuSisParams>(s: &[u16], e: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(P::N);
    for row in 0..P::N {
        let mut acc: u32 = 0;
        for col in 0..P::N {
            let a = sis_matrix_entry::<P>(row, col) as u32;
            let sv = s[col] as u32;
            acc = acc.wrapping_add(a.wrapping_mul(sv));
        }
        acc = acc.wrapping_add(e[row] as u32);
        out.push((acc % (P::Q as u32)) as u16);
    }
    out
}

fn sis_commit_vector<P: WeeWuSisParams>(values: &[Vec<u8>], aux: Option<&[u8]>) -> Vec<u16> {
    let context = sis_vector_context(values);
    let (s, e) = sis_opening::<P>(&context, aux);
    sis_commit::<P>(&s, &e)
}

fn sis_matrix_entry<P: WeeWuSisParams>(row: usize, col: usize) -> u16 {
    let mut hasher = Keccak256::new();
    hasher.update(SIS_SEED);
    hasher.update(P::PARAMS_ID.as_bytes());
    hasher.update(&(row as u32).to_be_bytes());
    hasher.update(&(col as u32).to_be_bytes());
    let digest: B256 = hasher.finalize().into();
    u16::from_be_bytes([digest.as_slice()[0], digest.as_slice()[1]]) % P::Q
}

fn sis_hash_to_u16s<P: WeeWuSisParams>(
    domain: &[u8],
    value: &[u8],
    aux: Option<&[u8]>,
    count: usize,
    modulus: u16,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut counter: u32 = 0;
    while out.len() < count {
        let mut hasher = Keccak256::new();
        hasher.update(domain);
        hasher.update(P::PARAMS_ID.as_bytes());
        hasher.update(value);
        if let Some(aux_bytes) = aux {
            hasher.update(aux_bytes);
        }
        hasher.update(&counter.to_be_bytes());
        let digest: B256 = hasher.finalize().into();
        for chunk in digest.as_slice().chunks(2) {
            if out.len() == count {
                break;
            }
            out.push(u16::from_be_bytes([chunk[0], chunk[1]]) % modulus);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn sis_hash_to_small<P: WeeWuSisParams>(
    domain: &[u8],
    value: &[u8],
    aux: Option<&[u8]>,
    count: usize,
    modulus: u16,
) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut counter: u32 = 0;
    while out.len() < count {
        let mut hasher = Keccak256::new();
        hasher.update(domain);
        hasher.update(P::PARAMS_ID.as_bytes());
        hasher.update(value);
        if let Some(aux_bytes) = aux {
            hasher.update(aux_bytes);
        }
        hasher.update(&counter.to_be_bytes());
        let digest: B256 = hasher.finalize().into();
        for byte in digest.as_slice() {
            if out.len() == count {
                break;
            }
            out.push((*byte as u16) % modulus);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn derive_aux(value: &[u8]) -> Vec<u8> {
    let mut hasher = Keccak256::new();
    hasher.update(b"sis-aux");
    hasher.update(value);
    let digest: B256 = hasher.finalize().into();
    digest.as_slice().to_vec()
}

fn params_blob_from_id(params_id: &str, n: u32, q: u16, eta: u16, hide: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes());
    let id_bytes = params_id.as_bytes();
    out.extend_from_slice(&(id_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(id_bytes);
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&q.to_be_bytes());
    out.extend_from_slice(&eta.to_be_bytes());
    out.push(hide as u8);
    out
}
