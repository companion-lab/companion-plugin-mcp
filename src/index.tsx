// MCP feature plugin: "MCP" settings tab with the MCP servers/tools section.
// The MCP tools themselves are registered on the Rust backend side via
// `tools()` as `{server_id}__{tool_name}` and reach the chat loop through the
// host's normal tool dispatch — the section only manages servers.

import { definePlugin, type PluginContext } from "@companion-lab/plugin";
import { McpSection } from "./McpSection";

export default definePlugin((ctx: PluginContext) => {
  ctx.log.info("activating mcp plugin");

  // Settings: same tab and section ids the core used ("tools" / "tools.mcp"),
  // so the plugin takes over the core slot seamlessly; the label is the
  // plugin's own. Icon copied from the host's builtin.tsx CORE_TABS tools
  // entry.
  ctx.settings.registerTab({
    id: "tools",
    label: "MCP",
    group: "System",
    order: 30,
    desc: "MCP servers and their tools for the avatar.",
    icon: ({ className }: { className?: string }) => (
      <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" /></svg>
    ),
  });
  ctx.settings.registerSection({
    id: "tools.mcp",
    tabId: "tools",
    slot: "top",
    order: 100,
    component: function McpSectionViaCtx() {
      return <McpSection ctx={ctx} />;
    },
  });
});
