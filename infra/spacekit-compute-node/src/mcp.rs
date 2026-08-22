//! In-process MCP server for the compute node.
//!
//! Replicates the storage-node MCP pattern: JSON-RPC 2.0 over stdio with a
//! versioned tool catalog.  The server wraps an `Arc<tokio::sync::RwLock<SwtchvmNode>>`
//! so that mutable operations (submit_transaction, mine_block) can be served
//! concurrently with read operations.
//!
//! Idempotency keys use the same BLAKE3 scheme as storage-node:
//!
//! ```text
//! BLAKE3("mcp:" || tool_name || ":" || canonical_json(args))[..16]  → hex
//! ```

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::spacekitvm::swtchvm_node::{
    SwtchvmAddress, SwtchvmContext, SwtchvmExecutionResult, SwtchvmNode, SwtchvmTransaction,
    TransactionSignature,
};

// ── Wire types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub version: u32,
}

#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: &'static str,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ── Tool catalog ────────────────────────────────────────────────────────

pub fn tool_catalog() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "account_get.v1".into(),
            description: "Load account state (balance, nonce, code hash, storage size).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["address"],
                "properties": {
                    "address": {"type": "string", "description": "Hex address (0x…) or DID"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "account_fund.v1".into(),
            description: "Credit an address from a DID (faucet). Returns new balance.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["did", "address"],
                "properties": {
                    "did": {"type": "string"},
                    "address": {"type": "string"},
                    "amount": {"type": "string", "description": "Optional amount override (decimal)"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "contract_deploy.v1".into(),
            description: "Deploy a WASM contract. Returns created address.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["deployer", "wasm_hex"],
                "properties": {
                    "deployer": {"type": "string", "description": "Deployer hex address"},
                    "wasm_hex": {"type": "string", "description": "Contract WASM bytes (hex-encoded)"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "contract_call.v1".into(),
            description: "Call a deployed contract function. Returns execution result.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["caller", "contract", "data_hex"],
                "properties": {
                    "caller": {"type": "string"},
                    "contract": {"type": "string"},
                    "data_hex": {"type": "string", "description": "Call data (hex-encoded)"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "wasm_execute.v1".into(),
            description: "Execute raw WASM without transaction semantics. Returns output.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["wasm_hex"],
                "properties": {
                    "wasm_hex": {"type": "string"},
                    "input_hex": {"type": "string", "default": ""}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "tx_submit.v1".into(),
            description: "Submit a signed transaction to the pending pool.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["from", "data_hex", "gas_limit", "gas_price"],
                "properties": {
                    "from": {"type": "string"},
                    "to": {"type": "string", "description": "Null for contract creation"},
                    "data_hex": {"type": "string"},
                    "gas_limit": {"type": "string"},
                    "gas_price": {"type": "string"},
                    "value": {"type": "string", "default": "0"},
                    "nonce": {"type": "integer"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "block_mine.v1".into(),
            description: "Mine pending transactions into a new block. Returns the block.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "block_get.v1".into(),
            description: "Get a block by number.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["number"],
                "properties": {
                    "number": {"type": "integer"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "block_latest.v1".into(),
            description: "Get the latest block.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "receipt_get.v1".into(),
            description: "Get a transaction receipt by tx hash (hex).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["tx_hash"],
                "properties": {
                    "tx_hash": {"type": "string", "description": "64-char hex hash"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "l1_manifest_get.v1".into(),
            description: "Read the L1 checkpoint manifest (if available).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            version: 1,
        },
    ]
}

// ── Server ──────────────────────────────────────────────────────────────

pub struct McpServer {
    pub node: Arc<RwLock<SwtchvmNode>>,
}

impl McpServer {
    pub fn new(node: Arc<RwLock<SwtchvmNode>>) -> Self {
        Self { node }
    }

    pub async fn handle(&self, req: McpRequest) -> McpResponse {
        let result = match req.method.as_str() {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "spacekit-compute-node", "version": env!("CARGO_PKG_VERSION")},
            })),
            "tools/list" => Ok(serde_json::json!({"tools": tool_catalog()})),
            "tools/call" => {
                self.dispatch_tool(req.params.unwrap_or(serde_json::Value::Null))
                    .await
            }
            "ping" => Ok(serde_json::json!({})),
            _ => Err(McpError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
                data: None,
            }),
        };
        match result {
            Ok(value) => McpResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: Some(value),
                error: None,
            },
            Err(e) => McpResponse {
                jsonrpc: "2.0",
                id: req.id,
                result: None,
                error: Some(e),
            },
        }
    }

    async fn dispatch_tool(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing tool name"))?
            .to_string();
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));

        let _idempotency_key = args
            .get("idempotency_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| derive_idempotency_key(&name, &args));

        let outcome = match name.as_str() {
            "account_get.v1" => self.account_get(args).await,
            "account_fund.v1" => self.account_fund(args).await,
            "contract_deploy.v1" => self.contract_deploy(args).await,
            "contract_call.v1" => self.contract_call(args).await,
            "wasm_execute.v1" => self.wasm_execute(args).await,
            "tx_submit.v1" => self.tx_submit(args).await,
            "block_mine.v1" => self.block_mine(args).await,
            "block_get.v1" => self.block_get(args).await,
            "block_latest.v1" => self.block_latest(args).await,
            "receipt_get.v1" => self.receipt_get(args).await,
            "l1_manifest_get.v1" => self.l1_manifest_get(args).await,
            other => {
                return Err(McpError {
                    code: -32601,
                    message: format!("Unknown tool: {}", other),
                    data: None,
                })
            }
        };
        outcome.map(|v| serde_json::json!({
            "content": [{"type": "text", "text": serde_json::to_string(&v).unwrap_or_default()}],
            "isError": false,
            "structuredContent": v,
        }))
    }

    // ── Tool handlers ───────────────────────────────────────────────────

    async fn account_get(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let addr_str = args
            .get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing address"))?;
        let addr = parse_address(addr_str)?;
        let node = self.node.read().await;
        match node.get_account(&addr).await {
            Some(acct) => Ok(serde_json::json!({
                "address": addr.to_string(),
                "balance": acct.balance.to_string(),
                "nonce": acct.nonce,
                "has_code": acct.code.is_some(),
                "storage_entries": acct.storage.len(),
            })),
            None => Ok(serde_json::json!({"address": addr.to_string(), "found": false})),
        }
    }

    async fn account_fund(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let did = args
            .get("did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing did"))?;
        let addr_str = args
            .get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing address"))?;
        let addr = parse_address(addr_str)?;
        let amount_override = args
            .get("amount")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u128>().ok());
        let node = self.node.read().await;
        let resp = node.apply_faucet(did, addr, amount_override).await;
        Ok(serde_json::to_value(&resp).map_err(internal)?)
    }

    async fn contract_deploy(
        &self,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let deployer_str = args
            .get("deployer")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing deployer"))?;
        let deployer = parse_address(deployer_str)?;
        let wasm_hex = args
            .get("wasm_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing wasm_hex"))?;
        let wasm_code =
            hex::decode(wasm_hex).map_err(|e| invalid_params(&format!("bad wasm_hex: {}", e)))?;

        let node = self.node.read().await;
        let ctx = SwtchvmContext {
            caller: deployer,
            origin: deployer,
            gas_price: 1,
            gas_limit: 10_000_000,
            gas_used: 0,
            block_number: node.get_latest_block().number,
            block_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            value: 0,
        };
        let result: SwtchvmExecutionResult = node
            .deploy_contract(&deployer, wasm_code, ctx)
            .await
            .map_err(internal)?;

        Ok(serde_json::json!({
            "success": result.success,
            "created_address": result.created_address.map(|a: SwtchvmAddress| a.to_string()),
            "gas_used": result.gas_used.to_string(),
            "return_data_hex": hex::encode(&result.return_data),
        }))
    }

    async fn contract_call(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let caller_str = args
            .get("caller")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing caller"))?;
        let contract_str = args
            .get("contract")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing contract"))?;
        let data_hex = args
            .get("data_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing data_hex"))?;

        let caller = parse_address(caller_str)?;
        let contract = parse_address(contract_str)?;
        let call_data =
            hex::decode(data_hex).map_err(|e| invalid_params(&format!("bad data_hex: {}", e)))?;

        let node = self.node.read().await;
        let ctx = SwtchvmContext {
            caller,
            origin: caller,
            gas_price: 1,
            gas_limit: 10_000_000,
            gas_used: 0,
            block_number: node.get_latest_block().number,
            block_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            value: 0,
        };
        let result: SwtchvmExecutionResult = node
            .call_contract(&caller, &contract, &call_data, ctx)
            .await
            .map_err(internal)?;

        Ok(serde_json::json!({
            "success": result.success,
            "gas_used": result.gas_used.to_string(),
            "return_data_hex": hex::encode(&result.return_data),
            "logs_count": result.logs.len(),
        }))
    }

    async fn wasm_execute(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let wasm_hex = args
            .get("wasm_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing wasm_hex"))?;
        let input_hex = args.get("input_hex").and_then(|v| v.as_str()).unwrap_or("");
        let wasm =
            hex::decode(wasm_hex).map_err(|e| invalid_params(&format!("bad wasm_hex: {}", e)))?;
        let input =
            hex::decode(input_hex).map_err(|e| invalid_params(&format!("bad input_hex: {}", e)))?;

        let node = self.node.read().await;
        let result: SwtchvmExecutionResult = node
            .execute_wasm_direct(&wasm, &input)
            .await
            .map_err(internal)?;

        Ok(serde_json::json!({
            "success": result.success,
            "return_data_hex": hex::encode(&result.return_data),
            "gas_used": result.gas_used.to_string(),
        }))
    }

    async fn tx_submit(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let from_str = args
            .get("from")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing from"))?;
        let from = parse_address(from_str)?;
        let to = args
            .get("to")
            .and_then(|v| v.as_str())
            .map(|s| parse_address(s))
            .transpose()?;
        let data_hex = args
            .get("data_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing data_hex"))?;
        let data =
            hex::decode(data_hex).map_err(|e| invalid_params(&format!("bad data_hex: {}", e)))?;
        let gas_limit: u128 = args
            .get("gas_limit")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(10_000_000);
        let gas_price: u128 = args
            .get("gas_price")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let value: u128 = args
            .get("value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let nonce: u64 = args.get("nonce").and_then(|v| v.as_u64()).unwrap_or(0);

        let tx = SwtchvmTransaction {
            from,
            to,
            data,
            gas_limit,
            gas_price,
            value,
            nonce,
            signature: TransactionSignature {
                v: 0,
                r: [0u8; 32],
                s: [0u8; 32],
            },
        };

        let mut node = self.node.write().await;
        let hash = node.submit_transaction(tx).await.map_err(internal)?;
        Ok(serde_json::json!({"tx_hash": hex::encode(hash)}))
    }

    async fn block_mine(&self, _args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let mut node = self.node.write().await;
        let block = node.mine_block().await.map_err(internal)?;
        Ok(serde_json::json!({
            "number": block.number,
            "hash": hex::encode(block.hash),
            "tx_count": block.transactions.len(),
            "gas_used": block.gas_used.to_string(),
            "timestamp": block.timestamp,
        }))
    }

    async fn block_get(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let number = args
            .get("number")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| invalid_params("missing number"))?;
        let node = self.node.read().await;
        match node.get_block_by_number(number) {
            Some(block) => Ok(serde_json::json!({
                "number": block.number,
                "hash": hex::encode(block.hash),
                "parent_hash": hex::encode(block.parent_hash),
                "tx_count": block.transactions.len(),
                "gas_used": block.gas_used.to_string(),
                "gas_limit": block.gas_limit.to_string(),
                "timestamp": block.timestamp,
            })),
            None => Ok(serde_json::json!({"found": false, "number": number})),
        }
    }

    async fn block_latest(&self, _args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let node = self.node.read().await;
        let block = node.get_latest_block();
        Ok(serde_json::json!({
            "number": block.number,
            "hash": hex::encode(block.hash),
            "tx_count": block.transactions.len(),
            "gas_used": block.gas_used.to_string(),
            "timestamp": block.timestamp,
        }))
    }

    async fn receipt_get(&self, args: serde_json::Value) -> Result<serde_json::Value, McpError> {
        let tx_hash_str = args
            .get("tx_hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing tx_hash"))?;
        let hash_bytes = hex::decode(tx_hash_str.trim_start_matches("0x"))
            .map_err(|e| invalid_params(&format!("bad tx_hash: {}", e)))?;
        if hash_bytes.len() != 32 {
            return Err(invalid_params("tx_hash must be 32 bytes"));
        }
        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(&hash_bytes);

        let node = self.node.read().await;
        match node.get_receipt(&hash_arr) {
            Some(receipt) => Ok(serde_json::to_value(receipt).map_err(internal)?),
            None => Ok(serde_json::json!({"found": false, "tx_hash": tx_hash_str})),
        }
    }

    async fn l1_manifest_get(
        &self,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let node = self.node.read().await;
        match node.read_l1_manifest() {
            Ok(Some(manifest)) => Ok(serde_json::to_value(&manifest).map_err(internal)?),
            Ok(None) => Ok(serde_json::json!({"available": false})),
            Err(e) => Err(internal(e)),
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn parse_address(s: &str) -> Result<SwtchvmAddress, McpError> {
    SwtchvmAddress::from_hex(s)
        .map_err(|e| invalid_params(&format!("bad address \"{}\": {}", s, e)))
}

fn invalid_params(message: &str) -> McpError {
    McpError {
        code: -32602,
        message: message.to_string(),
        data: None,
    }
}

fn internal<E: std::fmt::Display>(e: E) -> McpError {
    McpError {
        code: -32603,
        message: e.to_string(),
        data: None,
    }
}

// ── Idempotency ─────────────────────────────────────────────────────────

fn derive_idempotency_key(tool_name: &str, args: &serde_json::Value) -> String {
    let canonical = canonical_json(args);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mcp:");
    hasher.update(tool_name.as_bytes());
    hasher.update(b":");
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest.as_bytes()[..16])
}

fn canonical_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    canonical_json_rec(value, &mut out);
    out
}

fn canonical_json_rec(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::String(s) => {
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c < '\x20' => {
                        out.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        serde_json::Value::Array(arr) => {
            out.push('[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical_json_rec(item, out);
            }
            out.push(']');
        }
        serde_json::Value::Object(obj) => {
            out.push('{');
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical_json_rec(&serde_json::Value::String(k.to_string()), out);
                out.push(':');
                canonical_json_rec(&obj[*k], out);
            }
            out.push('}');
        }
    }
}

// ── Stdio transport ─────────────────────────────────────────────────────

pub async fn run_stdio(server: McpServer) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let req: McpRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = McpResponse {
                    jsonrpc: "2.0",
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(McpError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                let bytes = serde_json::to_vec(&resp)?;
                stdout.write_all(&bytes).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };
        let resp = server.handle(req).await;
        let bytes = serde_json::to_vec(&resp)?;
        stdout.write_all(&bytes).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_catalog_versions_are_set() {
        for tool in tool_catalog() {
            assert!(
                tool.name.ends_with(".v1"),
                "tool {} missing version suffix",
                tool.name
            );
            assert_eq!(tool.version, 1);
        }
    }

    #[test]
    fn tool_catalog_has_expected_count() {
        assert_eq!(tool_catalog().len(), 11);
    }

    #[test]
    fn idempotency_key_deterministic() {
        let args = serde_json::json!({"address": "0xabc", "extra": 1});
        let k1 = derive_idempotency_key("account_get.v1", &args);
        let k2 = derive_idempotency_key("account_get.v1", &args);
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 32); // 16 bytes = 32 hex chars
    }

    #[test]
    fn canonical_json_sorts_keys() {
        let args = serde_json::json!({"z": 1, "a": 2});
        let c = canonical_json(&args);
        assert_eq!(c, r#"{"a":2,"z":1}"#);
    }
}
