// Ported from the host's src/mcp/types.ts (verbatim shapes; the Rust backend
// serializes these structs 1:1).

export type McpTransport = "stdio" | "http";

export interface McpOAuthConfig {
  client_id?: string;
  client_secret?: string;
  scope?: string;
  callback_port?: number;
  redirect_uri?: string;
}

export type McpOAuthSetting = false | McpOAuthConfig;

export interface McpServerConfig {
  id: string;
  name: string;
  transport: McpTransport;
  enabled: boolean;
  command: string;
  args: string[];
  cwd: string;
  env: Record<string, string>;
  url: string;
  headers: Record<string, string>;
  require_approval: boolean;
  timeout_ms?: number | null;
  oauth?: McpOAuthSetting | null;
}

export interface McpToolInfo {
  server_id: string;
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
}

export interface McpServerStatus {
  id: string;
  connected: boolean;
  tool_count: number;
  last_error: string | null;
}

export interface McpCallResult {
  content: string;
  is_error: boolean;
}
