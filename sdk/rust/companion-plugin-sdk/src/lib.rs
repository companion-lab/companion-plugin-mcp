//! companion-plugin-sdk — Rust backend SDK for companion-tauri plugins.
//!
//! A plugin backend is a `cdylib` that implements [`Plugin`] and exports the
//! C ABI via [`export_plugin!`]:
//!
//! ```ignore
//! use companion_plugin_sdk::{Plugin, PluginCtx, export_plugin};
//! use serde_json::{json, Value};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn init(&mut self, _ctx: PluginCtx) -> Result<(), String> { Ok(()) }
//!
//!     fn handle(&mut self, method: &str, payload: Value) -> Result<Value, String> {
//!         match method {
//!             "ping" => Ok(json!({ "pong": true })),
//!             _ => Err(format!("unknown method: {method}")),
//!         }
//!     }
//! }
//!
//! export_plugin!(Hello);
//! ```
//!
//! Build with `crate-type = ["cdylib"]`, then ship the produced
//! `lib<name>.dylib` / `<name>.dll` / `lib<name>.so` next to the plugin's
//! `manifest.json`. The host routes every `plugin_call` to
//! [`Plugin::handle`]; all payloads and results are JSON.

mod abi;

pub use abi::{
    HostBridge, PluginVTable, RawBuffer, ABI_VERSION, LOG_ERROR, LOG_INFO, LOG_WARN,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::c_void;
use std::path::PathBuf;

/// A tool the plugin exposes to the chat model. The host merges these into
/// the tool list the model sees; tool calls arrive as regular `handle`
/// requests, conventionally with method `"tool"` and
/// `{"name": ..., "args": ...}` payloads (see the notes example).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

/// Host services available to a plugin. Cheap to clone; keep a copy if the
/// backend needs to emit events or log outside `init`.
#[derive(Clone)]
pub struct PluginCtx {
    bridge: HostBridge,
}

// Safety: the host guarantees `HostBridge` callbacks are thread-safe and that
// `user` stays valid for the plugin's lifetime (see module docs on `abi`).
unsafe impl Send for PluginCtx {}
unsafe impl Sync for PluginCtx {}

impl PluginCtx {
    /// Internal constructor used by `export_plugin!` (the macro expands in
    /// the plugin's crate, which cannot see private fields).
    #[doc(hidden)]
    pub fn from_bridge(bridge: HostBridge) -> Self {
        PluginCtx { bridge }
    }

    /// Emitted events are namespaced by the host as `plugin:{id}:{event}`.
    pub fn emit(&self, event: &str, payload: &Value) {
        if self.bridge.user.is_null() {
            return;
        }
        let event = event.as_bytes();
        let payload = payload.to_string().into_bytes();
        (self.bridge.emit_event)(
            self.bridge.user,
            event.as_ptr(),
            event.len(),
            payload.as_ptr(),
            payload.len(),
        );
    }

    pub fn info(&self, msg: &str) {
        self.log(LOG_INFO, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.log(LOG_WARN, msg);
    }

    pub fn error(&self, msg: &str) {
        self.log(LOG_ERROR, msg);
    }

    /// Writable directory scoped to this plugin (`{app_data}/plugins-data/{id}/`).
    /// The host creates it before `init`, so storing files here works
    /// immediately. `None` when the host could not provide a path.
    pub fn data_dir(&self) -> Option<PathBuf> {
        read_dir_path(self.bridge.data_dir, self.bridge.user)
    }

    /// Read-only directory of this plugin's package (`manifest.json`,
    /// frontend bundle, backend library, bundled `assets/`).
    pub fn plugin_dir(&self) -> Option<PathBuf> {
        read_dir_path(self.bridge.plugin_dir, self.bridge.user)
    }

    fn log(&self, level: u32, msg: &str) {
        if self.bridge.user.is_null() {
            return;
        }
        let bytes = msg.as_bytes();
        (self.bridge.log)(self.bridge.user, level, bytes.as_ptr(), bytes.len());
    }
}

/// Two-pass path read over a bridge callback: first call sizes the path, the
/// second copies it. Matches the `data_dir`/`plugin_dir` callback contract.
fn read_dir_path(
    get: extern "C" fn(user: *mut c_void, out: *mut u8, out_cap: usize) -> usize,
    user: *mut c_void,
) -> Option<PathBuf> {
    if user.is_null() {
        return None;
    }
    let needed = get(user, std::ptr::null_mut(), 0);
    if needed == 0 {
        return None;
    }
    let mut buf = vec![0u8; needed];
    let written = get(user, buf.as_mut_ptr(), buf.len());
    if written == 0 || written > needed {
        return None;
    }
    buf.truncate(written);
    Some(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
}

/// The trait a plugin backend implements.
pub trait Plugin: Send {
    /// Called once, before the first `handle`. Keep `ctx` if you need to emit
    /// events later.
    fn init(&mut self, ctx: PluginCtx) -> Result<(), String> {
        let _ = ctx;
        Ok(())
    }

    /// Handle one request. `method` plus a JSON `payload` in, JSON result out.
    fn handle(&mut self, method: &str, payload: Value) -> Result<Value, String>;

    /// Tools exposed to the chat model. Read at plugin load.
    fn tools(&mut self) -> Vec<ToolDef> {
        Vec::new()
    }

    /// Called before the instance is dropped (app exit or backend disable).
    fn shutdown(&mut self) {}
}

#[doc(hidden)]
/// Stands in for a plugin whose `init` failed, so a broken backend degrades
/// to "every call errors" instead of "host crashed".
pub struct FailedPlugin {
    error: String,
}

impl FailedPlugin {
    pub fn new(error: String) -> Self {
        FailedPlugin { error }
    }
}

impl Plugin for FailedPlugin {
    fn handle(&mut self, _method: &str, _payload: Value) -> Result<Value, String> {
        Err(self.error.clone())
    }
}

/// Export the plugin ABI. Accepts any expression producing a value
/// implementing [`Plugin`]:
///
/// ```ignore
/// export_plugin!(Notes);
/// // or with a constructor:
/// export_plugin!(Notes::new());
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($create:expr) => {
        #[no_mangle]
        pub extern "C" fn companion_plugin_abi_version() -> u32 {
            $crate::ABI_VERSION
        }

        #[no_mangle]
        pub extern "C" fn companion_plugin_create(
            bridge: $crate::HostBridge,
        ) -> *mut ::core::ffi::c_void {
            let ctx = $crate::PluginCtx::from_bridge(bridge);
            let boxed: Box<dyn $crate::Plugin> = match ::std::panic::catch_unwind(
                ::std::panic::AssertUnwindSafe(|| $create),
            ) {
                Ok(mut plugin) => match plugin.init(ctx) {
                    Ok(()) => Box::new(plugin),
                    Err(e) => Box::new($crate::FailedPlugin::new(e)),
                },
                Err(_) => Box::new($crate::FailedPlugin::new(
                    "plugin panicked during construction".to_string(),
                )),
            };
            // Double-box: the trait object is a fat pointer, but the FFI
            // boundary only carries a thin `*mut c_void`. Wrapping it in
            // `Box<Box<dyn Plugin>>` keeps the vtable intact.
            Box::into_raw(Box::new(boxed)) as *mut ::core::ffi::c_void
        }

        #[no_mangle]
        pub extern "C" fn companion_plugin_handle(
            instance: *mut ::core::ffi::c_void,
            method: *const u8,
            method_len: usize,
            payload: *const u8,
            payload_len: usize,
        ) -> $crate::RawBuffer {
            if instance.is_null() {
                let err = ::serde_json::json!({ "ok": false, "error": "null instance" });
                return $crate::RawBuffer::from_vec(err.to_string().into_bytes());
            }
            let plugin =
                unsafe { &mut *(instance as *mut Box<dyn $crate::Plugin>) };
            let method = if method.is_null() {
                String::new()
            } else {
                let bytes = unsafe { ::core::slice::from_raw_parts(method, method_len) };
                String::from_utf8_lossy(bytes).into_owned()
            };
            let parsed = if payload.is_null() || payload_len == 0 {
                Ok(::serde_json::Value::Null)
            } else {
                let bytes = unsafe { ::core::slice::from_raw_parts(payload, payload_len) };
                ::serde_json::from_slice(bytes)
                    .map_err(|e| format!("invalid JSON payload: {e}"))
            };
            let envelope = match parsed {
                Err(e) => ::serde_json::json!({ "ok": false, "error": e }),
                Ok(payload) => match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(|| plugin.handle(&method, payload)),
                ) {
                    Ok(Ok(value)) => ::serde_json::json!({ "ok": true, "value": value }),
                    Ok(Err(e)) => ::serde_json::json!({ "ok": false, "error": e }),
                    Err(_) => ::serde_json::json!({
                        "ok": false,
                        "error": "plugin panicked while handling a request"
                    }),
                },
            };
            $crate::RawBuffer::from_vec(envelope.to_string().into_bytes())
        }

        #[no_mangle]
        pub extern "C" fn companion_plugin_tools(
            instance: *mut ::core::ffi::c_void,
        ) -> $crate::RawBuffer {
            let tools = if instance.is_null() {
                Vec::new()
            } else {
                let plugin =
                    unsafe { &mut *(instance as *mut Box<dyn $crate::Plugin>) };
                match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    plugin.tools()
                })) {
                    Ok(tools) => tools,
                    Err(_) => Vec::new(),
                }
            };
            let json = ::serde_json::to_vec(&tools).unwrap_or_default();
            $crate::RawBuffer::from_vec(json)
        }

        #[no_mangle]
        pub extern "C" fn companion_plugin_destroy(instance: *mut ::core::ffi::c_void) {
            if instance.is_null() {
                return;
            }
            let plugin = instance as *mut Box<dyn $crate::Plugin>;
            unsafe {
                let mut boxed = Box::from_raw(plugin);
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(
                    || boxed.shutdown(),
                ));
                drop(boxed);
            }
        }

        #[no_mangle]
        pub extern "C" fn companion_plugin_free_buffer(buffer: $crate::RawBuffer) {
            if buffer.ptr.is_null() {
                return;
            }
            unsafe {
                drop(::std::vec::Vec::from_raw_parts(
                    buffer.ptr,
                    buffer.len,
                    buffer.cap,
                ));
            }
        }
    };
}
