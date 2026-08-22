use alloy_primitives::Address;
use chrono::{DateTime, Utc};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Network {
    pub id: i32,
    pub name: String,
    pub network_type: String,
    pub chain_id: Option<String>,
    pub rpc_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Network {
    pub fn new(
        name: String,
        network_type: String,
        chain_id: Option<String>,
        rpc_url: Option<String>,
    ) -> Self {
        Network {
            id: 0,
            name,
            network_type,
            chain_id,
            rpc_url,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct NetworkService {
    pub address: Address,
    pub service_details: String,
    pub is_active: bool,
}

impl NetworkService {
    pub fn new(address: Address, service_details: String, is_active: bool) -> Self {
        NetworkService {
            address,
            service_details,
            is_active,
        }
    }
}
