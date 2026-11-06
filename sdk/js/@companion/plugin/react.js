// Aliased from "react" when the build script bundles a plugin (see
// scripts/build-plugin.mjs). React itself comes from the host app via
// globalThis.__companion_host__ — plugins never bundle their own copy.

const host = globalThis.__companion_host__;

if (!host || !host.React) {
  throw new Error(
    "companion plugin runtime not present: this bundle must be activated by the companion host, not loaded directly",
  );
}

const React = host.React;

export default React;
export const {
  Children,
  Component,
  Fragment,
  PureComponent,
  StrictMode,
  Suspense,
  cloneElement,
  createContext,
  createElement,
  forwardRef,
  isValidElement,
  memo,
  useCallback,
  useContext,
  useDebugValue,
  useDeferredValue,
  useEffect,
  useId,
  useImperativeHandle,
  useInsertionEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
  useTransition,
} = React;
