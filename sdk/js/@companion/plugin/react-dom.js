// Aliased from "react-dom" / "react-dom/client" when building a plugin.

const host = globalThis.__companion_host__;

if (!host || !host.ReactDOMClient) {
  throw new Error(
    "companion plugin runtime not present: this bundle must be activated by the companion host, not loaded directly",
  );
}

export default host.ReactDOMClient;
export const { createRoot, flushSync, hydrateRoot } = host.ReactDOMClient;
