//! Startup module update check — warns users when installed modules have updates available.
//!
//! Same pattern as the CLI update check (`update_check.rs`):
//! - Check on first run of the day (24h cache)
//! - Print notice to stderr if any module has updates
//! - `homeboy module update <name>` to apply
//!
//! Shares the disable mechanisms with CLI update check:
//! - `HOMEBOY_NO_UPDATE_CHECK=1`
//! - `homeboy config set /update_check false`

use crate::module;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_FILENAME: &str = "module_update_check.json";
const CHECK_INTERVAL_SECS: u64 = 86400; // 24 hours

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleUpdateCache {
    /// Module ID -> number of commits behind
    pub modules_behind: HashMap<String, usize>,
    pub checked_at: u64,
}

fn cache_path() -> Option<std::path::PathBuf> {
    paths::homeboy().ok().map(|p| p.join(CACHE_FILENAME))
}

fn read_cache() -> Option<ModuleUpdateCache> {
    let path = cache_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cache: &ModuleUpdateCache) {
    let Some(path) = cache_path() else { return };
    let Ok(content) = serde_json::to_string_pretty(cache) else {
        return;
    };
    let _ = std::fs::write(&path, content);
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_cache_fresh(cache: &ModuleUpdateCache) -> bool {
    let elapsed = now_unix().saturating_sub(cache.checked_at);
    elapsed < CHECK_INTERVAL_SECS
}

fn is_disabled_by_env() -> bool {
    std::env::var("HOMEBOY_NO_UPDATE_CHECK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn is_disabled_by_config() -> bool {
    !crate::defaults::load_config().update_check
}

fn print_module_hints(modules_behind: &HashMap<String, usize>) {
    if modules_behind.is_empty() {
        return;
    }

    if modules_behind.len() == 1 {
        let (id, count) = modules_behind.iter().next().unwrap();
        log_status!(
            "update",
            "Module '{}' has {} new commit{}. Run `homeboy module update {}` to update.",
            id,
            count,
            if *count == 1 { "" } else { "s" },
            id
        );
    } else {
        let names: Vec<&String> = modules_behind.keys().collect();
        log_status!(
            "update",
            "{} modules have updates: {}. Run `homeboy module update <name>` to update.",
            modules_behind.len(),
            names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

/// Run the startup module update check. Prints hints to stderr if updates are available.
///
/// Silently returns on any error. Call from main.rs alongside the CLI update check.
pub fn run_startup_check() {
    if is_disabled_by_env() || is_disabled_by_config() {
        return;
    }

    let mut already_printed = false;

    // Read cache — if it has updates, print hint immediately
    let cached = read_cache();

    if let Some(ref cache) = cached {
        if !cache.modules_behind.is_empty() {
            print_module_hints(&cache.modules_behind);
            already_printed = true;
        }

        if is_cache_fresh(cache) {
            return;
        }
    }

    // Cache stale or missing — check all modules
    let module_ids = module::available_module_ids();
    let mut modules_behind: HashMap<String, usize> = HashMap::new();

    for id in &module_ids {
        if let Some(update) = module::check_update_available(id) {
            modules_behind.insert(update.module_id, update.behind_count);
        }
    }

    // Write refreshed cache
    write_cache(&ModuleUpdateCache {
        modules_behind: modules_behind.clone(),
        checked_at: now_unix(),
    });

    // Print hint if updates found and we haven't already
    if !already_printed && !modules_behind.is_empty() {
        print_module_hints(&modules_behind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_freshness_within_24h() {
        let cache = ModuleUpdateCache {
            modules_behind: HashMap::new(),
            checked_at: now_unix() - 100,
        };
        assert!(is_cache_fresh(&cache));
    }

    #[test]
    fn cache_stale_after_24h() {
        let cache = ModuleUpdateCache {
            modules_behind: HashMap::new(),
            checked_at: now_unix() - CHECK_INTERVAL_SECS - 1,
        };
        assert!(!is_cache_fresh(&cache));
    }

    #[test]
    fn cache_roundtrip() {
        let mut behind = HashMap::new();
        behind.insert("wordpress".to_string(), 3);
        let cache = ModuleUpdateCache {
            modules_behind: behind,
            checked_at: 1700000000,
        };
        let json = serde_json::to_string(&cache).unwrap();
        let parsed: ModuleUpdateCache = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.modules_behind.len(), 1);
        assert_eq!(parsed.modules_behind["wordpress"], 3);
        assert_eq!(parsed.checked_at, 1700000000);
    }
}
