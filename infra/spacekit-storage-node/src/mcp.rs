//! In-process MCP server (Phase 5).
//!
//! This module exposes [`crate::storage_facade::Facade`] as a catalogue of
//! agent-callable MCP tools. The wire format follows the JSON-RPC 2.0
//! conventions MCP uses; the actual transport (stdio, SSE) is selected at
//! startup time. The dispatcher is transport-independent so the
//! `mcp_subcommand` in `bin/standalone.rs` can wire either.
//!
//! ## Tool versioning
//!
//! Tool names embed a schema version (`tx_begin.v1`, `sandbox_create.v1`,
//! `graph_traverse.v1`, …). Old versions remain in the catalogue for one
//! deprecation cycle when a v2 ships, so production agents that hard-code
//! signatures don't break on a node upgrade.
//!
//! ## Idempotency keys
//!
//! Tool calls derive a **deterministic** idempotency key from
//! `(tool_name, canonical_args_hash)`:
//!
//! ```text
//! BLAKE3("mcp:" || tool_name || ":" || canonical_json(args))[..32]
//! ```
//!
//! This is the *default* — agents can pass an explicit `idempotency_key`
//! field to override. UUIDv4 auto-generation is *never* used: an agent's
//! retry of the same logical call produces the same key, hits the cache,
//! and returns the prior response.
//!
//! ## Observability tools
//!
//! `tx_trace.v1` and `sandbox_journal.v1` surface the in-memory trace logs
//! the [`TransactionManager`] and [`SandboxManager`] keep, so an agent
//! debugging a failed commit can read what subsystem each modification
//! hit, with timing.

#![deny(clippy::all)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::sandbox::{ConflictPolicy, SandboxConfig};
use crate::storage_facade::Facade;
use crate::transaction::{IsolationLevel, TransactionModification};

/// Hard cap on BFS depth for `graph_traverse.v1` (agents must not OOM the node).
const GRAPH_TRAVERSE_MAX_DEPTH: u64 = 50;

/// Registered tool descriptor served at `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    /// Schema version this tool was published under. Agents that hard-code
    /// signatures should pin to the version they know about.
    pub version: u32,
}

/// All tools we ship in Phase 5.
pub fn tool_catalog() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "tx_begin.v1".into(),
            description: "Begin a new ACID transaction. Returns a transaction id agents include in subsequent writes.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "isolation": {
                        "type": "string",
                        "enum": ["read_committed", "repeatable_read", "serializable"],
                        "default": "serializable"
                    },
                    "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 86400}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "tx_commit.v1".into(),
            description: "Commit a transaction. Acquires the global commit lock; serializable on the write path.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["transaction_id"],
                "properties": {"transaction_id": {"type": "string"}}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "tx_rollback.v1".into(),
            description: "Roll back a transaction. Discards the modification log without applying.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["transaction_id"],
                "properties": {"transaction_id": {"type": "string"}}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "tx_trace.v1".into(),
            description: "Return the modification log + per-step timing + which subsystem each mod hit. Agents read this when a commit fails.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["transaction_id"],
                "properties": {"transaction_id": {"type": "string"}}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "tx_record_modification.v1".into(),
            description: "Append a modification to an open transaction. Optional sandbox_id mirrors the same modification into that sandbox's journal (same rules as HTTP X-Sandbox-Id); pass caller_did for ACL when using a sandbox.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["transaction_id", "modification"],
                "properties": {
                    "transaction_id": {"type": "string"},
                    "modification": {"type": "object"},
                    "conflict_policy": {
                        "type": "string",
                        "enum": ["reject", "last_writer_wins", "three_way_merge", "optimistic_if_match"],
                        "default": "reject"
                    },
                    "bytes_written": {"type": "integer", "minimum": 0, "default": 0},
                    "sandbox_id": {"type": "string"},
                    "caller_did": {"type": "string"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "sandbox_create.v1".into(),
            description: "Open an ephemeral sandbox. Returns a sandbox id; pass on subsequent writes via the X-Sandbox-Id header.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "owner_did": {"type": "string"},
                    "ttl_seconds": {"type": "integer", "minimum": 1, "maximum": 86400, "default": 3600},
                    "max_bytes_written": {"type": "integer"},
                    "max_vector_ops": {"type": "integer"},
                    "max_fact_puts": {"type": "integer"},
                    "base_snapshot": {"type": "string"},
                    "collaborator_dids": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "DIDs that may read the sandbox and extend TTL (not commit/discard)."
                    },
                    "workspace_id": {
                        "type": "string",
                        "description": "Optional workspace; caps quotas and enforces collaborator ACL."
                    },
                    "caller_did": {
                        "type": "string",
                        "description": "DID performing the create (defaults to owner_did)."
                    }
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "workspace_create.v1".into(),
            description: "Create a spacekit:workspace:v1 document (owner + collaborators + quotas).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["owner_did", "workspace_id"],
                "properties": {
                    "owner_did": {"type": "string"},
                    "workspace_id": {"type": "string"},
                    "collaborators": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["did", "role"],
                            "properties": {
                                "did": {"type": "string"},
                                "role": {"type": "string"}
                            }
                        }
                    },
                    "associated_repos": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "quotas": {
                        "type": "object",
                        "properties": {
                            "max_sandbox_bytes": {"type": "integer"},
                            "max_storage_bytes": {"type": "integer"}
                        }
                    }
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "workspace_get.v1".into(),
            description: "Load a workspace by id for the given owner DID.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["owner_did", "workspace_id"],
                "properties": {
                    "owner_did": {"type": "string"},
                    "workspace_id": {"type": "string"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "workspace_list.v1".into(),
            description: "List workspace index rows for an owner DID.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["owner_did"],
                "properties": {"owner_did": {"type": "string"}}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "workspace_export.v1".into(),
            description: "Export a workspace bundle for federation handoff (fact id + content).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["owner_did", "workspace_id"],
                "properties": {
                    "owner_did": {"type": "string"},
                    "workspace_id": {"type": "string"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "workspace_import.v1".into(),
            description: "Import a workspace export bundle on this node (federation destination).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["bundle", "caller_did"],
                "properties": {
                    "caller_did": {"type": "string"},
                    "bundle": {"type": "object"},
                    "on_conflict": {
                        "type": "string",
                        "enum": ["reject", "replace"],
                        "default": "reject"
                    },
                    "owner_did": {
                        "type": "string",
                        "description": "Destination owner (defaults to bundle.owner_did)"
                    },
                    "replicate_blobs_from": {
                        "type": "string",
                        "description": "Source storage base URL for CAS pull"
                    },
                    "replicate_source_authorization": {
                        "type": "string",
                        "description": "Authorization header forwarded to source"
                    }
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "blobs_replicate.v1".into(),
            description: "Pull missing BLAKE3 blobs from a remote storage node.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["source_url", "hashes"],
                "properties": {
                    "source_url": {"type": "string"},
                    "hashes": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "source_authorization": {"type": "string"}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "upload_token_mint.v1".into(),
            description: "Mint a short-lived UploadToken for put_blob/get_blob/put_fact (requires signing secret on the node).".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["issuer_did", "operation"],
                "properties": {
                    "issuer_did": {"type": "string"},
                    "operation": {
                        "type": "string",
                        "enum": ["put_blob", "get_blob", "put_fact"]
                    },
                    "resource": {
                        "type": "string",
                        "description": "BLAKE3 hex, fact id, or *",
                        "default": "*"
                    },
                    "ttl_seconds": {"type": "integer", "minimum": 1, "maximum": 86400, "default": 900}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "sandbox_commit.v1".into(),
            description: "Commit a sandbox's journal. Set dry_run=true to preview conflicts without applying.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["sandbox_id"],
                "properties": {
                    "sandbox_id": {"type": "string"},
                    "dry_run": {"type": "boolean", "default": false}
                }
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "sandbox_discard.v1".into(),
            description: "Discard a sandbox and its journal. Idempotent.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["sandbox_id"],
                "properties": {"sandbox_id": {"type": "string"}}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "sandbox_journal.v1".into(),
            description: "Return the sandbox's journal entries + quota counters. Agents use this for debugging and quota planning.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["sandbox_id"],
                "properties": {"sandbox_id": {"type": "string"}}
            }),
            version: 1,
        },
        ToolDescriptor {
            name: "graph_traverse.v1".into(),
            description: "BFS over FactPackage.dependencies. Use this for relationship reasoning over the fact DAG without writing recursive subqueries.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["start_fact_id"],
                "properties": {
                    "start_fact_id": {"type": "string"},
                    "edge_kinds": {"type": "array", "items": {"type": "string"}},
                    "max_depth": {"type": "integer", "minimum": 1, "maximum": 50, "default": 4},
                    "direction": {"type": "string", "enum": ["forward", "reverse"], "default": "forward"}
                }
            }),
            version: 1,
        },
    ]
}

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response envelope.
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

/// Dispatcher. `handle` accepts a single MCP request and returns the
/// response envelope. Transports (stdio, SSE) call this in a loop.
pub struct McpServer {
    pub facade: Arc<Facade>,
}

impl McpServer {
    pub fn new(facade: Arc<Facade>) -> Self {
        Self { facade }
    }

    pub async fn handle(&self, req: McpRequest) -> McpResponse {
        let result = match req.method.as_str() {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "spacekit-storage-node", "version": env!("CARGO_PKG_VERSION")},
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
    ) -> std::result::Result<serde_json::Value, McpError> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing tool name"))?
            .to_string();
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));

        // Deterministic idempotency key derived from (tool_name, canonical_args).
        // Agents can override by passing `idempotency_key` in arguments.
        let _idempotency_key = args
            .get("idempotency_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| derive_idempotency_key(&name, &args));

        let outcome = match name.as_str() {
            "tx_begin.v1" => self.tx_begin(args).await,
            "tx_commit.v1" => self.tx_commit(args).await,
            "tx_rollback.v1" => self.tx_rollback(args).await,
            "tx_trace.v1" => self.tx_trace(args).await,
            "tx_record_modification.v1" => self.tx_record_modification(args).await,
            "sandbox_create.v1" => self.sandbox_create(args).await,
            "sandbox_commit.v1" => self.sandbox_commit(args).await,
            "sandbox_discard.v1" => self.sandbox_discard(args).await,
            "sandbox_journal.v1" => self.sandbox_journal(args).await,
            "workspace_create.v1" => self.workspace_create(args).await,
            "workspace_get.v1" => self.workspace_get(args).await,
            "workspace_list.v1" => self.workspace_list(args).await,
            "workspace_export.v1" => self.workspace_export(args).await,
            "workspace_import.v1" => self.workspace_import(args).await,
            "blobs_replicate.v1" => self.blobs_replicate(args).await,
            "upload_token_mint.v1" => self.upload_token_mint(args).await,
            "graph_traverse.v1" => self.graph_traverse(args).await,
            other => {
                return Err(McpError {
                    code: -32601,
                    message: format!("Unknown tool: {}", other),
                    data: None,
                })
            }
        };
        outcome.map(|v| {
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&v).unwrap_or_default()
                }],
                "isError": false,
                "structuredContent": v,
            })
        })
    }

    async fn tx_begin(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let isolation = args
            .get("isolation")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "read_committed" => IsolationLevel::ReadCommitted,
                "repeatable_read" => IsolationLevel::RepeatableRead,
                _ => IsolationLevel::Serializable,
            });
        let timeout = args.get("timeout_seconds").and_then(|v| v.as_u64());
        self.facade
            .begin_transaction(isolation, timeout)
            .await
            .map(|id| serde_json::json!({"transaction_id": id}))
            .map_err(internal)
    }

    async fn tx_commit(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let id = args
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing transaction_id"))?;
        self.facade
            .commit_transaction(id)
            .await
            .map(|_| serde_json::json!({"committed": true, "transaction_id": id}))
            .map_err(internal)
    }

    async fn tx_rollback(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let id = args
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing transaction_id"))?;
        self.facade
            .rollback_transaction(id)
            .await
            .map(|_| serde_json::json!({"rolled_back": true, "transaction_id": id}))
            .map_err(internal)
    }

    async fn tx_trace(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let id = args
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing transaction_id"))?;
        let tx = self
            .facade
            .get_transaction(id)
            .await
            .ok_or_else(|| invalid_params("transaction not found"))?;
        Ok(serde_json::json!({
            "transaction_id": tx.id,
            "state": format!("{:?}", tx.state),
            "trace": tx.trace,
        }))
    }

    async fn tx_record_modification(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let tx_id = args
            .get("transaction_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing transaction_id"))?;
        let mod_val = args
            .get("modification")
            .cloned()
            .ok_or_else(|| invalid_params("missing modification"))?;
        let modification: TransactionModification = serde_json::from_value(mod_val)
            .map_err(|e| invalid_params(&format!("invalid modification: {e}")))?;
        let policy = match args.get("conflict_policy").and_then(|v| v.as_str()) {
            Some("last_writer_wins") => ConflictPolicy::LastWriterWins,
            Some("three_way_merge") => ConflictPolicy::ThreeWayMerge,
            Some("optimistic_if_match") => ConflictPolicy::OptimisticIfMatch,
            Some("reject") | None => ConflictPolicy::Reject,
            Some(other) => {
                return Err(invalid_params(&format!("unknown conflict_policy: {other}")));
            }
        };
        let bytes_written = args
            .get("bytes_written")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let sandbox_id = args.get("sandbox_id").and_then(|v| v.as_str());
        let caller_did = args.get("caller_did").and_then(|v| v.as_str());
        self.facade
            .record_transaction_modification(
                tx_id,
                modification,
                policy,
                bytes_written,
                sandbox_id,
                caller_did,
            )
            .await
            .map(|_| serde_json::json!({"recorded": true, "transaction_id": tx_id}))
            .map_err(internal)
    }

    async fn sandbox_create(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let owner = args
            .get("owner_did")
            .and_then(|v| v.as_str())
            .unwrap_or("did:spacekit:anonymous")
            .to_string();
        let mut cfg = SandboxConfig::default();
        if let Some(v) = args.get("ttl_seconds").and_then(|v| v.as_u64()) {
            cfg.ttl_seconds = v;
        }
        if let Some(v) = args.get("max_bytes_written").and_then(|v| v.as_u64()) {
            cfg.max_bytes_written = v;
        }
        if let Some(v) = args.get("max_vector_ops").and_then(|v| v.as_u64()) {
            cfg.max_vector_ops = v;
        }
        if let Some(v) = args.get("max_fact_puts").and_then(|v| v.as_u64()) {
            cfg.max_fact_puts = v;
        }
        let base = args
            .get("base_snapshot")
            .and_then(|v| v.as_str())
            .map(String::from);
        let collabs: Vec<String> = args
            .get("collaborator_dids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let workspace_id = args
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let caller = args
            .get("caller_did")
            .and_then(|v| v.as_str())
            .unwrap_or(&owner);
        self.facade
            .create_sandbox(&owner, caller, cfg, base, collabs, workspace_id)
            .await
            .map(|sb| serde_json::to_value(sb).unwrap_or(serde_json::Value::Null))
            .map_err(internal)
    }

    async fn sandbox_commit(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let id = args
            .get("sandbox_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing sandbox_id"))?;
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.facade
            .sandboxes
            .commit(id, self.facade.transactions.clone(), dry_run)
            .await
            .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
            .map_err(internal)
    }

    async fn sandbox_discard(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let id = args
            .get("sandbox_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing sandbox_id"))?;
        self.facade
            .sandboxes
            .discard(id)
            .await
            .map(|_| serde_json::json!({"discarded": true, "sandbox_id": id}))
            .map_err(internal)
    }

    async fn workspace_create(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let owner = args
            .get("owner_did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing owner_did"))?;
        let workspace_id = args
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing workspace_id"))?;
        let collaborators = parse_workspace_collaborators(&args)?;
        let associated_repos: Vec<String> = args
            .get("associated_repos")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let quotas = args
            .get("quotas")
            .and_then(|v| {
                serde_json::from_value::<crate::workspace::WorkspaceQuotas>(v.clone()).ok()
            })
            .unwrap_or_default();
        let now = chrono::Utc::now().timestamp() as u64;
        let content = crate::workspace::WorkspaceContent {
            workspace_id: workspace_id.to_string(),
            owner_did: owner.to_string(),
            collaborators,
            associated_repos,
            quotas,
            default_access_policy: spacekit_primitives::v1::fact::AccessPolicy::Public,
            status: crate::workspace::WorkspaceStatus::Active,
            created_at: now,
            updated_at: now,
        };
        self.facade
            .create_workspace(content)
            .await
            .map(|fact_id| {
                serde_json::json!({
                    "fact_id": fact_id,
                    "workspace_id": workspace_id,
                })
            })
            .map_err(internal)
    }

    async fn workspace_get(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let owner = args
            .get("owner_did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing owner_did"))?;
        let workspace_id = args
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing workspace_id"))?;
        match self
            .facade
            .get_workspace(owner, workspace_id)
            .await
            .map_err(internal)?
        {
            Some(ws) => Ok(serde_json::to_value(ws).unwrap_or(serde_json::Value::Null)),
            None => Err(invalid_params("workspace not found")),
        }
    }

    async fn blobs_replicate(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let source_url = args
            .get("source_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing source_url"))?;
        let hashes: Vec<String> = args
            .get("hashes")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let cas = self
            .facade
            .cas_data_dir()
            .ok_or_else(|| internal("cas_data_dir not configured"))?;
        crate::federation::replicate_blobs_from_source(
            cas,
            source_url,
            &hashes,
            args.get("source_authorization").and_then(|v| v.as_str()),
        )
        .await
        .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
        .map_err(internal)
    }

    async fn workspace_import(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let caller = args
            .get("caller_did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing caller_did"))?;
        let bundle: crate::workspace::WorkspaceExportBundle = args
            .get("bundle")
            .ok_or_else(|| invalid_params("missing bundle"))
            .and_then(|v| {
                serde_json::from_value(v.clone())
                    .map_err(|e| invalid_params(&format!("invalid bundle: {e}")))
            })?;
        let conflict = args
            .get("on_conflict")
            .and_then(|v| v.as_str())
            .and_then(crate::workspace::WorkspaceImportConflict::parse)
            .unwrap_or_default();
        let owner_did = args
            .get("owner_did")
            .and_then(|v| v.as_str())
            .map(String::from);
        let replicate_from = args.get("replicate_blobs_from").and_then(|v| v.as_str());
        let replicate_auth = args
            .get("replicate_source_authorization")
            .and_then(|v| v.as_str());
        self.facade
            .import_workspace(
                caller,
                bundle,
                conflict,
                owner_did,
                replicate_from,
                replicate_auth,
            )
            .await
            .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
            .map_err(internal)
    }

    async fn workspace_export(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let owner = args
            .get("owner_did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing owner_did"))?;
        let workspace_id = args
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing workspace_id"))?;
        match self
            .facade
            .export_workspace(owner, workspace_id)
            .await
            .map_err(internal)?
        {
            Some(bundle) => Ok(serde_json::to_value(bundle).unwrap_or(serde_json::Value::Null)),
            None => Err(invalid_params("workspace not found")),
        }
    }

    async fn workspace_list(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let owner = args
            .get("owner_did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing owner_did"))?;
        let list = self
            .facade
            .list_workspaces_for_owner(owner)
            .await
            .map_err(internal)?;
        Ok(serde_json::json!({"owner_did": owner, "workspaces": list}))
    }

    async fn upload_token_mint(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let issuer = args
            .get("issuer_did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing issuer_did"))?;
        let operation = args
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing operation"))?;
        let op = match operation {
            "put_blob" => crate::upload_token::UploadOp::PutBlob,
            "get_blob" => crate::upload_token::UploadOp::GetBlob,
            "put_fact" => crate::upload_token::UploadOp::PutFact,
            other => return Err(invalid_params(&format!("unknown operation: {other}"))),
        };
        let resource = args
            .get("resource")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();
        let ttl_seconds = args
            .get("ttl_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(900);
        let secret = self
            .facade
            .upload_signing_secret()
            .map(|s| s.to_vec())
            .or_else(|| crate::upload_token::load_signing_secret(self.facade.cas_data_dir()))
            .ok_or_else(|| {
                internal(
                    "upload token signing not configured — set SPACEKIT_UPLOAD_TOKEN_SECRET before starting the node, or write data_dir/.upload_token_secret, then restart",
                )
            })?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let req = crate::upload_token::MintUploadTokenRequest {
            operation: op,
            resource,
            ttl_seconds,
        };
        crate::upload_token::mint_upload_token(&secret, issuer, &req, now)
            .map(|r| serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
            .map_err(internal)
    }

    async fn sandbox_journal(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let id = args
            .get("sandbox_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing sandbox_id"))?;
        let sb = self
            .facade
            .sandboxes
            .get(id)
            .await
            .ok_or_else(|| invalid_params("sandbox not found"))?;
        Ok(serde_json::json!({
            "sandbox_id": sb.id,
            "owner_did": sb.owner_did,
            "collaborator_dids": sb.collaborator_dids,
            "state": sb.state,
            "quotas": sb.quotas,
            "config": sb.config,
            "journal": sb.journal,
        }))
    }

    /// Minimal BFS over `FactPackage.dependencies` (the fact DAG). Returns
    /// the visited fact ids in order plus the edges traversed.
    async fn graph_traverse(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, McpError> {
        let start = args
            .get("start_fact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("missing start_fact_id"))?
            .to_string();
        let max_depth: usize = match args.get("max_depth") {
            None => 4,
            Some(v) => {
                let n = v
                    .as_u64()
                    .or_else(|| v.as_i64().and_then(|i| u64::try_from(i).ok()))
                    .ok_or_else(|| {
                        invalid_params("max_depth must be an integer between 1 and 50")
                    })?;
                if !(1..=GRAPH_TRAVERSE_MAX_DEPTH).contains(&n) {
                    return Err(invalid_params(&format!(
                        "max_depth must be between 1 and {} (server cap); received {}",
                        GRAPH_TRAVERSE_MAX_DEPTH, n
                    )));
                }
                n as usize
            }
        };
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .unwrap_or("forward")
            .to_string();
        let edge_kinds: Vec<String> = args
            .get("edge_kinds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // BFS using the database's fact metadata index. We don't dereference
        // every fact; we walk the precomputed `dependencies` field.
        use std::collections::{HashSet, VecDeque};
        let mut visited: HashSet<String> = HashSet::new();
        let mut order: Vec<String> = Vec::new();
        let mut edges: Vec<(String, String)> = Vec::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((start.clone(), 0));
        visited.insert(start.clone());
        order.push(start.clone());

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            let metadata = self
                .facade
                .database
                .get_fact_metadata(&current)
                .map_err(internal)?;
            let next_ids: Vec<String> = match metadata {
                Some(m) => {
                    if direction == "reverse" {
                        // Reverse edges: scan all metadata for facts that
                        // include `current` in their dependencies. This is
                        // O(N); fine for Phase 5's minimal traversal.
                        let all = self
                            .facade
                            .database
                            .select_all_fact_metadata()
                            .map_err(internal)?;
                        all.into_iter()
                            .filter(|f| f.dependencies.iter().any(|d| d == &current))
                            .map(|f| f.fact_id)
                            .collect()
                    } else {
                        if !edge_kinds.is_empty() {
                            // FactMetadataRecord doesn't currently expose typed
                            // edges; future versions will. For now, edge_kinds
                            // is reserved.
                        }
                        m.dependencies
                    }
                }
                None => Vec::new(),
            };
            for next in next_ids {
                edges.push((current.clone(), next.clone()));
                if visited.insert(next.clone()) {
                    order.push(next.clone());
                    queue.push_back((next, depth + 1));
                }
            }
        }

        Ok(serde_json::json!({
            "start_fact_id": start,
            "direction": direction,
            "max_depth": max_depth,
            "visited": order,
            "edges": edges.iter().map(|(a, b)| serde_json::json!({"from": a, "to": b})).collect::<Vec<_>>(),
        }))
    }
}

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
    // Stable canonical encoding: object keys sorted, no insignificant whitespace.
    fn rec(v: &serde_json::Value, out: &mut String) {
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
                        c if (c as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            serde_json::Value::Array(arr) => {
                out.push('[');
                let mut first = true;
                for item in arr {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    rec(item, out);
                }
                out.push(']');
            }
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                let mut first = true;
                for k in keys {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    rec(&serde_json::Value::String(k.clone()), out);
                    out.push(':');
                    rec(&map[k], out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    rec(value, &mut out);
    out
}

fn invalid_params(message: &str) -> McpError {
    McpError {
        code: -32602,
        message: message.to_string(),
        data: None,
    }
}

fn parse_workspace_collaborators(
    args: &serde_json::Value,
) -> std::result::Result<Vec<crate::workspace::WorkspaceCollaborator>, McpError> {
    let Some(arr) = args.get("collaborators").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in arr {
        let did = item
            .get("did")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid_params("collaborator missing did"))?;
        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("agent");
        out.push(crate::workspace::WorkspaceCollaborator {
            did: did.to_string(),
            role: role.to_string(),
        });
    }
    Ok(out)
}

fn internal<E: std::fmt::Display>(e: E) -> McpError {
    McpError {
        code: -32603,
        message: e.to_string(),
        data: None,
    }
}

/// Read JSON-RPC requests from stdin, write responses to stdout. Used by the
/// `mcp` subcommand of the standalone binary.
pub async fn run_stdio(server: McpServer) -> Result<()> {
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

#[allow(dead_code)]
fn _conflict_marker(p: ConflictPolicy) -> ConflictPolicy {
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_idempotency_key_is_stable() {
        let a = derive_idempotency_key(
            "sandbox_create.v1",
            &serde_json::json!({"ttl_seconds": 100, "owner_did": "did:x"}),
        );
        let b = derive_idempotency_key(
            "sandbox_create.v1",
            &serde_json::json!({"owner_did": "did:x", "ttl_seconds": 100}),
        );
        assert_eq!(a, b, "key must be deterministic across argument ordering");
    }

    #[test]
    fn deterministic_key_changes_on_args() {
        let a = derive_idempotency_key("tx_begin.v1", &serde_json::json!({}));
        let b = derive_idempotency_key("tx_begin.v1", &serde_json::json!({"timeout_seconds": 60}));
        assert_ne!(a, b);
    }

    #[test]
    fn canonical_json_sorts_keys() {
        let v = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(canonical_json(&v), "{\"a\":1,\"b\":2}");
    }

    #[test]
    fn tool_catalog_versions_are_set() {
        for tool in tool_catalog() {
            assert!(
                tool.name.ends_with(".v1"),
                "tool {} missing version",
                tool.name
            );
            assert_eq!(tool.version, 1);
        }
    }

    #[test]
    fn tool_catalog_includes_workspace_and_upload_tools() {
        let names: Vec<_> = tool_catalog().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"workspace_create.v1".to_string()));
        assert!(names.contains(&"workspace_get.v1".to_string()));
        assert!(names.contains(&"workspace_list.v1".to_string()));
        assert!(names.contains(&"upload_token_mint.v1".to_string()));
        assert!(names.contains(&"workspace_export.v1".to_string()));
        assert!(names.contains(&"workspace_import.v1".to_string()));
    }

    #[tokio::test]
    async fn mcp_workspace_create_get_list() {
        use crate::database::Database;
        use crate::storage_facade::{Facade, FacadeConfig};
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".upload_token_secret"), b"mcp-test-secret").unwrap();
        let db = Arc::new(Database::new(dir.path().join("db").to_str().unwrap()).unwrap());
        db.initialize().unwrap();
        let facade = Arc::new(
            Facade::new(
                db,
                FacadeConfig {
                    cas_data_dir: Some(dir.path().to_path_buf()),
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        );
        let server = McpServer::new(facade);
        let owner = "did:spacekit:mcp:owner";
        let create = server
            .handle(McpRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(1),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": "workspace_create.v1",
                    "arguments": {
                        "owner_did": owner,
                        "workspace_id": "mcp-team",
                        "collaborators": [{"did": "did:spacekit:mcp:bot", "role": "agent"}]
                    }
                })),
            })
            .await;
        assert!(create.error.is_none(), "{:?}", create.error);
        let get = server
            .handle(McpRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(2),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": "workspace_get.v1",
                    "arguments": {"owner_did": owner, "workspace_id": "mcp-team"}
                })),
            })
            .await;
        assert!(get.error.is_none());
        let list = server
            .handle(McpRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(3),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": "workspace_list.v1",
                    "arguments": {"owner_did": owner}
                })),
            })
            .await;
        assert!(list.error.is_none());
        let mint = server
            .handle(McpRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::json!(4),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": "upload_token_mint.v1",
                    "arguments": {
                        "issuer_did": owner,
                        "operation": "put_blob",
                        "resource": "*",
                        "ttl_seconds": 60
                    }
                })),
            })
            .await;
        assert!(mint.error.is_none(), "{:?}", mint.error);
    }
}
