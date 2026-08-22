//! Storage Primitives for SWTCH Network
//!
//! This module provides storage contract primitives for WCVM integration,
//! including quantum-safe storage operations and DID-based access control.

pub mod did_operations;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Storage Contract ABI Definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageContractABI {
    pub name: String,
    pub version: String,
    pub functions: Vec<StorageFunction>,
    pub events: Vec<StorageEvent>,
}

/// Storage Contract Function Definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageFunction {
    pub name: String,
    pub inputs: Vec<FunctionInput>,
    pub outputs: Vec<FunctionOutput>,
    pub mutability: StateMutability,
}

/// Storage Contract Event Definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageEvent {
    pub name: String,
    pub inputs: Vec<EventInput>,
    pub anonymous: bool,
}

/// Function Input Parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInput {
    pub name: String,
    pub type_name: String,
    pub indexed: bool,
}

/// Function Output Parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionOutput {
    pub name: String,
    pub type_name: String,
}

/// Event Input Parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventInput {
    pub name: String,
    pub type_name: String,
    pub indexed: bool,
}

/// State Mutability for Functions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateMutability {
    Pure,
    View,
    NonPayable,
    Payable,
}

/// Storage Contract Operation Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageOperationResult {
    pub success: bool,
    pub file_id: Option<String>,
    pub error: Option<String>,
    pub gas_used: u64,
    pub storage_cost: u64,
}

/// DID-based Storage Access Control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageAccessControl {
    pub did: String,
    pub permissions: StoragePermissions,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
}

/// Storage Permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePermissions {
    pub read: bool,
    pub write: bool,
    pub admin: bool,
    pub share: bool,
}

/// Storage Contract Primitives
///
/// This struct provides the core primitives for storage contract operations.
pub struct StorageContractPrimitives {
    pub contract_abi: StorageContractABI,
    pub access_control: HashMap<String, Vec<StorageAccessControl>>,
}

impl StorageContractPrimitives {
    /// Create new storage contract primitives
    pub fn new() -> Self {
        Self {
            contract_abi: Self::default_storage_abi(),
            access_control: HashMap::new(),
        }
    }

    /// Get default storage contract ABI
    pub fn default_storage_abi() -> StorageContractABI {
        StorageContractABI {
            name: "StorageContract".to_string(),
            version: "1.0.0".to_string(),
            functions: vec![
                StorageFunction {
                    name: "store_file".to_string(),
                    inputs: vec![
                        FunctionInput {
                            name: "owner_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "file_data".to_string(),
                            type_name: "bytes".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "encryption_algorithm".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                    ],
                    outputs: vec![
                        FunctionOutput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                        },
                        FunctionOutput {
                            name: "storage_cost".to_string(),
                            type_name: "uint64".to_string(),
                        },
                    ],
                    mutability: StateMutability::NonPayable,
                },
                StorageFunction {
                    name: "retrieve_file".to_string(),
                    inputs: vec![
                        FunctionInput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "requester_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                    ],
                    outputs: vec![FunctionOutput {
                        name: "file_data".to_string(),
                        type_name: "bytes".to_string(),
                    }],
                    mutability: StateMutability::View,
                },
                StorageFunction {
                    name: "grant_access".to_string(),
                    inputs: vec![
                        FunctionInput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "granter_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "grantee_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "permissions".to_string(),
                            type_name: "StoragePermissions".to_string(),
                            indexed: false,
                        },
                    ],
                    outputs: vec![FunctionOutput {
                        name: "success".to_string(),
                        type_name: "bool".to_string(),
                    }],
                    mutability: StateMutability::NonPayable,
                },
                StorageFunction {
                    name: "revoke_access".to_string(),
                    inputs: vec![
                        FunctionInput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "revoker_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                        FunctionInput {
                            name: "target_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: false,
                        },
                    ],
                    outputs: vec![FunctionOutput {
                        name: "success".to_string(),
                        type_name: "bool".to_string(),
                    }],
                    mutability: StateMutability::NonPayable,
                },
            ],
            events: vec![
                StorageEvent {
                    name: "FileStored".to_string(),
                    inputs: vec![
                        EventInput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                        EventInput {
                            name: "owner_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                        EventInput {
                            name: "size".to_string(),
                            type_name: "uint64".to_string(),
                            indexed: false,
                        },
                    ],
                    anonymous: false,
                },
                StorageEvent {
                    name: "AccessGranted".to_string(),
                    inputs: vec![
                        EventInput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                        EventInput {
                            name: "granter_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                        EventInput {
                            name: "grantee_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                    ],
                    anonymous: false,
                },
                StorageEvent {
                    name: "AccessRevoked".to_string(),
                    inputs: vec![
                        EventInput {
                            name: "file_id".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                        EventInput {
                            name: "revoker_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                        EventInput {
                            name: "target_did".to_string(),
                            type_name: "string".to_string(),
                            indexed: true,
                        },
                    ],
                    anonymous: false,
                },
            ],
        }
    }

    /// Grant access to a file
    pub fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: StoragePermissions,
    ) -> Result<bool> {
        let access_control = StorageAccessControl {
            did: grantee_did.to_string(),
            permissions,
            granted_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            expires_at: None,
        };

        self.access_control
            .entry(file_id.to_string())
            .or_insert_with(Vec::new)
            .push(access_control);

        Ok(true)
    }

    /// Check if DID has access to file
    pub fn has_access(&self, file_id: &str, did: &str) -> bool {
        if let Some(access_list) = self.access_control.get(file_id) {
            access_list.iter().any(|access| {
                access.did == did
                    && (access.expires_at.is_none()
                        || access.expires_at.unwrap()
                            > std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs())
            })
        } else {
            false
        }
    }

    /// Revoke access to a file
    pub fn revoke_access(
        &mut self,
        file_id: &str,
        _revoker_did: &str,
        target_did: &str,
    ) -> Result<bool> {
        if let Some(access_list) = self.access_control.get_mut(file_id) {
            access_list.retain(|access| access.did != target_did);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl Default for StorageContractPrimitives {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for StoragePermissions {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            admin: false,
            share: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_contract_abi_creation() {
        let abi = StorageContractPrimitives::default_storage_abi();
        assert_eq!(abi.name, "StorageContract");
        assert_eq!(abi.version, "1.0.0");
        assert!(!abi.functions.is_empty());
        assert!(!abi.events.is_empty());
    }

    #[test]
    fn test_access_control() {
        let mut primitives = StorageContractPrimitives::new();
        let file_id = "test_file";
        let granter_did = "did:swtch:granter";
        let grantee_did = "did:swtch:grantee";

        // Initially no access
        assert!(!primitives.has_access(file_id, grantee_did));

        // Grant access
        let permissions = StoragePermissions::default();
        let result = primitives.grant_access(file_id, granter_did, grantee_did, permissions);
        assert!(result.is_ok());

        // Now has access
        assert!(primitives.has_access(file_id, grantee_did));

        // Revoke access
        let result = primitives.revoke_access(file_id, granter_did, grantee_did);
        assert!(result.is_ok());

        // No longer has access
        assert!(!primitives.has_access(file_id, grantee_did));
    }

    #[test]
    fn test_storage_primitives_creation() {
        let primitives = StorageContractPrimitives::new();
        assert_eq!(primitives.contract_abi.name, "StorageContract");
        assert!(primitives.access_control.is_empty());
    }
}
