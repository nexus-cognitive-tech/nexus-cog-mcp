//! MCP tools — every tool is an `#[rmcp::tool]`-annotated
//! function that delegates to [`nexus_cog_commands`].
//!
//! The MCP server binary uses the `rmcp` 2.2 SDK to expose every
//! CLI command as an MCP tool. The CLI binary uses the same
//! [`nexus_cog_commands`] functions directly. They share a single
//! source of truth.

use std::path::PathBuf;

use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{
    prompt, prompt_handler, prompt_router, tool, tool_handler, tool_router, Error as McpError,
    ServerHandler,
};

use nexus_cog_commands as cmd;

#[derive(Clone, Default)]
pub struct Server {
    tool_router: ToolRouter<Self>,
}

impl Server {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Open (or create) the per-session workspace directory and
/// instantiate a fresh [`cmd::Ctx`].
async fn ctx_from_tool() -> Result<cmd::Ctx, String> {
    let workspace: PathBuf = std::env::var_os("NEXUS_COG_MCP_DEFAULT_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/nexus-cog-mcp/_default"));
    let _ = std::fs::create_dir_all(&workspace);
    let db_path = workspace.join(".nexus-cog/palace.db");
    cmd::Ctx::open(db_path).map_err(|e| format!("ctx open: {e:?}"))
}

/// Wrap a `serde_json::Value` into a successful [`CallToolResult`].

fn err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

fn ok_json(value: serde_json::Value) -> Result<CallToolResult, McpError> {
    let content = ContentBlock::json(value)?;
    Ok(CallToolResult::success(vec![content]))
}

#[tool_router(server_handler)]
impl Server {
    // ───────────────────────────────────────────────────────────────
    // Cortex — the brain itself
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Run one tick of the brain-like cortex. Inputs are thalamic-channel probability vectors keyed by channel label (e.g. `channel.0`).")]
    async fn cortex_tick(
        &self,
        parameters: Parameters<CortexTickParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let task = parameters.0.task.unwrap_or_else(|| "mcp.tick".to_string());
        let value = serde_json::to_value(
            cmd::cognitive::think(
                &mut ctx,
                &task,
                parameters.0.context.as_deref(),
                parameters.0.response.as_deref(),
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Run one NREM/REM consolidation cycle on the cortex.")]
    async fn cortex_sleep(
        &self,
        parameters: Parameters<CortexSleepParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::decay::apply(&ctx, 14.0, 0.05, parameters.0.replay_per_cycle)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Produce a self-explanation of the cortex state — exposes amygdala valence, neuromodulator levels, replay length.")]
    async fn cortex_explain(&self) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::cognitive::mirror(&ctx, "explain", "no-response")
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Intent — security drift detector
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Run the security drift detector against the supplied code. Returns findings, IPI score and a per-finding severity.")]
    async fn intent_check(
        &self,
        parameters: Parameters<IntentCheckParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::intent::check(
                &mut ctx,
                &parameters.0.module,
                &parameters.0.current_code,
                parameters.0.strict.unwrap_or(false),
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Declare a module's intent.")]
    async fn intent_declare(
        &self,
        parameters: Parameters<IntentDeclareParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::intent::declare(&mut ctx, &parameters.0.module, &parameters.0.purpose)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Causal
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Compute blast-radius analysis for a causal node.")]
    async fn causal_blast(
        &self,
        parameters: Parameters<EntityParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::causal::blast(&ctx, &parameters.0.entity)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Forward traversal from a causal node.")]
    async fn causal_forward(
        &self,
        parameters: Parameters<EntityParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::causal::forward(&ctx, &parameters.0.entity)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Pre-mortem analysis on a causal node — enumerates failure scenarios derived from the graph.")]
    async fn causal_pre_mortem(
        &self,
        parameters: Parameters<EntityParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::causal::pre_mortem(&ctx, &parameters.0.entity)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Add a node to the causal graph.")]
    async fn causal_add_node(
        &self,
        parameters: Parameters<CausalAddNodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::causal::add_node(
                &mut ctx,
                &parameters.0.id,
                &parameters.0.name,
                parameters.0.r#type.as_deref(),
                parameters.0.description.as_deref(),
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Add an edge to the causal graph.")]
    async fn causal_add_edge(
        &self,
        parameters: Parameters<CausalAddEdgeParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::causal::add_edge(&mut ctx, &parameters.0.from, &parameters.0.to, None, None)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Provenance
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Record a provenance entry with SHA-256 content hash. The returned id can be used with `provenance.explain`.")]
    async fn provenance_record(
        &self,
        parameters: Parameters<ProvenanceRecordParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::provenance::record(
                &mut ctx,
                &parameters.0.artifact,
                &parameters.0.origin,
                &parameters.0.content,
                &parameters.0.source,
                &parameters.0.prompt,
                None,
                None,
                None,
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Explain the lineage of a record by short id.")]
    async fn provenance_explain(
        &self,
        parameters: Parameters<IdParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::provenance::explain(&ctx, &parameters.0.id, None)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Full-text search over provenance records.")]
    async fn provenance_search(
        &self,
        parameters: Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::provenance::search(&ctx, &parameters.0.query)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Intel — hippocampal recall and store
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Recall hippocampal episodes by similarity to a query.")]
    async fn intel_recall(
        &self,
        parameters: Parameters<IntelRecallParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::intel::recall(
                &mut ctx,
                &parameters.0.query,
                parameters.0.limit,
                parameters.0.category.as_deref(),
                parameters.0.min_importance,
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Store a memory entry in working memory.")]
    async fn intel_store(
        &self,
        parameters: Parameters<IntelStoreParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::intel::store(
                &mut ctx,
                &parameters.0.key,
                &parameters.0.value,
                parameters.0.category.as_deref(),
                parameters.0.importance,
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Palace
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Workspace summary (rooms, items, ticks).")]
    async fn palace_summary(&self) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::palace::summary(&ctx).map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Recall hippocampal episodes by similarity to a query (BM25 over working memory).")]
    async fn palace_recall(
        &self,
        parameters: Parameters<LimitParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::palace::recall(&ctx, &parameters.0.query, parameters.0.limit.unwrap_or(10), None, None, None)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Brain
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Run the 8-check code verifier.")]
    async fn brain_verify(
        &self,
        parameters: Parameters<CodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let value = serde_json::to_value(
            cmd::brain::verify(&parameters.0.code).map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Detect security / performance / reliability risks.")]
    async fn brain_risks(
        &self,
        parameters: Parameters<CodeFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let value = serde_json::to_value(
            cmd::brain::risks(&parameters.0.code, parameters.0.file.as_deref())
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "A/B comparison with the 7-criterion decision matrix.")]
    async fn brain_hypothesis(
        &self,
        parameters: Parameters<BrainHypothesisParams>,
    ) -> Result<CallToolResult, McpError> {
        let value = serde_json::to_value(
            cmd::brain::hypothesis(
                &parameters.0.title,
                &parameters.0.description,
                &parameters.0.code_a,
                &parameters.0.code_b,
                parameters.0.criteria,
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Patterns
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "List every built-in pattern.")]
    async fn patterns_list(&self) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::patterns::list(&ctx).map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    #[tool(description = "Match known patterns in source code.")]
    async fn patterns_match_code(
        &self,
        parameters: Parameters<CodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::patterns::match_code(&ctx, &parameters.0.code)
                .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Antifragile
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Generate paginated adversarial inputs.")]
    async fn antifragile_adversarial(
        &self,
        parameters: Parameters<AntifragileParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool().await.map_err(|e| err(e))?;
        let value = serde_json::to_value(
            cmd::antifragile::adversarial(
                &ctx,
                parameters.0.target.as_deref(),
                parameters.0.limit,
                parameters.0.offset,
                None,
                None,
            )
            .map_err(|e| err(e))?,
        )
        .map_err(|e| err(e))?;
        ok_json(value)
    }
}



#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct ThinkAboutSecurityParams {
    #[schemars(description = "Source code to analyse")]
    pub code: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct DiagnoseCausalChainParams {
    #[schemars(description = "Target entity")]
    pub target: String,
    #[schemars(description = "Symptoms observed")]
    pub symptoms: String,
}

// ───────────────────────────────────────────────────────────────────────
// Tool parameter types — auto-derived JSON Schema via rmcp.
// ───────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CortexTickParams {
    #[schemars(description = "Task description")]
    pub task: Option<String>,
    #[schemars(description = "Optional context snippet")]
    pub context: Option<String>,
    #[schemars(description = "Optional model response to analyse against the 6 phases")]
    pub response: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CortexSleepParams {
    #[schemars(description = "Max episodes to replay (default 32)")]
    pub replay_per_cycle: usize,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct EntityParams {
    #[schemars(description = "Node id")]
    pub entity: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IdParams {
    #[schemars(description = "Record id (UUID or prefix)")]
    pub id: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct QueryParams {
    #[schemars(description = "Search query")]
    pub query: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct LimitParams {
    #[schemars(description = "Query")]
    pub query: String,
    #[schemars(description = "Max results")]
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CodeParams {
    #[schemars(description = "Source code")]
    pub code: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CodeFileParams {
    #[schemars(description = "Source code")]
    pub code: String,
    #[schemars(description = "Optional filename for context")]
    pub file: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntentCheckParams {
    #[schemars(description = "Module name (must have been declared)")]
    pub module: String,
    #[schemars(description = "Source code to inspect")]
    pub current_code: String,
    #[schemars(description = "Treat Info-severity findings as violations")]
    pub strict: Option<bool>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntentDeclareParams {
    #[schemars(description = "Module name")]
    pub module: String,
    #[schemars(description = "Purpose statement")]
    pub purpose: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CausalAddNodeParams {
    #[schemars(description = "Node id")]
    pub id: String,
    #[schemars(description = "Display name")]
    pub name: String,
    #[schemars(description = "Node type (feature|code_entity|invariant|...)")]
    pub r#type: Option<String>,
    #[schemars(description = "Optional description")]
    pub description: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CausalAddEdgeParams {
    #[schemars(description = "Source node id")]
    pub from: String,
    #[schemars(description = "Target node id")]
    pub to: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct ProvenanceRecordParams {
    #[schemars(description = "Artifact identifier")]
    pub artifact: String,
    #[schemars(description = "Origin (model name, tool name, etc.)")]
    pub origin: String,
    #[schemars(description = "Content to record")]
    pub content: String,
    #[schemars(description = "Source kind")]
    pub source: String,
    #[schemars(description = "Prompt that produced the artifact")]
    pub prompt: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntelRecallParams {
    #[schemars(description = "Query text")]
    pub query: String,
    #[schemars(description = "Max results (default 10)")]
    pub limit: Option<usize>,
    #[schemars(description = "Optional category filter")]
    pub category: Option<String>,
    #[schemars(description = "Optional salience floor in [0,1]")]
    pub min_importance: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntelStoreParams {
    #[schemars(description = "Entry key")]
    pub key: String,
    #[schemars(description = "Entry value")]
    pub value: String,
    #[schemars(description = "Optional category")]
    pub category: Option<String>,
    #[schemars(description = "Optional importance in [0,1]")]
    pub importance: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct BrainHypothesisParams {
    #[schemars(description = "Short title")]
    pub title: String,
    #[schemars(description = "What the hypothesis is about")]
    pub description: String,
    #[schemars(description = "Source code of approach A")]
    pub code_a: String,
    #[schemars(description = "Source code of approach B")]
    pub code_b: String,
    #[schemars(description = "Optional criterion list")]
    pub criteria: Option<Vec<String>>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct AntifragileParams {
    #[schemars(description = "Target description")]
    pub target: Option<String>,
    #[schemars(description = "Max inputs to return (default 50, cap 500)")]
    pub limit: Option<usize>,
    #[schemars(description = "Pagination offset (default 0)")]
    pub offset: Option<usize>,
}

