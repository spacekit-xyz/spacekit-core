# Workspaces (`spacekit:workspace:v1`)

Workspaces are first-class documents that bind an **owner DID**, **collaborators**
(human and agent), **associated repos**, and **quotas**. They are stored as
signed facts plus a `workspace_index` document for discovery.

## HTTP API (agentic routes)

Requires a storage node with the agentic facade and `data_dir` / `cas_data_dir`
configured.

| Method | Path | Auth |
|--------|------|------|
| `POST` | `/api/workspaces` | `Authorization: DID <owner>` |
| `GET` | `/api/workspaces/{workspace_id}` | `Authorization: DID <owner>` |
| `GET` | `/api/workspaces?owner_did=<did>` | none (public listing by owner) |
| `GET` | `/api/workspaces/{workspace_id}/export` | `Authorization: DID <owner>` |
| `POST` | `/api/workspaces/import` | `Authorization: DID <destination owner>` |

### Create body

```json
{
  "workspace_id": "my-team",
  "collaborators": [
    { "did": "did:spacekit:agent:bot", "role": "agent" }
  ],
  "associated_repos": ["my-repo"],
  "quotas": {
    "max_sandbox_bytes": 67108864,
    "max_storage_bytes": 1073741824
  }
}
```

`quotas` is optional (server defaults apply).

## MCP tools

When running `spacekit-storage-node mcp` (stdio JSON-RPC), agents can call:

| Tool | Arguments |
|------|-----------|
| `workspace_create.v1` | `owner_did`, `workspace_id`, optional `collaborators`, `associated_repos`, `quotas` |
| `workspace_get.v1` | `owner_did`, `workspace_id` |
| `workspace_list.v1` | `owner_did` |
| `workspace_export.v1` | `owner_did`, `workspace_id` |
| `workspace_import.v1` | `caller_did`, `bundle`, optional `on_conflict`, `owner_did` |

Pair with `sandbox_create.v1` (`workspace_id`) for quota-scoped agent sessions.
See [`mcp.md`](./mcp.md).

## CLI

```bash
spacekit workspace create my-team \
  --storage-url http://127.0.0.1:3030 \
  --collaborator did:spacekit:agent:bot:agent \
  --repo my-repo

spacekit workspace show my-team --storage-url http://127.0.0.1:3030

spacekit workspace list --owner-did did:spacekit:user:alice

spacekit workspace export my-team -o ./my-team.json

spacekit workspace import ./my-team.json --owner-did did:spacekit:dest:owner
```

## Sandboxes scoped to a workspace

Pass `workspace_id` on `POST /api/sandboxes` (or MCP `sandbox_create` with
`workspace_id`). The facade:

1. Loads the workspace fact for the sandbox **owner** DID.
2. Checks the **caller** (`Authorization: DID`) is the owner or a collaborator.
3. Caps `max_bytes_written` to `min(request, workspace.quotas.max_sandbox_bytes)`.
4. Rejects create when summed `bytes_written` on active sandboxes for that
   `workspace_id` is already at `workspace.quotas.max_storage_bytes`.

```json
{
  "workspace_id": "team-alpha",
  "ttl_seconds": 600,
  "max_bytes_written": 67108864
}
```

## Relation to sandboxes and repos

- **Repos** hold code via `spacekit:repo:commit:v1` facts (`spacekit repo` CLI).
- **Sandboxes** hold ephemeral multi-model state; use `RepoTree` modifications to
  commit code in the same sandbox session as DB/vector writes.
- **Workspaces** name the collaboration boundary and enforce sandbox quotas.

See also: [`sandboxes.md`](sandboxes.md), [`spacekit-repository-hosting.md`](spacekit-repository-hosting.md).
