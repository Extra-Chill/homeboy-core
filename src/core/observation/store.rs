use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::{paths, Error, Result};

pub const CURRENT_SCHEMA_VERSION: i64 = 1;

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runs (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            component_id TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT NOT NULL,
            command TEXT,
            cwd TEXT,
            homeboy_version TEXT,
            git_sha TEXT,
            rig_id TEXT,
            metadata_json TEXT NOT NULL DEFAULT '{}'
        );

        CREATE TABLE IF NOT EXISTS artifacts (
            id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            path TEXT NOT NULL,
            sha256 TEXT,
            size_bytes INTEGER,
            mime TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY(run_id) REFERENCES runs(id)
        );
    "#,
}];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObservationDbStatus {
    pub path: String,
    pub exists: bool,
    pub schema_version: i64,
    pub migration_count: i64,
    pub table_count: i64,
}

pub struct ObservationStore {
    connection: Connection,
    path: PathBuf,
}

impl ObservationStore {
    /// Open and lazily initialize the local observed-state database.
    pub fn open_initialized() -> Result<Self> {
        let path = database_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Error::internal_io(
                    e.to_string(),
                    Some(format!("create observation store dir {}", parent.display())),
                )
            })?;
        }

        let connection = open_connection(&path)?;
        apply_migrations(&connection)?;
        Ok(Self { connection, path })
    }

    pub fn status(&self) -> Result<ObservationDbStatus> {
        status_for_open_connection(&self.connection, self.path.clone(), true)
    }
}

pub fn database_path() -> Result<PathBuf> {
    paths::observation_db()
}

/// Read local observation-store status without creating the database.
pub fn status() -> Result<ObservationDbStatus> {
    let path = database_path()?;
    if !path.exists() {
        return Ok(ObservationDbStatus {
            path: path.to_string_lossy().to_string(),
            exists: false,
            schema_version: 0,
            migration_count: 0,
            table_count: 0,
        });
    }

    let connection = open_connection(&path)?;
    status_for_open_connection(&connection, path, true)
}

fn apply_migrations(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
        "#,
        )
        .map_err(sqlite_error("create schema_migrations"))?;

    for migration in MIGRATIONS {
        if migration_applied(connection, migration.version)? {
            continue;
        }

        let tx = connection
            .unchecked_transaction()
            .map_err(sqlite_error("begin observation migration"))?;
        tx.execute_batch(migration.sql)
            .map_err(sqlite_error(format!(
                "apply migration {}",
                migration.version
            )))?;
        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![migration.version, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(sqlite_error(format!(
            "record migration {}",
            migration.version
        )))?;
        tx.commit().map_err(sqlite_error(format!(
            "commit migration {}",
            migration.version
        )))?;
    }

    Ok(())
}

fn migration_applied(connection: &Connection, version: i64) -> Result<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
            [version],
            |row| row.get(0),
        )
        .map_err(sqlite_error(format!("check migration {}", version)))?;
    Ok(count > 0)
}

fn status_for_open_connection(
    connection: &Connection,
    path: PathBuf,
    exists: bool,
) -> Result<ObservationDbStatus> {
    Ok(ObservationDbStatus {
        path: path.to_string_lossy().to_string(),
        exists,
        schema_version: current_schema_version(connection)?,
        migration_count: migration_count(connection)?,
        table_count: table_count(connection)?,
    })
}

fn current_schema_version(connection: &Connection) -> Result<i64> {
    if !table_exists(connection, "schema_migrations")? {
        return Ok(0);
    }

    connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error("read current schema version"))
}

fn migration_count(connection: &Connection) -> Result<i64> {
    if !table_exists(connection, "schema_migrations")? {
        return Ok(0);
    }

    connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .map_err(sqlite_error("count schema migrations"))
}

fn table_count(connection: &Connection) -> Result<i64> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error("count observation tables"))
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .map_err(sqlite_error(format!("check table {}", table)))?;
    Ok(count > 0)
}

fn open_connection(path: &Path) -> Result<Connection> {
    Connection::open(path).map_err(sqlite_error(format!(
        "open observation store {}",
        path.display()
    )))
}

fn sqlite_error(context: impl Into<String>) -> impl FnOnce(rusqlite::Error) -> Error {
    let context = context.into();
    move |error| {
        Error::internal_unexpected(format!(
            "SQLite observation store error: {context}: {error}"
        ))
    }
}

#[cfg(test)]
#[path = "../../../tests/core/observation/store_test.rs"]
mod store_test;
