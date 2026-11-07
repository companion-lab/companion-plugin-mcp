// Thin re-wrap of ctx.backend.call, ported from the host's src/mcp/client.ts.
// The host version invoked `mcp_*` Tauri commands directly; each is replaced
// here by the corresponding `mcp.*` plugin backend method (same payloads and
// reply shapes — see features/mcp/backend/src/lib.rs `route`).
//
// Serialization notes that matter for `mcp.update_server`: the call goes over
// Tauri invoke, which JSON-encodes the payload, so `undefined` fields are
// dropped (= keep current value) while explicit `null` for timeout_ms / oauth
// clears them.

import type { PluginContext } from "../../../sdk/js/@companion/plugin";
import type { McpCallResult, McpServerConfig, McpServerStatus, McpToolInfo } from "./types";

export type AddServerInput = {
  name: string;
  transport: string;
  command?: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
  timeout_ms?: number | null;
  oauth?: unknown;
};

export type UpdateServerInput = {
  name?: string;
  command?: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
  timeout_ms?: number | null | undefined;
  oauth?: unknown | null | undefined;
  require_approval?: boolean;
};

export function makeMcpClient(ctx: PluginContext) {
  return {
    /** `mcp.list_servers` → { servers } */
    listServers: (): Promise<McpServerConfig[]> =>
      ctx.backend
        .call<{ servers: McpServerConfig[] }>("mcp.list_servers")
        .then((r) => r.servers),

    /** `mcp.add_server` → { server } */
    addServer: (input: AddServerInput): Promise<McpServerConfig> =>
      ctx.backend
        .call<{ server: McpServerConfig }>("mcp.add_server", {
          name: input.name,
          transport: input.transport,
          command: input.command ?? "",
          args: input.args ?? [],
          cwd: input.cwd ?? "",
          env: input.env ?? {},
          url: input.url ?? "",
          headers: input.headers ?? {},
          timeout_ms: input.timeout_ms ?? null,
          oauth: input.oauth ?? null,
        })
        .then((r) => r.server),

    /** `mcp.update_server` → { server } (timeout_ms/oauth accept null to clear) */
    updateServer: (id: string, input: UpdateServerInput): Promise<McpServerConfig> =>
      ctx.backend
        .call<{ server: McpServerConfig }>("mcp.update_server", { id, ...input })
        .then((r) => r.server),

    /** `mcp.remove_server` */
    removeServer: (id: string): Promise<unknown> =>
      ctx.backend.call("mcp.remove_server", { id }),

    /** `mcp.set_enabled` */
    setServerEnabled: (id: string, enabled: boolean): Promise<unknown> =>
      ctx.backend.call("mcp.set_enabled", { id, enabled }),

    /** `mcp.status` → McpServerStatus[] */
    serverStatus: (): Promise<McpServerStatus[]> =>
      ctx.backend.call<McpServerStatus[]>("mcp.status"),

    /** `mcp.list_tools` → { tools } */
    listTools: (serverId?: string): Promise<McpToolInfo[]> =>
      ctx.backend
        .call<{ tools: McpToolInfo[] }>(
          "mcp.list_tools",
          serverId ? { server_id: serverId } : undefined,
        )
        .then((r) => r.tools),

    /** `mcp.call_tool` → { content, is_error } */
    callTool: (serverId: string, tool: string, args?: unknown): Promise<McpCallResult> =>
      ctx.backend.call<McpCallResult>("mcp.call_tool", {
        server_id: serverId,
        tool,
        args: args ?? {},
      }),

    /** `mcp.stop_server` */
    stopServer: (id: string): Promise<unknown> =>
      ctx.backend.call("mcp.stop_server", { id }),
  };
}

export type McpClient = ReturnType<typeof makeMcpClient>;
