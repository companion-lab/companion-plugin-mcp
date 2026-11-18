import { describe, expect, it, vi } from "vitest";
import { makeMcpClient } from "./client";

// client.ts only imports types (PluginContext, ./types), so a loose fake ctx
// with a mock backend.call is enough to drive the real implementation.
type AnyCtx = Parameters<typeof makeMcpClient>[0];

function fakeCtx() {
  // Default reply shaped so every unwrap ({ server } / { servers } / { tools })
  // works; per-test mockResolvedValueOnce overrides it when the return value
  // itself is asserted.
  const call = vi.fn().mockResolvedValue({ server: {}, servers: [], tools: [] });
  const ctx = { backend: { call } } as unknown as AnyCtx;
  return { call, client: makeMcpClient(ctx) };
}

describe("listServers", () => {
  it("calls mcp.list_servers with no payload and unwraps { servers }", async () => {
    const { call, client } = fakeCtx();
    const servers = [{ name: "s" }];
    call.mockResolvedValueOnce({ servers });
    await expect(client.listServers()).resolves.toBe(servers);
    // Exactly one argument: no payload object is passed at all.
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("mcp.list_servers");
  });
});

describe("addServer", () => {
  it("fills empty defaults for missing fields and unwraps { server }", async () => {
    const { call, client } = fakeCtx();
    const server = { name: "n" };
    call.mockResolvedValueOnce({ server });
    await expect(client.addServer({ name: "n", transport: "stdio" })).resolves.toBe(server);
    // Missing optional fields become "" / [] / {} / null — never undefined,
    // so every key survives JSON encoding toward the Tauri backend.
    expect(call).toHaveBeenCalledWith("mcp.add_server", {
      name: "n",
      transport: "stdio",
      command: "",
      args: [],
      cwd: "",
      env: {},
      url: "",
      headers: {},
      timeout_ms: null,
      oauth: null,
    });
  });

  it("passes every provided field through unchanged", async () => {
    const { call, client } = fakeCtx();
    const input = {
      name: "n",
      transport: "http",
      command: "cmd",
      args: ["-x"],
      cwd: "/tmp",
      env: { A: "1" },
      url: "http://x",
      headers: { H: "v" },
      timeout_ms: 5000,
      oauth: { k: "v" },
    };
    await client.addServer(input);
    expect(call).toHaveBeenCalledWith("mcp.add_server", input);
  });

  it("keeps explicit nulls for timeout_ms/oauth", async () => {
    const { call, client } = fakeCtx();
    await client.addServer({ name: "n", transport: "stdio", timeout_ms: null, oauth: null });
    const payload = call.mock.calls[0][1] as Record<string, unknown>;
    // ?? null only replaces null/undefined, so explicit nulls stay null.
    expect(payload.timeout_ms).toBeNull();
    expect(payload.oauth).toBeNull();
  });
});

describe("updateServer", () => {
  it("spreads the input after id and unwraps { server }", async () => {
    const { call, client } = fakeCtx();
    const server = { name: "renamed" };
    call.mockResolvedValueOnce({ server });
    await expect(client.updateServer("srv1", { name: "renamed", require_approval: true })).resolves.toBe(server);
    expect(call).toHaveBeenCalledWith("mcp.update_server", {
      id: "srv1",
      name: "renamed",
      require_approval: true,
    });
  });

  it("leaves undefined fields on the payload, which JSON drops (= keep current)", async () => {
    const { call, client } = fakeCtx();
    await client.updateServer("srv1", { name: "keep", timeout_ms: undefined, oauth: undefined });
    const payload = call.mock.calls[0][1] as Record<string, unknown>;
    // The plain spread puts the keys on the object with undefined values…
    expect("timeout_ms" in payload).toBe(true);
    expect(payload.timeout_ms).toBeUndefined();
    // …and JSON encoding (Tauri invoke) then drops them entirely, so the
    // backend never sees the field and keeps its current value.
    expect(JSON.stringify(payload)).toBe(JSON.stringify({ id: "srv1", name: "keep" }));
  });

  it("sends explicit null so the backend clears the field", async () => {
    const { call, client } = fakeCtx();
    await client.updateServer("srv1", { timeout_ms: null });
    const payload = call.mock.calls[0][1] as Record<string, unknown>;
    expect(payload.timeout_ms).toBeNull();
    // null survives JSON encoding — that is what makes "clear" expressible.
    expect(JSON.stringify(payload)).toBe(JSON.stringify({ id: "srv1", timeout_ms: null }));
  });
});

describe("passthrough methods", () => {
  it("removeServer sends { id } and returns the raw backend reply", async () => {
    const { call, client } = fakeCtx();
    call.mockResolvedValueOnce("raw");
    await expect(client.removeServer("srv")).resolves.toBe("raw");
    expect(call).toHaveBeenCalledWith("mcp.remove_server", { id: "srv" });
  });

  it("setServerEnabled sends { id, enabled }", async () => {
    const { call, client } = fakeCtx();
    await client.setServerEnabled("srv", false);
    expect(call).toHaveBeenCalledWith("mcp.set_enabled", { id: "srv", enabled: false });
  });

  it("serverStatus returns the backend array as-is", async () => {
    const { call, client } = fakeCtx();
    const statuses = [{ name: "s", connected: true }];
    call.mockResolvedValueOnce(statuses);
    await expect(client.serverStatus()).resolves.toBe(statuses);
    expect(call).toHaveBeenCalledWith("mcp.status");
  });

  it("listTools filters by { server_id } when given, else sends undefined", async () => {
    const { call, client } = fakeCtx();
    const tools = [{ name: "t" }];
    call.mockResolvedValueOnce({ tools });
    await expect(client.listTools("s1")).resolves.toBe(tools);
    expect(call).toHaveBeenCalledWith("mcp.list_tools", { server_id: "s1" });

    call.mockResolvedValueOnce({ tools: [] });
    await client.listTools();
    // No filter → the payload argument is explicitly undefined.
    expect(call).toHaveBeenLastCalledWith("mcp.list_tools", undefined);
  });

  it("callTool defaults missing args to {} and returns the raw result", async () => {
    const { call, client } = fakeCtx();
    const result = { content: "ok", is_error: false };
    call.mockResolvedValueOnce(result);
    await expect(client.callTool("srv", "tool_name", { q: 1 })).resolves.toBe(result);
    expect(call).toHaveBeenCalledWith("mcp.call_tool", {
      server_id: "srv",
      tool: "tool_name",
      args: { q: 1 },
    });

    const fallback = { content: "", is_error: false };
    call.mockResolvedValueOnce(fallback);
    await expect(client.callTool("srv", "tool_name")).resolves.toBe(fallback);
    // args ?? {} keeps the args key present with an empty object.
    expect(call).toHaveBeenLastCalledWith("mcp.call_tool", {
      server_id: "srv",
      tool: "tool_name",
      args: {},
    });
  });

  it("stopServer sends { id }", async () => {
    const { call, client } = fakeCtx();
    await client.stopServer("srv");
    expect(call).toHaveBeenCalledWith("mcp.stop_server", { id: "srv" });
  });
});
