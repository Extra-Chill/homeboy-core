use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::records::{ArtifactRecord, NewRunRecord, RunListFilter, RunRecord, RunStatus};
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

    pub fn start_run(&self, run: NewRunRecord) -> Result<RunRecord> {
        validate_required("kind", &run.kind)?;
        let id = Uuid::new_v4().to_string();
        let started_at = chrono::Utc::now().to_rfc3339();
        let metadata_json = serialize_metadata(&run.metadata_json)?;

        self.connection
            .execute(
                r#"
                INSERT INTO runs(
                    id,
                    kind,
                    component_id,
                    started_at,
                    status,
                    command,
                    cwd,
                    homeboy_version,
                    git_sha,
                    rig_id,
                    metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                params![
                    id,
                    run.kind,
                    run.component_id,
                    started_at,
                    RunStatus::Running.as_str(),
                    run.command,
                    run.cwd,
                    run.homeboy_version,
                    run.git_sha,
                    run.rig_id,
                    metadata_json,
                ],
            )
            .map_err(sqlite_error("insert run record"))?;

        self.get_run(&id)?.ok_or_else(|| {
            Error::internal_unexpected(format!(
                "Inserted run record {id} but could not read it back"
            ))
        })
    }

    pub fn finish_run(
        &self,
        run_id: &str,
        status: RunStatus,
        metadata_json: Option<serde_json::Value>,
    ) -> Result<RunRecord> {
        validate_required("run_id", run_id)?;
        let finished_at = chrono::Utc::now().to_rfc3339();
        let rows = match metadata_json {
            Some(metadata_json) => {
                let serialized = serialize_metadata(&metadata_json)?;
                self.connection
                    .execute(
                        r#"
                        UPDATE runs
                        SET finished_at = ?1, status = ?2, metadata_json = ?3
                        WHERE id = ?4
                        "#,
                        params![finished_at, status.as_str(), serialized, run_id],
                    )
                    .map_err(sqlite_error("finish run record with metadata"))?
            }
            None => self
                .connection
                .execute(
                    r#"
                    UPDATE runs
                    SET finished_at = ?1, status = ?2
                    WHERE id = ?3
                    "#,
                    params![finished_at, status.as_str(), run_id],
                )
                .map_err(sqlite_error("finish run record"))?,
        };

        if rows == 0 {
            return Err(Error::validation_invalid_argument(
                "run_id",
                format!("run record not found: {run_id}"),
                Some(run_id.to_string()),
                None,
            ));
        }

        self.get_run(run_id)?.ok_or_else(|| {
            Error::internal_unexpected(format!(
                "Finished run record {run_id} but could not read it back"
            ))
        })
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>> {
        validate_required("run_id", run_id)?;
        self.connection
            .query_row(
                r#"
                SELECT id, kind, component_id, started_at, finished_at, status, command, cwd,
                       homeboy_version, git_sha, rig_id, metadata_json
                FROM runs
                WHERE id = ?1
                "#,
                [run_id],
                row_to_run_record,
            )
            .optional()
            .map_err(sqlite_error("read run record"))
    }

    pub fn list_runs(&self, filter: RunListFilter) -> Result<Vec<RunRecord>> {
        let limit = filter.limit.unwrap_or(100).clamp(1, 1000);
        let mut statement = self
            .connection
            .prepare(
                r#"
                SELECT id, kind, component_id, started_at, finished_at, status, command, cwd,
                       homeboy_version, git_sha, rig_id, metadata_json
                FROM runs
                WHERE (?1 IS NULL OR kind = ?1)
                  AND (?2 IS NULL OR component_id = ?2)
                  AND (?3 IS NULL OR status = ?3)
                  AND (?4 IS NULL OR rig_id = ?4)
                ORDER BY started_at DESC
                LIMIT ?5
                "#,
            )
            .map_err(sqlite_error("prepare list run records"))?;
        let rows = statement
            .query_map(
                params![
                    filter.kind.as_deref(),
                    filter.component_id.as_deref(),
                    filter.status.as_deref(),
                    filter.rig_id.as_deref(),
                    limit,
                ],
                row_to_run_record,
            )
            .map_err(sqlite_error("list run records"))?;

        collect_rows(rows, "collect run records")
    }

    pub fn record_artifact(
        &self,
        run_id: &str,
        kind: &str,
        path: impl AsRef<Path>,
    ) -> Result<ArtifactRecord> {
        validate_required("run_id", run_id)?;
        validate_required("kind", kind)?;
        if self.get_run(run_id)?.is_none() {
            return Err(Error::validation_invalid_argument(
                "run_id",
                format!("run record not found: {run_id}"),
                Some(run_id.to_string()),
                None,
            ));
        }

        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Error::validation_invalid_argument(
                    "path",
                    format!("artifact file not found: {}", path.display()),
                    Some(path.to_string_lossy().to_string()),
                    None,
                );
            }
            Error::internal_io(
                e.to_string(),
                Some(format!("read artifact metadata {}", path.display())),
            )
        })?;
        if !metadata.is_file() {
            return Err(Error::validation_invalid_argument(
                "path",
                format!("artifact path is not a file: {}", path.display()),
                Some(path.to_string_lossy().to_string()),
                None,
            ));
        }

        let id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let path_string = path.to_string_lossy().to_string();
        let size_bytes = i64::try_from(metadata.len()).ok();
        let sha256 = Some(sha256_file(path)?);
        let mime = mime_from_path(path);

        self.connection
            .execute(
                r#"
                INSERT INTO artifacts(id, run_id, kind, path, sha256, size_bytes, mime, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                params![
                    id,
                    run_id,
                    kind,
                    path_string,
                    sha256,
                    size_bytes,
                    mime,
                    created_at,
                ],
            )
            .map_err(sqlite_error("insert artifact record"))?;

        self.list_artifacts(run_id)?
            .into_iter()
            .find(|artifact| artifact.id == id)
            .ok_or_else(|| {
                Error::internal_unexpected(format!(
                    "Inserted artifact record {id} but could not read it back"
                ))
            })
    }

    pub fn list_artifacts(&self, run_id: &str) -> Result<Vec<ArtifactRecord>> {
        validate_required("run_id", run_id)?;
        let mut statement = self
            .connection
            .prepare(
                r#"
                SELECT id, run_id, kind, path, sha256, size_bytes, mime, created_at
                FROM artifacts
                WHERE run_id = ?1
                ORDER BY created_at ASC
                "#,
            )
            .map_err(sqlite_error("prepare list artifact records"))?;
        let rows = statement
            .query_map([run_id], row_to_artifact_record)
            .map_err(sqlite_error("list artifact records"))?;

        collect_rows(rows, "collect artifact records")
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

fn validate_required(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::validation_invalid_argument(
            field,
            "value cannot be empty",
            None,
            None,
        ));
    }
    Ok(())
}

fn serialize_metadata(metadata_json: &serde_json::Value) -> Result<String> {
    serde_json::to_string(metadata_json).map_err(|e| {
        Error::internal_json(e.to_string(), Some("serialize run metadata".to_string()))
    })
}

fn parse_metadata(raw: String) -> rusqlite::Result<serde_json::Value> {
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            raw.len(),
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })
}

fn row_to_run_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    Ok(RunRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        component_id: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        status: row.get(5)?,
        command: row.get(6)?,
        cwd: row.get(7)?,
        homeboy_version: row.get(8)?,
        git_sha: row.get(9)?,
        rig_id: row.get(10)?,
        metadata_json: parse_metadata(row.get(11)?)?,
    })
}

fn row_to_artifact_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactRecord> {
    Ok(ArtifactRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        sha256: row.get(4)?,
        size_bytes: row.get(5)?,
        mime: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
    context: &'static str,
) -> Result<Vec<T>> {
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(sqlite_error(context))?);
    }
    Ok(records)
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| {
        Error::internal_io(
            e.to_string(),
            Some(format!("read artifact bytes {}", path.display())),
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn mime_from_path(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let mime = match extension.as_str() {
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => return None,
    };
    Some(mime.to_string())
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
