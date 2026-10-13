# `spacekit agent map` v2 + `spacekit agent pack`

Multi-language project graph and LLM context packer. Extends the original
Python-only `agent map` to the full C#/Azure + Rust stack, and adds a `pack`
command that turns the graph into model-ready context for a task.

## What changed

| File | Change |
|------|--------|
| `Cargo.toml` | Added `tree-sitter` 0.27 + `tree-sitter-c-sharp`/`-typescript`/`-rust`/`-bicep` and `regex` (versions pinned; verified to compile together). |
| `src/full_client.rs` | `mod repo_lang;`; new `AgentCommands::Pack {..}`; `Map`/`Pack` now dispatch to `repo_lang::handle_map`/`handle_pack`. |
| `src/full_client/repo_lang.rs` | **New.** Self-contained: tree-sitter analyzers, repo walk, cross-file resolution, and the context packer. |
| `src/full_client/code_session.rs` | Added `pub(crate) fn python_lang_files` — an adapter that reuses the existing embedded-`ast` Python analyzer so Python keeps full symbol/import extraction under the new builder. |

The old `build_repo_map` / `handle_map` / `RepoMapArgs` in `code_session.rs` are
now unused (dead-code warnings only). The embedded Python analyzer they relied on
(`analyze_python_files`, `resolve_py_import`, `REPO_MAP_PY`) stays live via the new
adapter. Remove the old repo-map code whenever convenient.

## Usage

```bash
spacekit agent map  --root ./MyProject --out myproject.repo.json
spacekit agent pack --root ./MyProject --query "add rate limiting to the greeter" --budget 40000 --out context.md
spacekit agent pack --root ./MyProject --seed src/Services/Greeter.cs --hops 2
```

`map` emits a `spacekit:repo-map:v2` node/edge graph. `pack` emits a Markdown
bundle: a structure outline, the resolved relationships, and the full contents of
the files the graph says are relevant, under a token budget.

## Language coverage

| Language | Parser | Symbols | Relationships |
|----------|--------|---------|---------------|
| C# | tree-sitter-c-sharp | namespace, class, interface, struct, record, enum, method, property, field | `references` (using→file), `extends`, `calls`, `tests` |
| TypeScript/TSX | tree-sitter-typescript | class, interface, enum, type, function, method, property | `imports` (path→file), `extends`, `calls` |
| Rust | tree-sitter-rust | mod, struct, enum, trait, impl methods, fn (+`#[test]`) | `imports`, `calls` |
| Bicep | tree-sitter-bicep | resource, module, param, output | `imports` (module→.bicep) |
| SQL (incl. T-SQL) | regex (dialect-agnostic) | table, view, procedure, function, trigger, index, type | `references` (FROM/JOIN→object) |
| Python | existing embedded `ast` | function, class, method | `imports`, `calls` |
| Pipelines (YAML) | `serde_yaml` | pipeline_stage, pipeline_job (Azure Pipelines stages/jobs/deployments + GitHub Actions jobs) | `imports` (`template:`/`uses:`→file) |
| MSBuild (`.csproj`) | regex | project | `imports` (`ProjectReference`→.csproj, `PackageReference`→external) |

`.github/workflows/` is traversed even though it's a dot-directory. Tests are detected by path (`*.Tests`, `*.test.ts`, `*_test.rs`, `tests/`,
`__tests__`) and attribute (`[Fact]`/`[Test]`, `#[test]`), and linked to the file
under test by namespace/import and by name.

## Build caveats

- **musl / static builds** (`build-linux.sh`): the tree-sitter grammars compile a
  little C, so a fully static musl build needs a musl C toolchain (e.g. `musl-gcc`)
  reachable by `cc`. The SQL path (regex) has no C dependency.
- **tree-sitter version unification**: after adding the deps, run
  `cargo tree -i tree-sitter`. If another workspace crate (e.g. `growformer`)
  already pulls a different tree-sitter core, Cargo must unify to one version; the
  grammars here target the 0.27 core via `tree-sitter-language` 0.1.x.

## RAG helpers

Three commands turn the graph into retrieval inputs for a coding model (works with
any model — a local Qwen Coder as well as a hosted one):

```bash
# 1) Always-include skeleton: directory tree + per-file symbol summary (small).
#    Prepend this to every prompt so the model knows what exists.
spacekit agent map --root . --structure --out structure.txt

# 2) Per-symbol chunks (JSONL) for a vector store — one row per top-level
#    definition with its source, structural metadata, and rolled-up graph edges
#    (the symbol's calls + its file's imports/references). Chunk by symbol, not
#    fixed windows, so functions/classes stay whole.
spacekit agent chunks --root . --out chunks.jsonl

# 3) Agent query surface over the graph (build once with `map`, then query the JSON):
spacekit agent nav --map project.repo.json --list src/Services
spacekit agent nav --map project.repo.json --open Greeter.cs          # symbols + edges
spacekit agent nav --map project.repo.json --open Greeter.cs --code   # + contents
spacekit agent nav --map project.repo.json --neighbors GreeterService # 1-hop edges
```

Recommended RAG shape: keep the **structure** in context on every call (it's the
index — don't embed or retrieve it), and select **content** either with `pack`
(graph retrieval) or with a vector store built from `chunks` and expanded over the
edges each chunk carries (graph-augmented RAG). For an agent loop, hand the model
`nav list`/`open`/`neighbors` as tools and let it walk the graph on demand.

## Possible future work

- **Pipeline steps**: currently stages and jobs are captured; individual `task:`/
  `script:` steps could become `pipeline_step` nodes if step-level detail helps.
- **Precise pipeline line numbers**: names are located by string search (serde_yaml
  drops spans); a span-preserving YAML parse would make lines exact.

## Design notes

- `extends`/`calls` edges are **resolved-only**: drawn only when a name resolves to
  a single symbol repo-wide (conservative; avoids false links in overload-heavy C#).
- Token budget uses a `bytes/4` estimate; swap in a real tokenizer for exact budgets.
- The graph is a flat node/edge list (`spacekit:repo-map:v2`) so it drops into
  graph-ML pipelines as well as a prompt.
