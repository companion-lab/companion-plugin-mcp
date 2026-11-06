// Host react-dom compat for the vendored react-big-calendar bundle (see
// scripts/build-vendor.mjs) and for the plugin's own modal components, which
// need createPortal.
//
// The host shares only react-dom/client (createRoot/hydrateRoot); React 19
// dropped createPortal/findDOMNode from that entry. The host reconciler still
// renders portal elements though — it matches them structurally via the
// shared Symbol.for("react.portal") type (createFiberFromPortal in
// react-dom-client only touches children/key/containerInfo/implementation) —
// so createPortal below recreates exactly the element react-dom's own
// createPortal would produce, and the host renderer handles the rest.

const host = globalThis.__companion_host__;

if (!host || !host.ReactDOMClient) {
  throw new Error(
    "companion plugin runtime not present: this bundle must be activated by the companion host, not loaded directly",
  );
}

const ReactDOMClient = host.ReactDOMClient;

const REACT_PORTAL_TYPE = Symbol.for("react.portal");
const REACT_OPTIMISTIC_KEY = Symbol.for("react.optimistic_key");

export function createPortal(children, container, key = null) {
  if (
    !container ||
    (container.nodeType !== 1 &&
      container.nodeType !== 9 &&
      container.nodeType !== 11)
  ) {
    throw new Error("Target container is not a DOM element.");
  }
  return {
    $$typeof: REACT_PORTAL_TYPE,
    key:
      key == null
        ? null
        : key === REACT_OPTIMISTIC_KEY
          ? REACT_OPTIMISTIC_KEY
          : "" + key,
    children,
    containerInfo: container,
    implementation: null,
  };
}

// findDOMNode was removed in React 19. react-overlays calls it via
// safeFindDOMNode only for non-DOM refs and guards the result with `?? null`,
// so returning null matches the modern behavior.
export function findDOMNode() {
  return null;
}

const ReactDOM = { ...ReactDOMClient, createPortal, findDOMNode };
export default ReactDOM;
export const { createRoot, hydrateRoot } = ReactDOMClient;
