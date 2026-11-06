// @companion/plugin — authoring SDK for companion-tauri plugin frontends.
//
// A plugin bundle exports `activate(ctx)` (or uses definePlugin below). The
// host imports the bundle as a module, calls activate with a scoped
// PluginContext, and the registrations stream into the app's contribution
// registry.
//
// ```tsx
// import { definePlugin } from "@companion/plugin";
// import { useState } from "react"; // aliased to the host's React at build time
//
// export default definePlugin((ctx) => {
//   ctx.settings.registerTab({
//     id: "notes",
//     label: "Notes",
//     group: "Plugins",
//     desc: "My plugin tab",
//     component: NotesTab,
//   });
//   ctx.tools.register({
//     name: "save_note",
//     description: "Save a note for the user",
//     inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] },
//     execute: async (args) => {
//       await ctx.backend.call("notes.add", args);
//       return "saved";
//     },
//   });
// });
// ```

/** Read/write settings-page state (settings window only). */
export function useSettingsState() {
  const host = globalThis.__companion_host__;
  if (!host || !host.settings || !host.settings.useSettingsState) {
    throw new Error(
      "useSettingsState is only available inside the settings window",
    );
  }
  return host.settings.useSettingsState();
}

/** Wrap an activation function; also usable as the default export shape. */
export function definePlugin(
  activate: (ctx: PluginContext) => void | Promise<void>,
): { activate: (ctx: PluginContext) => void | Promise<void> } {
  return { activate };
}

// ---------------------------------------------------------------------------
// Context types (mirror of the host's src/plugins/types.ts)
// ---------------------------------------------------------------------------

import type { ComponentType, ReactNode } from "react";

export type SettingsTabContribution = {
  id: string;
  label: string;
  group: string;
  desc: string;
  icon?: (props: { className?: string }) => ReactNode;
  order?: number;
  component?: ComponentType;
};

export type SectionContribution = {
  id: string;
  tabId: string;
  slot: "top" | "bottom";
  order?: number;
  component: ComponentType;
};

/** `main.backdrop` widgets render behind the core interaction canvas:
 *  non-interactive pixel layers (the avatar engine's WebGL canvas). */
export type MainSlot = "main.backdrop" | "main.top" | "main.bottom" | "main.overlay";

export type WidgetContribution = {
  slot: MainSlot;
  order?: number;
  component: ComponentType;
};

export type WidgetTargetId = "chatPanel" | "alarmBanner" | "errorOverlay";

export type ContextMenuItemContribution = {
  id: string;
  label: string;
  icon?: ReactNode;
  danger?: boolean;
  order?: number;
  onSelect: () => void;
};

export type ToolContribution = {
  name: string;
  description: string;
  inputSchema?: Record<string, unknown>;
  execute: (args: unknown, signal?: AbortSignal) => Promise<string> | string;
};

/** Instruction text a plugin contributes to the chat system prompt. */
export type SystemPromptContribution = {
  /** Stable id within the plugin — re-registering replaces the fragment. */
  id: string;
  text: string;
  /** Sort weight among all plugin fragments (lower comes earlier). */
  order?: number;
};

// ---------------------------------------------------------------------------
// Avatar & service surfaces (host ABI-2 capabilities)
// ---------------------------------------------------------------------------

export type MotionDef = {
  name: string;
  pose: unknown;
  duration: number;
  hipsPos?: unknown;
  version?: number;
  description?: string;
};

export type EmotionDef = {
  name: string;
  preset: string;
  resetMs: number;
};

export type AvatarCapabilities = {
  emotions: string[];
  motions: string[];
};

export type AvatarSurface = {
  setEmotion: (name: string) => void;
  triggerMotion: (name: string) => void;
  setViseme: (name: string, weight?: number) => void;
  registerMotion: (def: MotionDef) => void;
  registerEmotion: (def: EmotionDef) => void;
  loadModel: (source: { id?: string; base64?: string }) => Promise<void>;
  loadVrmaBase64: (
    data: string,
    name: string,
  ) => Promise<{
    name: string;
    duration: number;
    pose: unknown;
    hipsPos?: unknown;
    version: number;
    expressions: Array<{ preset: string }>;
  }>;
  reloadLibrary: (
    motions: Array<{
      name: string;
      pose: unknown;
      duration: number;
      hipsPos?: unknown;
      version?: number;
      description?: string;
    }>,
    emotions: Array<{ name: string; preset: string; resetMs: number }>,
  ) => void;
  describeCapabilities: () => AvatarCapabilities;
  /**
   * Install the live avatar handlers (the engine side of the bus). The
   * provider is whatever plugin owns the avatar runtime (currently the vrm
   * plugin); last install wins, missing entries stay no-ops, and calls made
   * before any install are queued and replayed on install.
   */
  provide: (handlers: AvatarEngineHandlers) => void;
};

/** Handler set the avatar engine provider installs via `avatar.provide`. */
export type AvatarEngineHandlers = {
  setEmotion?: (name: string) => void;
  triggerMotion?: (name: string) => void;
  setViseme?: (name: string, weight?: number) => void;
  registerMotion?: (def: MotionDef) => void;
  registerEmotion?: (def: EmotionDef) => void;
  loadModel?: (source: { id?: string; base64?: string }) => Promise<void>;
  loadVrmaBase64?: (
    data: string,
    name: string,
  ) => Promise<{
    name: string;
    duration: number;
    pose: unknown;
    hipsPos?: unknown;
    version: number;
    expressions: Array<{ preset: string }>;
  }>;
  reloadLibrary?: (
    motions: Array<{
      name: string;
      pose: unknown;
      duration: number;
      hipsPos?: unknown;
      version?: number;
      description?: string;
    }>,
    emotions: Array<{ name: string; preset: string; resetMs: number }>,
  ) => void;
  describeCapabilities?: () => AvatarCapabilities;
  pixelAlphaAt?: (cssX: number, cssY: number) => number | null;
};

export type TtsStream = {
  push: (token: string) => void;
  flush: () => void;
  stop: () => void;
};

export type TtsSettingsLike = Record<string, unknown> & {
  volume?: number;
  ttsProvider?: string;
};

export type TtsProvider = {
  id: string;
  speak: (
    text: string,
    settings: TtsSettingsLike,
    onViseme: (name: string, weight?: number) => void,
    onEnd: () => void,
    opts?: { noFallback?: boolean },
  ) => Promise<void>;
  speakLoop: (
    text: string,
    settings: TtsSettingsLike,
    onViseme: (name: string, weight?: number) => void,
    onEnd: () => void,
  ) => Promise<void>;
  createStream: (
    settings: TtsSettingsLike,
    onViseme: (name: string, weight?: number) => void,
    onAllDone: () => void,
    onSpeakingChange?: (speaking: boolean) => void,
  ) => TtsStream;
  stop: () => void;
  setVolume: (v: number) => void;
};

export type SttEngineKind = "local" | "cloud";

export type SttEngine = {
  start: () => Promise<void>;
  stop: () => Promise<unknown>;
  setBargeInListener: (fn: ((energy: number) => void) | null) => void;
  discardPending: () => void;
  shouldSend: (text: string) => boolean;
};

export type SttProvider = {
  id: string;
  createEngine: (
    kind: SttEngineKind,
    onLiveText: (text: string) => void,
    onSpeechEnd: (text: string) => void,
    onError?: (err: string) => void,
  ) => SttEngine;
  setSuppress: (suppress: boolean) => void | Promise<void>;
  micEnergy: () => Promise<number>;
};

export type ServicesSurface = {
  registerTts: (provider: TtsProvider) => void;
  registerStt: (provider: SttProvider) => void;
  tts: () => TtsProvider;
  stt: () => SttProvider | null;
};

export type SecretsSurface = {
  set: (name: string, value: string) => Promise<void>;
  get: (name: string) => Promise<string | null>;
  remove: (name: string) => Promise<void>;
};

export interface PluginContext {
  readonly id: string;
  readonly log: {
    info: (msg: string) => void;
    warn: (msg: string) => void;
    error: (msg: string) => void;
  };
  readonly settings: {
    registerTab: (tab: SettingsTabContribution) => void;
    overrideTab: (id: string, component: ComponentType) => void;
    registerSection: (section: SectionContribution) => void;
    replaceSection: (id: string, component: ComponentType) => void;
  };
  readonly main: {
    registerWidget: (widget: WidgetContribution) => void;
    overrideWidget: (targetId: WidgetTargetId, component: ComponentType) => void;
    registerContextMenuItem: (item: ContextMenuItemContribution) => void;
  };
  readonly tools: {
    register: (tool: ToolContribution) => void;
  };
  /** Contribute instruction text to the chat system prompt (live across turns
   *  while the plugin is enabled). */
  readonly systemPrompt: {
    register: (fragment: SystemPromptContribution) => void;
  };
  readonly backend: {
    call: <T = unknown>(method: string, payload?: unknown) => Promise<T>;
  };
  readonly events: {
    on: (event: string, cb: (payload: unknown) => void) => () => void;
    emit: (event: string, payload?: unknown) => void;
  };
  readonly storage: {
    get: <T = unknown>(key: string) => Promise<T | null>;
    set: (key: string, value: unknown) => Promise<void>;
    remove: (key: string) => Promise<void>;
  };
  /** Live avatar control (no-op outside the main window). */
  readonly avatar: AvatarSurface;
  /** Publish/consume capability providers (TTS, STT). */
  readonly services: ServicesSurface;
  /** OS keychain scoped to this plugin. */
  readonly secrets: SecretsSurface;
  /** Shared UI kit of the host app (Button, FormField, SectionCard, …). */
  readonly ui: Record<string, any>;
}

// Module augmentation so `globalThis.__companion_host__` is typed.
declare global {
  // eslint-disable-next-line no-var
  var __companion_host__:
    | {
        React: unknown;
        ReactDOMClient: unknown;
        settings?: { useSettingsState?: () => any };
        ui: Record<string, unknown>;
        /** Frontend tool registry (host bundle only): qualified
         * `{pluginId}__{name}` → the registered contribution. */
        tools?: {
          get?: () => Map<string, ToolContribution>;
        };
      }
    | undefined;
}
