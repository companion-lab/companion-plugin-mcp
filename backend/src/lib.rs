//! MCP feature plugin backend.
//!
//! Port of the former host module `src-tauri/src/mcp.rs`, minus the builtin
//! alarm/calendar server (those tools moved into the alarms/calendar plugins).
//!
//! - Storage: `{data_dir}/mcp.json` (the dir the host passes via
//!   `PluginCtx::data_dir()`; the legacy `{app_data}/mcp.json` is migrated by
//!   the host).
//! - Sessions: one rmcp client session per enabled server, created lazily and
//!   reused until it dies. The plugin owns a tokio runtime for the lifetime of
//!   the instance; the sync `Plugin::handle` bridges onto it with
//!   `block_on`.
//! - Tools: cached tool lists are exposed to the chat model as
//!   `{server_id}__{tool_name}` (the host qualifies them further as
//!   `mcp__{server_id}__{tool}`); calls arrive as
//!   `handle("tool", {"name", "args"})`. `tools()` is a fast snapshot read of
//!   the cache and never connects; config mutations trigger a background
//!   re-probe so the cache stays fresh.

use companion_plugin_sdk::{export_plugin, Plugin, PluginCtx, ToolDef};
use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;

const MAX_OUTPUT_CHARS: usize = 32_000;
const RPC_TIMEOUT_SECS: u64 = 30;

// ─── Config types (ported verbatim from mcp.rs) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    pub enabled: bool,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub require_approval: bool,
    /// Per-server request timeout in ms. None = global default (30s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub oauth: Option<McpOAuthSetting>,
}

/// Mirrors opencode's `mcp.<name>.oauth`: an object enables OAuth with the
/// given options, `false` disables auto-detection, absent = auto-detect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpOAuthSetting {
    Disabled(bool),
    Config(McpOAuthConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpOAuthConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub callback_port: Option<u16>,
    #[serde(default)]
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub id: String,
    pub connected: bool,
    pub tool_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCallResult {
    pub content: String,
    pub is_error: bool,
}

// ─── State ───

/// Shared plugin state. Lives in an `Arc` so the request router (which runs
/// on `&self` via `block_on`) and the background probe tasks can hold it
/// together.
pub struct Mcp {
    configs: Mutex<Vec<McpServerConfig>>,
    // cached tool lists per server id
    tools: Mutex<HashMap<String, Vec<McpToolInfo>>>,
    errors: Mutex<HashMap<String, String>>,
    sessions: TokioMutex<HashMap<String, Arc<TokioMutex<McpClient>>>>,
    /// Handle onto the runtime owned by `McpPlugin` (kept alive there).
    rt: tokio::runtime::Handle,
    data_dir: PathBuf,
    stop: Arc<AtomicBool>,
}

/// One live rmcp client session. `RunningService` owns the background
/// transport task; dropping it closes the child / HTTP session.
struct McpClient {
    service: RunningService<RoleClient, ()>,
    timeout: Duration,
}

impl Mcp {
    fn config_path(&self) -> PathBuf {
        self.data_dir.join("mcp.json")
    }

    fn load_configs(&self) {
        let path = self.config_path();
        let list: Vec<McpServerConfig> = match fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        if let Ok(mut guard) = self.configs.lock() {
            *guard = list;
        }
    }

    fn save_configs(&self, configs: &[McpServerConfig]) -> Result<(), String> {
        let path = self.config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(configs).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| format!("write mcp config: {e}"))
    }

    /// The config file plus an initial tool cache, used by tests and as the
    /// round-trip target of `save_configs`.
    #[cfg(test)]
    fn stored_configs(&self) -> Vec<McpServerConfig> {
        self.configs.lock().map(|g| g.clone()).unwrap_or_default()
    }

    // ── request router (former tauri commands, 1:1) ──

    async fn route(self: &Arc<Self>, ctx: &PluginCtx, method: &str, payload: Value) -> Result<Value, String> {
        match method {
            "mcp.list_servers" => {
                let servers = self
                    .configs
                    .lock()
                    .map_err(|e| e.to_string())?
                    .clone();
                Ok(json!({ "servers": servers }))
            }
            "mcp.add_server" => self.op_add_server(ctx, payload).await,
            "mcp.update_server" => self.op_update_server(ctx, payload).await,
            "mcp.remove_server" => {
                let id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("id is required")?
                    .to_string();
                {
                    let mut configs = self.configs.lock().map_err(|e| e.to_string())?;
                    configs.retain(|c| c.id != id);
                    self.save_configs(&configs)?;
                }
                if let Ok(mut tools) = self.tools.lock() {
                    tools.remove(&id);
                }
                if let Ok(mut errors) = self.errors.lock() {
                    errors.remove(&id);
                }
                drop_session(self, &id).await;
                ctx.emit("mcp-servers-changed", &Value::Null);
                Ok(json!({ "ok": true }))
            }
            "mcp.set_enabled" => {
                let id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("id is required")?
                    .to_string();
                let enabled = payload
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or("enabled is required")?;
                {
                    let mut configs = self.configs.lock().map_err(|e| e.to_string())?;
                    let Some(cfg) = configs.iter_mut().find(|c| c.id == id) else {
                        return Err(format!("server not found: {id}"));
                    };
                    cfg.enabled = enabled;
                    if !enabled {
                        if let Ok(mut tools) = self.tools.lock() {
                            tools.remove(&id);
                        }
                    }
                    self.save_configs(&configs)?;
                }
                if !enabled {
                    drop_session(self, &id).await;
                }
                // Keep the cached tool list used by tools() fresh on enable;
                // on disable the re-probe is a no-op (config is disabled).
                self.spawn_reprobe(id.clone());
                ctx.emit("mcp-servers-changed", &Value::Null);
                Ok(json!({ "ok": true }))
            }
            "mcp.status" => self.op_status().await,
            "mcp.list_tools" => {
                let server_id = payload
                    .get("server_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.op_list_tools(server_id).await
            }
            "mcp.call_tool" => {
                let server_id = payload
                    .get("server_id")
                    .and_then(Value::as_str)
                    .ok_or("server_id is required")?
                    .to_string();
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .ok_or("tool is required")?
                    .to_string();
                let args = payload.get("args").cloned();
                self.op_call_tool(&server_id, &tool, args).await
            }
            "mcp.stop_server" => {
                let id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("id is required")?
                    .to_string();
                drop_session(self, &id).await;
                if let Ok(mut tools) = self.tools.lock() {
                    tools.remove(&id);
                }
                Ok(json!({ "ok": true }))
            }
            "mcp.ping" => Ok(json!({ "pong": true })),
            "tool" => {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("tool name missing")?;
                let args = payload.get("args").cloned().unwrap_or(json!({}));
                self.handle_tool(name, args).await
            }
            other => Err(format!("unknown method: {other}")),
        }
    }

    async fn op_add_server(self: &Arc<Self>, ctx: &PluginCtx, payload: Value) -> Result<Value, String> {
        let req: AddServerReq =
            serde_json::from_value(payload).map_err(|e| format!("bad payload: {e}"))?;
        let cfg = build_new_server(req)?;
        {
            let mut configs = self.configs.lock().map_err(|e| e.to_string())?;
            if configs.iter().any(|c| c.id == cfg.id) {
                return Err(format!("server id already exists: {}", cfg.id));
            }
            configs.push(cfg.clone());
            self.save_configs(&configs)?;
        }
        self.spawn_reprobe(cfg.id.clone());
        ctx.emit("mcp-servers-changed", &Value::Null);
        Ok(json!({ "server": cfg }))
    }

    async fn op_update_server(self: &Arc<Self>, ctx: &PluginCtx, payload: Value) -> Result<Value, String> {
        let req: UpdateServerReq =
            serde_json::from_value(payload).map_err(|e| format!("bad payload: {e}"))?;
        let updated = {
            let mut configs = self.configs.lock().map_err(|e| e.to_string())?;
            let Some(cfg) = configs.iter_mut().find(|c| c.id == req.id) else {
                return Err(format!("server not found: {}", req.id));
            };
            if let Some(name) = req.name {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err("Server name required".into());
                }
                cfg.name = name;
            }
            if let Some(v) = req.command { cfg.command = v; }
            if let Some(v) = req.args { cfg.args = v; }
            if let Some(v) = req.cwd { cfg.cwd = v; }
            if let Some(v) = req.env { cfg.env = v; }
            if let Some(v) = req.url { cfg.url = v; }
            if let Some(v) = req.headers { cfg.headers = v; }
            // JSON has no "double option": absent = keep, null = clear.
            if let Some(v) = req.timeout_ms { cfg.timeout_ms = v; }
            if let Some(v) = req.oauth {
                cfg.oauth = v.map(parse_oauth_setting).transpose()?;
            }
            if let Some(v) = req.require_approval { cfg.require_approval = v; }
            let out = cfg.clone();
            self.save_configs(&configs)?;
            out
        };
        drop_session(self, &req.id).await;
        if let Ok(mut tools) = self.tools.lock() {
            tools.remove(&req.id);
        }
        if let Ok(mut errors) = self.errors.lock() {
            errors.remove(&req.id);
        }
        self.spawn_reprobe(req.id.clone());
        ctx.emit("mcp-servers-changed", &Value::Null);
        Ok(json!({ "server": updated }))
    }

    /// Former `mcp_status`: cached counts when available, otherwise a live
    /// probe per enabled server.
    async fn op_status(self: &Arc<Self>) -> Result<Value, String> {
        // Snapshot configs, then probe each enabled server (cached tools reused).
        let configs: Vec<McpServerConfig> = self
            .configs
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        let mut out = Vec::new();
        for cfg in &configs {
            if !cfg.enabled {
                out.push(McpServerStatus {
                    id: cfg.id.clone(),
                    connected: false,
                    tool_count: 0,
                    last_error: None,
                });
                continue;
            }
            // Return cached if present.
            let cached: Option<Vec<McpToolInfo>> = self
                .tools
                .lock()
                .map(|t| t.get(&cfg.id).cloned())
                .unwrap_or(None);
            if let Some(tools) = cached {
                out.push(McpServerStatus {
                    id: cfg.id.clone(),
                    connected: true,
                    tool_count: tools.len(),
                    last_error: None,
                });
                continue;
            }
            match list_tools_live(self, cfg).await {
                Ok(tools) => {
                    let n = tools.len();
                    if let Ok(mut inner) = self.tools.lock() {
                        inner.insert(cfg.id.clone(), tools);
                    }
                    if let Ok(mut inner) = self.errors.lock() {
                        inner.remove(&cfg.id);
                    }
                    out.push(McpServerStatus {
                        id: cfg.id.clone(),
                        connected: true,
                        tool_count: n,
                        last_error: None,
                    });
                }
                Err(e) => {
                    eprintln!("[mcp] {} probe failed: {e}", cfg.id);
                    if let Ok(mut inner) = self.errors.lock() {
                        inner.insert(cfg.id.clone(), e.clone());
                    }
                    out.push(McpServerStatus {
                        id: cfg.id.clone(),
                        connected: false,
                        tool_count: 0,
                        last_error: Some(e),
                    });
                }
            }
        }
        serde_json::to_value(&out).map_err(|e| e.to_string())
    }

    /// Former `mcp_list_tools` minus the builtin merge: cached lists plus a
    /// live probe for uncached enabled servers.
    async fn op_list_tools(self: &Arc<Self>, server_id: Option<String>) -> Result<Value, String> {
        // Collect configs to probe (enabled only).
        let configs: Vec<McpServerConfig> = self
            .configs
            .lock()
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|c| c.enabled && server_id.as_ref().map(|id| id == &c.id).unwrap_or(true))
            .cloned()
            .collect();
        let mut out = Vec::new();
        for cfg in &configs {
            let cached: Option<Vec<McpToolInfo>> = self
                .tools
                .lock()
                .map(|t| t.get(&cfg.id).cloned())
                .unwrap_or(None);
            if let Some(tools) = cached {
                out.extend(tools);
                continue;
            }
            match list_tools_live(self, cfg).await {
                Ok(tools) => {
                    out.extend(tools.clone());
                    if let Ok(mut inner) = self.tools.lock() {
                        inner.insert(cfg.id.clone(), tools);
                    }
                    if let Ok(mut inner) = self.errors.lock() {
                        inner.remove(&cfg.id);
                    }
                }
                Err(e) => {
                    eprintln!("[mcp] {} tools/list failed: {e}", cfg.id);
                    if let Ok(mut inner) = self.errors.lock() {
                        inner.insert(cfg.id.clone(), e);
                    }
                }
            }
        }
        Ok(json!({ "tools": out }))
    }

    /// Former `mcp_call_tool` minus the builtin branch.
    async fn op_call_tool(
        self: &Arc<Self>,
        server_id: &str,
        tool: &str,
        args: Option<Value>,
    ) -> Result<Value, String> {
        let cfg: McpServerConfig = self
            .configs
            .lock()
            .map_err(|e| e.to_string())?
            .iter()
            .find(|c| c.id == server_id)
            .cloned()
            .ok_or_else(|| format!("server not found: {server_id}"))?;
        if !cfg.enabled {
            return Err(format!("server disabled: {server_id}"));
        }
        eprintln!("[mcp] call {server_id}/{tool}");
        let result = call_tool_live(self, &cfg, tool, args.unwrap_or_else(|| json!({}))).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }

    /// Chat-model tool call: `name` is `{server_id}__{tool_name}`. Server ids
    /// are `sanitize_id` output (alphanumeric + dash), so they never contain
    /// `__` and splitting at the first `__` is unambiguous.
    async fn handle_tool(self: &Arc<Self>, name: &str, args: Value) -> Result<Value, String> {
        let Some((server_id, tool)) = name.split_once("__") else {
            return Err(format!("invalid tool name: {name}"));
        };
        let cfg: McpServerConfig = self
            .configs
            .lock()
            .map_err(|e| e.to_string())?
            .iter()
            .find(|c| c.id == server_id)
            .cloned()
            .ok_or_else(|| format!("server not found: {server_id}"))?;
        if !cfg.enabled {
            return Err(format!("server disabled: {server_id}"));
        }
        let result = call_tool_live(self, &cfg, tool, args).await?;
        if result.is_error {
            Err(result.content)
        } else {
            Ok(Value::String(result.content))
        }
    }

    // ── background probes ──

    /// Delayed startup probe: after 2s, list tools of every enabled server to
    /// warm the cache used by `tools()`. Never emits — cache updates are
    /// invisible until the next `plugin_list_tools`.
    fn spawn_initial_probe(self: &Arc<Self>) {
        let mcp = Arc::clone(self);
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if mcp.stop.load(Ordering::Relaxed) {
                return;
            }
            let configs: Vec<McpServerConfig> = match mcp.configs.lock() {
                Ok(guard) => guard.iter().filter(|c| c.enabled).cloned().collect(),
                Err(_) => return,
            };
            for cfg in &configs {
                if mcp.stop.load(Ordering::Relaxed) {
                    break;
                }
                probe_one(&mcp, cfg).await;
            }
        });
    }

    /// Re-probe one server after a config mutation so the cached tool list
    /// used by `tools()` stays fresh. No-ops for disabled/removed servers.
    fn spawn_reprobe(self: &Arc<Self>, server_id: String) {
        let mcp = Arc::clone(self);
        self.rt.spawn(async move {
            if mcp.stop.load(Ordering::Relaxed) {
                return;
            }
            let cfg = match mcp.configs.lock() {
                Ok(guard) => guard
                    .iter()
                    .find(|c| c.id == server_id)
                    .filter(|c| c.enabled)
                    .cloned(),
                Err(_) => None,
            };
            if let Some(cfg) = cfg {
                probe_one(&mcp, &cfg).await;
            }
        });
    }
}

/// Probe a single server and refresh the tools/errors caches. Never emits.
async fn probe_one(state: &Mcp, cfg: &McpServerConfig) {
    match list_tools_live(state, cfg).await {
        Ok(tools) => {
            if let Ok(mut inner) = state.tools.lock() {
                inner.insert(cfg.id.clone(), tools);
            }
            if let Ok(mut inner) = state.errors.lock() {
                inner.remove(&cfg.id);
            }
        }
        Err(e) => {
            eprintln!("[mcp] {} probe failed: {e}", cfg.id);
            if let Ok(mut inner) = state.errors.lock() {
                inner.insert(cfg.id.clone(), e);
            }
        }
    }
}

// ─── Helpers (ported verbatim from mcp.rs) ───

fn sanitize_id(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let clean = clean.trim_matches('-').to_lowercase();
    if clean.is_empty() {
        format!("server-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0))
    } else {
        clean.chars().take(64).collect()
    }
}

fn server_timeout(cfg: &McpServerConfig) -> Duration {
    Duration::from_millis(cfg.timeout_ms.filter(|t| *t > 0).unwrap_or(RPC_TIMEOUT_SECS * 1000))
}

fn truncate(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_CHARS {
        return s.to_string();
    }
    format!("{}…[truncated {} chars]", &s[..MAX_OUTPUT_CHARS], s.len() - MAX_OUTPUT_CHARS)
}

fn parse_oauth_setting(v: serde_json::Value) -> Result<McpOAuthSetting, String> {
    if let Some(b) = v.as_bool() {
        return Ok(McpOAuthSetting::Disabled(b));
    }
    serde_json::from_value::<McpOAuthConfig>(v)
        .map(McpOAuthSetting::Config)
        .map_err(|e| format!("invalid oauth config: {e}"))
}

// ─── rmcp session lifecycle (ported verbatim from mcp.rs) ───

impl McpClient {
    /// Spawn the transport and let rmcp run `initialize` +
    /// `notifications/initialized` for us.
    async fn connect(cfg: &McpServerConfig) -> Result<Self, String> {
        let timeout = server_timeout(cfg);
        match cfg.transport {
            McpTransport::Stdio => {
                if cfg.command.trim().is_empty() {
                    return Err("stdio server needs a command".into());
                }
                let mut cmd = tokio::process::Command::new(&cfg.command);
                cmd.args(&cfg.args);
                if !cfg.cwd.trim().is_empty() {
                    cmd.current_dir(cfg.cwd.trim());
                }
                for (k, v) in &cfg.env {
                    cmd.env(k, v);
                }
                let (transport, stderr) = TokioChildProcess::builder(cmd)
                    .spawn()
                    .map_err(|e| format!("spawn {}: {e}", cfg.command))?;
                if let Some(stderr) = stderr {
                    let id = cfg.id.clone();
                    tokio::spawn(async move {
                        use tokio::io::{AsyncBufReadExt, BufReader};
                        let mut lines = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            eprintln!("[mcp {id}] stderr: {line}");
                        }
                    });
                }
                let service = ().serve(transport).await
                    .map_err(|e| format!("connect {}: {e}", cfg.name))?;
                Ok(Self { service, timeout })
            }
            McpTransport::Http => {
                if cfg.url.trim().is_empty() {
                    return Err("http server needs a url".into());
                }
                // HeaderName/HeaderValue aliases live on the reqwest-based
                // impl block. The default rmcp reqwest client is used;
                // per-request timeouts come from tokio::time::timeout below.
                let mut config =
                    StreamableHttpClientTransportConfig::default();
                config.uri = cfg.url.clone().into();
                config.custom_headers = cfg
                    .headers
                    .iter()
                    .filter_map(|(k, v)| {
                        Some((
                            k.parse().ok()?,
                            v.parse().ok()?,
                        ))
                    })
                    .collect();
                let transport = StreamableHttpClientTransport::from_config(config);
                let service = ().serve(transport).await
                    .map_err(|e| format!("connect {}: {e}", cfg.name))?;
                Ok(Self { service, timeout })
            }
        }
    }

    fn is_alive(&self) -> bool {
        !self.service.is_closed()
    }

    async fn list_tools(&self, server_id: &str) -> Result<Vec<McpToolInfo>, String> {
        let tools = tokio::time::timeout(self.timeout, self.service.list_all_tools())
            .await
            .map_err(|_| "tools/list timeout".to_string())?
            .map_err(|e| format!("tools/list: {e}"))?;
        Ok(tools
            .into_iter()
            .map(|t| McpToolInfo {
                server_id: server_id.to_string(),
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                input_schema: serde_json::Value::Object(t.input_schema.as_ref().clone()),
            })
            .collect())
    }

    async fn call_tool(
        &self,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, String> {
        let arguments: JsonObject = match args {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = tokio::time::timeout(self.timeout, self.service.call_tool(params))
            .await
            .map_err(|_| "tools/call timeout".to_string())?
            .map_err(|e| format!("tools/call: {e}"))?;
        let mut parts: Vec<String> = Vec::new();
        for block in &result.content {
            match block {
                rmcp::model::ContentBlock::Text(t) => parts.push(t.text.clone()),
                other => parts.push(serde_json::to_string(other).unwrap_or_else(|_| format!("{other:?}"))),
            }
        }
        if let Some(structured) = &result.structured_content {
            parts.push(serde_json::to_string(structured).unwrap_or_default());
        }
        Ok(McpCallResult {
            content: truncate(&parts.join("\n")),
            is_error: result.is_error.unwrap_or(false),
        })
    }
}

async fn get_session(
    state: &Mcp,
    cfg: &McpServerConfig,
) -> Result<Arc<TokioMutex<McpClient>>, String> {
    // Fast path: existing live session.
    {
        let sessions = state.sessions.lock().await;
        if let Some(handle) = sessions.get(&cfg.id) {
            let alive = { handle.lock().await.is_alive() };
            if alive {
                return Ok(handle.clone());
            }
        }
    }
    // Remove dead entry, then connect outside the map lock.
    {
        let mut sessions = state.sessions.lock().await;
        sessions.remove(&cfg.id);
    }
    let fresh = McpClient::connect(cfg).await?;
    let handle = Arc::new(TokioMutex::new(fresh));
    {
        let mut sessions = state.sessions.lock().await;
        // Another task may have beaten us; prefer the existing live one.
        if let Some(existing) = sessions.get(&cfg.id) {
            let alive = { existing.lock().await.is_alive() };
            if alive {
                return Ok(existing.clone());
            }
        }
        sessions.insert(cfg.id.clone(), handle.clone());
    }
    eprintln!("[mcp] {} session started (rmcp)", cfg.id);
    Ok(handle)
}

async fn drop_session(state: &Mcp, id: &str) {
    if let Some(handle) = state.sessions.lock().await.remove(id) {
        tokio::spawn(async move {
            let mut sess = handle.lock().await;
            let _ = sess.service.close().await;
        });
    }
}

async fn invalidate_session(state: &Mcp, id: &str) {
    drop_session(state, id).await;
}

// ─── Shared ops (rmcp-backed, ported verbatim from mcp.rs) ───

async fn list_tools_live(state: &Mcp, cfg: &McpServerConfig) -> Result<Vec<McpToolInfo>, String> {
    let handle = get_session(state, cfg).await?;
    let sess = handle.lock().await;
    let result = sess.list_tools(&cfg.id).await;
    if result.is_err() {
        drop(sess);
        invalidate_session(state, &cfg.id).await;
    }
    result
}

async fn call_tool_live(
    state: &Mcp,
    cfg: &McpServerConfig,
    tool: &str,
    args: serde_json::Value,
) -> Result<McpCallResult, String> {
    let handle = get_session(state, cfg).await?;
    let sess = handle.lock().await;
    let result = sess.call_tool(tool, args).await;
    if result.is_err() {
        drop(sess);
        invalidate_session(state, &cfg.id).await;
    }
    result
}

// ─── Request payloads ───

#[derive(Debug, Deserialize)]
struct AddServerReq {
    name: String,
    transport: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    oauth: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct UpdateServerReq {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    /// Absent = keep, null = clear, number = set.
    #[serde(default, deserialize_with = "double_option")]
    timeout_ms: Option<Option<u64>>,
    /// Absent = keep, null = clear, object/bool = set.
    #[serde(default, deserialize_with = "double_option")]
    oauth: Option<Option<serde_json::Value>>,
    #[serde(default)]
    require_approval: Option<bool>,
}

/// `deserialize_with` shim so `Option<Option<T>>` fields can tell JSON
/// `null` ("clear") apart from an absent field ("keep").
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

/// Validate and build a new server config (former body of `mcp_add_server`).
fn build_new_server(req: AddServerReq) -> Result<McpServerConfig, String> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err("Server name required".into());
    }
    let transport = match req.transport.to_lowercase().as_str() {
        "stdio" => McpTransport::Stdio,
        "http" => McpTransport::Http,
        other => return Err(format!("unknown transport: {other}")),
    };
    Ok(McpServerConfig {
        id: sanitize_id(&name),
        name,
        transport,
        enabled: true,
        command: req.command.unwrap_or_default(),
        args: req.args.unwrap_or_default(),
        cwd: req.cwd.unwrap_or_default(),
        env: req.env.unwrap_or_default(),
        url: req.url.unwrap_or_default(),
        headers: req.headers.unwrap_or_default(),
        require_approval: true,
        timeout_ms: req.timeout_ms,
        oauth: req.oauth.map(parse_oauth_setting).transpose()?,
    })
}

// ─── tools() snapshot ───

/// The ABI tool list: cached tools of enabled servers, names qualified as
/// `{server_id}__{tool_name}` (the host further qualifies them as
/// `mcp__{server_id}__{tool}`). Pure cache read — never connects.
fn cached_tool_defs(
    configs: &[McpServerConfig],
    tools: &HashMap<String, Vec<McpToolInfo>>,
) -> Vec<ToolDef> {
    let mut out = Vec::new();
    for cfg in configs.iter().filter(|c| c.enabled) {
        if let Some(list) = tools.get(&cfg.id) {
            for t in list {
                out.push(ToolDef {
                    name: format!("{}__{}", cfg.id, t.name),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Plugin impl
// ---------------------------------------------------------------------------

/// Concrete plugin type handed to `export_plugin!`. Owns the tokio runtime
/// (which must outlive the state: background probe tasks need it) and keeps
/// the ctx so the request router can emit events.
pub struct McpPlugin {
    inner: Option<Arc<Mcp>>,
    ctx: Option<PluginCtx>,
    /// Declared last so it drops after `inner`.
    rt: Option<tokio::runtime::Runtime>,
}

impl Default for McpPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl McpPlugin {
    pub fn new() -> Self {
        McpPlugin {
            inner: None,
            ctx: None,
            rt: None,
        }
    }
}

impl Plugin for McpPlugin {
    fn init(&mut self, ctx: PluginCtx) -> Result<(), String> {
        let data_dir = ctx.data_dir().ok_or("host did not provide a data dir")?;
        fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        let rt = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
        let mcp = Arc::new(Mcp {
            configs: Mutex::new(Vec::new()),
            tools: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            sessions: TokioMutex::new(HashMap::new()),
            rt: rt.handle().clone(),
            data_dir,
            stop: Arc::new(AtomicBool::new(false)),
        });
        mcp.load_configs();
        mcp.spawn_initial_probe();
        ctx.info("mcp ready");
        self.ctx = Some(ctx);
        self.rt = Some(rt);
        self.inner = Some(mcp);
        Ok(())
    }

    fn handle(&mut self, method: &str, payload: Value) -> Result<Value, String> {
        let (mcp, ctx) = match (self.inner.as_ref(), self.ctx.as_ref()) {
            (Some(m), Some(c)) => (m, c),
            _ => return Err("mcp plugin failed to initialise".into()),
        };
        let rt = match self.rt.as_ref() {
            Some(rt) => rt,
            None => return Err("mcp plugin failed to initialise".into()),
        };
        rt.block_on(async { mcp.route(ctx, method, payload).await })
    }

    fn tools(&mut self) -> Vec<ToolDef> {
        let Some(mcp) = self.inner.as_ref() else {
            return Vec::new();
        };
        let configs = match mcp.configs.lock() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };
        let tools = match mcp.tools.lock() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };
        cached_tool_defs(&configs, &tools)
    }

    fn shutdown(&mut self) {
        let Some(mcp) = self.inner.as_ref() else {
            return;
        };
        mcp.stop.store(true, Ordering::Relaxed);
        if let Some(rt) = self.rt.as_ref() {
            // Close every live session inline so the rmcp transports shut
            // down while the runtime still exists.
            let _ = rt.block_on(async {
                let mut sessions = mcp.sessions.lock().await;
                for (_, handle) in sessions.drain() {
                    let mut sess = handle.lock().await;
                    let _ = sess.service.close().await;
                }
            });
        }
    }
}

export_plugin!(McpPlugin::new());

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mcp-plugin-test-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    extern "C" fn noop_emit(_: *mut c_void, _: *const u8, _: usize, _: *const u8, _: usize) {}
    extern "C" fn noop_log(_: *mut c_void, _: u32, _: *const u8, _: usize) {}
    extern "C" fn noop_dir(_: *mut c_void, _: *mut u8, _: usize) -> usize {
        0
    }

    /// A ctx whose host bridge is a no-op: emits and logs vanish, `data_dir()`
    /// is absent (the state under test carries its own temp dir).
    fn test_ctx() -> PluginCtx {
        PluginCtx::from_bridge(companion_plugin_sdk::HostBridge {
            user: std::ptr::null_mut(),
            emit_event: noop_emit,
            log: noop_log,
            data_dir: noop_dir,
            plugin_dir: noop_dir,
        })
    }

    fn test_mcp(dir: PathBuf) -> (tokio::runtime::Runtime, Arc<Mcp>) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mcp = Arc::new(Mcp {
            configs: Mutex::new(Vec::new()),
            tools: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            sessions: TokioMutex::new(HashMap::new()),
            rt: rt.handle().clone(),
            data_dir: dir,
            stop: Arc::new(AtomicBool::new(false)),
        });
        // Declared so `mcp` drops before `rt`.
        (rt, mcp)
    }

    fn server(id: &str, enabled: bool) -> McpServerConfig {
        McpServerConfig {
            id: id.into(),
            name: id.into(),
            transport: McpTransport::Stdio,
            enabled,
            // Empty command: a background re-probe fails fast in `connect`
            // without ever spawning a process.
            command: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            env: HashMap::new(),
            url: String::new(),
            headers: HashMap::new(),
            require_approval: true,
            timeout_ms: None,
            oauth: None,
        }
    }

    fn live_tool(server_id: &str, name: &str) -> McpToolInfo {
        McpToolInfo {
            server_id: server_id.into(),
            name: name.into(),
            description: format!("{server_id} {name}"),
            input_schema: json!({ "type": "object" }),
        }
    }

    #[test]
    fn sanitize_id_keeps_alphanumerics_and_dashes() {
        assert_eq!(sanitize_id("My Server!"), "my-server");
        assert_eq!(sanitize_id("Playwright 2"), "playwright-2");
        assert_eq!(sanitize_id("--edge--case--"), "edge--case");
        assert_eq!(sanitize_id("UPPER"), "upper");
        assert_eq!(
            sanitize_id(&"a".repeat(100)).chars().count(),
            64,
            "ids are capped at 64 chars"
        );
        assert!(
            sanitize_id("!!!").starts_with("server-"),
            "fully symbolic names fall back to server-<millis>"
        );
    }

    #[test]
    fn config_survives_save_and_reload() {
        let dir = temp_dir("roundtrip");
        let (_rt, a) = test_mcp(dir.clone());
        let mut cfg = server("demo", true);
        cfg.name = "Demo Server".into();
        cfg.timeout_ms = Some(5000);
        cfg.oauth = Some(McpOAuthSetting::Config(McpOAuthConfig {
            client_id: "cid".into(),
            client_secret: "sec".into(),
            scope: "read".into(),
            callback_port: Some(8080),
            redirect_uri: "http://127.0.0.1:8080".into(),
        }));
        a.save_configs(&[cfg.clone()]).unwrap();

        // A fresh instance over the same data dir must read back exactly what
        // was written (compared through serde so the file shape is checked).
        let (_rt2, b) = test_mcp(dir.clone());
        b.load_configs();
        let loaded = b.stored_configs();
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            serde_json::to_value(&loaded[0]).unwrap(),
            serde_json::to_value(&cfg).unwrap(),
            "reloaded config must round-trip through mcp.json unchanged"
        );
        assert!(dir.join("mcp.json").exists());
    }

    #[test]
    fn legacy_config_json_still_loads() {
        let json = r#"{
            "id": "http-one",
            "name": "HTTP one",
            "transport": "http",
            "enabled": true,
            "url": "http://localhost:9",
            "headers": {"Authorization": "Bearer x"},
            "require_approval": true
        }"#;
        let cfg: McpServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.id, "http-one");
        assert!(matches!(cfg.transport, McpTransport::Http));
        assert_eq!(cfg.url, "http://localhost:9");
        assert_eq!(
            cfg.headers.get("Authorization").map(String::as_str),
            Some("Bearer x")
        );
        assert!(cfg.timeout_ms.is_none(), "absent timeout_ms defaults to None");
        assert!(cfg.oauth.is_none(), "absent oauth defaults to None");
        assert!(cfg.require_approval);
        // Transports serialize lowercase, matching the legacy file shape.
        assert!(
            serde_json::to_string(&cfg).unwrap().contains(r#""transport":"http""#),
            "transport must keep its lowercase serde form"
        );
    }

    #[test]
    fn oauth_setting_accepts_bool_or_object() {
        assert!(matches!(
            parse_oauth_setting(json!(false)).unwrap(),
            McpOAuthSetting::Disabled(false)
        ));
        assert!(matches!(
            parse_oauth_setting(json!(true)).unwrap(),
            McpOAuthSetting::Disabled(true)
        ));
        match parse_oauth_setting(json!({"client_id": "x"})).unwrap() {
            McpOAuthSetting::Config(c) => assert_eq!(c.client_id, "x"),
            other => panic!("expected config, got {other:?}"),
        }
        assert!(parse_oauth_setting(json!("nope")).is_err());
    }

    #[test]
    fn add_server_rejects_empty_name_and_unknown_transport() {
        let (rt, mcp) = test_mcp(temp_dir("validation"));
        let ctx = test_ctx();
        let err = rt
            .block_on(mcp.route(&ctx, "mcp.add_server", json!({ "name": "   ", "transport": "stdio" })))
            .unwrap_err();
        assert!(err.contains("name"), "{err}");
        let err = rt
            .block_on(mcp.route(&ctx, "mcp.add_server", json!({ "name": "Demo", "transport": "carrier-pigeon" })))
            .unwrap_err();
        assert!(err.contains("unknown transport"), "{err}");
        assert!(
            mcp.stored_configs().is_empty(),
            "rejected adds must not mutate the config"
        );
    }

    #[test]
    fn add_server_assigns_sanitized_id_and_persists() {
        let (rt, mcp) = test_mcp(temp_dir("add"));
        mcp.stop.store(true, Ordering::Relaxed);
        let ctx = test_ctx();
        let payload = json!({ "name": "Demo Server", "transport": "stdio", "command": "" });
        let out = rt.block_on(mcp.route(&ctx, "mcp.add_server", payload.clone())).unwrap();
        assert_eq!(out["server"]["id"], "demo-server");
        assert_eq!(out["server"]["require_approval"], true, "add defaults require_approval on");
        // Duplicate id (same sanitized name) is rejected.
        let err = rt.block_on(mcp.route(&ctx, "mcp.add_server", payload)).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert_eq!(mcp.stored_configs().len(), 1);
        assert!(mcp.data_dir.join("mcp.json").exists(), "add persists to mcp.json");
    }

    #[test]
    fn update_server_distinguishes_absent_null_and_value() {
        let (rt, mcp) = test_mcp(temp_dir("update"));
        mcp.stop.store(true, Ordering::Relaxed);
        let ctx = test_ctx();
        let mut cfg = server("demo", true);
        cfg.timeout_ms = Some(5000);
        cfg.oauth = Some(McpOAuthSetting::Disabled(false));
        *mcp.configs.lock().unwrap() = vec![cfg];

        // Absent fields keep their value.
        let out = rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "demo" }))).unwrap();
        assert_eq!(out["server"]["timeout_ms"], 5000);
        assert_eq!(out["server"]["oauth"], false);
        // null clears.
        let out = rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "demo", "timeout_ms": null }))).unwrap();
        assert!(out["server"]["timeout_ms"].is_null(), "null timeout_ms must clear the field");
        // A value sets it.
        let out = rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "demo", "timeout_ms": 15000 }))).unwrap();
        assert_eq!(out["server"]["timeout_ms"], 15000);
        // Plain optional fields patch without touching the rest.
        let out = rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "demo", "name": "Renamed" }))).unwrap();
        assert_eq!(out["server"]["name"], "Renamed");
        assert_eq!(out["server"]["timeout_ms"], 15000, "unrelated fields survive a patch");
        // Empty name is rejected.
        let err = rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "demo", "name": "  " }))).unwrap_err();
        assert!(err.contains("name"), "{err}");
        // Unknown id errors.
        let err = rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "nope" }))).unwrap_err();
        assert!(err.contains("server not found"), "{err}");
    }

    #[test]
    fn update_and_remove_clear_cached_tools_and_errors() {
        let (rt, mcp) = test_mcp(temp_dir("caches"));
        mcp.stop.store(true, Ordering::Relaxed);
        let ctx = test_ctx();
        *mcp.configs.lock().unwrap() = vec![server("demo", true)];
        mcp.tools.lock().unwrap().insert("demo".into(), vec![live_tool("demo", "search")]);
        mcp.errors.lock().unwrap().insert("demo".into(), "stale".into());

        rt.block_on(mcp.route(&ctx, "mcp.update_server", json!({ "id": "demo" }))).unwrap();
        assert!(mcp.tools.lock().unwrap().get("demo").is_none(), "update clears the tools cache");
        assert!(mcp.errors.lock().unwrap().get("demo").is_none(), "update clears the error");

        rt.block_on(mcp.route(&ctx, "mcp.remove_server", json!({ "id": "demo" }))).unwrap();
        assert!(mcp.stored_configs().is_empty());
        assert!(mcp.tools.lock().unwrap().get("demo").is_none());
        assert!(mcp.errors.lock().unwrap().get("demo").is_none());
    }

    #[test]
    fn set_enabled_false_clears_tools_and_persists() {
        let (rt, mcp) = test_mcp(temp_dir("enabled"));
        mcp.stop.store(true, Ordering::Relaxed);
        let ctx = test_ctx();
        *mcp.configs.lock().unwrap() = vec![server("demo", true)];
        mcp.tools.lock().unwrap().insert("demo".into(), vec![live_tool("demo", "search")]);

        rt.block_on(mcp.route(&ctx, "mcp.set_enabled", json!({ "id": "demo", "enabled": false }))).unwrap();
        assert!(!mcp.stored_configs()[0].enabled);
        assert!(mcp.tools.lock().unwrap().get("demo").is_none(), "disabling clears the tools cache");

        rt.block_on(mcp.route(&ctx, "mcp.set_enabled", json!({ "id": "demo", "enabled": true }))).unwrap();
        assert!(mcp.stored_configs()[0].enabled);
    }

    #[test]
    fn cached_tool_defs_qualify_names_and_skip_disabled() {
        let configs = vec![server("alpha", true), server("beta", false)];
        let mut tools = HashMap::new();
        tools.insert("alpha".into(), vec![live_tool("alpha", "search"), live_tool("alpha", "fetch")]);
        tools.insert("beta".into(), vec![live_tool("beta", "hidden")]);
        let defs = cached_tool_defs(&configs, &tools);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["alpha__search", "alpha__fetch"],
            "qualified as server_id__tool; disabled servers contribute nothing"
        );
        assert_eq!(defs[0].description, "alpha search");
        assert_eq!(defs[0].input_schema, json!({ "type": "object" }));
    }

    #[test]
    fn server_timeout_defaults_to_30s() {
        let mut cfg = server("demo", true);
        assert_eq!(server_timeout(&cfg), Duration::from_secs(30), "None = global default");
        cfg.timeout_ms = Some(0);
        assert_eq!(server_timeout(&cfg), Duration::from_secs(30), "0 means the default");
        cfg.timeout_ms = Some(5000);
        assert_eq!(server_timeout(&cfg), Duration::from_millis(5000));
    }

    #[test]
    fn truncate_marks_long_output() {
        let exact = "x".repeat(MAX_OUTPUT_CHARS);
        assert_eq!(truncate(&exact), exact, "output at the cap is untouched");
        let long = "y".repeat(MAX_OUTPUT_CHARS + 5);
        let t = truncate(&long);
        assert!(t.starts_with(&"y".repeat(MAX_OUTPUT_CHARS)));
        assert!(t.contains("[truncated 5 chars]"), "{t}");
    }
}
