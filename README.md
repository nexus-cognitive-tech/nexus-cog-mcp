# nexus-cog-mcp

Brain-aware Model Context Protocol (MCP) server for the Nexus Cog stack.

The server exposes every [`nexus_cog_commands`] operation as an MCP tool,
so hosts such as Claude Desktop or OpenCode can call the cortex, causal
graph, provenance store, pattern matcher and more over stdio or HTTP.

## Running

```bash
cargo run --bin nexus-cog-mcp-server
```

The server speaks MCP over stdio by default.

## Environment variables

| Variable | Purpose |
|---|---|
| `NEXUS_COG_MCP_TRANSPORT` | `stdio` (default) or `http` |
| `NEXUS_COG_DB` | Path to the SQLite DB file. This is the variable the `nexus-cog` CLI sets. |
| `NEXUS_COG_MCP_DEFAULT_WORKSPACE` | Workspace directory; DB is placed at `<workspace>/.nexus-cog/palace.db`. |

If neither `NEXUS_COG_DB` nor `NEXUS_COG_MCP_DEFAULT_WORKSPACE` is set, the
server falls back to `/tmp/nexus-cog-mcp/_default`.

## HTTP transport

Build with the `http` feature:

```bash
cargo run --bin nexus-cog-mcp-server --features http
```

The server then binds to `0.0.0.0:8080` and mounts the MCP service at `/mcp`.

## From the Nexus Cog CLI

```bash
nexus-cog mcp
```

The CLI spawns `nexus-cog-mcp-server` and passes the selected profile DB via
`NEXUS_COG_DB`.

## Tools

The server exposes the following MCP tools:

- `cortex_tick`, `cortex_sleep`, `cortex_explain`
- `intent_check`, `intent_declare`
- `causal_add_node`, `causal_add_edge`, `causal_forward`, `causal_blast`, `causal_pre_mortem`
- `provenance_record`, `provenance_explain`, `provenance_search`
- `intel_store`, `intel_recall`
- `palace_summary`, `palace_recall`
- `brain_verify`, `brain_risks`, `brain_hypothesis`
- `patterns_list`, `patterns_match_code`
- `antifragile_adversarial`
