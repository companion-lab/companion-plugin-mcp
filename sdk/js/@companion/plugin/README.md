# @companion/plugin

Authoring SDK for [companion-tauri](https://github.com/companion-lab) plugin
frontends — TypeScript types, the `definePlugin` helper, and the host-bridge
shims that let a plugin bundle share the app's React.

A plugin frontend is a bundle exporting `activate(ctx)` (or
`definePlugin((ctx) => …)`). The companion host imports the bundle as a
module, calls `activate` with a scoped `PluginContext`, and everything the
plugin registers streams into the app's contribution registry.

> **Important:** a plugin bundle never bundles its own React. The shims
> (`react.js`, `jsx-runtime.js`) read React from the host app via
> `globalThis.__companion_host__` — importing the bundle outside the
> companion host throws. Build plugins with companion's plugin CLI, which
> aliases `react` / `react/jsx-runtime` / `react-dom` to these shims.

## Install

```bash
npm install @companion/plugin
```

(Inside the companion repo, plugins use a `file:` dependency on
`sdk/js/@companion/plugin` instead.)

## Quick start

```tsx
import { definePlugin } from "@companion/plugin";
import { useState } from "react"; // aliased to the host's React at build time

export default definePlugin((ctx) => {
  ctx.settings.registerTab({
    id: "notes",
    label: "Notes",
    group: "Plugins",
    desc: "My plugin tab",
  });
  ctx.settings.registerSection({
    id: "notes.list",
    tabId: "notes",
    slot: "top",
    component: function NotesSection() {
      const [notes, setNotes] = useState<string[]>([]);
      return (
        <button onClick={() => ctx.backend.call("notes.add", { text: "hi" })}>
          Add note
        </button>
      );
    },
  });

  ctx.tools.register({
    name: "save_note",
    description: "Save a note for the user",
    inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] },
    execute: async (args) => {
      await ctx.backend.call("notes.add", args);
      return "saved";
    },
  });
});
```

Pair the frontend with a Rust backend (see
[`companion-plugin-sdk`](https://github.com/companion-lab/companion-sdk-rs))
that answers the `notes.*` methods.

## The PluginContext

| Surface            | Purpose                                                            |
| ------------------ | ------------------------------------------------------------------ |
| `ctx.settings`     | Register settings tabs, sections, overrides (settings window).      |
| `ctx.main`         | Register main-window widgets (`main.backdrop/top/bottom/overlay`), context-menu items, widget overrides. |
| `ctx.tools`        | Tools offered to the chat model; calls route through the host.      |
| `ctx.systemPrompt` | Instruction fragments added to the chat system prompt.              |
| `ctx.backend`      | `call(method, payload)` into the plugin's Rust backend.             |
| `ctx.events`       | Namespaced pub/sub (`plugin:{id}:{event}`) across windows.          |
| `ctx.storage`      | Namespaced key/value store persisted by the host.                   |
| `ctx.avatar`       | Drive the live avatar (emotions, motions, model loads) or provide the engine side. |
| `ctx.services`     | Publish/consume TTS & STT providers.                                |
| `ctx.secrets`      | OS keychain scoped to the plugin.                                   |
| `ctx.ui`           | The host's shared UI kit (buttons, modals, cards …).                |

Disabling or uninstalling a plugin removes all of its contributions and
service providers live — no restart needed for frontend surfaces.

## Building

Use companion's plugin CLI (esbuild-based) so the host-bridge aliases are
applied; the resulting `frontend.js` ships next to the plugin's
`manifest.json`. See PLUGINS.md in the companion app repo for the full
package format.

## License

MIT
