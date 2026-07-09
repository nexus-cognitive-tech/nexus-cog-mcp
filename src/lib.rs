//! # nexus-cog-mcp
//!
//! Brain-aware Model Context Protocol server for the Nexus Cog
//! stack. Built on the `rmcp` 2.2 SDK.
//!
//! Every tool is a `#[tool]`-annotated function that delegates to
//! [`nexus_cog_commands`] — no business logic lives in this
//! crate. The CLI binary uses the same [`nexus_cog_commands`]
//! functions directly; the MCP server exposes them over JSON-RPC.
//!
//! The binary `nexus-cog-mcp-server` runs the MCP server over
//! stdio (default) or streamable HTTP. The CLI binary
//! `nexus-cog mcp` spawns that binary.

pub mod tools;

pub use tools::Server;
