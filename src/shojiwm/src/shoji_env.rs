//! One-time snapshot of the `SHOJI_*` process environment.
//!
//! Compositor behaviour/debug knobs are consulted all over the render,
//! commit and input paths (gap/firefox/popup/… debug flags, scanout toggles,
//! latency diagnostics). Reading them through `std::env::var_os` every time
//! is wasteful: that API scans the whole process environment linearly on
//! each call and clones the value when present, and the "unset" fallback is
//! re-derived on every frame.
//!
//! Instead the compositor freezes every `SHOJI_*` variable once at startup:
//! [`capture`] is called from `main()` right after CLI arguments are merged
//! into the environment and inherited compositor environment is sanitized.
//! All later lookups are served from that snapshot — no repeated environment
//! scans, no per-call `OsString` allocations, and the values are fixed for
//! the whole process life (mutating the environment afterwards has no effect
//! on these lookups, which is the intended contract for `SHOJI_*` knobs).
//!
//! Startup-only readers that run before/around [`capture`] (CLI parsing and
//! log setup in `main.rs`, path resolution in `install_paths.rs`,
//! `config.rs`/`backend/mod.rs` TTY device selection, xwayland-satellite
//! spawning) intentionally keep reading the live environment — they run a
//! handful of times at most, and some must observe the CLI-args-merged
//! values.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;

struct EnvSnapshot {
    vars: HashMap<Box<str>, OsString>,
}

static SNAPSHOT: OnceLock<EnvSnapshot> = OnceLock::new();

impl EnvSnapshot {
    fn from_current_env() -> Self {
        EnvSnapshot {
            vars: std::env::vars_os()
                .filter(|(key, _)| key.as_os_str().as_encoded_bytes().starts_with(b"SHOJI_"))
                .map(|(key, value)| {
                    // `SHOJI_*` keys are ASCII by construction; a non-UTF-8
                    // key would be unreachable through `var_os` anyway, so it
                    // is dropped rather than kept under a lossy key.
                    let key = key.into_string().unwrap_or_default();
                    (key.into_boxed_str(), value)
                })
                .collect(),
        }
    }
}

/// Freeze all `SHOJI_*` environment variables for the rest of the process.
///
/// Called once from `main()` after CLI overrides have been applied
/// (`apply_runtime_overrides`) and the inherited compositor environment has
/// been sanitized. Idempotent: calling it again (or letting [`var_os`] lazily
/// capture on first use) keeps the first snapshot.
pub fn capture() {
    let _ = SNAPSHOT.get_or_init(EnvSnapshot::from_current_env);
}

#[inline]
fn snapshot() -> &'static EnvSnapshot {
    // `capture()` is called from `main()` before any compositor code is
    // reachable; the lazy fallback only covers hypothetical early callers.
    SNAPSHOT.get_or_init(EnvSnapshot::from_current_env)
}

/// Cached lookup of a `SHOJI_*` variable from the startup snapshot —
/// equivalent of `std::env::var_os` without the per-call environment scan.
///
/// Returns `None` when the variable was unset (or non-UTF-8-keyed, which
/// cannot happen for `SHOJI_*`) at freeze time; it never observes later
/// mutations.
#[inline]
pub fn var_os(name: &str) -> Option<&'static OsStr> {
    snapshot()
        .vars
        .get(name)
        .map(OsString::as_os_str)
}

/// Cached lookup of a `SHOJI_*` variable from the startup snapshot, for
/// UTF-8 values only — equivalent of `std::env::var(...).ok()` without the
/// per-call environment scan. Non-UTF-8 values are reported as unset; use
/// [`var_os`] if raw bytes matter.
#[inline]
pub fn var(name: &str) -> Option<&'static str> {
    var_os(name).and_then(OsStr::to_str)
}
