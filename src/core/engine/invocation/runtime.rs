//! Short, platform-aware invocation runtime root.
//!
//! Downstream workloads place UNIX domain sockets, FIFOs, and other
//! path-length-sensitive primitives under `HOMEBOY_INVOCATION_STATE_DIR`.
//! macOS `sockaddr_un` only accepts socket paths up to 104 bytes (108 on
//! Linux), so the prefix that Homeboy hands out must stay short enough for
//! a typical filename to fit underneath without any per-workload defense.
//!
//! ## Root selection
//!
//! In priority order:
//! 1. `HOMEBOY_INVOCATION_RUNTIME_DIR` env override (tests, advanced users).
//! 2. `$TMPDIR/hb` on macOS when set (typically `/var/folders/<short>/T/hb`).
//! 3. `$XDG_RUNTIME_DIR/hb` on Linux when set.
//! 4. `/tmp/hb` on Linux when no XDG runtime dir is available.
//! 5. `~/.cache/homeboy/inv` fallback on every platform.
//!
//! Each invocation is one directory directly beneath the chosen root:
//! `<root>/<short-id>/{state,artifacts,tmp}`. There is no `homeboy-run-<uuid>`
//! / `invocations/inv-<uuid>` nesting like the legacy layout used.
//!
//! ## Path budget
//!
//! [`enforce_path_budget`] checks that the longest hand-out path leaves at
//! least [`SOCKET_HEADROOM_BYTES`] bytes of headroom under the platform
//! `sockaddr_un` limit, and fails fast with a clear error when an unusually
//! long `$HOME` or `$TMPDIR` would otherwise hand out a directory that no
//! UDS-using workload can use.

use crate::error::{Error, Result};
#[cfg(windows)]
use crate::paths;
use std::env;
use std::path::{Path, PathBuf};

/// Override env var that pins the invocation runtime root to a specific path.
///
/// Primarily for tests and unusual host configurations. When set and
/// non-empty, it bypasses the platform detection ladder entirely.
pub const HOMEBOY_INVOCATION_RUNTIME_DIR_ENV: &str = "HOMEBOY_INVOCATION_RUNTIME_DIR";

/// Bytes of headroom reserved beneath `sockaddr_un` so workloads can append
/// a typical socket filename (e.g. `daemon/daemon.sock`) without overflowing.
pub const SOCKET_HEADROOM_BYTES: usize = 32;

/// Platform `sockaddr_un` `sun_path` capacity in bytes (excluding NUL).
#[cfg(target_os = "macos")]
pub const SUN_PATH_CAPACITY: usize = 104;
#[cfg(all(unix, not(target_os = "macos")))]
pub const SUN_PATH_CAPACITY: usize = 108;
#[cfg(not(unix))]
pub const SUN_PATH_CAPACITY: usize = 108;

/// Resolve the short, platform-aware root that holds invocation directories.
///
/// The root is *not* created on this call. Callers that allocate an
/// invocation directory under it are responsible for `create_dir_all`.
pub fn invocation_runtime_root() -> Result<PathBuf> {
    if let Some(override_root) = override_root() {
        return Ok(override_root);
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(tmpdir) = non_empty_env("TMPDIR") {
            return Ok(PathBuf::from(tmpdir).join("hb"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = non_empty_env("XDG_RUNTIME_DIR") {
            return Ok(PathBuf::from(xdg).join("hb"));
        }
        return Ok(PathBuf::from("/tmp").join("hb"));
    }

    // Generic fallback: ~/.cache/homeboy/inv (also Windows).
    cache_fallback_root()
}

fn override_root() -> Option<PathBuf> {
    let raw = env::var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV).ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn non_empty_env(key: &str) -> Option<String> {
    let raw = env::var(key).ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn cache_fallback_root() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        // Reuse the existing homeboy() resolver on Windows; it lands under
        // %APPDATA%\homeboy which is the closest analog to a per-user cache
        // root and keeps the implementation simple.
        return Ok(paths::homeboy()?.join("inv"));
    }

    #[cfg(not(windows))]
    {
        if let Some(xdg_cache) = non_empty_env("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(xdg_cache).join("homeboy").join("inv"));
        }
        let home = env::var("HOME").map_err(|_| {
            Error::internal_unexpected(
                "HOME environment variable not set on Unix-like system".to_string(),
            )
        })?;
        Ok(PathBuf::from(home)
            .join(".cache")
            .join("homeboy")
            .join("inv"))
    }
}

/// Verify that handing out `path` leaves room for a downstream socket
/// filename within the `sockaddr_un` budget.
///
/// Fails fast with a clear error message when the platform budget cannot
/// accommodate at least [`SOCKET_HEADROOM_BYTES`] beyond `path`'s length.
pub fn enforce_path_budget(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    let path_len = path_str.len();
    // +1 reserves the path-separator byte before the workload's filename.
    let needed = path_len
        .saturating_add(1)
        .saturating_add(SOCKET_HEADROOM_BYTES);
    if needed > SUN_PATH_CAPACITY {
        return Err(Error::internal_unexpected(format!(
            "Homeboy invocation runtime path is too long for the platform sockaddr_un limit: \
             path is {path_len} bytes, need {needed} bytes (path + 1 separator + \
             {SOCKET_HEADROOM_BYTES} bytes socket filename headroom), but sun_path capacity is \
             {SUN_PATH_CAPACITY} bytes. Set {HOMEBOY_INVOCATION_RUNTIME_DIR_ENV} to a shorter \
             root (path: {path_str})."
        )));
    }
    Ok(())
}

/// Generate a short opaque path component (10 lowercase hex chars).
///
/// 40 bits of entropy ≈ 1.1e12 unique values is far beyond the number of
/// concurrent invocations any single host will run. The lease index lock in
/// `InvocationGuard::acquire` guarantees uniqueness against the live set.
pub fn short_invocation_id() -> String {
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    uuid[..10].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn override_env_takes_priority() {
        let _guard = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let prior = env::var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV).ok();
        env::set_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV, dir.path());

        let root = invocation_runtime_root().expect("root resolves");
        assert_eq!(root, dir.path());

        match prior {
            Some(value) => env::set_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV, value),
            None => env::remove_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV),
        }
    }

    #[test]
    fn override_env_blank_falls_back_to_platform() {
        let _guard = env_lock().lock().expect("env lock");
        let prior = env::var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV).ok();
        env::set_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV, "   ");

        let root = invocation_runtime_root().expect("root resolves");
        assert!(root.ends_with("hb") || root.ends_with("inv"));

        match prior {
            Some(value) => env::set_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV, value),
            None => env::remove_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV),
        }
    }

    #[test]
    fn short_id_is_ten_lowercase_hex_chars() {
        let id = short_invocation_id();
        assert_eq!(id.len(), 10, "id={id}");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "id should be lowercase hex: {id}"
        );
    }

    #[test]
    fn short_ids_are_unique_across_calls() {
        // 40 bits of entropy: collisions in 1000 calls are vanishingly rare.
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(ids.insert(short_invocation_id()));
        }
    }

    #[test]
    fn budget_accepts_short_path() {
        let path = PathBuf::from("/tmp/hb/abc1234567/state");
        enforce_path_budget(&path).expect("short path fits");
    }

    #[test]
    fn budget_reserves_socket_headroom() {
        // Build a path that leaves <32 bytes under the platform limit.
        let mut s = String::from("/");
        // Aim for a path length that leaves headroom < SOCKET_HEADROOM_BYTES.
        let target_len = SUN_PATH_CAPACITY - SOCKET_HEADROOM_BYTES;
        while s.len() < target_len {
            s.push('a');
        }
        let path = PathBuf::from(s);
        let err = enforce_path_budget(&path).expect_err("should reject overlong path");
        let message = err.to_string();
        assert!(
            message.contains("sockaddr_un"),
            "error should mention sockaddr_un: {message}"
        );
        assert!(
            message.contains(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV),
            "error should suggest the override env: {message}"
        );
    }

    #[test]
    fn budget_headroom_is_at_least_thirty_two_bytes() {
        // For a maximum-length accepted path, the remaining bytes after
        // appending '/' + a 32-byte socket filename must not overflow.
        let mut s = String::from("/");
        let max_accepted = SUN_PATH_CAPACITY - SOCKET_HEADROOM_BYTES - 1;
        while s.len() < max_accepted {
            s.push('b');
        }
        let path = PathBuf::from(&s);
        enforce_path_budget(&path).expect("max-accepted path fits");

        // Verify the headroom: appending a 32-byte filename stays under cap.
        let appended_len = s.len() + 1 + SOCKET_HEADROOM_BYTES;
        assert!(
            appended_len <= SUN_PATH_CAPACITY,
            "appended path {appended_len} should fit in {SUN_PATH_CAPACITY}"
        );
    }
}
