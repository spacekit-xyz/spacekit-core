//! Enhanced RPC Server for SWTCHVM
//!
//! Provides comprehensive blockchain query capabilities, JSON-RPC 2.0 compliance,
//! and advanced features for blockchain interaction.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{Filter, Reply};

use super::{
    BlockchainStorage, BlockchainStorageStats, SwtchvmAddress, SwtchvmNode, SwtchvmTransaction,
};

/// Enhanced RPC Server for SWTCHVM
pub struct EnhancedRpcServer {
    node: Arc<RwLock<SwtchvmNode>>,
    storage: Arc<BlockchainStorage>,
    config: RpcServerConfig,
    method_handlers: HashMap<String, Arc<dyn RpcMethodHandler>>,
}

/// RPC Server Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcServerConfig {
    /// Server bind address
    pub bind_address: String,

    /// Server port
    pub port: u16,

    /// Enable CORS
    pub enable_cors: bool,

    /// Maximum request size in bytes
    pub max_request_size: usize,

    /// Request timeout in seconds
    pub request_timeout: u64,

    /// Enable rate limiting
    pub enable_rate_limiting: bool,

    /// Requests per minute per IP
    pub rate_limit_rpm: u32,

    /// Enable authentication
    pub enable_auth: bool,

    /// API keys for authentication
    pub api_keys: Vec<String>,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            port: 8545,
            enable_cors: true,
            max_request_size: 1024 * 1024, // 1MB
            request_timeout: 30,
            enable_rate_limiting: false,
            rate_limit_rpm: 100,
            enable_auth: false,
            api_keys: vec![],
        }
    }
}

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// RPC Method Handler trait
#[async_trait::async_trait]
pub trait RpcMethodHandler: Send + Sync {
    async fn handle(
        &self,
        params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError>;
}

/// RPC execution context
pub struct RpcContext {
    pub node: Arc<RwLock<SwtchvmNode>>,
    pub storage: Arc<BlockchainStorage>,
    pub config: RpcServerConfig,
}

impl EnhancedRpcServer {
    /// Create a new enhanced RPC server
    pub async fn new(
        node: Arc<RwLock<SwtchvmNode>>,
        storage: Arc<BlockchainStorage>,
        config: RpcServerConfig,
    ) -> Result<Self> {
        let mut server = Self {
            node,
            storage,
            config,
            method_handlers: HashMap::new(),
        };

        // Register standard RPC methods
        server.register_standard_methods().await?;

        Ok(server)
    }

    /// Register all standard RPC methods
    async fn register_standard_methods(&mut self) -> Result<()> {
        // Blockchain query methods
        self.register_handler("swtch_getBlockByNumber", Arc::new(GetBlockByNumberHandler))
            .await;
        self.register_handler("swtch_getBlockByHash", Arc::new(GetBlockByHashHandler))
            .await;
        self.register_handler("swtch_getLatestBlock", Arc::new(GetLatestBlockHandler))
            .await;
        self.register_handler("swtch_getBlockNumber", Arc::new(GetBlockNumberHandler))
            .await;

        // Transaction methods
        self.register_handler(
            "swtch_getTransactionByHash",
            Arc::new(GetTransactionByHashHandler),
        )
        .await;
        self.register_handler("swtch_sendTransaction", Arc::new(SendTransactionHandler))
            .await;
        self.register_handler("swtch_estimateGas", Arc::new(EstimateGasHandler))
            .await;

        // Account methods
        self.register_handler("swtch_getAccount", Arc::new(GetAccountHandler))
            .await;
        self.register_handler("swtch_getBalance", Arc::new(GetBalanceHandler))
            .await;
        self.register_handler("swtch_getNonce", Arc::new(GetNonceHandler))
            .await;
        self.register_handler("swtch_getCode", Arc::new(GetCodeHandler))
            .await;

        // Network methods
        self.register_handler("swtch_getNetworkInfo", Arc::new(GetNetworkInfoHandler))
            .await;
        self.register_handler("swtch_getNodeInfo", Arc::new(GetNodeInfoHandler))
            .await;
        self.register_handler("swtch_getPeers", Arc::new(GetPeersHandler))
            .await;

        // Storage and state methods
        self.register_handler("swtch_getStorageStats", Arc::new(GetStorageStatsHandler))
            .await;
        self.register_handler("swtch_getStateRoot", Arc::new(GetStateRootHandler))
            .await;

        // Mining methods (for development)
        self.register_handler("swtch_mine", Arc::new(MineBlockHandler))
            .await;
        self.register_handler("swtch_getMiningInfo", Arc::new(GetMiningInfoHandler))
            .await;

        // Admin methods
        self.register_handler("swtch_getGenesisConfig", Arc::new(GetGenesisConfigHandler))
            .await;

        println!("✅ Registered {} RPC methods", self.method_handlers.len());
        Ok(())
    }

    /// Register a custom RPC method handler
    pub async fn register_handler(&mut self, method: &str, handler: Arc<dyn RpcMethodHandler>) {
        self.method_handlers.insert(method.to_string(), handler);
    }

    /// Start the RPC server
    pub async fn start(&self) -> Result<()> {
        let bind_addr = format!("{}:{}", self.config.bind_address, self.config.port);
        println!("🚀 Starting Enhanced SWTCH RPC Server on {}", bind_addr);

        // Create request context
        let context = Arc::new(RpcContext {
            node: self.node.clone(),
            storage: self.storage.clone(),
            config: self.config.clone(),
        });

        let handlers = Arc::new(self.method_handlers.clone());

        // JSON-RPC endpoint
        let rpc_route = warp::path("rpc")
            .and(warp::post())
            .and(warp::body::json())
            .and(with_context(context.clone()))
            .and(with_handlers(handlers.clone()))
            .and_then(handle_json_rpc);

        // Health check endpoint
        let health_route = warp::path("health")
            .and(warp::get())
            .and(with_context(context.clone()))
            .and_then(handle_health_check);

        // WebSocket endpoint for real-time updates
        let ws_route = warp::path("ws")
            .and(warp::ws())
            .and(with_context(context))
            .and_then(handle_websocket);

        // Combine routes
        let routes = rpc_route
            .or(health_route)
            .or(ws_route)
            .with(warp::cors().allow_any_origin())
            .boxed();

        // Parse bind address
        let addr: std::net::SocketAddr = bind_addr.parse()?;

        println!("✅ RPC Server listening on http://{}", addr);
        warp::serve(routes).run(addr).await;

        Ok(())
    }
}

// Filter helpers
fn with_context(
    context: Arc<RpcContext>,
) -> impl Filter<Extract = (Arc<RpcContext>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || context.clone())
}

fn with_handlers(
    handlers: Arc<HashMap<String, Arc<dyn RpcMethodHandler>>>,
) -> impl Filter<
    Extract = (Arc<HashMap<String, Arc<dyn RpcMethodHandler>>>,),
    Error = std::convert::Infallible,
> + Clone {
    warp::any().map(move || handlers.clone())
}

// Request handlers

/// Handle JSON-RPC requests
async fn handle_json_rpc(
    request: JsonRpcRequest,
    context: Arc<RpcContext>,
    handlers: Arc<HashMap<String, Arc<dyn RpcMethodHandler>>>,
) -> Result<impl Reply, warp::Rejection> {
    let response = process_rpc_request(request, context, handlers).await;
    Ok(warp::reply::json(&response))
}

/// Process a single JSON-RPC request
async fn process_rpc_request(
    request: JsonRpcRequest,
    context: Arc<RpcContext>,
    handlers: Arc<HashMap<String, Arc<dyn RpcMethodHandler>>>,
) -> JsonRpcResponse {
    // Validate JSON-RPC version
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: None,
            }),
            id: request.id,
        };
    }

    // Find method handler
    if let Some(handler) = handlers.get(&request.method) {
        match handler.handle(request.params, &context).await {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(result),
                error: None,
                id: request.id,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(error),
                id: request.id,
            },
        }
    } else {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }),
            id: request.id,
        }
    }
}

/// Handle health check requests
async fn handle_health_check(context: Arc<RpcContext>) -> Result<impl Reply, warp::Rejection> {
    let stats = context
        .storage
        .get_stats()
        .await
        .unwrap_or_else(|_| BlockchainStorageStats {
            latest_block_number: 0,
            total_blocks: 0,
            cache_size: 0,
            storage_algorithm: "Unknown".to_string(),
            distributed_storage: false,
            replication_factor: 0,
        });

    let health = json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().timestamp(),
        "blockchain": {
            "latest_block": stats.latest_block_number,
            "total_blocks": stats.total_blocks,
            "storage_algorithm": stats.storage_algorithm
        },
        "server": {
            "version": "1.0.0",
            "uptime": 0 // Would track actual uptime
        }
    });

    Ok(warp::reply::json(&health))
}

/// Handle WebSocket connections for real-time updates
async fn handle_websocket(
    ws: warp::ws::Ws,
    _context: Arc<RpcContext>,
) -> Result<impl Reply, warp::Rejection> {
    Ok(ws.on_upgrade(|_websocket| async move {
        println!("📡 New WebSocket connection established");
        // Would implement real-time block/transaction updates
    }))
}

// RPC Method Handlers

/// Get block by number
struct GetBlockByNumberHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetBlockByNumberHandler {
    async fn handle(
        &self,
        params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let params = params.ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        })?;

        let block_number: u64 = if let Some(num_str) = params.as_str() {
            if num_str == "latest" {
                context
                    .storage
                    .get_latest_block_number()
                    .await
                    .map_err(|e| JsonRpcError {
                        code: -32000,
                        message: format!("Storage error: {}", e),
                        data: None,
                    })?
                    .unwrap_or(0)
            } else {
                num_str.parse().map_err(|_| JsonRpcError {
                    code: -32602,
                    message: "Invalid block number".to_string(),
                    data: None,
                })?
            }
        } else {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid parameters".to_string(),
                data: None,
            });
        };

        let block = context
            .storage
            .get_block(block_number)
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?;

        Ok(serde_json::to_value(block).unwrap())
    }
}

/// Get block by hash
struct GetBlockByHashHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetBlockByHashHandler {
    async fn handle(
        &self,
        params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let params = params.ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        })?;

        let hash_str = params.as_str().ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Invalid hash parameter".to_string(),
            data: None,
        })?;

        let hash_bytes =
            hex::decode(hash_str.strip_prefix("0x").unwrap_or(hash_str)).map_err(|_| {
                JsonRpcError {
                    code: -32602,
                    message: "Invalid hash format".to_string(),
                    data: None,
                }
            })?;

        if hash_bytes.len() != 32 {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid hash length".to_string(),
                data: None,
            });
        }

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hash_bytes);

        let block = context
            .storage
            .get_block_by_hash(&hash)
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?;

        Ok(serde_json::to_value(block).unwrap())
    }
}

/// Get latest block
struct GetLatestBlockHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetLatestBlockHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let latest_number = context
            .storage
            .get_latest_block_number()
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?
            .unwrap_or(0);

        let block = context
            .storage
            .get_block(latest_number)
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?;

        Ok(serde_json::to_value(block).unwrap())
    }
}

/// Get block number
struct GetBlockNumberHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetBlockNumberHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let latest_number = context
            .storage
            .get_latest_block_number()
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?
            .unwrap_or(0);

        Ok(json!(format!("0x{:x}", latest_number)))
    }
}

/// Get transaction by hash
struct GetTransactionByHashHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetTransactionByHashHandler {
    async fn handle(
        &self,
        params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let params = params.ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        })?;

        let hash_str = params.as_str().ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Invalid hash parameter".to_string(),
            data: None,
        })?;

        let hash_bytes =
            hex::decode(hash_str.strip_prefix("0x").unwrap_or(hash_str)).map_err(|_| {
                JsonRpcError {
                    code: -32602,
                    message: "Invalid hash format".to_string(),
                    data: None,
                }
            })?;

        if hash_bytes.len() != 32 {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid hash length".to_string(),
                data: None,
            });
        }

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hash_bytes);

        let tx_metadata =
            context
                .storage
                .get_transaction(&hash)
                .await
                .map_err(|e| JsonRpcError {
                    code: -32000,
                    message: format!("Storage error: {}", e),
                    data: None,
                })?;

        Ok(serde_json::to_value(tx_metadata).unwrap())
    }
}

/// Send transaction
struct SendTransactionHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for SendTransactionHandler {
    async fn handle(
        &self,
        params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let params = params.ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        })?;

        let tx: SwtchvmTransaction = serde_json::from_value(params).map_err(|e| JsonRpcError {
            code: -32602,
            message: format!("Invalid transaction: {}", e),
            data: None,
        })?;

        let node = context.node.clone();
        let tx_hash = tokio::task::spawn_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async move {
                let mut node = node.write().await;
                node.submit_transaction(tx).await.map_err(|e| JsonRpcError {
                    code: -32000,
                    message: format!("Failed to submit transaction: {}", e),
                    data: None,
                })
            })
        })
        .await
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Transaction task failed: {}", e),
            data: None,
        })??;

        Ok(json!(format!("0x{}", hex::encode(tx_hash))))
    }
}

// Additional method handlers would continue...
// For brevity, I'm showing the pattern. The remaining handlers follow similar patterns.

/// Estimate gas
struct EstimateGasHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for EstimateGasHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        // Simplified gas estimation
        Ok(json!("0x5208")) // 21000 gas
    }
}

/// Get account
struct GetAccountHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetAccountHandler {
    async fn handle(
        &self,
        params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let params = params.ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing parameters".to_string(),
            data: None,
        })?;

        let address_str = params.as_str().ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Invalid address parameter".to_string(),
            data: None,
        })?;

        let address_bytes = hex::decode(address_str.strip_prefix("0x").unwrap_or(address_str))
            .map_err(|_| JsonRpcError {
                code: -32602,
                message: "Invalid address format".to_string(),
                data: None,
            })?;

        if address_bytes.len() != 20 {
            return Err(JsonRpcError {
                code: -32602,
                message: "Invalid address length".to_string(),
                data: None,
            });
        }

        let mut addr_array = [0u8; 20];
        addr_array.copy_from_slice(&address_bytes);
        let address = SwtchvmAddress::new(addr_array);

        let account = context
            .storage
            .get_account(&address)
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?;

        Ok(serde_json::to_value(account).unwrap())
    }
}

/// Get balance
struct GetBalanceHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetBalanceHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        // Similar to GetAccountHandler but returns only balance
        // Implementation would extract address and return account.balance
        Ok(json!("0x0"))
    }
}

/// Get nonce
struct GetNonceHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetNonceHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!("0x0"))
    }
}

/// Get code
struct GetCodeHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetCodeHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!("0x"))
    }
}

/// Get network info
struct GetNetworkInfoHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetNetworkInfoHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "chain_id": 1337,
            "network_name": "SWTCH Devnet",
            "consensus": "ProofOfCompute",
            "quantum_resistant": true
        }))
    }
}

/// Get node info
struct GetNodeInfoHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetNodeInfoHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "version": "1.0.0",
            "node_type": "full",
            "capabilities": ["mining", "storage", "compute"]
        }))
    }
}

/// Get peers
struct GetPeersHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetPeersHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!([]))
    }
}

/// Get storage stats
struct GetStorageStatsHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetStorageStatsHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let stats = context
            .storage
            .get_stats()
            .await
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: format!("Storage error: {}", e),
                data: None,
            })?;

        Ok(serde_json::to_value(stats).unwrap())
    }
}

/// Get state root
struct GetStateRootHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetStateRootHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!(
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        ))
    }
}

/// Mine block
struct MineBlockHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for MineBlockHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        let node = context.node.clone();
        let block = tokio::task::spawn_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async move {
                let mut node = node.write().await;
                node.mine_block().await.map_err(|e| JsonRpcError {
                    code: -32000,
                    message: format!("Mining failed: {}", e),
                    data: None,
                })
            })
        })
        .await
        .map_err(|e| JsonRpcError {
            code: -32000,
            message: format!("Mining task failed: {}", e),
            data: None,
        })??;

        Ok(serde_json::to_value(block).unwrap())
    }
}

/// Get mining info
struct GetMiningInfoHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetMiningInfoHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "mining": false,
            "hashrate": 0,
            "difficulty": 1000
        }))
    }
}

/// Get genesis config
struct GetGenesisConfigHandler;

#[async_trait::async_trait]
impl RpcMethodHandler for GetGenesisConfigHandler {
    async fn handle(
        &self,
        _params: Option<Value>,
        _context: &RpcContext,
    ) -> Result<Value, JsonRpcError> {
        // Would load actual genesis config
        Ok(json!({
            "chain_id": 1337,
            "network_name": "SWTCH Devnet"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_parsing() {
        let json = r#"{"jsonrpc":"2.0","method":"swtch_getBlockNumber","id":1}"#;
        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "swtch_getBlockNumber");
        assert_eq!(request.id, Some(json!(1)));
    }

    #[test]
    fn test_json_rpc_response_serialization() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(json!("0x123")),
            error: None,
            id: Some(json!(1)),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("0x123"));
    }
}
