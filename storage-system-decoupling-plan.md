# Storage System Decoupling Plan (Revised)

## Overview

Refactor homeboy's storage architecture from direct file I/O to a trait-based system where:
- **Core** depends only on a `Storage` trait (completely agnostic)
- **Implementations** are Rust structs that implement the trait
- **Config-driven** extension selection via `homeboy.json`
- **Extensions declare capabilities** in their manifest

## Goals

- **Decoupling**: Core doesn't know storage implementations, only the trait
- **Performance**: Direct function calls, no subprocess overhead
- **Config-driven**: Explicit extension selection, no filesystem scanning
- **Extensibility**: New storage backends = implement the Rust trait
- **No breaking changes**: Filesystem behavior preserved as default

---

## Architecture

### Core (Trait Consumer)

Homeboy core:
- Depends only on `trait Storage`
- Receives storage implementation at startup
- Has no knowledge of filesystem, database, or any specific backend

### Storage Implementations (Trait Providers)

Built-in implementations:
- `FilesystemStorage` - default, for CLI mode
- `PostgresStorage` - for server mode (future)
- `SqliteStorage` - for lightweight server (future)

---

## Data Contract

### Storage Trait

```rust
use std::path::Path;
use crate::error::Result;

pub trait Storage: Send + Sync {
    fn read(&self, path: &Path) -> Result<String>;
    fn write(&self, path: &Path, content: &str) -> Result<()>;
    fn list(&self, dir: &Path) -> Result<Vec<StorageEntry>>;
    fn delete(&self, path: &Path) -> Result<()>;
    fn ensure_dir(&self, dir: &Path) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Debug, Clone)]
pub struct StorageEntry {
    pub path: String,
    pub is_directory: bool,
}
```

---

## Configuration

### homeboy.json (App Config)

```json
{
  "storage": "builtin-filesystem",
  "installedModules": ["builtin-filesystem", "wordpress", "wp-scripts"]
}
```

- `storage`: Extension ID that provides storage (must declare `storage` capability)
- `installedModules`: Explicit list of active extensions (no directory scanning)

### Extension Manifest (Capability Declaration)

```json
// extensions/builtin-filesystem/builtin-filesystem.json
{
  "name": "Filesystem Storage",
  "version": "1.0.0",
  "capabilities": ["storage"],
  "storageBackend": "filesystem"
}
```

- `capabilities`: Array of capabilities this extension provides
- `storageBackend`: Identifies which Rust implementation to use

---

## Implementation Phases

### Phase 1: Define Storage Trait

**Create `src/core/storage.rs`**

```rust
use std::path::Path;
use crate::error::Result;

pub trait Storage: Send + Sync {
    fn read(&self, path: &Path) -> Result<String>;
    fn write(&self, path: &Path, content: &str) -> Result<()>;
    fn list(&self, dir: &Path) -> Result<Vec<StorageEntry>>;
    fn delete(&self, path: &Path) -> Result<()>;
    fn ensure_dir(&self, dir: &Path) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Debug, Clone)]
pub struct StorageEntry {
    pub path: String,
    pub is_directory: bool,
}
```

### Phase 2: Filesystem Implementation

**Create `src/core/storage/filesystem.rs`**

```rust
use super::{Storage, StorageEntry};
use crate::error::{Error, Result};
use std::fs;
use std::path::Path;

pub struct FilesystemStorage;

impl FilesystemStorage {
    pub fn new() -> Self {
        Self
    }
}

impl Storage for FilesystemStorage {
    fn read(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path)
            .map_err(|e| Error::internal_io(e.to_string(), Some(path.display().to_string())))
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::internal_io(e.to_string(), Some(parent.display().to_string())))?;
        }
        fs::write(path, content)
            .map_err(|e| Error::internal_io(e.to_string(), Some(path.display().to_string())))
    }

    fn list(&self, dir: &Path) -> Result<Vec<StorageEntry>> {
        let entries = fs::read_dir(dir)
            .map_err(|e| Error::internal_io(e.to_string(), Some(dir.display().to_string())))?;

        let mut result = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            result.push(StorageEntry {
                path: path.to_string_lossy().to_string(),
                is_directory: path.is_dir(),
            });
        }
        Ok(result)
    }

    fn delete(&self, path: &Path) -> Result<()> {
        if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
        .map_err(|e| Error::internal_io(e.to_string(), Some(path.display().to_string())))
    }

    fn ensure_dir(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)
            .map_err(|e| Error::internal_io(e.to_string(), Some(dir.display().to_string())))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}
```

### Phase 3: App Config Schema

**Update `src/core/config.rs` or create `src/core/app_config.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_storage")]
    pub storage: String,

    #[serde(default)]
    pub installed_modules: Vec<String>,
}

fn default_storage() -> String {
    "builtin-filesystem".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            storage: default_storage(),
            installed_modules: vec!["builtin-filesystem".to_string()],
        }
    }
}
```

### Phase 4: Extension Capability Declaration

**Update `ExtensionManifest` in `src/core/extension.rs`**

```rust
pub struct ExtensionManifest {
    // ... existing fields ...

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_backend: Option<String>,
}

impl ExtensionManifest {
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }

    pub fn provides_storage(&self) -> bool {
        self.has_capability("storage")
    }
}
```

### Phase 5: Storage Initialization

**Create `src/core/storage/mod.rs`**

```rust
mod filesystem;
pub use filesystem::FilesystemStorage;

use crate::error::{Error, Result};
use crate::extension::load_module;
use std::path::Path;
use std::sync::Arc;

pub trait Storage: Send + Sync {
    fn read(&self, path: &Path) -> Result<String>;
    fn write(&self, path: &Path, content: &str) -> Result<()>;
    fn list(&self, dir: &Path) -> Result<Vec<StorageEntry>>;
    fn delete(&self, path: &Path) -> Result<()>;
    fn ensure_dir(&self, dir: &Path) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Debug, Clone)]
pub struct StorageEntry {
    pub path: String,
    pub is_directory: bool,
}

pub fn create_storage(extension_id: &str) -> Result<Arc<dyn Storage>> {
    let extension = load_module(extension_id).ok_or_else(|| {
        Error::extension_not_found(extension_id.to_string(), vec![])
    })?;

    if !extension.provides_storage() {
        return Err(Error::validation_invalid_argument(
            "storage",
            format!("Extension '{}' does not provide storage capability", extension_id),
            Some(extension_id.to_string()),
            None,
        ));
    }

    let backend = extension.storage_backend.as_deref().unwrap_or("filesystem");

    match backend {
        "filesystem" => Ok(Arc::new(FilesystemStorage::new())),
        // Future: "postgres" => Ok(Arc::new(PostgresStorage::new()?)),
        // Future: "sqlite" => Ok(Arc::new(SqliteStorage::new()?)),
        other => Err(Error::validation_invalid_argument(
            "storage_backend",
            format!("Unknown storage backend: {}", other),
            Some(other.to_string()),
            None,
        )),
    }
}
```

### Phase 6: Bootstrap Flow

**Update initialization in `src/lib.rs` or main entry point**

```rust
use std::sync::OnceLock;

static STORAGE: OnceLock<Arc<dyn Storage>> = OnceLock::new();

pub fn init_storage() -> Result<()> {
    // Bootstrap: read homeboy.json with direct filesystem I/O
    // (this is the ONLY place we use direct fs, to break the chicken-egg problem)
    let config_path = paths::app_config()?;
    let app_config: AppConfig = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| Error::internal_io(e.to_string(), Some("read app config".to_string())))?;
        serde_json::from_str(&content)?
    } else {
        AppConfig::default()
    };

    let storage = storage::create_storage(&app_config.storage)?;
    STORAGE.set(storage).map_err(|_| Error::other("Storage already initialized"))?;

    Ok(())
}

pub fn storage() -> &'static Arc<dyn Storage> {
    STORAGE.get().expect("Storage not initialized - call init_storage() first")
}
```

### Phase 7: Migrate Call Sites

**Replace all `local_files::local()` calls with `storage()`**

```rust
// Before
let content = local_files::local().read(&path)?;

// After
let content = storage().read(&path)?;
```

**Files to update:**
- `src/core/config.rs` (~8 locations)
- `src/core/version.rs` (~10 locations)
- `src/core/changelog.rs` (~3 locations)
- `src/core/release.rs` (~1 location)
- `src/core/extension.rs` (~3 locations)
- `src/core/server.rs` (~3 locations)

### Phase 8: Create Default Extension

**Create `extensions/builtin-filesystem/builtin-filesystem.json`**

```json
{
  "name": "Filesystem Storage",
  "version": "1.0.0",
  "description": "Default filesystem storage for local CLI usage",
  "capabilities": ["storage"],
  "storageBackend": "filesystem"
}
```

**Auto-install on first run** (in `ensure_default_storage_module()`):
- Create extension directory if missing
- Write manifest file
- Add to `installedModules` in `homeboy.json`

### Phase 9: Update Extension Install/Uninstall

**Update `src/core/extension.rs`**

```rust
pub fn install(source: &str, id_override: Option<&str>) -> Result<InstallResult> {
    // ... existing clone/symlink logic ...

    // NEW: Add to installedModules in homeboy.json
    let mut app_config = load_app_config()?;
    if !app_config.installed_modules.contains(&extension_id) {
        app_config.installed_modules.push(extension_id.clone());
        save_app_config(&app_config)?;
    }

    Ok(result)
}

pub fn uninstall(extension_id: &str) -> Result<PathBuf> {
    // Prevent uninstalling active storage extension
    let app_config = load_app_config()?;
    if app_config.storage == extension_id {
        return Err(Error::validation_invalid_argument(
            "extension_id",
            "Cannot uninstall active storage extension. Change storage setting first.",
            Some(extension_id.to_string()),
            None,
        ));
    }

    // ... existing delete logic ...

    // NEW: Remove from installedModules in homeboy.json
    let mut app_config = load_app_config()?;
    app_config.installed_modules.retain(|m| m != extension_id);
    save_app_config(&app_config)?;

    Ok(path)
}
```

### Phase 10: Cleanup

- Delete `src/core/local_files.rs`
- Remove `FileSystem` trait
- Update `src/core/mod.rs` to export `storage` extension
- Run `cargo test --release` to verify

---

## File Structure After Refactor

```
src/core/
├── mod.rs              (add: pub mod storage)
├── storage/
│   ├── mod.rs          (trait + create_storage + global accessor)
│   └── filesystem.rs   (FilesystemStorage impl)
├── app_config.rs       (AppConfig struct)
├── config.rs           (updated to use storage())
├── extension.rs           (updated: capabilities, install/uninstall)
└── ...
```

---

## Future: Server Mode

When ready to add server deployment:

### Add PostgresStorage

```rust
// src/core/storage/postgres.rs
use sqlx::PgPool;

pub struct PostgresStorage {
    pool: PgPool,
}

impl Storage for PostgresStorage {
    fn read(&self, path: &Path) -> Result<String> {
        // SELECT content FROM storage WHERE path = $1
    }
    // ... etc
}
```

### Add Server Binary

```rust
// crates/homeboy-api/src/main.rs
#[tokio::main]
async fn main() {
    let pool = PgPool::connect(&env::var("DATABASE_URL")?).await?;
    let storage = Arc::new(PostgresStorage::new(pool));

    // Use storage for all operations
    // Expose HTTP API via axum
}
```

---

## Benefits

- **Decoupled**: Core only knows `trait Storage`
- **Fast**: Direct function calls, no subprocess/IPC overhead
- **Type-safe**: Compile-time guarantees
- **Config-driven**: Explicit extension selection in `homeboy.json`
- **Extensible**: New backends = implement trait in Rust
- **Testable**: Easy to mock storage in tests

---

## Testing Checklist

- [ ] `FilesystemStorage` passes all existing tests
- [ ] `storage()` global accessor works correctly
- [ ] Bootstrap reads `homeboy.json` with direct fs (breaks chicken-egg)
- [ ] Extension capability validation works
- [ ] Install/uninstall updates `homeboy.json`
- [ ] Cannot uninstall active storage extension
- [ ] Default extension auto-created on first run
- [ ] All `local_files::local()` calls migrated
- [ ] `local_files.rs` deleted
- [ ] `cargo test --release` passes

---

## Implementation Order

| Phase | Focus | Risk | Est. |
|-------|-------|------|------|
| 1 | Storage trait | Low | 0.5 day |
| 2 | FilesystemStorage | Low | 0.5 day |
| 3 | AppConfig schema | Low | 0.5 day |
| 4 | Extension capabilities | Low | 0.5 day |
| 5 | Storage initialization | Medium | 1 day |
| 6 | Bootstrap flow | Medium | 0.5 day |
| 7 | Migrate call sites | Medium | 2 days |
| 8 | Default extension | Low | 0.5 day |
| 9 | Install/uninstall | Low | 0.5 day |
| 10 | Cleanup | Low | 0.5 day |
| **Total** | | | **7 days** |
