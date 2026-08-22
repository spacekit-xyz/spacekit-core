//! Signs stdin with ML-DSA-65 using KEYMASTER_IDENTITY_SEED_B64 (32-byte seed, base64).
//!
//! Modes:
//!   (default)           sign raw stdin bytes
//!   --manifest-body     stdin is manifest JSON without `sig`; signs canonical body JSON
//!   --verify-manifest   stdin is full manifest JSON; env SIGNER_PK_B64 required; exit 0 if valid

use std::io::{self, Read, Write};

use base64::Engine;
use clap::Parser;
use spacekit_keymaster::manifest_sig::{sign_manifest_body, verify_manifest};
use spacekit_keymaster::pq_crypto::sign;
use spacekit_keymaster::types::Manifest;

#[derive(Parser)]
struct Args {
    /// Sign canonical manifest body JSON (stdin = manifest fields without sig).
    #[arg(long)]
    manifest_body: bool,
    /// Verify manifest signature (stdin = full manifest JSON).
    #[arg(long)]
    verify_manifest: bool,
}

fn read_stdin() -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

fn load_seed() -> anyhow::Result<Vec<u8>> {
    let seed_b64 = std::env::var("KEYMASTER_IDENTITY_SEED_B64")
        .map_err(|_| anyhow::anyhow!("KEYMASTER_IDENTITY_SEED_B64 not set"))?;
    let seed = base64::engine::general_purpose::STANDARD.decode(seed_b64.trim())?;
    if seed.len() != 32 {
        anyhow::bail!("identity seed must be 32 bytes, got {}", seed.len());
    }
    Ok(seed)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let input = read_stdin()?;

    if args.verify_manifest {
        let pk_b64 = std::env::var("SIGNER_PK_B64")
            .map_err(|_| anyhow::anyhow!("SIGNER_PK_B64 not set"))?;
        let manifest: Manifest = serde_json::from_slice(&input)?;
        verify_manifest(&manifest, &pk_b64)?;
        return Ok(());
    }

    let seed = load_seed()?;

    if args.manifest_body {
        let mut manifest: Manifest = serde_json::from_slice(&input)?;
        manifest.sig.clear();
        let sig = sign_manifest_body(&seed, &manifest)?;
        io::stdout().write_all(&sig)?;
        return Ok(());
    }

    let sig = sign(&seed, &input)?;
    io::stdout().write_all(&sig)?;
    Ok(())
}
