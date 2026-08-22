//! Tool catalog merger and routing table.
//!
//! Merges tool catalogs from storage and compute MCP backends. Tool names
//! are served flat (no prefix) when there are no collisions. If a collision
//! is detected, conflicting tools are prefixed with `storage.` / `compute.`.

use crate::mcp_proxy::{StdioBackend, ToolDescriptor};
use anyhow::Result;
use std::collections::HashMap;

pub enum BackendId {
    Storage,
    Compute,
}

pub struct CatalogEntry {
    pub descriptor: ToolDescriptor,
    pub backend: BackendId,
}

pub struct MergedCatalog {
    pub tools: Vec<CatalogEntry>,
    pub routing: HashMap<String, BackendId>,
}

/// Connect to both backends, issue `tools/list`, and merge the catalogs.
pub async fn build_catalog(
    storage: &StdioBackend,
    compute: &StdioBackend,
) -> Result<MergedCatalog> {
    let storage_resp = storage.call("tools/list", None).await?;
    let compute_resp = compute.call("tools/list", None).await?;

    let storage_tools = extract_tools(storage_resp.result)?;
    let compute_tools = extract_tools(compute_resp.result)?;

    let mut tools = Vec::new();
    let mut routing = HashMap::new();
    let mut seen: HashMap<String, BackendId> = HashMap::new();

    for t in storage_tools {
        let name = t.name.clone();
        if seen.contains_key(&name) {
            let prefixed = format!("storage.{}", name);
            routing.insert(prefixed.clone(), BackendId::Storage);
            tools.push(CatalogEntry {
                descriptor: ToolDescriptor {
                    name: prefixed,
                    ..t
                },
                backend: BackendId::Storage,
            });
        } else {
            seen.insert(name.clone(), BackendId::Storage);
            routing.insert(name.clone(), BackendId::Storage);
            tools.push(CatalogEntry {
                descriptor: t,
                backend: BackendId::Storage,
            });
        }
    }

    for t in compute_tools {
        let name = t.name.clone();
        if seen.contains_key(&name) {
            let prefixed = format!("compute.{}", name);
            routing.insert(prefixed.clone(), BackendId::Compute);
            tools.push(CatalogEntry {
                descriptor: ToolDescriptor {
                    name: prefixed,
                    ..t
                },
                backend: BackendId::Compute,
            });
        } else {
            seen.insert(name.clone(), BackendId::Compute);
            routing.insert(name.clone(), BackendId::Compute);
            tools.push(CatalogEntry {
                descriptor: t,
                backend: BackendId::Compute,
            });
        }
    }

    Ok(MergedCatalog { tools, routing })
}

fn extract_tools(result: Option<serde_json::Value>) -> Result<Vec<ToolDescriptor>> {
    let val = result.ok_or_else(|| anyhow::anyhow!("tools/list returned no result"))?;
    let tools_val = val
        .get("tools")
        .cloned()
        .unwrap_or(serde_json::Value::Array(vec![]));
    let tools: Vec<ToolDescriptor> = serde_json::from_value(tools_val)?;
    Ok(tools)
}
