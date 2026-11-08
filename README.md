# companion-plugin-mcp

Model Context Protocol client: connect MCP servers and expose their tools to the avatar.

One of the official feature plugins for **Companion**, a plugin-based desktop
companion app (Tauri + React). A plugin is a `manifest.json` plus an optional
bundled React frontend and an optional Rust cdylib backend; see the
companion-tauri repository (`app/PLUGINS.md`) for the full plugin format and
host API documentation.

## Layout

| Path | Contents |
|---|---|
| `manifest.json` | Plugin metadata: id, ABI version, permissions, entry points |
| `src/` | React frontend source (bundled with esbuild at build time) |
| `backend/` | Rust cdylib backend (MCP server connections + tool bridge) |

React is never bundled into the plugin: `react` / `react-dom` are aliased to
the host's copies at build time, so plugin frontends always match the app.

## Building

Install dependencies first (pulls `@companion-lab/plugin` from npm and `companion-plugin-sdk` from crates.io):

```bash
npm install
```


The `companion-cli` CLI ([npm](https://www.npmjs.com/package/companion-cli),
install with `npm install -g companion-cli`) bundles the frontend
(and builds the Rust backend when present) into a zip importable via the app's
**Settings → Plugins → Import plugin…**:

```bash
companion-cli build /path/to/companion-plugin-mcp --backend
```

Install dependencies first if you use a package manager lockfile
(`pnpm install` / `npm install`).

## Downloading releases

CI builds installable zips on every push to `main` (the "Build plugin"
workflow). To publish a release, push a tag matching the version in
`manifest.json`:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

The release gets one zip per platform — download the one for your machine
and import it in the Companion app via **Settings → Plugins → Import
plugin…**:

| File | Platform |
|---|---|
| `mcp-0.1.0-linux-x86_64.zip` | Linux (Intel/AMD 64-bit) |
| `mcp-0.1.0-macos-arm64.zip` | macOS (Apple Silicon) |
| `mcp-0.1.0-macos-x86_64.zip` | macOS (Intel) |
| `mcp-0.1.0-windows-x86_64.zip` | Windows (64-bit) |

Non-tag runs of the workflow publish the same zips as run artifacts for
testing. CI never touches the zips committed under `dist/`.
