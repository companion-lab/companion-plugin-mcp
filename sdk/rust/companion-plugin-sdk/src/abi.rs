//! C ABI shared between the companion plugin host (`src-tauri`) and plugins
//! built with this SDK. Everything that crosses a `dylib` boundary is plain
//! `#[repr(C)]` data plus UTF-8 JSON buffers — no Rust types with layout that
//! rustc could change.
//!
//! Safety contract (upheld by the host):
//! - `HostBridge` callbacks are thread-safe and the `user` pointer stays valid
//!   for the whole life of the plugin instance.
//! - Buffers returned by the plugin are freed by calling the plugin's own
//!   `companion_plugin_free_buffer` symbol, keeping allocators matched.
//! - The host checks `companion_plugin_abi_version` before calling anything
//!   else and refuses to load a plugin built against a different ABI.

use std::ffi::c_void;

/// Bump whenever the vtable below changes shape or semantics. Plugins built
/// against a different version are rejected at load time.
///
/// v2: added `data_dir` / `plugin_dir` path callbacks to [`HostBridge`]
/// (writable per-plugin data dir; read-only package dir for assets).
pub const ABI_VERSION: u32 = 2;

/// A byte buffer handed across the FFI boundary. Ownership transfers: the
/// receiver must free it through the original owner's `free_buffer`.
#[repr(C)]
pub struct RawBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl RawBuffer {
    pub const NULL: RawBuffer = RawBuffer {
        ptr: std::ptr::null_mut(),
        len: 0,
        cap: 0,
    };

    pub fn from_vec(mut v: Vec<u8>) -> Self {
        let ptr = v.as_mut_ptr();
        let len = v.len();
        let cap = v.capacity();
        std::mem::forget(v);
        RawBuffer { ptr, len, cap }
    }

    pub const fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}

/// Log levels understood by the host's `log` callback.
pub const LOG_INFO: u32 = 0;
pub const LOG_WARN: u32 = 1;
pub const LOG_ERROR: u32 = 2;

/// Function pointers the host hands to a plugin so it can call back into the
/// app (emit events to windows, write to the host log). Raw pointers keep this
/// `#[repr(C)]`; the host guarantees thread safety of the callbacks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostBridge {
    pub user: *mut c_void,
    /// Emit a Tauri event named `event` with a JSON `payload` to all windows.
    pub emit_event: extern "C" fn(
        user: *mut c_void,
        event: *const u8,
        event_len: usize,
        payload: *const u8,
        payload_len: usize,
    ),
    pub log:
        extern "C" fn(user: *mut c_void, level: u32, msg: *const u8, msg_len: usize),
    /// v2: Write the plugin's writable data directory (UTF-8 path, no NUL)
    /// into `out`. Returns the number of bytes the path needs. When `out` is
    /// null or `out_cap` is smaller than that, nothing is copied — call again
    /// with a big enough buffer. Returns 0 if the host has no data dir.
    pub data_dir:
        extern "C" fn(user: *mut c_void, out: *mut u8, out_cap: usize) -> usize,
    /// v2: Same protocol as `data_dir`, but for the read-only plugin package
    /// directory (holds `manifest.json`, the frontend bundle, the backend
    /// library, and any bundled `assets/`).
    pub plugin_dir:
        extern "C" fn(user: *mut c_void, out: *mut u8, out_cap: usize) -> usize,
}

/// The plugin side of the ABI, produced by `export_plugin!`.
#[repr(C)]
pub struct PluginVTable {
    pub abi_version: extern "C" fn() -> u32,
    /// Build the plugin instance, run `init`, return an opaque handle.
    pub create: extern "C" fn(bridge: HostBridge) -> *mut c_void,
    /// Handle one request. Returns a JSON envelope:
    /// `{"ok":true,"value":...}` or `{"ok":false,"error":"..."}`.
    pub handle: extern "C" fn(
        instance: *mut c_void,
        method: *const u8,
        method_len: usize,
        payload: *const u8,
        payload_len: usize,
    ) -> RawBuffer,
    /// JSON array of `ToolDef`s this plugin exposes to the chat model.
    pub tools: extern "C" fn(instance: *mut c_void) -> RawBuffer,
    /// Drop the instance. Host must not use the instance afterwards.
    pub destroy: extern "C" fn(instance: *mut c_void),
    /// Free a buffer the plugin previously returned.
    pub free_buffer: extern "C" fn(buffer: RawBuffer),
}
