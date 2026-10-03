//! WASM imports wired by SpacekitVM-JS (`host.ts`): tools, messaging, remote storage, payments,
//! session keys, and paymaster (ERC-4337–inspired account abstraction).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::ContractError;

fn map_buffered_send(code: i32) -> Result<(), ContractError> {
    match code {
        1 => Ok(()),
        -1 => Err(ContractError::ToolNotConfigured),
        _ => Err(ContractError::HostError),
    }
}

fn map_effect_buffer(code: i32, mut buf: Vec<u8>) -> Result<Vec<u8>, ContractError> {
    match code {
        n if n > 0 => {
            buf.truncate(n as usize);
            Ok(buf)
        }
        -3 => Err(ContractError::NeedsTools),
        -1 => Err(ContractError::ToolNotConfigured),
        _ => Err(ContractError::HostError),
    }
}

pub mod tools {
    use super::*;

    #[link(wasm_import_module = "spacekit_tools")]
    extern "C" {
        #[link_name = "web_search"]
        fn sym_web_search(
            query_ptr: *const u8,
            query_len: usize,
            max_results: i32,
            dest_ptr: *mut u8,
            max_len: usize,
        ) -> i32;
    }

    /// Returns UTF-8 JSON (`SearchResult[]` from JS docs). Routed via Messaging Node `tool-request`.
    pub fn web_search(query: &str, max_results: u32, max_response_bytes: usize) -> Result<String, ContractError> {
        let mut buf = vec![0u8; max_response_bytes];
        let n = unsafe {
            sym_web_search(
                query.as_ptr(),
                query.len(),
                max_results as i32,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        let out = map_effect_buffer(n, buf)?;
        String::from_utf8(out).map_err(|_| ContractError::InvalidInput)
    }
}

pub mod messaging {
    use super::*;

    #[link(wasm_import_module = "spacekit_messaging")]
    extern "C" {
        #[link_name = "messaging_send"]
        fn sym_messaging_send(
            recipient_ptr: *const u8,
            recipient_len: usize,
            payload_ptr: *const u8,
            payload_len: usize,
        ) -> i32;
    }

    pub fn messaging_send(recipient_did: &str, payload: &[u8]) -> Result<(), ContractError> {
        let code = unsafe {
            sym_messaging_send(
                recipient_did.as_ptr(),
                recipient_did.len(),
                payload.as_ptr(),
                payload.len(),
            )
        };
        map_buffered_send(code)
    }
}

pub mod remote_storage {
    use super::*;

    #[link(wasm_import_module = "spacekit_remote_storage")]
    extern "C" {
        #[link_name = "remote_storage_put"]
        fn sym_remote_storage_put(
            data_ptr: *const u8,
            data_len: usize,
            ref_dest: *mut u8,
            ref_max: usize,
        ) -> i32;
        #[link_name = "remote_storage_get"]
        fn sym_remote_storage_get(
            ref_ptr: *const u8,
            ref_len: usize,
            dest_ptr: *mut u8,
            max_len: usize,
        ) -> i32;
    }

    pub fn remote_storage_put(data: &[u8], ref_buf_max: usize) -> Result<String, ContractError> {
        let mut buf = vec![0u8; ref_buf_max];
        let n = unsafe {
            sym_remote_storage_put(data.as_ptr(), data.len(), buf.as_mut_ptr(), buf.len())
        };
        let out = map_effect_buffer(n, buf)?;
        String::from_utf8(out).map_err(|_| ContractError::InvalidInput)
    }

    pub fn remote_storage_get(reference: &str, out_max: usize) -> Result<Vec<u8>, ContractError> {
        let mut buf = vec![0u8; out_max];
        let n = unsafe {
            sym_remote_storage_get(
                reference.as_ptr(),
                reference.len(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        map_effect_buffer(n, buf)
    }
}

pub mod payments {
    use super::*;

    #[link(wasm_import_module = "spacekit_payments")]
    extern "C" {
        #[link_name = "payment_transfer"]
        fn sym_payment_transfer(
            to_ptr: *const u8,
            to_len: usize,
            asset_ptr: *const u8,
            asset_len: usize,
            amount: i64,
        ) -> i32;
    }

    /// Pay `amount` wei of ASTRA from this contract's own balance to `to`
    /// (`0x…` address or `did:spacekit:<address>`). `asset` must be
    /// `"ASTRA"`: SpaceKit settles only in ASTRA, and any other asset is
    /// refused. The payment happens immediately and is undone if the
    /// transaction fails. For amounts above `i64::MAX` use `transfer_u128`.
    pub fn payment_transfer(to: &str, asset: &str, amount: i64) -> Result<(), ContractError> {
        let code = unsafe {
            sym_payment_transfer(
                to.as_ptr(),
                to.len(),
                asset.as_ptr(),
                asset.len(),
                amount,
            )
        };
        map_buffered_send(code)
    }

}

// ═══════════════════════════════════════════════════════════════════════════════
// ERC-4337–inspired account abstraction: session keys + paymaster
// ═══════════════════════════════════════════════════════════════════════════════

/// Session-key management for delegated agent execution (ERC-4337 concept).
///
/// A session key grants a **delegate DID** scoped, time-limited authority to act
/// on behalf of an **owner DID**.  Agents use session keys to execute
/// transfers and contract calls without requiring the owner to sign each
/// operation individually.
///
/// The host VM validates sessions; contracts only see the safe wrappers here.
pub mod session_keys {
    use super::*;

    #[link(wasm_import_module = "spacekit_session")]
    extern "C" {
        /// Create a session granting `delegate` scoped authority on behalf of the
        /// caller DID.  Returns >0 (session-id byte length written to dest) on
        /// success, or a negative error code.
        #[link_name = "session_create"]
        fn sym_session_create(
            delegate_ptr: *const u8,
            delegate_len: usize,
            scope_ptr: *const u8,
            scope_len: usize,
            expires_at: i64,
            dest_ptr: *mut u8,
            dest_max: usize,
        ) -> i32;

        /// Validate whether the caller holds a valid session for `operation`
        /// under `owner`.  Returns 1 = valid, 0 = invalid/expired, <0 = error.
        #[link_name = "session_validate"]
        fn sym_session_validate(
            owner_ptr: *const u8,
            owner_len: usize,
            operation_ptr: *const u8,
            operation_len: usize,
        ) -> i32;

        /// Revoke a session by its 32-byte id.  Only the session owner can
        /// revoke.  Returns 1 = ok, <0 = error.
        #[link_name = "session_revoke"]
        fn sym_session_revoke(
            session_id_ptr: *const u8,
            session_id_len: usize,
        ) -> i32;
    }

    /// Permission scope constants — use as the `scope` argument to
    /// [`session_create`].  Multiple scopes can be combined by
    /// concatenating with `|` (e.g. `"transfer|contract_call"`).
    pub mod scope {
        pub const TRANSFER: &str      = "transfer";
        pub const CONTRACT_CALL: &str = "contract_call";
        pub const MESSAGING: &str     = "messaging";
        pub const STORAGE: &str       = "storage";
        pub const ALL: &str           = "*";
    }

    /// Create a session key granting `delegate_did` scoped authority until
    /// `expires_at` (unix seconds).  Returns the session id as a hex string.
    pub fn session_create(
        delegate_did: &str,
        scope: &str,
        expires_at: u64,
        id_buf_max: usize,
    ) -> Result<String, ContractError> {
        let mut buf = vec![0u8; id_buf_max];
        let n = unsafe {
            sym_session_create(
                delegate_did.as_ptr(),
                delegate_did.len(),
                scope.as_ptr(),
                scope.len(),
                expires_at as i64,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        let out = map_effect_buffer(n, buf)?;
        String::from_utf8(out).map_err(|_| ContractError::InvalidInput)
    }

    /// Check whether the current caller holds a valid session for `operation`
    /// under `owner_did`.
    pub fn session_validate(owner_did: &str, operation: &str) -> Result<bool, ContractError> {
        let code = unsafe {
            sym_session_validate(
                owner_did.as_ptr(),
                owner_did.len(),
                operation.as_ptr(),
                operation.len(),
            )
        };
        match code {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err(ContractError::HostError),
        }
    }

    /// Revoke a session by its 32-byte id (hex-encoded).
    pub fn session_revoke(session_id: &str) -> Result<(), ContractError> {
        let code = unsafe {
            sym_session_revoke(session_id.as_ptr(), session_id.len())
        };
        map_buffered_send(code)
    }
}

/// Paymaster / sponsored operations (ERC-4337 concept).
///
/// Sponsorship is the `spacekit-paymaster` contract
/// (`spacekit-standard-library/payments/spacekit-paymaster`): a sponsor
/// deposits native ASTRA into it and sets a policy, and permitted callers draw
/// on that deposit. Budgets are the ASTRA the contract actually holds.
///
/// The former host-side `spacekit_paymaster` imports kept budgets outside the
/// chain and were removed. These constants describe the contract's wire format.
pub mod paymaster {
    /// Opcodes of the paymaster contract. Amounts are u128 little-endian wei;
    /// strings are u16 little-endian length-prefixed UTF-8.
    pub mod op {
        /// Payload empty; the call's value is the deposit.
        pub const DEPOSIT: u8        = 0x01;
        /// `[amount:u128]` — back to the sponsor's address.
        pub const WITHDRAW: u8       = 0x02;
        /// `[policy_json:str]`.
        pub const SET_POLICY: u8     = 0x03;
        /// `[sponsor_did:str][amount:u128][operation:str]` — pays the caller.
        pub const SPONSOR_CHARGE: u8 = 0x04;
        /// `[sponsor_did:str]` → `[1][u128]`.
        pub const GET_BUDGET: u8     = 0x05;
        /// `[sponsor_did:str]` → `[1][json]`.
        pub const GET_POLICY: u8     = 0x06;
    }
}

/// Entitlement protocol constants shared between the `astra-entitlement-ledger`
/// contract and any marketplace contracts that need to compose with it.
///
/// The canonical contract lives at
/// `spacekit-standard-library/marketplace/astra-entitlement-ledger`.
pub mod entitlement {
    /// Opcodes for the entitlement ledger contract.
    pub mod op {
        pub const CREATE_LISTING: u8   = 0x01;
        pub const PURCHASE: u8         = 0x02;
        pub const VERIFY: u8           = 0x03;
        pub const REVOKE: u8           = 0x04;
        pub const GET_LISTING: u8      = 0x05;
        pub const GET_ENTITLEMENT: u8  = 0x06;
        /// Publisher-only approve/grant (no payment).
        pub const GRANT: u8            = 0x07;
    }

    /// Pricing type bytes stored in listing records.
    pub mod pricing {
        pub const ONE_TIME: u8     = 1;
        pub const SUBSCRIPTION: u8 = 2;
    }

    /// Status byte values returned by `OP_VERIFY`.
    pub mod status {
        pub const VALID: u8       = 1;
        pub const EXPIRED: u8     = 0;
        pub const WRONG_BUYER: u8 = 2;
        pub const WRONG_FILE: u8  = 3;
        pub const REVOKED: u8     = 4;
        pub const WRONG_PK: u8    = 5;
    }

    /// Event type emitted by the entitlement contract on successful purchase.
    ///
    /// Payload layout: `[entitlement_id: 32 bytes][buyer_did_utf8][0x00][listing_id_utf8]`
    pub const EVENT_GRANTED: &str = "entitlement:granted";

    /// Event type emitted when an entitlement is revoked by the publisher.
    ///
    /// Payload: `[entitlement_id: 32 bytes]`
    pub const EVENT_REVOKED: &str = "entitlement:revoked";
}
