//! OS RNG for rand_core 0.10 (used by ml-dsa and slh-dsa). rand_core 0.10 does not provide OsRng;
//! we use getrandom to implement the required traits.

use rand_core::{Infallible, TryCryptoRng, TryRng};

/// OS-backed RNG implementing rand_core 0.10's `TryRng` and `CryptoRng`.
pub struct OsRng;

impl TryRng for OsRng {
    type Error = Infallible;

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        getrandom::fill(dest).map_err(|_| unreachable!())
    }

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf).expect("getrandom failed");
        Ok(u32::from_le_bytes(buf))
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut buf = [0u8; 8];
        getrandom::fill(&mut buf).expect("getrandom failed");
        Ok(u64::from_le_bytes(buf))
    }
}

impl TryCryptoRng for OsRng {}
