// Types for the vendored host react-dom compat shim (the shim ships as plain
// JS; see react-dom-compat.js for how the portal element is recreated).

import type { ReactElement, ReactNode } from "react";

export function createPortal(
  children: ReactNode,
  container: Element | Document | DocumentFragment,
  key?: string | null,
): ReactElement;
