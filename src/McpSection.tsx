// MCP settings section (ported from the host's organisms/McpSection.tsx;
// UI kit comes from the host via ctx.ui, data from the plugin backend via
// useMcpServers — same tab/slot the core "Tools" tab used). Above the
// external-server cards sits the Default Tools card: every tool the avatar
// can already call from the enabled plugins (alarms, calendar, …) plus
// chat's always-available ask_user.

import { useCallback, useEffect, useState } from "react";
import type { PluginContext } from "../../sdk/js/@companion/plugin";
import { useMcpServers } from "./backend";
import { ServerFormModal } from "./ServerFormModal";
import type { McpOAuthSetting, McpServerConfig, McpToolInfo } from "./types";
import { isTauri, tauriInvoke, tauriListen } from "./tauri";

type KvRow = { key: string; value: string };

/** Mirror of the Rust `PluginInfo` (camelCase over IPC) — only what the
 * default-tools grouping needs. */
type PluginInfoLite = {
  id: string;
  name: string;
  enabled: boolean;
  backendLoaded: boolean;
};

/** Mirror of the Rust `PluginToolInfo` (camelCase over IPC). */
type PluginToolInfo = {
  pluginId: string;
  name: string;
  description: string;
  inputSchema: unknown;
};

/** Chat's always-available clarify tool, shown first in the default tools. */
const ASK_USER_TOOL: PluginToolInfo = {
  pluginId: "builtin",
  name: "ask_user",
  description:
    "Ask the user a clarifying question and wait for the answer (always available).",
  inputSchema: {},
};

function kvRows(map: Record<string, string> | undefined): KvRow[] {
  const rows = Object.entries(map ?? {}).map(([key, value]) => ({ key, value }));
  return rows.length ? rows : [{ key: "", value: "" }];
}

function kvMap(rows: KvRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) {
    const k = r.key.trim();
    if (k) out[k] = r.value;
  }
  return out;
}

function KeyValueEditor({ ui, rows, onChange, keyPlaceholder, valuePlaceholder }: {
  ui: PluginContext["ui"];
  rows: KvRow[];
  onChange: (rows: KvRow[]) => void;
  keyPlaceholder: string;
  valuePlaceholder: string;
}) {
  const { Button, TextInput } = ui;
  const set = (i: number, patch: Partial<KvRow>) => {
    onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {rows.map((r, i) => (
        <div key={i} style={{ display: "flex", gap: 6 }}>
          <TextInput
            value={r.key}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => set(i, { key: e.target.value })}
            placeholder={keyPlaceholder}
            style={{ flex: 1, fontSize: 12 }}
          />
          <TextInput
            value={r.value}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => set(i, { value: e.target.value })}
            placeholder={valuePlaceholder}
            style={{ flex: 2, fontSize: 12 }}
          />
          <Button
            variant="secondary"
            onClick={() => onChange(rows.filter((_, j) => j !== i))}
            disabled={rows.length <= 1 && !r.key && !r.value}
            style={{ padding: "5px 10px", fontSize: 11, flexShrink: 0 }}
          >
            ✕
          </Button>
        </div>
      ))}
      <div>
        <Button
          variant="secondary"
          onClick={() => onChange([...rows, { key: "", value: "" }])}
          style={{ padding: "5px 10px", fontSize: 11 }}
        >
          + Add
        </Button>
      </div>
    </div>
  );
}

function oauthMode(oauth: McpOAuthSetting | null | undefined): "auto" | "object" | "false" {
  if (oauth === false) return "false";
  if (oauth && typeof oauth === "object") return "object";
  return "auto";
}

/**
 * "Default Tools": every tool the avatar can call besides external MCP
 * servers — each enabled plugin's backend tools (alarms, calendar, …) plus
 * chat's always-available ask_user. Refreshes on the host's plugins-changed
 * event, so adding or enabling a plugin adds its tools here immediately;
 * the chat loop picks them up on its next message.
 */
function DefaultToolsCard({ ui }: { ui: PluginContext["ui"] }) {
  const { SectionCard } = ui;
  const [plugins, setPlugins] = useState<PluginInfoLite[]>([]);
  const [tools, setTools] = useState<PluginToolInfo[]>([]);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const [infos, toolInfos] = await Promise.all([
        tauriInvoke<PluginInfoLite[]>("plugin_list"),
        tauriInvoke<PluginToolInfo[]>("plugin_list_tools"),
      ]);
      setPlugins(infos);
      setTools(toolInfos);
    } catch (e) {
      // Plugin host unavailable (non-Tauri run) — the card just stays empty.
      console.warn("[mcp] default tools unavailable:", e);
    }
  }, []);

  useEffect(() => {
    void refresh();
    // Host-global event fired by the Plugins tab (enable/disable/import/
    // uninstall); the payload is the fresh PluginInfo[] but re-reading both
    // lists keeps this in one code path.
    const off = tauriListen("plugins-changed", () => void refresh());
    return () => {
      void off.then((unlisten) => unlisten());
    };
  }, [refresh]);

  // ask_user lives in the chat plugin's frontend, not a backend tools()
  // list, so plugin_list_tools never carries it — add it when chat runs.
  const allTools: PluginToolInfo[] = plugins.some((p) => p.id === "chat" && p.enabled)
    ? [ASK_USER_TOOL, ...tools]
    : tools;
  // Group by owning plugin, chat's ask_user under its own header.
  const groups = new Map<string, PluginToolInfo[]>();
  for (const t of allTools) {
    const list = groups.get(t.pluginId) ?? [];
    list.push(t);
    groups.set(t.pluginId, list);
  }
  const pluginName = (id: string): string =>
    id === "builtin" ? "Chat (built-in)" : plugins.find((p) => p.id === id)?.name ?? id;

  return (
    <SectionCard title="Default Tools" badge={`${allTools.length} tools`}>
      {allTools.length === 0 ? (
        <div style={{ fontSize: 12, opacity: 0.6 }}>
          No default tools yet. Enable a plugin that provides tools (e.g. Alarms or Calendar).
        </div>
      ) : (
        <div style={{ marginBottom: 10 }}>
          {[...groups.entries()].map(([pluginId, list]) => (
            <div key={pluginId} style={{ marginBottom: 8 }}>
              <div className="item-meta" style={{ marginBottom: 4, opacity: 0.7 }}>
                {pluginName(pluginId)} ({pluginId})
              </div>
              {list.map((t) => (
                <div key={`${pluginId}__${t.name}`} className="item-row">
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="item-title">{pluginId === "builtin" ? t.name : `${pluginId}__${t.name}`}</div>
                    <div className="item-meta">
                      {t.description ? t.description.slice(0, 160) : ""}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
      <div style={{ fontSize: 12, opacity: 0.6 }}>
        These tools are always offered to the avatar. External MCP servers below add more.
      </div>
    </SectionCard>
  );
}

export function McpSection({ ctx }: { ctx: PluginContext }) {
  const { ui } = ctx;
  const { SectionCard, FormField, ConfirmModal, Button, TextInput, Select } = ui;
  const { servers, status, tools, error, setError, refresh, refreshTools, api } = useMcpServers(ctx);

  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [calling, setCalling] = useState<string | null>(null);
  const [callResult, setCallResult] = useState<string | null>(null);
  const [toolArgs, setToolArgs] = useState<Record<string, string>>({});

  const [name, setName] = useState("");
  const [transport, setTransport] = useState<"stdio" | "http">("stdio");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [cwd, setCwd] = useState("");
  const [envRows, setEnvRows] = useState<KvRow[]>([{ key: "", value: "" }]);
  const [url, setUrl] = useState("");
  const [headerRows, setHeaderRows] = useState<KvRow[]>([{ key: "", value: "" }]);
  const [timeoutMs, setTimeoutMs] = useState("");
  const [oauthSelect, setOauthSelect] = useState<"auto" | "object" | "false">("auto");
  const [oauthClientId, setOauthClientId] = useState("");
  const [oauthClientSecret, setOauthClientSecret] = useState("");
  const [oauthScope, setOauthScope] = useState("");
  const [oauthCallbackPort, setOauthCallbackPort] = useState("");
  const [oauthRedirectUri, setOauthRedirectUri] = useState("");
  const [saving, setSaving] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);

  const isStdioForm = editingId
    ? servers.find((s) => s.id === editingId)?.transport === "stdio"
    : transport === "stdio";

  const resetForm = () => {
    setName("");
    setTransport("stdio");
    setCommand("");
    setArgs("");
    setCwd("");
    setEnvRows([{ key: "", value: "" }]);
    setUrl("");
    setHeaderRows([{ key: "", value: "" }]);
    setTimeoutMs("");
    setOauthSelect("auto");
    setOauthClientId("");
    setOauthClientSecret("");
    setOauthScope("");
    setOauthCallbackPort("");
    setOauthRedirectUri("");
    setEditingId(null);
    setFormOpen(false);
  };

  const openAddForm = () => {
    resetForm();
    setFormOpen(true);
  };

  const startEdit = (s: McpServerConfig) => {
    setEditingId(s.id);
    setFormOpen(true);
    setName(s.name);
    setTransport(s.transport);
    setCommand(s.command);
    setArgs(s.args.join(" "));
    setCwd(s.cwd ?? "");
    setEnvRows(kvRows(s.env));
    setUrl(s.url);
    setHeaderRows(kvRows(s.headers));
    setTimeoutMs(s.timeout_ms != null ? String(s.timeout_ms) : "");
    setOauthSelect(oauthMode(s.oauth));
    const o = s.oauth && typeof s.oauth === "object" ? s.oauth : null;
    setOauthClientId(o?.client_id ?? "");
    setOauthClientSecret(o?.client_secret ?? "");
    setOauthScope(o?.scope ?? "");
    setOauthCallbackPort(o?.callback_port != null ? String(o.callback_port) : "");
    setOauthRedirectUri(o?.redirect_uri ?? "");
    setError(null);
  };

  const oauthPayload = (): unknown | null | undefined => {
    if (transport !== "http") return undefined;
    if (oauthSelect === "auto") return null;
    if (oauthSelect === "false") return false;
    const cfg: Record<string, unknown> = {};
    if (oauthClientId.trim()) cfg.client_id = oauthClientId.trim();
    if (oauthClientSecret) cfg.client_secret = oauthClientSecret;
    if (oauthScope.trim()) cfg.scope = oauthScope.trim();
    const port = parseInt(oauthCallbackPort, 10);
    if (Number.isFinite(port) && port >= 1 && port <= 65535) cfg.callback_port = port;
    if (oauthRedirectUri.trim()) cfg.redirect_uri = oauthRedirectUri.trim();
    return cfg;
  };

  const timeoutPayload = (): number | null | undefined => {
    const t = timeoutMs.trim();
    if (!t) return null;
    const n = parseInt(t, 10);
    return Number.isFinite(n) && n > 0 ? n : null;
  };

  const handleSave = async () => {
    if (!name.trim()) return;
    setSaving(true);
    setError(null);
    try {
      if (editingId) {
        await api.updateServer(editingId, {
          name: name.trim(),
          command: command.trim(),
          args: args.split(/\s+/).filter(Boolean),
          cwd: cwd.trim(),
          env: kvMap(envRows),
          url: url.trim(),
          headers: kvMap(headerRows),
          timeout_ms: timeoutPayload(),
          oauth: oauthPayload(),
        });
      } else {
        await api.addServer({
          name: name.trim(),
          transport,
          command: command.trim(),
          args: args.split(/\s+/).filter(Boolean),
          cwd: cwd.trim(),
          env: kvMap(envRows),
          url: url.trim(),
          headers: kvMap(headerRows),
          timeout_ms: timeoutPayload(),
          oauth: oauthPayload(),
        });
      }
      resetForm();
      await refresh();
      await refreshTools();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setDeleting(true);
    try {
      await api.removeServer(pendingDelete);
      setPendingDelete(null);
      await refresh();
      await refreshTools();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleting(false);
    }
  };

  const toggleEnabled = async (id: string, enabled: boolean) => {
    try {
      await api.setServerEnabled(id, enabled);
      await refresh();
      await refreshTools();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleCall = async (t: McpToolInfo) => {
    const key = `${t.server_id}__${t.name}`;
    setCalling(key);
    setCallResult(null);
    setError(null);
    try {
      const raw = (toolArgs[key] ?? "").trim();
      const parsed = raw ? JSON.parse(raw) : {};
      const r = await api.callTool(t.server_id, t.name, parsed);
      setCallResult(r.is_error ? `error: ${r.content}` : r.content);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCalling(null);
    }
  };

  const handleStop = async (id: string) => {
    try {
      await api.stopServer(id);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const configSummary = (s: McpServerConfig): string => {
    const bits: string[] = [s.transport === "stdio" ? "local" : "remote"];
    if (s.transport === "stdio") {
      const cmd = `${s.command} ${s.args.join(" ")}`.trim();
      if (cmd) bits.push(cmd);
      if (s.cwd) bits.push(`cwd=${s.cwd}`);
      const envCount = Object.keys(s.env ?? {}).length;
      if (envCount) bits.push(`${envCount} env`);
    } else {
      if (s.url) bits.push(s.url);
      const hCount = Object.keys(s.headers ?? {}).length;
      if (hCount) bits.push(`${hCount} headers`);
      bits.push(`oauth=${oauthMode(s.oauth)}`);
    }
    if (s.timeout_ms != null) bits.push(`timeout=${s.timeout_ms}ms`);
    return bits.join(" • ");
  };

  return (
    <>
      <DefaultToolsCard ui={ui} />
      <SectionCard
        title="MCP Servers"
        badge={`${servers.filter((s) => s.enabled).length}/${servers.length} enabled`}
        action={
          <Button variant="primary" onClick={openAddForm} style={{ padding: "5px 10px", fontSize: 11 }}>
            + Add MCP server
          </Button>
        }
      >
        {servers.length === 0 && (
          <div style={{ fontSize: 12, opacity: 0.6, marginBottom: 10 }}>
            No MCP servers yet. Add a local command or remote endpoint below.
          </div>
        )}
        <div style={{ marginBottom: 10 }}>
          {servers.map((s) => {
            const st = status[s.id];
            const badge = !s.enabled ? "disabled" : st == null ? "…" : st.connected ? `${st.tool_count} tools` : "unreachable";
            return (
              <div key={s.id} className="item-row" style={{ display: "block" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="item-title">{s.name}</div>
                    <div className="item-meta">
                      {configSummary(s)} • {badge}
                      {st?.last_error ? ` • ${st.last_error.slice(0, 120)}` : ""}
                    </div>
                  </div>
                  <div style={{ display: "flex", gap: 6, flexShrink: 0 }}>
                    <Button
                      variant="secondary"
                      onClick={() => toggleEnabled(s.id, !s.enabled)}
                      style={{ padding: "5px 10px", fontSize: 11 }}
                    >
                      {s.enabled ? "Disable" : "Enable"}
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => startEdit(s)}
                      style={{ padding: "5px 10px", fontSize: 11 }}
                    >
                      Edit
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => handleStop(s.id)}
                      style={{ padding: "5px 10px", fontSize: 11 }}
                    >
                      Stop
                    </Button>
                    <Button
                      variant="danger"
                      onClick={() => setPendingDelete(s.id)}
                      style={{ padding: "5px 10px", fontSize: 11 }}
                    >
                      Delete
                    </Button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
        {formOpen && (
          <ServerFormModal
            ui={ui}
            title={editingId ? "Edit MCP server" : "Add MCP server"}
            busy={saving}
            canSave={!!name.trim()}
            saveLabel={editingId ? "Save changes" : "Add server"}
            onSave={handleSave}
            onClose={() => { if (!saving) resetForm(); }}
          >
            <FormField label="Name">
              <TextInput value={name} maxLength={64} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setName(e.target.value)} placeholder="e.g. local-tools" />
            </FormField>
            {!editingId && (
              <FormField label="Type" hint="Local runs a command via stdio. Remote connects to an HTTP endpoint.">
                <Select value={transport} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setTransport(e.target.value as "stdio" | "http")}>
                  <option value="stdio">local (stdio command)</option>
                  <option value="http">remote (http endpoint)</option>
                </Select>
              </FormField>
            )}
            {isStdioForm ? (
              <>
                <FormField label="Command" hint="Binary on PATH, e.g. npx or python3.">
                  <TextInput value={command} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setCommand(e.target.value)} placeholder="npx" />
                </FormField>
                <FormField label="Args" hint="Space-separated, e.g. -y @modelcontextprotocol/server-time">
                  <TextInput value={args} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setArgs(e.target.value)} placeholder="-y @modelcontextprotocol/server-time" />
                </FormField>
                <FormField label="Working directory" hint="cwd for the server process. Empty = app default.">
                  <TextInput value={cwd} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setCwd(e.target.value)} placeholder="/path/to/workspace" />
                </FormField>
                <FormField label="Environment" hint="Key/value pairs passed to the server process.">
                  <KeyValueEditor ui={ui} rows={envRows} onChange={setEnvRows} keyPlaceholder="KEY" valuePlaceholder="value" />
                </FormField>
              </>
            ) : (
              <>
                <FormField label="URL" hint="Streamable HTTP endpoint, e.g. http://localhost:8000/mcp">
                  <TextInput value={url} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setUrl(e.target.value)} placeholder="http://localhost:8000/mcp" />
                </FormField>
                <FormField label="Headers" hint="Key/value request headers.">
                  <KeyValueEditor ui={ui} rows={headerRows} onChange={setHeaderRows} keyPlaceholder="Name" valuePlaceholder="Value" />
                </FormField>
                <FormField label="OAuth" hint="auto = detect, object = custom client, false = disable.">
                  <Select value={oauthSelect} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setOauthSelect(e.target.value as "auto" | "object" | "false")}>
                    <option value="auto">auto (default)</option>
                    <option value="object">custom OAuth client</option>
                    <option value="false">disabled</option>
                  </Select>
                </FormField>
                {oauthSelect === "object" && (
                  <>
                    <FormField label="OAuth client ID">
                      <TextInput value={oauthClientId} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setOauthClientId(e.target.value)} placeholder="client id" />
                    </FormField>
                    <FormField label="OAuth client secret">
                      <TextInput value={oauthClientSecret} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setOauthClientSecret(e.target.value)} placeholder="client secret" />
                    </FormField>
                    <FormField label="OAuth scope">
                      <TextInput value={oauthScope} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setOauthScope(e.target.value)} placeholder="openid profile" />
                    </FormField>
                    <FormField label="OAuth callback port" hint="Default 19876. Empty = default.">
                      <TextInput value={oauthCallbackPort} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setOauthCallbackPort(e.target.value)} placeholder="19876" />
                    </FormField>
                    <FormField label="OAuth redirect URI" hint="Overrides callback port when set.">
                      <TextInput value={oauthRedirectUri} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setOauthRedirectUri(e.target.value)} placeholder="http://127.0.0.1:19876/mcp/oauth/callback" />
                    </FormField>
                  </>
                )}
              </>
            )}
            <FormField label="Request timeout" hint="Timeout in ms per MCP request. Empty = 30000ms default.">
              <TextInput value={timeoutMs} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setTimeoutMs(e.target.value)} placeholder="5000" />
            </FormField>
            {error && <div className="error-banner">{error}</div>}
          </ServerFormModal>
        )}
        {!formOpen && error && <div className="error-banner">{error}</div>}
        {pendingDelete && (
          <ConfirmModal
            title="Delete MCP server?"
            message={`Delete "${servers.find((s) => s.id === pendingDelete)?.name ?? pendingDelete}"? Tools from this server stop working.`}
            busy={deleting}
            onClose={() => !deleting && setPendingDelete(null)}
            onConfirm={confirmDelete}
          />
        )}
      </SectionCard>

      <SectionCard title="MCP Tools" badge={`${tools.length} tools`}>
        {tools.length === 0 ? (
          <div style={{ fontSize: 12, opacity: 0.6 }}>No tools discovered. Enable a server above.</div>
        ) : (
          <div style={{ marginBottom: 10 }}>
            {tools.map((t) => {
              const key = `${t.server_id}__${t.name}`;
              return (
                <div key={key} className="item-row" style={{ display: "block" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <div style={{ minWidth: 0, flex: 1 }}>
                      <div className="item-title">{t.name}</div>
                      <div className="item-meta">
                        {t.server_id}
                        {t.description ? ` • ${t.description.slice(0, 140)}` : ""}
                      </div>
                    </div>
                    <Button
                      variant="primary"
                      onClick={() => handleCall(t)}
                      disabled={calling === key}
                      style={{ padding: "5px 10px", fontSize: 11, flexShrink: 0 }}
                    >
                      {calling === key ? "Calling…" : "Call"}
                    </Button>
                  </div>
                  <div style={{ marginTop: 6 }}>
                    <TextInput
                      value={toolArgs[key] ?? ""}
                      onChange={(e: React.ChangeEvent<HTMLInputElement>) => setToolArgs((p) => ({ ...p, [key]: e.target.value }))}
                      placeholder='Args JSON, e.g. {"url": "https://example.com"}'
                      style={{ width: "100%", fontSize: 12 }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        )}
        {callResult && (
          <div style={{ fontSize: 12, whiteSpace: "pre-wrap", opacity: 0.85, maxHeight: 200, overflowY: "auto" }}>
            {callResult}
          </div>
        )}
      </SectionCard>
    </>
  );
}
