// Aliased from "react/jsx-runtime" and "react/jsx-dev-runtime" when building
// a plugin (automatic JSX). React comes from the host via
// globalThis.__companion_host__.

const host = globalThis.__companion_host__;

if (!host || !host.React) {
  throw new Error(
    "companion plugin runtime not present: this bundle must be activated by the companion host, not loaded directly",
  );
}

const R = host.React;

// React 19 exports jsx/jsxs directly; fall back to createElement for hosts
// that don't.
const jsxImpl =
  typeof R.jsx === "function"
    ? R.jsx
    : (type, props, key) =>
        R.createElement(type, key === undefined ? props : { ...props, key }, props?.children);
const jsxsImpl =
  typeof R.jsxs === "function"
    ? R.jsxs
    : (type, props, key) =>
        R.createElement(type, key === undefined ? props : { ...props, key }, props?.children);

export const jsx = jsxImpl;
export const jsxs = jsxsImpl;
export const jsxDEV = jsxImpl;
export const Fragment = R.Fragment;
