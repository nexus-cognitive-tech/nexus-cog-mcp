//! MCP tools — every tool is an `#[rmcp::tool]`-annotated
//! function that delegates to [`nexus_cog_commands`].
//!
//! The MCP server binary uses the `rmcp` 2.2 SDK to expose every
//! CLI command as an MCP tool. The CLI binary uses the same
//! [`nexus_cog_commands`] functions directly. They share a single
//! source of truth.
//!
//! ## Singleton lifecycle
//!
//! [`Server`] owns an [`Arc<OnceLock<Result<Cmd, String>>>`]; the
//! first call to [`AppState::get`] opens the database, hydrates the
//! cortex and builds the orthogonal engines. Every subsequent call
//! returns a clone of that very same [`Cmd::ctx`]. Without this,
//! every MCP tool invocation would build a fresh `Cortex` from
//! scratch — hippocampal episodes, modulator levels, declared
//! intents and the last response would all evaporate between calls,
//! defeating the entire persistence layer.
//!
//! If opening the context fails, the error is stored in the
//! `OnceLock` and surfaced as an MCP internal error on every
//! subsequent call. The server never panics because of a DB
//! failure.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_router, ErrorData as McpError};

use nexus_cog_commands::common::encode_text_to_sdr;
use nexus_cog_commands::{self as cmd, Ctx};

/// Application state shared by every tool invocation.
///
/// The `OnceLock` ensures the cortex and the orthogonal engines
/// are constructed exactly once per process. Even if the HTTP
/// transport clones the [`Server`] for each request, every clone
/// shares the same `Arc` and therefore the same `CmdCtx`.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<OnceLock<Result<Cmd, String>>>,
}

#[derive(Clone)]
struct Cmd {
    ctx: Ctx,
}

impl AppState {
    /// Lazily build the state on first access, then return a
    /// reference to the same `CmdCtx` forever.
    ///
    /// Errors are stored in the `OnceLock`, so a failed open is
    /// reproducible and cheap on subsequent calls.
    fn get(&self) -> Result<&Ctx, String> {
        let res = self.inner.get_or_init(|| {
            Cmd::open_default().map_err(|e| {
                tracing::error!(error = %e, "failed to open nexus-cog MCP context");
                e
            })
        });
        match res {
            Ok(cmd) => Ok(&cmd.ctx),
            Err(e) => Err(e.clone()),
        }
    }
}

impl Cmd {
    /// Resolve the DB path from the environment and open the
    /// shared context.
    ///
    /// Resolution order:
    /// 1. `NEXUS_COG_DB` — used as-is (matches the CLI convention).
    /// 2. `NEXUS_COG_MCP_DEFAULT_WORKSPACE` — DB is placed at
    ///    `<workspace>/.nexus-cog/palace.db`.
    /// 3. Fallback `/tmp/nexus-cog-mcp/_default`.
    fn open_default() -> Result<Self, String> {
        let db_path = resolve_db_path()?;
        let ctx = open_ctx(db_path)?;
        Ok(Self { ctx })
    }
}

fn resolve_db_path() -> Result<PathBuf, String> {
    if let Some(db) = std::env::var_os("NEXUS_COG_DB") {
        return Ok(PathBuf::from(db));
    }

    let workspace: PathBuf = std::env::var_os("NEXUS_COG_MCP_DEFAULT_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/nexus-cog-mcp/_default"));
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("create workspace {workspace:?}: {e}"))?;
    Ok(workspace.join(".nexus-cog/palace.db"))
}

fn open_ctx(db_path: PathBuf) -> Result<Ctx, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create db dir {parent:?}: {e}"))?;
    }
    Ctx::open(db_path).map_err(|e| format!("ctx open: {e:?}"))
}

/// Return the singleton [`Ctx`] for this process.
fn ctx_from_tool(state: &AppState) -> Result<cmd::Ctx, String> {
    state.get().cloned()
}

fn err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

fn ok_json(value: serde_json::Value) -> Result<CallToolResult, McpError> {
    let content = ContentBlock::json(value)?;
    Ok(CallToolResult::success(vec![content]))
}

#[derive(Clone)]
pub struct Server {
    state: AppState,
}

impl Server {
    /// Build a new MCP server. The cortex is not opened until the
    /// first tool call, so server construction is cheap and
    /// infallible.
    pub fn new() -> Self {
        Self::default()
    }

    /// Eagerly open the cortex and fail fast if the environment
    /// points to an unusable workspace.
    ///
    /// Useful for the stdio/HTTP binaries so that DB errors surface
    /// at startup instead of inside the first tool call.
    pub fn initialize(self) -> Result<Self, String> {
        self.state.get()?;
        Ok(self)
    }

    /// Build a server pre-wired to a specific SQLite file. Useful
    /// for tests and for `nexus-cog mcp --db <path>`.
    pub fn with_workspace(db_path: PathBuf) -> Result<Self, String> {
        let ctx = open_ctx(db_path)?;
        let inner = Arc::new(OnceLock::new());
        let _ = inner.set(Ok(Cmd { ctx }));
        Ok(Self {
            state: AppState { inner },
        })
    }
}

impl Default for Server {
    fn default() -> Self {
        Self {
            state: AppState {
                inner: Arc::new(OnceLock::new()),
            },
        }
    }
}

#[tool_router(server_handler)]
impl Server {
    // ───────────────────────────────────────────────────────────────
    // Cortex — the brain itself
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Run one tick of the cortical simulation. Returns the ThoughtBroadcast and records the response in the episode metadata for later recall through cortex_explain.")]
    async fn cortex_tick(
        &self,
        parameters: Parameters<CortexTickParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let p = parameters.0;
        let mut inputs = std::collections::HashMap::new();
        inputs.insert(
            "channel.0".to_string(),
            encode_text_to_sdr(&p.task),
        );
        if let Some(context) = &p.context {
            inputs.insert(
                "channel.1".to_string(),
                encode_text_to_sdr(context),
            );
        }
        let broadcast = ctx
            .cortex
            .tick(inputs, p.response.as_deref())
            .map_err(err)?;
        let payload = serde_json::json!({
            "tick": broadcast.tick,
            "chosen_action": broadcast.chosen_action,
            "loop_phase": format!("{:?}", broadcast.loop_phase),
            "activations": broadcast.activations,
            "valence": {
                "reward": broadcast.valence.reward,
                "threat": broadcast.valence.threat,
                "novelty": broadcast.valence.novelty,
            },
        });
        ok_json(payload)
    }

    #[tool(description = "Run a sleep / consolidation cycle on the cortex. Replays the most-salient hippocampal episodes and updates the cortex's internal state.")]
    async fn cortex_sleep(
        &self,
        parameters: Parameters<CortexSleepParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let replay_per_cycle = parameters.0.replay_per_cycle.max(1);
        let before = ctx.cortex.read().hippocampus().len();
        let report = ctx.cortex.sleep(replay_per_cycle).map_err(err)?;
        let after = ctx.cortex.read().hippocampus().len();
        let phase = if before == 0 {
            "empty_cortex"
        } else if report.episodes_replayed == 0 {
            "below_salience_floor"
        } else {
            "consolidated"
        };
        let payload = serde_json::json!({
            "phase": phase,
            "episodes_replayed": report.episodes_replayed,
            "unique_patterns": report.unique_patterns,
            "avg_target_overlap": report.avg_target_overlap,
            "elapsed_ms": report.elapsed_ms,
            "episodes_before": before,
            "episodes_after": after,
            "replay_per_cycle": replay_per_cycle,
        });
        ok_json(payload)
    }

    #[tool(description = "Return a self-explanation of the cortex's current state: dopamine / serotonin / norepinephrine levels, episode count, last action and last response. Values come from the persistent backend so they survive a server restart.")]
    async fn cortex_explain(&self) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let cortex = ctx.cortex.read();
        let stats = cortex.stats();
        let mods = cortex.modulators();
        let total_ticks = ctx
            .persistence
            .state
            .get(cmd::persistence::keys::TOTAL_TICKS)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        // Sanity guard: persisted counter is the source of truth,
        // but `stats.ticks` is hydrated from it on startup, so the
        // two should agree modulo in-flight ticks. If they don't,
        // surface the discrepancy explicitly rather than papering
        // over it.
        let derived_ticks = stats.ticks.max(total_ticks as u64);
        let payload = serde_json::json!({
            "subject": "cortex",
            "ticks": derived_ticks,
            "in_memory_ticks": stats.ticks,
            "persisted_ticks": total_ticks,
            "ticks_consistent": (stats.ticks as i64) == total_ticks,
            "n_episodes": stats.n_episodes,
            "n_columns": stats.n_columns,
            "loop_phase": format!("{:?}", stats.loop_phase),
            "dopamine": mods.dopamine.level,
            "serotonin": mods.serotonin.level,
            "norepinephrine": mods.norepinephrine.level,
            "last_action": stats.last_action,
            "last_response": stats.last_response,
        });
        ok_json(payload)
    }

    // ───────────────────────────────────────────────────────────────
    // Intent — security drift detector + module intent declaration
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Run the security drift detector against the supplied code. Returns an IPI score (0-100, higher = safer), the list of findings, and — if a matching module intent was previously declared — its purpose.")]
    async fn intent_check(
        &self,
        parameters: Parameters<IntentCheckParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::intent::check(
                &mut ctx,
                &parameters.0.module,
                &parameters.0.current_code,
                parameters.0.strict.unwrap_or(false),
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Declare the purpose of `module` so future intent_check calls can compare the declared intent against the current implementation.")]
    async fn intent_declare(
        &self,
        parameters: Parameters<IntentDeclareParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::intent::declare(&mut ctx, &parameters.0.module, &parameters.0.purpose)
                .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Causal — graph queries
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Compute the blast radius of a node: every entity that depends on it (downstream) and every entity it depends on (upstream), with edge-type and strength breakdown.")]
    async fn causal_blast(
        &self,
        parameters: Parameters<EntityParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::causal::blast(&ctx, &parameters.0.entity).map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Forward traversal: list every direct or indirect downstream of `entity`.")]
    async fn causal_forward(
        &self,
        parameters: Parameters<EntityParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::causal::forward(&ctx, &parameters.0.entity).map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Pre-mortem: enumerate plausible failure scenarios for `entity`, ranked by likelihood × impact, based on the causal graph and the entity's neighbourhood.")]
    async fn causal_pre_mortem(
        &self,
        parameters: Parameters<EntityParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::causal::pre_mortem(&ctx, &parameters.0.entity).map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Add a node to the causal graph. `type` must be one of: code_entity, behavior, feature, invariant, assumption, decision, constraint, bug, external_dep.")]
    async fn causal_add_node(
        &self,
        parameters: Parameters<CausalAddNodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::causal::add_node(
                &mut ctx,
                &parameters.0.id,
                &parameters.0.name,
                parameters.0.r#type.as_deref(),
                parameters.0.description.as_deref(),
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Add a directed edge between two nodes in the causal graph.")]
    async fn causal_add_edge(
        &self,
        parameters: Parameters<CausalAddEdgeParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::causal::add_edge(
                &mut ctx,
                &parameters.0.from,
                &parameters.0.to,
                parameters.0.kind.as_deref(),
                parameters.0.strength,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Provenance — artifact lineage
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Record a provenance entry. `source` must be one of: model_output, tool_execution, test_run, user_input, reasoning, code_extraction, file_load, composition, inference.")]
    async fn provenance_record(
        &self,
        parameters: Parameters<ProvenanceRecordParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::provenance::record(
                &mut ctx,
                &parameters.0.artifact,
                &parameters.0.origin,
                &parameters.0.content,
                parameters.0.source.id(),
                &parameters.0.prompt,
                parameters.0.parent.as_deref(),
                parameters.0.agent.as_deref(),
                parameters.0.confidence,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Explain the lineage of a single provenance record by short id or full uuid.")]
    async fn provenance_explain(
        &self,
        parameters: Parameters<IdParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::provenance::explain(&ctx, &parameters.0.id, None).map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Full-text search over every stored provenance record.")]
    async fn provenance_search(
        &self,
        parameters: Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::provenance::search(&ctx, &parameters.0.query).map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Intel — hippocampal store + recall
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Recall hippocampal episodes similar to `query`, ranked by semantic similarity. Filterable by category and minimum salience. Subsystem: `hippocampus` (long-term episodic memory; survives restarts). For short-term working-memory items use `palace_recall` instead — the two subsystems are independent.")]
    async fn intel_recall(
        &self,
        parameters: Parameters<IntelRecallParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::intel::recall(
                &ctx,
                &parameters.0.query,
                parameters.0.limit,
                parameters.0.category.as_deref(),
                parameters.0.min_importance,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Store a memory entry. Backed by the same hippocampal store used by intel_recall, so the entry is immediately queryable and persists across restarts.")]
    async fn intel_store(
        &self,
        parameters: Parameters<IntelStoreParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::intel::store(
                &mut ctx,
                &parameters.0.key,
                &parameters.0.value,
                parameters.0.category.as_deref(),
                parameters.0.importance,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Palace
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Workspace summary (rooms, items, ticks, episodes, last action).")]
    async fn palace_summary(&self) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(cmd::palace::summary(&ctx).map_err(err)?)
            .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Recall working-memory items via SDR similarity. Subsystem: `working_memory` (short-term, capacity-limited, decays per tick). For long-term structured knowledge use `intel_recall` instead — palace and intel are two different storage subsystems and do not share data.")]
    async fn palace_recall(
        &self,
        parameters: Parameters<LimitParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::palace::recall(
                &ctx,
                &parameters.0.query,
                parameters.0.limit.unwrap_or(10),
                None,
                None,
                None,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Brain — code analysis
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Run the 8-check code verifier (unwrap/expect/panic/TODO density).")]
    async fn brain_verify(
        &self,
        parameters: Parameters<CodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let value = serde_json::to_value(
            cmd::brain::verify(&parameters.0.code).map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Detect security / performance / reliability risks. Reuses the security drift detector from intent_check — every finding is reproducible, locatable, and carries a concrete suggested_fix.")]
    async fn brain_risks(
        &self,
        parameters: Parameters<CodeFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let value = serde_json::to_value(
            cmd::brain::risks(&parameters.0.code, parameters.0.file.as_deref())
                .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "A/B comparison with the configurable decision matrix. Returns per-criterion scores, weighted total and a winner.")]
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
                parameters.0.score_eps,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Patterns — built-in catalog + matcher
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "List every built-in pattern. `pattern_type` is always a snake_case string (e.g. `error_handling`, `iterator`).")]
    async fn patterns_list(&self) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(cmd::patterns::list(&ctx).map_err(err)?)
            .map_err(err)?;
        ok_json(value)
    }

    #[tool(description = "Match known patterns in source code. `language` defaults to `rust`; pass `any` to consider every language, or one of: rust, typescript, python, go, java, cpp.")]
    async fn patterns_match_code(
        &self,
        parameters: Parameters<PatternsMatchParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::patterns::match_code(
                &ctx,
                &parameters.0.code,
                parameters.0.language.as_deref(),
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }

    // ───────────────────────────────────────────────────────────────
    // Antifragile — adversarial input generator
    // ───────────────────────────────────────────────────────────────

    #[tool(description = "Generate paginated adversarial inputs for `target`. Categories: empty, boundary, special_characters, injection, type_confusion, oversized.")]
    async fn antifragile_adversarial(
        &self,
        parameters: Parameters<AntifragileParams>,
    ) -> Result<CallToolResult, McpError> {
        let ctx = ctx_from_tool(&self.state).map_err(err)?;
        let value = serde_json::to_value(
            cmd::antifragile::adversarial(
                &ctx,
                parameters.0.target.as_deref(),
                parameters.0.limit,
                parameters.0.offset,
                parameters.0.categories.as_ref().cloned(),
                None,
            )
            .map_err(err)?,
        )
        .map_err(err)?;
        ok_json(value)
    }
}

// ────────────────────────────────────────────────────────────────
// Parameter shapes — JSON Schema is generated by `schemars`.
// ────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CortexTickParams {
    #[schemars(description = "Task description that drives the cortical input")]
    pub task: String,
    #[schemars(description = "Optional additional context")]
    pub context: Option<String>,
    #[schemars(
        description = "Optional text response emitted by the model for this tick. Stored in the hippocampal episode metadata and surfaced through cortex_explain."
    )]
    pub response: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CortexSleepParams {
    #[schemars(description = "Maximum number of episodes to replay per cycle")]
    pub replay_per_cycle: usize,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct EntityParams {
    #[schemars(description = "Entity id in the causal graph")]
    pub entity: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IdParams {
    #[schemars(description = "Record id (uuid or short prefix)")]
    pub id: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct QueryParams {
    #[schemars(description = "Full-text query")]
    pub query: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct LimitParams {
    #[schemars(description = "Query text")]
    pub query: String,
    #[schemars(description = "Maximum number of hits")]
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CodeParams {
    #[schemars(description = "Source code to analyse")]
    pub code: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CodeFileParams {
    #[schemars(description = "Source code to analyse")]
    pub code: String,
    #[schemars(description = "Optional filename for the source (used in error messages)")]
    pub file: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntentCheckParams {
    #[schemars(description = "Module identifier")]
    pub module: String,
    #[schemars(description = "Current implementation of the module")]
    pub current_code: String,
    #[schemars(description = "Strict mode: even Info-level findings count")]
    pub strict: Option<bool>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntentDeclareParams {
    #[schemars(description = "Module identifier")]
    pub module: String,
    #[schemars(description = "Plain-text purpose of the module")]
    pub purpose: String,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct CausalAddNodeParams {
    #[schemars(description = "Stable identifier for the node")]
    pub id: String,
    #[schemars(description = "Human-readable name")]
    pub name: String,
    #[schemars(
        description = "Node type. One of: code_entity, behavior, feature, invariant, assumption, decision, constraint, bug, external_dep."
    )]
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
    #[schemars(description = "Edge kind. One of: causes, enables, prevents, mitigates, correlates.")]
    pub kind: Option<String>,
    #[schemars(description = "Edge strength in [0.0, 1.0]")]
    pub strength: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct ProvenanceRecordParams {
    #[schemars(description = "Artifact identifier")]
    pub artifact: String,
    #[schemars(description = "Origin (model name, tool name, etc.)")]
    pub origin: String,
    #[schemars(description = "Content to record")]
    pub content: String,
    #[schemars(
        description = "Source kind",
        rename = "source"
    )]
    pub source: ProvenanceSourceWire,
    #[schemars(description = "Prompt that produced the artifact")]
    pub prompt: String,
    #[schemars(description = "Optional parent record id")]
    pub parent: Option<String>,
    #[schemars(description = "Optional agent name (defaults to `origin`)")]
    pub agent: Option<String>,
    #[schemars(description = "Optional confidence in [0.0, 1.0]")]
    pub confidence: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSourceWire {
    ModelOutput,
    ToolExecution,
    TestRun,
    UserInput,
    Reasoning,
    CodeExtraction,
    FileLoad,
    Composition,
    Inference,
}

impl ProvenanceSourceWire {
    /// Stable snake_case id used by [`nexus_cog_commands::provenance`].
    pub fn id(&self) -> &'static str {
        match self {
            Self::ModelOutput => "model_output",
            Self::ToolExecution => "tool_execution",
            Self::TestRun => "test_run",
            Self::UserInput => "user_input",
            Self::Reasoning => "reasoning",
            Self::CodeExtraction => "code_extraction",
            Self::FileLoad => "file_load",
            Self::Composition => "composition",
            Self::Inference => "inference",
        }
    }
}

impl std::fmt::Display for ProvenanceSourceWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntelRecallParams {
    #[schemars(description = "Query text")]
    pub query: String,
    #[schemars(description = "Maximum number of hits")]
    pub limit: Option<usize>,
    #[schemars(description = "Filter by source prefix (e.g. `intel` or `intel.mcp_test`)")]
    pub category: Option<String>,
    #[schemars(description = "Minimum salience in [0.0, 1.0]")]
    pub min_importance: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct IntelStoreParams {
    #[schemars(description = "Stable key for the entry")]
    pub key: String,
    #[schemars(description = "Value to store")]
    pub value: String,
    #[schemars(description = "Optional category (used as `source` prefix and as a recall filter)")]
    pub category: Option<String>,
    #[schemars(description = "Salience / importance in [0.0, 1.0]")]
    pub importance: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct BrainHypothesisParams {
    #[schemars(description = "Short title")]
    pub title: String,
    #[schemars(description = "What is being compared")]
    pub description: String,
    #[schemars(description = "Source code for approach A")]
    pub code_a: String,
    #[schemars(description = "Source code for approach B")]
    pub code_b: String,
    #[schemars(description = "Optional list of criteria (defaults to the 7 built-ins)")]
    pub criteria: Option<Vec<String>>,
    #[schemars(description = "Score band that counts as a tie. Defaults to 0.02.")]
    pub score_eps: Option<f64>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct PatternsMatchParams {
    #[schemars(description = "Source code to analyse")]
    pub code: String,
    #[schemars(
        description = "Language of the source. One of: rust, typescript, python, go, java, cpp, any. Defaults to `rust`."
    )]
    pub language: Option<String>,
}

#[derive(serde::Deserialize, rmcp::schemars::JsonSchema)]
pub struct AntifragileParams {
    #[schemars(description = "Target under test (function name, endpoint, etc.)")]
    pub target: Option<String>,
    #[schemars(description = "Maximum number of inputs to return (1..=500)")]
    pub limit: Option<usize>,
    #[schemars(description = "Pagination offset")]
    pub offset: Option<usize>,
    #[schemars(
        description = "Optional category filter: empty, boundary, special_characters, injection, type_confusion, oversized."
    )]
    pub categories: Option<Vec<String>>,
}
