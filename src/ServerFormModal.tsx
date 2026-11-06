// ServerFormModal (copied from the host's ui/molecules/ServerFormModal.tsx).
// Not part of the shared ctx.ui kit, so it lives in the plugin and takes the
// kit pieces it needs via the `ui` prop.

import { useEffect } from "react";
// Portal via the local host-compat shim: the host shares only
// react-dom/client, whose React 19 entry no longer carries createPortal.
import { createPortal } from "./vendor/react-dom-compat.js";
import type { PluginContext } from "../../sdk/js/@companion/plugin";
import type { ReactNode } from "react";

type Props = {
  ui: PluginContext["ui"];
  title: string;
  busy?: boolean;
  canSave: boolean;
  saveLabel?: string;
  onSave: () => void;
  onClose: () => void;
  children: ReactNode;
  /** Optional destructive action (e.g. Delete), rendered left of Cancel. */
  danger?: ReactNode;
};

export function ServerFormModal({
  ui,
  title,
  busy,
  canSave,
  saveLabel = "Add server",
  onSave,
  onClose,
  children,
  danger,
}: Props) {
  const { Button } = ui;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  return createPortal(
    <div
      className="modal-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="modal-card modal-card-wide"
      >
        <div className="modal-head">
          <div className="modal-title">{title}</div>
          <button
            type="button"
            className="modal-x"
            aria-label="Close"
            disabled={busy}
            onClick={onClose}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round"><path d="M18 6 6 18M6 6l12 12" /></svg>
          </button>
        </div>
        <div className="modal-form">{children}</div>
        <div className="modal-actions">
          {danger}
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" onClick={onSave} disabled={busy || !canSave}>
            {busy ? "Saving…" : saveLabel}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
