use anchor_lang::prelude::*;
use anchor_lang::solana_program::program_error::ProgramError;
use std::collections::HashMap;

declare_id!("QuantumDIDProgram1111111111111111111111111");

/// Quantum DID Program for Solana
#[program]
pub mod quantum_did_solana {
    use super::*;

    /// Register a quantum DID on Solana
    pub fn register_quantum_did(
        ctx: Context<RegisterQuantumDID>,
        did: String,
        quantum_public_key: Vec<u8>,
        did_document: String,
        quantum_signature: Vec<u8>,
    ) -> Result<()> {
        let identity_account = &mut ctx.accounts.identity_account;
        let user = &ctx.accounts.user;

        // Verify that the DID is not already registered
        require!(!identity_account.is_active, ErrorCode::DIDAlreadyRegistered);

        // Verify quantum signature proves control of quantum keys
        let message = format!("{}{}{}", did, hex::encode(&quantum_public_key), user.key());
        require!(
            verify_quantum_signature(&message.as_bytes(), &quantum_signature, &quantum_public_key)?,
            ErrorCode::InvalidQuantumSignature
        );

        // Store the quantum identity
        identity_account.did = did;
        identity_account.quantum_public_key = quantum_public_key;
        identity_account.did_document = did_document;
        identity_account.solana_address = *user.key;
        identity_account.key_rotation_count = 0;
        identity_account.is_active = true;
        identity_account.last_updated = Clock::get()?.slot;

        emit!(IdentityRegistered {
            solana_address: *user.key,
            did: identity_account.did.clone(),
            quantum_public_key: identity_account.quantum_public_key.clone(),
        });

        Ok(())
    }

    /// Rotate quantum keys for enhanced security
    pub fn rotate_quantum_keys(
        ctx: Context<RotateQuantumKeys>,
        new_quantum_public_key: Vec<u8>,
        new_did_document: String,
        quantum_signature: Vec<u8>,
    ) -> Result<()> {
        let identity_account = &mut ctx.accounts.identity_account;
        let user = &ctx.accounts.user;

        require!(identity_account.is_active, ErrorCode::DIDNotRegistered);
        require!(identity_account.solana_address == *user.key, ErrorCode::UnauthorizedOperation);

        // Create message for signature verification with old key
        let current_slot = Clock::get()?.slot;
        let message = format!(
            "KEY_ROTATION:{}:{}:{}:{}",
            hex::encode(&new_quantum_public_key),
            identity_account.key_rotation_count + 1,
            user.key(),
            current_slot
        );

        // Verify signature with current (old) quantum key
        require!(
            verify_quantum_signature(
                &message.as_bytes(),
                &quantum_signature,
                &identity_account.quantum_public_key
            )?,
            ErrorCode::InvalidQuantumSignature
        );

        // Update to new key
        identity_account.quantum_public_key = new_quantum_public_key;
        identity_account.did_document = new_did_document;
        identity_account.key_rotation_count += 1;
        identity_account.last_updated = current_slot;

        emit!(IdentityUpdated {
            solana_address: *user.key,
            key_rotation_count: identity_account.key_rotation_count,
        });

        Ok(())
    }

    /// Issue a verifiable credential on Solana
    pub fn issue_credential(
        ctx: Context<IssueCredential>,
        credential_hash: [u8; 32],
        subject: Pubkey,
        credential_type: String,
        expires_at: u64, // 0 for no expiry
        quantum_signature: Vec<u8>,
    ) -> Result<()> {
        let issuer_identity = &ctx.accounts.issuer_identity;
        let credential_account = &mut ctx.accounts.credential_account;
        let issuer = &ctx.accounts.issuer;
        let current_slot = Clock::get()?.slot;

        require!(issuer_identity.is_active, ErrorCode::DIDNotRegistered);
        require!(issuer_identity.solana_address == *issuer.key, ErrorCode::UnauthorizedOperation);

        // Verify issuer's quantum signature
        let message = format!(
            "ISSUE_CREDENTIAL:{}:{}:{}:{}:{}",
            hex::encode(credential_hash),
            subject,
            credential_type,
            expires_at,
            issuer.key()
        );

        require!(
            verify_quantum_signature(
                &message.as_bytes(),
                &quantum_signature,
                &issuer_identity.quantum_public_key
            )?,
            ErrorCode::InvalidQuantumSignature
        );

        // Store credential
        credential_account.credential_hash = credential_hash;
        credential_account.issuer = *issuer.key;
        credential_account.subject = subject;
        credential_account.credential_type = credential_type.clone();
        credential_account.issued_at = current_slot;
        credential_account.expires_at = expires_at;
        credential_account.is_revoked = false;

        emit!(CredentialIssued {
            credential_hash,
            issuer: *issuer.key,
            subject,
            credential_type,
        });

        Ok(())
    }

    /// Verify a quantum-signed credential proof
    pub fn verify_credential_proof(
        ctx: Context<VerifyCredentialProof>,
        quantum_signature: Vec<u8>,
        verification_message: String,
    ) -> Result<bool> {
        let credential_account = &ctx.accounts.credential_account;
        let issuer_identity = &ctx.accounts.issuer_identity;
        let verifier = &ctx.accounts.verifier;
        let current_slot = Clock::get()?.slot;

        // Check if credential exists and is not revoked
        require!(!credential_account.is_revoked, ErrorCode::CredentialRevoked);

        // Check expiration
        if credential_account.expires_at > 0 && current_slot > credential_account.expires_at {
            return Ok(false);
        }

        // Verify quantum signature from issuer
        let is_valid = verify_quantum_signature(
            &verification_message.as_bytes(),
            &quantum_signature,
            &issuer_identity.quantum_public_key,
        )?;

        emit!(CredentialVerified {
            credential_hash: credential_account.credential_hash,
            verifier: *verifier.key,
            is_valid,
        });

        Ok(is_valid)
    }

    /// Revoke a credential (only by issuer)
    pub fn revoke_credential(
        ctx: Context<RevokeCredential>,
        quantum_signature: Vec<u8>,
    ) -> Result<()> {
        let credential_account = &mut ctx.accounts.credential_account;
        let issuer_identity = &ctx.accounts.issuer_identity;
        let issuer = &ctx.accounts.issuer;
        let current_slot = Clock::get()?.slot;

        require!(credential_account.issuer == *issuer.key, ErrorCode::UnauthorizedOperation);
        require!(!credential_account.is_revoked, ErrorCode::CredentialRevoked);

        // Verify quantum signature for revocation
        let message = format!(
            "REVOKE_CREDENTIAL:{}:{}",
            hex::encode(credential_account.credential_hash),
            current_slot
        );

        require!(
            verify_quantum_signature(
                &message.as_bytes(),
                &quantum_signature,
                &issuer_identity.quantum_public_key
            )?,
            ErrorCode::InvalidQuantumSignature
        );

        credential_account.is_revoked = true;

        emit!(CredentialRevoked {
            credential_hash: credential_account.credential_hash,
            issuer: *issuer.key,
        });

        Ok(())
    }
}

/// Verify quantum signature (placeholder implementation)
/// In production, this would use actual SPHINCS+ verification
fn verify_quantum_signature(
    message: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> Result<bool> {
    // SPHINCS+ verification is not feasible on-chain at this time.
    // Verify off-chain with the spacekit-did Rust library before submitting transactions.
    // See programs/EXPERIMENTAL.md.
    require!(!signature.is_empty(), ErrorCode::EmptySignature);
    require!(!public_key.is_empty(), ErrorCode::EmptyPublicKey);
    require!(!message.is_empty(), ErrorCode::EmptyMessage);

    Err(ErrorCode::OnChainVerificationUnsupported.into())
}

// Account structures
#[account]
pub struct QuantumIdentity {
    pub did: String,                    // Quantum DID string
    pub quantum_public_key: Vec<u8>,    // SPHINCS+ public key
    pub did_document: String,           // JSON DID document
    pub solana_address: Pubkey,         // Associated Solana address
    pub key_rotation_count: u64,        // Track key rotations
    pub is_active: bool,                // Whether DID is active
    pub last_updated: u64,              // Last update slot
}

#[account]
pub struct VerifiableCredential {
    pub credential_hash: [u8; 32],      // Hash of credential content
    pub issuer: Pubkey,                 // Issuer's Solana address
    pub subject: Pubkey,                // Subject's Solana address
    pub credential_type: String,        // Type of credential
    pub issued_at: u64,                 // Issuance slot
    pub expires_at: u64,                // Expiration slot (0 for no expiry)
    pub is_revoked: bool,               // Whether credential is revoked
}

// Context structures
#[derive(Accounts)]
#[instruction(did: String)]
pub struct RegisterQuantumDID<'info> {
    #[account(
        init,
        payer = user,
        space = 8 + 32 + 4 + did.len() + 4 + 2048 + 4 + 1024 + 32 + 8 + 1 + 8,
        seeds = [b"quantum_identity", user.key().as_ref()],
        bump
    )]
    pub identity_account: Account<'info, QuantumIdentity>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RotateQuantumKeys<'info> {
    #[account(
        mut,
        seeds = [b"quantum_identity", user.key().as_ref()],
        bump
    )]
    pub identity_account: Account<'info, QuantumIdentity>,
    
    #[account(mut)]
    pub user: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(credential_hash: [u8; 32], credential_type: String)]
pub struct IssueCredential<'info> {
    #[account(
        seeds = [b"quantum_identity", issuer.key().as_ref()],
        bump
    )]
    pub issuer_identity: Account<'info, QuantumIdentity>,
    
    #[account(
        init,
        payer = issuer,
        space = 8 + 32 + 32 + 32 + 4 + credential_type.len() + 8 + 8 + 1,
        seeds = [b"credential", credential_hash.as_ref()],
        bump
    )]
    pub credential_account: Account<'info, VerifiableCredential>,
    
    #[account(mut)]
    pub issuer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct VerifyCredentialProof<'info> {
    #[account(
        seeds = [b"credential", credential_account.credential_hash.as_ref()],
        bump
    )]
    pub credential_account: Account<'info, VerifiableCredential>,
    
    #[account(
        seeds = [b"quantum_identity", credential_account.issuer.as_ref()],
        bump
    )]
    pub issuer_identity: Account<'info, QuantumIdentity>,
    
    pub verifier: Signer<'info>,
}

#[derive(Accounts)]
pub struct RevokeCredential<'info> {
    #[account(
        mut,
        seeds = [b"credential", credential_account.credential_hash.as_ref()],
        bump
    )]
    pub credential_account: Account<'info, VerifiableCredential>,
    
    #[account(
        seeds = [b"quantum_identity", issuer.key().as_ref()],
        bump
    )]
    pub issuer_identity: Account<'info, QuantumIdentity>,
    
    #[account(mut)]
    pub issuer: Signer<'info>,
}

// Events
#[event]
pub struct IdentityRegistered {
    pub solana_address: Pubkey,
    pub did: String,
    pub quantum_public_key: Vec<u8>,
}

#[event]
pub struct IdentityUpdated {
    pub solana_address: Pubkey,
    pub key_rotation_count: u64,
}

#[event]
pub struct CredentialIssued {
    pub credential_hash: [u8; 32],
    pub issuer: Pubkey,
    pub subject: Pubkey,
    pub credential_type: String,
}

#[event]
pub struct CredentialRevoked {
    pub credential_hash: [u8; 32],
    pub issuer: Pubkey,
}

#[event]
pub struct CredentialVerified {
    pub credential_hash: [u8; 32],
    pub verifier: Pubkey,
    pub is_valid: bool,
}

// Error codes
#[error_code]
pub enum ErrorCode {
    #[msg("DID already registered")]
    DIDAlreadyRegistered,
    #[msg("DID not registered")]
    DIDNotRegistered,
    #[msg("Invalid quantum signature")]
    InvalidQuantumSignature,
    #[msg("Unauthorized operation")]
    UnauthorizedOperation,
    #[msg("Credential expired")]
    CredentialExpired,
    #[msg("Credential revoked")]
    CredentialRevoked,
    #[msg("Empty signature")]
    EmptySignature,
    #[msg("Empty public key")]
    EmptyPublicKey,
    #[msg("Empty message")]
    EmptyMessage,
    #[msg("On-chain SPHINCS+ verification is not supported; verify off-chain")]
    OnChainVerificationUnsupported,
} 