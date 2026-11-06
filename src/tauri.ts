// Minimal vendored Tauri bridge, same pattern as the chat feature's
// ./tauri.ts: the plugin bundle cannot resolve @tauri-apps/api (features/ has
// no node_modules chain to the app's), so the primitives are read straight
// off window.__TAURI_INTERNALS__, mirroring @tauri-apps/api v2 core.js.
//
// Used ONLY for host-global surfaces the plugin context cannot reach:
// - plugin_list / plugin_list_tools (the default-tools view needs every
//   plugin's backend tools, not just this plugin's),
// - the host-global "plugins-changed" event, so the tools list refreshes
//   when a plugin is added/enabled/disabled in the Plugins tab.
// Everything else goes through ctx.*.

interface TauriInternals {
  invoke: (cmd: string, args?: Record<string, unknown>, options?: unknown) => Promise<unknown>;
  transformCallback: (callback?: (response: unknown) => void, once?: boolean) => number;
}

function internals(): TauriInternals | undefined {
  return typeof window !== "undefined"
    ? (window as unknown as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__
    : undefined;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Raw Tauri command invocation (escape hatch — prefer ctx.* surfaces). */
export function tauriInvoke<T = unknown>(
  cmd: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  const t = internals();
  if (!t || typeof t.invoke !== "function") {
    return Promise.reject(new Error("Tauri IPC unavailable (not running inside companion)"));
  }
  return t.invoke(cmd, args) as Promise<T>;
}

/** Listen to a host-global (non-plugin-namespaced) Tauri event. */
export function tauriListen<T = unknown>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  const t = internals();
  if (!t || typeof t.invoke !== "function" || typeof t.transformCallback !== "function") {
    return Promise.reject(new Error("Tauri IPC unavailable (not running inside companion)"));
  }
  const handlerId = t.transformCallback((e: unknown) => {
    handler((e as { payload?: T })?.payload as T);
  });
  return t
    .invoke("plugin:event|listen", { event, target: { kind: "Any" }, handler: handlerId })
    .then((eventId) => {
      return () => {
        void t
          .invoke("plugin:event|unlisten", { event, eventId })
          .catch(() => {});
      };
    });
}
