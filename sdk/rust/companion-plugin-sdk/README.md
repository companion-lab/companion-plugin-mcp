# companion-plugin-sdk

Rust backend SDK for [companion-tauri](https://github.com/companion-lab) plugins.

A plugin backend is a `cdylib` that implements the [`Plugin`] trait and exports
the C ABI through the `export_plugin!` macro. The companion host loads the
library, checks the ABI version, and routes every `plugin_call` from the
webviews to your `handle` method. All payloads and results are JSON —
everything that crosses the `dylib` boundary is plain `#[repr(C)]` data plus
UTF-8 JSON buffers.

## Quick start

```rust
use companion_plugin_sdk::{export_plugin, Plugin, PluginCtx};
use serde_json::{json, Value};

struct Hello;

impl Plugin for Hello {
    fn init(&mut self, _ctx: PluginCtx) -> Result<(), String> { Ok(()) }

    fn handle(&mut self, method: &str, payload: Value) -> Result<Value, String> {
        match method {
            "ping" => Ok(json!({ "pong": true })),
            _ => Err(format!("unknown method: {method}")),
        }
    }
}

export_plugin!(Hello);
```

`Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
companion-plugin-sdk = "0.1"
serde_json = "1"
```

Ship the produced `lib<name>.dylib` / `<name>.dll` / `lib<name>.so` next to the
plugin's `manifest.json`:

```json
{
  "id": "notes",
  "name": "Notes",
  "version": "0.1.0",
  "description": "Example plugin.",
  "abi": 2,
  "permissions": ["storage", "events"],
  "backend": { "library": "libnotes_backend.dylib" }
}
```

Drop the package into the app's `plugins/` directory (or import it through
**Settings → Plugins → Import plugin…**) and the host discovers it on the next
launch.

## What you get

- **`Plugin` trait** — `init`, `handle`, `tools`, `shutdown`. Tools are merged
  into the chat model's tool list; tool calls arrive as regular `handle`
  requests (conventionally method `"tool"` with `{"name": ..., "args": ...}`).
- **`PluginCtx`** — host services:
  - `data_dir()` — writable per-plugin directory
    (`{app_data}/plugins-data/{id}/`), created before `init`.
  - `plugin_dir()` — read-only package directory (manifest, bundles,
    bundled `assets/`).
  - `emit(event, payload)` — events delivered to every webview, namespaced
    as `plugin:{id}:{event}`.
  - `info` / `warn` / `error` — routed to the host console.
- **`ToolDef`** — tool definition merged into the chat tool list.
- **`export_plugin!`** — exports the C ABI entry points
  (`companion_plugin_abi_version` / `_create` / `_handle` / `_tools` /
  `_destroy` / `_free_buffer`).

## Concurrency & lifetime

`handle` calls are serialized by the host (one call at a time per backend), so
`&mut self` is safe. Calls run on a blocking-pool thread — a slow call delays
only its own plugin. Backends are loaded eagerly for enabled plugins and never
unloaded while the app runs; a `shutdown` is best-effort at process teardown.
If `init` fails, the host keeps the plugin alive as a `FailedPlugin` that
errors every call instead of crashing.

## ABI versioning

`ABI_VERSION` (currently **2** — v2 added the `data_dir`/`plugin_dir` path
callbacks) is checked by the host before anything else; plugins built against
a different ABI are rejected at load time. Bump the crate version whenever
`ABI_VERSION` changes and rebuild your plugin against the matching release.

## License

MIT
