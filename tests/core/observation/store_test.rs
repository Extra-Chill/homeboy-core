//! Observation-store foundation tests.
//!
//! These isolate `HOME` / `XDG_DATA_HOME` so the developer's real local DB is
//! never read or written.

use crate::observation::store::{self, ObservationStore, CURRENT_SCHEMA_VERSION};
use crate::test_support::with_isolated_home;

struct XdgGuard {
    prior: Option<String>,
}

impl XdgGuard {
    fn unset() -> Self {
        let prior = std::env::var("XDG_DATA_HOME").ok();
        std::env::remove_var("XDG_DATA_HOME");
        Self { prior }
    }

    fn set(value: &std::path::Path) -> Self {
        let prior = std::env::var("XDG_DATA_HOME").ok();
        std::env::set_var("XDG_DATA_HOME", value);
        Self { prior }
    }
}

impl Drop for XdgGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }
}

#[test]
fn status_reports_missing_database_without_initializing() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::unset();

        let status = store::status().expect("status");

        assert!(!status.exists);
        assert_eq!(status.schema_version, 0);
        assert_eq!(status.migration_count, 0);
        assert_eq!(status.table_count, 0);
        assert_eq!(
            status.path,
            home.path()
                .join(".local/share/homeboy/homeboy.sqlite")
                .to_string_lossy()
        );
        assert!(
            !std::path::Path::new(&status.path).exists(),
            "read-only status must not create the DB"
        );
    });
}

#[test]
fn xdg_data_home_overrides_default_database_path() {
    with_isolated_home(|home| {
        let data_home = home.path().join("xdg-data");
        let _xdg = XdgGuard::set(&data_home);

        let path = store::database_path().expect("db path");

        assert_eq!(path, data_home.join("homeboy/homeboy.sqlite"));
    });
}

#[test]
fn initialization_creates_schema_and_status_reports_version() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();

        let store = ObservationStore::open_initialized().expect("init store");
        let status = store.status().expect("status");

        assert!(status.exists);
        assert_eq!(status.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(status.migration_count, 1);
        assert_eq!(status.table_count, 3);
    });
}

#[test]
fn initialization_is_idempotent() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();

        ObservationStore::open_initialized().expect("first init");
        let second = ObservationStore::open_initialized().expect("second init");
        let status = second.status().expect("status");

        assert_eq!(status.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(status.migration_count, 1);
        assert_eq!(status.table_count, 3);
    });
}
