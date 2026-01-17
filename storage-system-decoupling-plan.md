# Storage System Decoupling Plan

## Overview

Refactor homeboy's storage architecture from direct file I/O to an event-based system where:
- **Core** emits storage events and validates responses (data contract enforcer)
- **Modules** consume events and return data in standard JSON format
- **Any language** can implement storage (Python, PHP, Go, Rust, Bash, etc.)
- **All storage is provided by modules** - no built-in Rust implementation

## Goals

✅ **1-for-1 functionality**: Exactly one storage backend active
✅ **Enhanced extensibility**: Any module can provide storage without core changes
✅ **Loose coupling**: Core doesn't know storage implementations, only data contract
✅ **Runtime validation**: No compile-time enums, string-based module types
✅ **No breaking changes**: Existing filesystem behavior preserved via default module
✅ **Performance optimization**: Batch operations reduce subprocess overhead

---

## Architecture

### Core (Data Emitter + Contract Enforcer)

Homeboy core:
- Emits storage events as JSON to module stdin
- Validates module responses match data contract
- Doesn't know how/where data is stored
- Tracks exactly one active storage module
- Provides batch operation APIs for efficiency

### Modules (Data Consumers - ANY Language)

Storage modules:
- Read JSON events from stdin
- Handle operations using their own logic
- Return responses in standard JSON format to stdout
- Can be PHP, Python, Go, Rust, Bash, Node.js, etc.
- **Default module**: `builtin-filesystem` shipped with homeboy

---

## Data Contract

### Homeboy Emits (stdin → module)

```json
{
  "operation": "read|write|list|delete|ensure_dir|batch",
  "path": "/path/to/file",
  "content": "file contents",
  "is_directory": true,
  "operations": [ ... ]  // For batch operations
}
```

### Module MUST Return (stdout → homeboy)

```json
{
  "success": true|false,
  "data": "response data",
  "entries": [
    {"path": "/file1", "is_directory": false},
    {"path": "/dir1", "is_directory": true}
  ],
  "error": "error message",
  "results": [ ... ]  // For batch operations
}
```

### Response Validation Rules

- `success=true` + `error` set → Error (conflicting)
- `success=false` + no `error` field → Error (missing info)
- `success=true` + read operation but no `data` field → Error
- `success=true` + list operation but no `entries` field → Error
- `success=true` + batch operation but no `results` field → Error

---

## Implementation Phases

### Phase 1: Define Data Contract

**Create `core/storage.rs` with event/response types**

```rust
use std::path::PathBuf;
use crate::error::Result;

/// Storage event emitted by homeboy core to modules
#[derive(Debug, Serialize)]
pub struct StorageEvent {
    pub operation: String,
    pub path: String,
    pub content: Option<String>,
    pub is_directory: Option<bool>,
}

/// Batch storage event for multiple operations
#[derive(Debug, Serialize)]
pub struct BatchStorageEvent {
    pub operations: Vec<StorageEvent>,
}

/// Storage response that modules MUST return in standard format
#[derive(Debug, Deserialize)]
pub struct StorageResponse {
    pub success: bool,
    pub data: Option<String>,
    pub entries: Option<Vec<StorageEntry>>,
    pub error: Option<String>,
}

/// Batch storage response
#[derive(Debug, Deserialize)]
pub struct BatchStorageResponse {
    pub results: Vec<StorageResponse>,
    pub overall_success: bool,
}

#[derive(Debug, Deserialize)]
pub struct StorageEntry {
    pub path: String,
    pub is_directory: bool,
}
```

### Phase 2: Event Emitter Functions

**Core functions that emit events to modules (all 5 operations)**

```rust
pub fn read(path: &Path) -> Result<String> {
    let request = StorageEvent {
        operation: "read".to_string(),
        path: path.to_string_lossy().to_string(),
        content: None,
        is_directory: None,
    };

    let response = emit_storage_event(request)?;

    response.data.ok_or_else(|| {
        Error::other("Module returned success=true but no data field".to_string())
    })
}

pub fn write(path: &Path, content: &str) -> Result<()> {
    let request = StorageEvent {
        operation: "write".to_string(),
        path: path.to_string_lossy().to_string(),
        content: Some(content.to_string()),
        is_directory: None,
    };

    let response = emit_storage_event(request)?;

    if !response.success {
        return Err(Error::other(format!(
            "Storage operation failed: {}",
            response.error.unwrap_or_default()
        )));
    }

    Ok(())
}

pub fn list(dir: &Path) -> Result<Vec<StorageEntry>> {
    let request = StorageEvent {
        operation: "list".to_string(),
        path: dir.to_string_lossy().to_string(),
        content: None,
        is_directory: None,
    };

    let response = emit_storage_event(request)?;

    response.entries.ok_or_else(|| {
        Error::other("Module returned success=true but no entries field".to_string())
    })
}

pub fn delete(path: &Path) -> Result<()> {
    let request = StorageEvent {
        operation: "delete".to_string(),
        path: path.to_string_lossy().to_string(),
        content: None,
        is_directory: None,
    };

    let response = emit_storage_event(request)?;

    if !response.success {
        return Err(Error::other(format!(
            "Storage operation failed: {}",
            response.error.unwrap_or_default()
        )));
    }

    Ok(())
}

pub fn ensure_dir(dir: &Path) -> Result<()> {
    let request = StorageEvent {
        operation: "ensure_dir".to_string(),
        path: dir.to_string_lossy().to_string(),
        content: None,
        is_directory: Some(true),
    };

    let response = emit_storage_event(request)?;

    if !response.success {
        return Err(Error::other(format!(
            "Storage operation failed: {}",
            response.error.unwrap_or_default()
        )));
    }

    Ok(())
}

/// Batch operations for efficiency
pub fn read_many(paths: &[&Path]) -> Result<HashMap<String, String>> {
    let operations: Vec<_> = paths
        .iter()
        .map(|p| StorageEvent {
            operation: "read".to_string(),
            path: p.to_string_lossy().to_string(),
            content: None,
            is_directory: None,
        })
        .collect();

    let batch_event = BatchStorageEvent { operations };
    let response = emit_batch_storage_event(batch_event)?;

    let mut result = HashMap::new();
    for (op, resp) in response.results.iter() {
        if let Some(data) = &resp.data {
            result.insert(op.path.clone(), data.clone());
        }
    }

    Ok(result)
}

pub fn write_many(items: &[(&Path, &str)]) -> Result<()> {
    let operations: Vec<_> = items
        .iter()
        .map(|(path, content)| StorageEvent {
            operation: "write".to_string(),
            path: path.to_string_lossy().to_string(),
            content: Some(content.to_string()),
            is_directory: None,
        })
        .collect();

    let batch_event = BatchStorageEvent { operations };
    let response = emit_batch_storage_event(batch_event)?;

    if !response.overall_success {
        return Err(Error::other("Batch write operation failed".to_string()));
    }

    Ok(())
}

pub fn delete_many(paths: &[&Path]) -> Result<()> {
    let operations: Vec<_> = paths
        .iter()
        .map(|p| StorageEvent {
            operation: "delete".to_string(),
            path: p.to_string_lossy().to_string(),
            content: None,
            is_directory: None,
        })
        .collect();

    let batch_event = BatchStorageEvent { operations };
    let response = emit_batch_storage_event(batch_event)?;

    if !response.overall_success {
        return Err(Error::other("Batch delete operation failed".to_string()));
    }

    Ok(())
}
```

### Phase 3: Event Emitter (Module Communication)

**Function that calls active storage module subprocess with 30s timeout**

```rust
use std::process::{Command, Stdio};
use std::time::Duration;

const STORAGE_TIMEOUT_SECS: u64 = 30;

fn emit_storage_event(event: StorageEvent) -> Result<StorageResponse> {
    let module_info = get_active_storage_module()?;

    // Emit JSON to module via stdin
    let request_json = serde_json::to_string(&event)?;

    let mut child = Command::new(&module_info.executable)
        .args(["--storage-operation"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.as_mut().ok_or_else(|| {
        Error::other("Failed to open stdin for storage module".to_string())
    })?;

    stdin.write_all(request_json.as_bytes())?;

    let timeout = Duration::from_secs(STORAGE_TIMEOUT_SECS);
    let output = match child.wait_timeout(timeout) {
        Ok(Some(status)) if status.success() => {
            child.wait_with_output()?
        },
        Ok(Some(_)) => {
            return Err(Error::other(format!(
                "Storage module exited with non-zero status"
            )));
        },
        Ok(None) => {
            child.kill()?;
            return Err(Error::other(format!(
                "Storage operation timed out after {} seconds",
                STORAGE_TIMEOUT_SECS
            )));
        },
        Err(e) => {
            return Err(Error::other(format!(
                "Failed to wait for storage module: {}",
                e
            )));
        }
    };

    // Validate response format (data contract enforcer)
    let response: StorageResponse = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))
        .map_err(|e| Error::validation_invalid_json(e, Some("parse module response".to_string())))?;

    // Validate contract rules
    if response.success && response.error.is_some() {
        return Err(Error::other("Module returned success=true with error field set".to_string()));
    }

    if !response.success && response.error.is_none() {
        return Err(Error::other("Module returned success=false with no error message".to_string()));
    }

    Ok(response)
}

fn emit_batch_storage_event(event: BatchStorageEvent) -> Result<BatchStorageResponse> {
    let module_info = get_active_storage_module()?;
    let request_json = serde_json::to_string(&event)?;

    let mut child = Command::new(&module_info.executable)
        .args(["--storage-operation"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child.stdin.as_mut().unwrap().write_all(request_json.as_bytes())?;

    let timeout = Duration::from_secs(STORAGE_TIMEOUT_SECS);
    let output = match child.wait_timeout(timeout) {
        Ok(Some(status)) if status.success() => child.wait_with_output()?,
        Ok(None) => {
            child.kill()?;
            return Err(Error::other(format!(
                "Batch storage operation timed out after {} seconds",
                STORAGE_TIMEOUT_SECS
            )));
        },
        Err(e) => {
            return Err(Error::other(format!("Failed to wait for storage module: {}", e)));
        }
    };

    serde_json::from_str(&String::from_utf8_lossy(&output.stdout))
        .map_err(|e| Error::validation_invalid_json(e, Some("parse batch response".to_string())))
}
```

### Phase 4: Active Storage Module Tracking

**Track exactly one active storage module**

```rust
use std::sync::RwLock;

static ACTIVE_STORAGE_MODULE: RwLock<Option<StorageModuleInfo>> = RwLock::new(None);

#[derive(Debug, Clone)]
pub struct StorageModuleInfo {
    pub module_id: String,
    pub storage_type: String,
    pub executable: String,
    pub supports_batch: bool,
}

pub fn set_active_storage_module(
    module_id: String,
    storage_type: String,
    executable: String,
    supports_batch: bool,
) -> Result<()> {
    let mut storage = ACTIVE_STORAGE_MODULE.write().unwrap();

    if storage.is_some() {
        return Err(Error::validation_invalid_argument(
            "storage",
            "Multiple storage modules detected. Only one storage provider can be active at a time.",
            None,
            None,
        ));
    }

    *storage = Some(StorageModuleInfo {
        module_id,
        storage_type,
        executable,
        supports_batch,
    });

    Ok(())
}

pub fn get_active_storage_module() -> Result<StorageModuleInfo> {
    ACTIVE_STORAGE_MODULE.read().unwrap()
        .as_ref()
        .cloned()
        .ok_or_else(|| Error::internal_unexpected(
            "No storage provider registered. Install a module with 'storage_provider' capability.".to_string()
        ))
}
```

### Phase 5: Default Filesystem Module

**Create `modules/builtin-filesystem/` as the default storage provider**

**`modules/builtin-filesystem/builtin-filesystem.json`:**

```json
{
  "name": "Filesystem Storage",
  "version": "1.0.0",
  "description": "Default filesystem storage module",
  "runtime": {
    "executable": "bash storage.sh",
    "supports_batch": true
  },
  "storage_provider": {
    "storage_type": "filesystem"
  }
}
```

**`modules/builtin-filesystem/storage.sh`:**

```bash
#!/bin/bash
set -euo pipefail

read -r input

operation=$(echo "$input" | jq -r '.operation')

case "$operation" in
  read)
    path=$(echo "$input" | jq -r '.path')
    if [ -f "$path" ]; then
      jq -n '{"success": true, "data": $data}' --rawfile data "$path"
    else
      jq -n '{"success": false, "error": "File not found"}'
    fi
    ;;

  write)
    path=$(echo "$input" | jq -r '.path')
    content=$(echo "$input" | jq -r '.content')

    # Atomic write: write to temp file, then rename
    parent=$(dirname "$path")
    filename=$(basename "$path")
    tmp_path="${parent}/.${filename}.tmp"

    echo "$content" > "$tmp_path"
    mv "$tmp_path" "$path"

    jq -n '{"success": true}'
    ;;

  list)
    path=$(echo "$input" | jq -r '.path')
    if [ ! -d "$path" ]; then
      jq -n '{"success": false, "error": "Directory not found"}'
      exit 0
    fi

    # Build entries array
    entries=$(find "$path" -maxdepth 1 -mindepth 1 | jq -R 'split("\n") | map(select(length > 0)) | map({
      path: .,
      is_directory: (if test -d . then true else false end)
    })')

    jq -n '{"success": true, "entries": $entries}' --argjson entries "$entries"
    ;;

  delete)
    path=$(echo "$input" | jq -r '.path')
    if [ ! -e "$path" ]; then
      jq -n '{"success": false, "error": "File not found"}'
      exit 0
    fi

    rm "$path"
    jq -n '{"success": true}'
    ;;

  ensure_dir)
    path=$(echo "$input" | jq -r '.path')
    mkdir -p "$path"
    jq -n '{"success": true}'
    ;;

  batch)
    results=$(echo "$input" | jq -c '.operations[]' | while read -r op; do
      op_json=$(echo "$op" | jq -c)

      # Re-invoke this script for each operation
      echo "$op_json" | "$0"
    done | jq -s '.')

    overall_success=$(echo "$results" | jq 'all(.success)')

    jq -n '{
      results: $results,
      overall_success: $overall_success
    }' --argjson results "$results" --argjson overall_success "$overall_success"
    ;;

  *)
    jq -n '{"success": false, "error": "Unknown operation"}'
    exit 1
    ;;
esac
```

**Auto-install default module on first run:**

```rust
// In module.rs or initialization
fn ensure_default_storage_module() -> Result<()> {
    let module_dir = paths::module("builtin-filesystem")?;
    let manifest_path = module_dir.join("builtin-filesystem.json");

    if !module_dir.exists() {
        // Create default module directory
        fs::create_dir_all(&module_dir)?;

        // Write default manifest
        let manifest = r#"{
  "name": "Filesystem Storage",
  "version": "1.0.0",
  "runtime": {
    "executable": "bash storage.sh"
  },
  "storage_provider": {
    "storage_type": "filesystem"
  }
}"#;

        let storage_script = include_str!("../../modules/builtin-filesystem/storage.sh");

        fs::write(&manifest_path, manifest)?;
        fs::write(module_dir.join("storage.sh"), storage_script)?;

        // Make script executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&module_dir.join("storage.sh"))?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(module_dir.join("storage.sh"), perms)?;
        }
    }

    Ok(())
}
```

### Phase 6: Module Discovery and Activation

**Update `load_all_modules()` in `core/module.rs`**

```rust
pub fn load_all_modules() -> Vec<ModuleManifest> {
    // Ensure default storage module exists
    let _ = ensure_default_storage_module();

    let mut modules = /* existing module loading logic */;

    // Discover storage providers
    let storage_modules: Vec<_> = modules
        .iter()
        .filter(|m| m.storage_provider.is_some())
        .collect();

    // Activate storage based on module count
    match storage_modules.len() {
        0 => {
            eprintln!("[warn] No storage module found. Using builtin-filesystem.");
            let _ = storage::set_active_storage_module(
                "builtin-filesystem".to_string(),
                "filesystem".to_string(),
                format!(
                    "bash {}/storage.sh",
                    paths::module("builtin-filesystem").unwrap().display()
                ),
                true,  // supports batch
            );
        }
        1 => {
            // Exactly one storage module - activate it
            let module = &storage_modules[0];
            let storage_info = module.storage_provider.as_ref().unwrap();

            // Get executable from runtime config
            let executable = module.runtime.as_ref()
                .and_then(|r| r.executable.clone())
                .unwrap_or_else(|| {
                    // Default: module path + "storage.sh"
                    let path = PathBuf::from(&module.module_path.as_ref().unwrap());
                    path.join("storage.sh").to_string_lossy().to_string()
                });

            let supports_batch = module.runtime.as_ref()
                .and_then(|r| r.supports_batch)
                .unwrap_or(false);

            if let Err(e) = storage::set_active_storage_module(
                module.id.clone(),
                storage_info.storage_type.clone(),
                executable,
                supports_batch,
            ) {
                eprintln!("[warn] Failed to activate storage module '{}': {}", module.id, e);
            }
        }
        _ => {
            // Multiple storage modules - error with helpful message
            let available: Vec<_> = storage_modules
                .iter()
                .map(|m| format!(
                    "{} ({})",
                    m.id,
                    m.storage_provider.as_ref().unwrap().storage_type
                ))
                .collect();

            eprintln!(
                "[error] Multiple storage modules detected:\n\
                 {}\n\
                 Only one storage provider can be active at a time.\n\
                 Uninstall module you don't want:\n\
                 homeboy module uninstall <module-id>\n\
                 Keep only the one you want to use.",
                available.join("\n  ")
            );
        }
    }

    modules
}
```

### Phase 7: Module Schema Updates

**Add storage capability to ModuleManifest**

```rust
pub struct ModuleManifest {
    // ... existing fields ...

    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_provider: Option<StorageProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageProvider {
    /// Storage type identifier (string key, runtime validated)
    pub storage_type: String,

    /// Backend-specific config (optional, module-defined)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}
```

**Add executable and supports_batch fields to RuntimeConfig**

```rust
pub struct RuntimeConfig {
    // ... existing fields ...

    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_batch: Option<bool>,
}
```

### Phase 8: Update Config Operations

**Replace all direct file I/O with event emissions**

```rust
// Before (in core/config.rs)
let content = local_files::local().read(&path)?;

// After
let content = storage::read(&path)?;
```

**Update all config CRUD functions:**
- `load()` → `storage::read()`
- `save()` → `storage::write()`
- `list()` → `storage::list()`
- `delete()` → `storage::delete()`

**Search and replace `local_files::local()` calls in:**
- `src/core/version.rs` (10 locations)
- `src/core/changelog.rs` (3 locations)
- `src/core/release.rs` (1 location)
- `src/core/module.rs` (3 locations)
- `src/core/server.rs` (3 locations)
- `src/core/config.rs` (8 locations)

**Update imports:**
- Remove: `use crate::local_files::{self, FileSystem};`
- Add: `use crate::storage;`

### Phase 9: Remove Legacy Code

**Delete `src/core/local_files.rs`**

```bash
rm src/core/local_files.rs
```

**Remove FileSystem trait from any remaining imports**

**Update lib.rs** to remove module declaration if present.

---

## Error Messages

### No storage registered

```rust
Error::internal_unexpected(
    "No storage provider registered. Install a module with 'storage_provider' capability.".to_string()
)
```

### Multiple storage modules

```rust
Error::validation_invalid_argument(
    "storage",
    format!(
        "Multiple storage modules detected. Only one storage provider can be active at a time.\n\
         Uninstall module you don't want: homeboy module uninstall <module-id>\n\
         Installed storage modules: {}",
        available.join(", ")
    ),
    None,
    None,
)
```

### Storage operation timeout

```rust
Error::other(format!(
    "Storage operation timed out after {} seconds",
    STORAGE_TIMEOUT_SECS
))
```

---

## Module Examples

### Example 1: PHP Filesystem Storage

**`modules/filesystem-php/storage.php`**

```php
<?php
$input = file_get_contents('php://stdin');
$request = json_decode($input, true);

switch ($request['operation']) {
    case 'read':
        $content = file_get_contents($request['path']);
        echo json_encode([
            'success' => true,
            'data' => $content
        ]);
        break;

    case 'write':
        file_put_contents($request['path'], $request['content']);
        echo json_encode(['success' => true]);
        break;

    case 'list':
        $entries = [];
        foreach (scandir($request['path']) as $item) {
            if ($item == '.' || $item == '..') continue;
            $entries[] = [
                'path' => $request['path'] . '/' . $item,
                'is_directory' => is_dir($request['path'] . '/' . $item)
            ];
        }
        echo json_encode([
            'success' => true,
            'entries' => $entries
        ]);
        break;

    case 'delete':
        if (file_exists($request['path'])) {
            unlink($request['path']);
            echo json_encode(['success' => true]);
        } else {
            echo json_encode([
                'success' => false,
                'error' => 'File not found'
            ]);
        }
        break;

    case 'batch':
        $results = [];
        foreach ($request['operations'] as $op) {
            // Handle each operation (re-invoke self)
            // ...
        }
        echo json_encode([
            'success' => true,
            'results' => $results,
            'overall_success' => true
        ]);
        break;
}
```

### Example 2: SQLite Storage (Python)

**`modules/sqlite-storage/storage.py`**

```python
#!/usr/bin/env python3
import sys, json, sqlite3

request = json.load(sys.stdin)

conn = sqlite3.connect('homeboy.db')
cursor = conn.cursor()

if request['operation'] == 'read':
    cursor.execute('SELECT content FROM storage WHERE path = ?', (request['path'],))
    result = cursor.fetchone()
    if result:
        data = result[0]
        print(json.dumps({'success': True, 'data': data}))
    else:
        print(json.dumps({'success': False, 'error': 'Not found'}))

elif request['operation'] == 'write':
    cursor.execute('''
        INSERT OR REPLACE INTO storage (path, content)
        VALUES (?, ?)
    ''', (request['path'], request['content']))
    conn.commit()
    print(json.dumps({'success': True}))

elif request['operation'] == 'list':
    cursor.execute('SELECT path FROM storage WHERE path LIKE ?', (request['path'] + '%',))
    results = cursor.fetchall()
    entries = [{'path': r[0], 'is_directory': False} for r in results]
    print(json.dumps({'success': True, 'entries': entries}))

elif request['operation'] == 'delete':
    cursor.execute('DELETE FROM storage WHERE path = ?', (request['path'],))
    conn.commit()
    print(json.dumps({'success': True}))

elif request['operation'] == 'batch':
    results = []
    for op in request['operations']:
        # Process each operation
        # ...
        results.append({'success': True})
    print(json.dumps({
        'success': True,
        'results': results,
        'overall_success': True
    }))

conn.close()
```

---

## Example Module Manifests

### Default Filesystem Module (builtin)

```json
{
  "name": "Filesystem Storage",
  "version": "1.0.0",
  "runtime": {
    "executable": "bash storage.sh",
    "supports_batch": true
  },
  "storage_provider": {
    "storage_type": "filesystem"
  }
}
```

### SQLite Storage Module

```json
{
  "name": "SQLite Storage",
  "version": "1.0.0",
  "runtime": {
    "executable": "python3 storage.py",
    "supports_batch": true
  },
  "storage_provider": {
    "storage_type": "sqlite",
    "config": {
      "database_path": "homeboy.db"
    }
  }
}
```

### PHP Storage Module

```json
{
  "name": "PHP Filesystem",
  "version": "1.0.0",
  "runtime": {
    "executable": "php storage.php",
    "supports_batch": false
  },
  "storage_provider": {
    "storage_type": "php-filesystem"
  }
}
```

---

## Architecture Summary

| Aspect | Before | After |
|--------|---------|--------|
| **Core knows** | Direct file I/O implementation | Only data contract |
| **Modules provide** | Rust traits only | JSON I/O (any language) |
| **Coupling** | Tight (compile-time linking) | Loose (runtime events) |
| **Extensibility** | Limited to Rust modules | Any language (PHP, Python, Go, etc.) |
| **Validation** | Compile-time (enum types) | Runtime (data contract validation) |
| **Default storage** | Built-in Rust code | Default module (`builtin-filesystem`) |
| **Performance** | Direct function calls | Subprocess + batch operations |

---

## Implementation Order

| Phase | Focus | Key Changes | Risk | Est. Time |
|--------|--------|--------------|------|-----------|
| 1 | Data contract | Create types in `core/storage.rs` | Low | 0.5 day |
| 2 | Event emitters | All 5 ops + batch APIs in `core/storage.rs` | Medium | 1 day |
| 3 | Module tracking | `set/get_active_storage_module()` | Low | 0.5 day |
| 4 | Default module | Create `builtin-filesystem` module | Low | 1 day |
| 5 | Module discovery | Update `load_all_modules()` for auto-activation | Medium | 1-2 days |
| 6 | Module schema | Add `storage_provider`, `executable`, `supports_batch` | Low | 0.5 day |
| 7 | Config migration | Replace 28 `local_files::local()` calls | Medium | 2-3 days |
| 8 | Cleanup | Delete `local_files.rs`, remove `FileSystem` trait | Low | 0.5 day |
| 9 | Error messages | Add timeout, validation error handling | Low | 0.5 day |
| 10 | Documentation | Module authoring guide, examples | Low | 1 day |
| **Total** | | | **7-10 days** |

---

## Benefits

✅ **1-for-1 functionality** - Exactly one storage backend active
✅ **Enhanced extensibility** - Any language can implement storage
✅ **Loose coupling** - Core doesn't know implementation details
✅ **Runtime validation** - No compile-time enum restrictions
✅ **No breaking changes** - Existing filesystem behavior preserved via default module
✅ **PHP storage works** - Any language can provide storage
✅ **Data contract enforced** - Modules must return valid JSON
✅ **Module-owned business logic** - Modules handle migration, optimization
✅ **Performance optimization** - Batch operations reduce subprocess overhead
✅ **Timeout protection** - 30s timeout prevents hanging modules

---

## Testing Checklist

- [ ] Default filesystem module works identically to current implementation
- [ ] Event emitter correctly spawns module subprocess
- [ ] Module responses are validated against data contract
- [ ] Single storage module activates automatically
- [ ] Multiple storage modules show helpful error with list
- [ ] `local_files::local()` calls replaced with event emitters
- [ ] All existing tests pass
- [ ] No performance regression for default filesystem
- [ ] Subprocess communication handles JSON correctly
- [ ] 30s timeout prevents hanging operations
- [ ] Batch operations work correctly
- [ ] Error messages are clear and actionable
- [ ] PHP module example works end-to-end
- [ ] `local_files.rs` successfully removed
- [ ] `FileSystem` trait successfully removed

---

## Future Enhancements (v2)

### Persistent Process Architecture

**Concept:**
Instead of spawning a new subprocess for each operation, storage modules can run as persistent daemons that maintain state, connection pools, and caches.

**Implementation Options:**

1. **Unix Domain Socket:**
   - Module creates Unix socket at `~/.config/homeboy/storage/<module-id>.sock`
   - Core connects to socket, sends JSON, receives response
   - Faster than spawning subprocess
   - Maintains DB connections, HTTP clients, caches

2. **Named Pipe (FIFO):**
   - Module creates FIFO at `~/.config/homeboy/storage/<module-id>.fifo`
   - Core writes JSON, reads response
   - Simpler than socket but single-client

3. **Process Pool:**
   - Core spawns N persistent worker processes
   - Round-robin operations across workers
   - Parallel processing for bulk operations

**Module Manifest Extensions:**

```json
{
  "runtime": {
    "persistent": true,
    "socket_path": "~/.config/homeboy/storage/my-storage.sock",
    "ready_check": "test -S ~/.config/homeboy/storage/my-storage.sock"
  }
}
```

**Lifecycle:**
1. Homeboy starts
2. `load_all_modules()` discovers storage modules
3. For persistent modules: check if socket exists, if not, start daemon
4. Core connects to socket for operations
5. Homeboy stops: send shutdown signal to daemons

**Benefits:**
- Dramatically reduced overhead for frequent operations
- Module maintains expensive resources (DB pools, HTTP clients)
- Better for high-frequency storage operations

**Trade-offs:**
- More complex (process lifecycle, signal handling)
- Resource leaks if module crashes
- Harder to debug (background processes)
- Need to handle module restart/recovery

**Decision:** Defer to v2. Subprocess + batch operations provides good performance for now.
