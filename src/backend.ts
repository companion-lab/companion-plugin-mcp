// Backend access layer: typed wrappers over ctx.backend.call plus a hook
// replacement for the state block the host's McpSection kept inline
// (servers/status/tools + refresh-on-"mcp-servers-changed"), following the
// alarms/calendar plugin pattern.

import { useCallback, useEffect, useState } from "react";
import type { PluginContext } from "../../../sdk/js/@companion/plugin";
import { makeMcpClient, type McpClient } from "./client";
import type { McpServerConfig, McpServerStatus, McpToolInfo } from "./types";

export function makeMcpApi(ctx: PluginContext): McpClient {
  return makeMcpClient(ctx);
}

export type McpApi = ReturnType<typeof makeMcpApi>;

/**
 * The host section's data flow, hooked: servers + live status + discovered
 * tools, `refresh`/`refreshTools` re-runs, and auto-refresh whenever the
 * backend emits "mcp-servers-changed" (arrives namespaced as
 * `plugin:mcp:mcp-servers-changed`, which ctx.events.on subscribes to).
 */
export function useMcpServers(ctx: PluginContext) {
  const [servers, setServers] = useState<McpServerConfig[]>([]);
  const [status, setStatus] = useState<Record<string, McpServerStatus>>({});
  const [tools, setTools] = useState<McpToolInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  // The api object is cheap; keep a stable ref so hooks below stay put.
  const api = makeMcpApi(ctx);

  const refresh = useCallback(async () => {
    try {
      const [svrs, st] = await Promise.all([
        api.listServers(),
        api.serverStatus().catch(() => []),
      ]);
      setServers(svrs);
      const map: Record<string, McpServerStatus> = {};
      for (const s of st) map[s.id] = s;
      setStatus(map);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const refreshTools = useCallback(async () => {
    try {
      setTools(await api.listTools());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void refresh();
    void refreshTools();
    // Backend pushes this on any config mutation (add/update/remove/enable).
    return ctx.events.on("mcp-servers-changed", () => {
      void refresh();
      void refreshTools();
    });
  }, [refresh, refreshTools, ctx]);

  return { servers, status, tools, error, setError, refresh, refreshTools, api };
}
