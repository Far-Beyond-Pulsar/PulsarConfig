# PulsarConfig

> **A schema-first, plugin-isolated, concurrent configuration library for games and applications.**

PulsarConfig is a centralized in-memory settings store built around three ideas:

1. **Schemas are declared upfront.** Every key, its type, default value, and validation rules are registered before any value is written.
2. **Owners are isolated.** Each plugin or subsystem receives a scoped `OwnerHandle`. It can only write to its own keys — cross-owner writes are structurally impossible.
3. **Reads are fast and concurrent.** The backing store is a `DashMap`, giving fine-grained shard locking with near-contention-free concurrent reads.

---

## Features

| | |
|---|---|
| **N-level owner paths** | Owner paths can be any depth: `"audio"`, `"plugin/audio"`, `"subsystem/physics/main"` |
| **Schema-first** | Declare keys, types, defaults, and validators before the first write |
| **Plugin isolation** | Each subsystem holds an `OwnerHandle` — no accidental cross-owner mutations |
| **Concurrent** | `DashMap`-backed; reads are effectively contention-free |
| **Reactive** | RAII `ListenerId` guards for key-level, owner-level, and global listeners |
| **Type-safe accessors** | `get_bool`, `get_int`, `get_float`, `get_string`, `get_color` — all return `Result` |
| **Rich validation** | Integer/float ranges, string-length limits, allowlists, and custom closures |
| **Full-text search** | Query every key, description, and tag across all owners in one call |
| **TOML persistence** | `ConfigStore` saves and loads each owner's settings to a human-editable `.toml` file |

---

## The Three-Tier Hierarchy

```
namespace          ("editor" vs "project")
  └── owner path   ("subsystem/physics/main")   ← any depth, slash-separated
        └── key    ("gravity")
```

- **Namespace** — top-level scope, typically one per domain (e.g. `"editor"`, `"runtime"`, `"project"`).
- **Owner path** — identifies the specific plugin or subsystem. Segments are slash-separated and can be arbitrarily deep.
- **Key** — a single setting within that owner.

---

## Quick Start

```rust
use pulsar_config::{
    Color, ConfigManager, NamespaceSchema, SchemaEntry, Validator,
};

// 1. Create the manager — share it throughout your app (Clone + Send + Sync).
let manager = ConfigManager::new();

// 2. A plugin declares its schema and registers with a namespace + owner path.
let schema = NamespaceSchema::new("Shadow Renderer", "Controls shadow rendering")
    .setting("enabled",
        SchemaEntry::new("Enable shadow rendering", true))
    .setting("max_distance",
        SchemaEntry::new("Max draw distance (world units)", 500.0_f64)
            .validator(Validator::float_range(0.0, 10_000.0))
            .tag("performance"))
    .setting("quality",
        SchemaEntry::new("Quality preset", "high")
            .validator(Validator::string_one_of(["low", "medium", "high", "ultra"]))
            .tags(["rendering", "quality"]))
    .setting("tint",
        SchemaEntry::new("Shadow tint color", Color::rgba(0, 0, 0, 128)));

// `shadows` is the plugin's private write handle.
let shadows = manager.register("editor", "renderer/shadows", schema).unwrap();

// 3. Read — always returns a value; defaults are pre-seeded at registration.
let enabled      = shadows.get_bool("enabled").unwrap();
let max_distance = shadows.get_float("max_distance").unwrap();

// 4. Write — validated against the schema.
shadows.set("max_distance", 1_000.0_f64).unwrap();
shadows.set("max_distance", 99_999.0_f64).unwrap_err(); // out of range → error

// 5. Subscribe to changes. The listener is removed when `_guard` is dropped.
let _guard = shadows.on_change("enabled", |event| {
    println!("enabled changed to {:?}", event.new_value);
}).unwrap();

// 6. Cross-owner read (no handle required).
let v = manager.get("editor", "renderer/shadows", "quality").unwrap();

// 7. Search across every owner, namespace, and tag.
let results = manager.search("performance");
```

---

## Core Concepts

### `ConfigManager`

The root object. Create one per application and clone it freely — it is `Clone + Send + Sync`.

```rust
let manager = ConfigManager::new();

// Register an owner and receive its handle.
let handle = manager.register("editor", "plugin/audio", schema).unwrap();

// Cross-owner read (read-only path).
let v = manager.get("editor", "plugin/audio", "master_volume")?;

// Retrieve an existing handle by path.
let h = manager.owner_handle("editor", "plugin/audio").unwrap();

// Discovery.
let namespaces: Vec<String>         = manager.list_namespaces();
let owners: Vec<Vec<String>>        = manager.list_owners("editor");
let all: Vec<(String, Vec<String>)> = manager.list_all_owners();

// Metadata for a settings-UI panel.
let infos = manager.list_settings("editor", "plugin/audio").unwrap();

// Full-text search across all keys, descriptions, and tags.
let results = manager.search("performance");

// Global change listener.
let _guard = manager.on_any_change(|event| {
    println!("{}/{}/{} changed", event.namespace, event.owner_path(), event.key);
});
```

### `OwnerHandle`

The write handle returned by `register`. It is `Clone + Send + Sync` — safe to store in an `Arc` or share across threads.

```rust
// Typed reads.
let vol   = handle.get_int("master_volume")?;
let label = handle.get_string("display_name")?;
let tint  = handle.get_color("ui_tint")?;

// Typed write (validated against schema).
handle.set("master_volume", 75_i64)?;

// Reset a key to its schema default.
handle.reset_to_default("master_volume")?;

// Accessors.
handle.namespace();   // → &str
handle.owner();       // → &[String]   (individual path segments)
handle.owner_path();  // → String      (slash-joined, e.g. "plugin/audio")

// Owner-scoped listener — fires for any key write on this owner.
let _guard = handle.on_any_change(|e| println!("{} → {}", e.key, e.new_value));

// Key-scoped listener.
let _guard = handle.on_change("master_volume", |e| { /* ... */ })?;
```

### N-Level Owner Paths

Owner paths can be as shallow or deep as your architecture requires. Leading/trailing slashes and consecutive slashes are normalized automatically.

```rust
manager.register("ns", "audio",                  schema)?; // 1 segment
manager.register("ns", "plugin/audio",           schema)?; // 2 segments
manager.register("ns", "subsystem/physics/main", schema)?; // 3 segments
manager.register("ns", "a/b/c/d/e/f",            schema)?; // 6 segments

// These all resolve to the same owner:
manager.owner_handle("ns", "plugin/audio");
manager.owner_handle("ns", "/plugin/audio");
manager.owner_handle("ns", "/plugin/audio/");
```

Owners sharing a common prefix are completely independent — `"sub/core"` and `"sub/core/extra"` never interfere with each other.

### `NamespaceSchema` & `SchemaEntry`

Schemas are built with a fluent API and passed once to `register`.

```rust
let schema = NamespaceSchema::new("Audio Engine", "Audio engine settings")
    .setting("master_volume",
        SchemaEntry::new("Master volume (0–100)", 80_i64)
            .validator(Validator::int_range(0, 100))
            .tag("audio"))
    .setting("sample_rate",
        SchemaEntry::new("Output sample rate (Hz)", 48_000_i64)
            .read_only())            // prevents runtime writes
    .setting("reverb",
        SchemaEntry::new("Enable reverb", true)
            .description("Applies convolution reverb to the master bus")
            .tags(["audio", "quality"]));
```

### Validators

Chain as many validators as needed — all must pass or the write is rejected and the previous value is preserved.

| Validator | Description |
|-----------|-------------|
| `Validator::int_range(min, max)` | Inclusive integer bounds |
| `Validator::float_range(min, max)` | Inclusive float bounds |
| `Validator::string_max_length(n)` | Maximum byte length |
| `Validator::string_one_of([...])` | Exact-match allowlist (case-sensitive) |
| `Validator::custom(fn)` | Arbitrary `Send + Sync` closure |

```rust
SchemaEntry::new("Thread count", 4_i64)
    .validator(Validator::int_range(1, 64))
    .validator(Validator::custom(|v| {
        let n = v.as_int()?;
        if n.count_ones() == 1 { Ok(()) }
        else { Err("must be a power of two".into()) }
    }));
```

### `ConfigValue` — Supported Types

| Variant | Rust type | Accessor |
|---------|-----------|----------|
| `Bool` | `bool` | `as_bool()` / `get_bool()` |
| `Int` | `i64` | `as_int()` / `get_int()` |
| `Float` | `f64` | `as_float()` / `get_float()` (also accepts `Int`) |
| `String` | `String` | `as_str()` / `get_string()` |
| `Color` | `Color` | `as_color()` / `get_color()` |
| `Array` | `Vec<ConfigValue>` | `as_array()` |

Standard Rust primitives (`bool`, `i32`, `i64`, `u32`, `f32`, `f64`, `&str`, `String`) and `Color` all implement `Into<ConfigValue>` automatically.

### `Color`

An sRGB RGBA color (8 bits per channel).

```rust
let red   = Color::rgb(255, 0, 0);
let tint  = Color::rgba(0, 0, 0, 128);
let white = Color::from_hex(0xFFFFFFFF);

// Predefined constants.
let _ = Color::WHITE;
let _ = Color::BLACK;
let _ = Color::TRANSPARENT;

let packed: u32      = red.to_hex();
let linear: [f32; 4] = tint.to_linear_f32(); // sRGB → linear, for GPU upload
```

### Change Listeners & `ListenerId`

Listeners are automatically removed when their `ListenerId` guard is dropped — no manual deregistration needed.

```rust
// Key-level — fires only when "enabled" changes on this owner.
let _key_guard = handle.on_change("enabled", |e| {
    println!("old={:?}  new={:?}", e.old_value, e.new_value);
})?;

// Owner-level — fires for any key write on this owner.
let _owner_guard = handle.on_any_change(|e| println!("{}", e.key));

// Global — fires for every change across every namespace and owner.
let _global_guard = manager.on_any_change(|e| {
    println!("{}/{}/{}", e.namespace, e.owner_path(), e.key);
});

// ChangeEvent fields:
//   e.namespace    → String
//   e.owner        → Vec<String>   (individual segments)
//   e.key          → String
//   e.old_value    → Option<ConfigValue>   (Some(default) on the first write)
//   e.new_value    → ConfigValue
//   e.owner_path() → String               (slash-joined)
```

---

## Plugin Registration Pattern

Each plugin calls `register` exactly once (at startup or load time) and stores the returned `OwnerHandle` for its lifetime.

```rust
use pulsar_config::{ConfigManager, NamespaceSchema, SchemaEntry, Validator, OwnerHandle};

pub struct AudioPlugin {
    config: OwnerHandle,
}

impl AudioPlugin {
    pub fn new(manager: &ConfigManager) -> Self {
        let schema = NamespaceSchema::new("Audio", "Audio engine settings")
            .setting("master_volume",
                SchemaEntry::new("Master volume (0–100)", 80_i64)
                    .validator(Validator::int_range(0, 100)))
            .setting("sample_rate",
                SchemaEntry::new("Output sample rate", 48_000_i64)
                    .read_only())
            .setting("reverb",
                SchemaEntry::new("Enable reverb", true)
                    .tag("quality"));

        let config = manager
            .register("editor", "plugin/audio", schema)
            .expect("plugin/audio already registered");

        Self { config }
    }

    pub fn master_volume(&self) -> i64 {
        self.config.get_int("master_volume").unwrap()
    }

    pub fn set_master_volume(&self, v: i64) -> Result<(), pulsar_config::ConfigError> {
        self.config.set("master_volume", v)
    }
}
```

---

## Persistence

`ConfigStore` wraps a `ConfigManager` and maps each `(namespace, owner)` pair to a human-editable `.toml` file. Files mirror the owner hierarchy inside a root directory.

### Platform directories (`ConfigStore::new`)

| Platform | Location |
|----------|----------|
| Linux    | `~/.config/<app_name>/` |
| macOS    | `~/Library/Application Support/<app_name>/` |
| Windows  | `%APPDATA%\<app_name>\` |

Use `ConfigStore::with_dir` to supply an explicit path (useful in tests or portable deployments).

### File layout

```
<config_dir>/
  editor/
    plugin/
      audio.toml          ← "editor" / "plugin/audio"
    renderer/
      shadows.toml        ← "editor" / "renderer/shadows"
  project/
    audio.toml            ← "project" / "audio"
```

### File format

```toml
# PulsarConfig — editor/plugin/audio
# Edit this file to override application defaults.
# Missing keys fall back to the schema default.
# Read-only settings are managed by the application and are not listed here.

master_volume = 80
reverb = true
```

### Usage

```rust
use pulsar_config::{ConfigManager, ConfigStore, NamespaceSchema, SchemaEntry, Validator};

let manager = ConfigManager::new();
let schema  = NamespaceSchema::new("Audio", "")
    .setting("master_volume", SchemaEntry::new("", 80_i64)
        .validator(Validator::int_range(0, 100)));
let audio = manager.register("editor", "plugin/audio", schema).unwrap();

// Wrap with persistence (resolves to the platform config directory).
let store = ConfigStore::new(manager, "my_app").unwrap();

// Save one owner.
store.save("editor", "plugin/audio").unwrap();

// Save every owner in a namespace.
store.save_namespace("editor").unwrap();

// Save everything.
store.save_all().unwrap();

// Load — returns false if no file exists yet (first run).
if !store.load(&audio).unwrap() {
    println!("first run — schema defaults are active");
}

// Load a list of handles; returns the ones with no persisted file.
let first_run_owners = store.load_all([&audio]).unwrap();
```

**Resilience guarantees during load:**

| Situation | Behaviour |
|-----------|-----------|
| Key in file, not in schema | Silently skipped |
| Key in schema, not in file | Schema default retained |
| Type mismatch in file | Silently skipped; schema default retained |
| Value fails validation | Silently skipped; schema default retained |
| Read-only key in file | Silently skipped |
| Malformed TOML | Returns `PersistError::TomlParse` |

---

## Error Handling

All fallible operations return a typed `Result`:

```rust
use pulsar_config::ConfigError;

match handle.set("gravity", 9999.0_f64) {
    Ok(())                                             => { /* accepted */ }
    Err(ConfigError::ValidationFailed { key, reason, .. }) =>
        eprintln!("{key} rejected: {reason}"),
    Err(ConfigError::ReadOnly { key, .. })             =>
        eprintln!("{key} is read-only"),
    Err(ConfigError::UnknownKey { key, .. })           =>
        eprintln!("no such key: {key}"),
    Err(e)                                             => eprintln!("error: {e}"),
}
```

| Variant | Meaning |
|---------|---------|
| `OwnerAlreadyRegistered` | `register` called twice for the same `(namespace, owner)` |
| `OwnerNotFound` | `get` / `list_settings` called for an unregistered owner |
| `UnknownKey` | Key does not exist in the owner's schema |
| `ValidationFailed` | A validator rejected the value |
| `ReadOnly` | Attempted write to a read-only key |
| `TypeMismatch` | Typed accessor called on the wrong variant |
| `InvalidIdentifier` | Namespace or owner segment contains a null byte |

---

## Thread Safety

`ConfigManager`, `OwnerHandle`, and `ConfigStore` are all `Clone + Send + Sync`. Pass them to threads, store them in `Arc`, or use them from async tasks without additional synchronization.

```rust
use std::{sync::Arc, thread};

let manager = Arc::new(ConfigManager::new());
let handle  = manager.register("ns", "worker/a", schema).unwrap();
let h2      = handle.clone();
let m2      = Arc::clone(&manager);

thread::spawn(move || {
    h2.set("key", 42_i64).unwrap();
    let v = m2.get("ns", "worker/a", "key").unwrap();
    println!("{v}");
});
```

---

## Testing

The crate ships with **280+ integration tests** covering:

- All `ConfigValue` variants and the `Color` type
- Every validator (int/float ranges, string length, one-of, custom)
- `NamespaceSchema` and `SchemaEntry` metadata
- `ConfigManager` registration, discovery, and search
- `OwnerHandle` typed reads/writes, validation, reset, and clone
- Change listeners (key-level, owner-level, global, RAII cleanup, ordering)
- N-level owner path parsing and isolation edge cases
- Multi-threaded concurrent reads, writes, and registration
- `ConfigStore` save/load round-trips to a temp directory (color, type-mismatch resilience, partial files, malformed TOML)
- `ConfigError` display, equality, clone, and `std::error::Error`

```sh
cargo test
# or, with cargo-nextest:
cargo nextest run
```

---

## License

MIT — see [LICENSE](LICENSE).
# PulsarConfig

> **A schema-first, plugin-isolated, concurrent settings library for games and applications.**

PulsarConfig is the single source of truth for your application's configuration. It is built especially for large-scale settings screens in complex applications — the kind where dozens of plugins each own a slice of the config, users can search and filter thousands of keys in real time, and multiple threads need to read values simultaneously without contention.

You declare a schema. PulsarConfig handles namespacing, defaults, type coercion, validation, and change notification — without a single lock visible in your code.

---

## Features

| | |
|---|---|
| **Schema-first** | Declare every key, its type, its default, and its constraints before the first value is written. No surprises at runtime. |
| **Plugin isolation** | Each subsystem receives a scoped `NamespaceHandle`. It can write only to its own keys — cross-namespace writes are a compile-time impossibility. |
| **Concurrent reads** | Backed by `DashMap` shard-locking. Reads from multiple threads are effectively contention-free in the common case. |
| **Reactive** | Subscribe to a single key, an entire namespace, or every change globally. Listeners are RAII-guarded — drop the `ListenerId` and the callback is gone. |
| **Type-safe accessors** | `get_bool`, `get_int`, `get_float`, `get_string`, `get_color` — all return `Result`, none panic. |
| **Zero-cost defaults** | Values are pre-seeded from schema defaults at registration time. `get` is a single hash-map lookup. |
| **Rich validation** | Range checks, string-length limits, allowlists, and fully custom async-safe closures — all composable on a single key. |
| **Full-text search** | Query every key, description, and tag across all namespaces in one call. |

---

## Quick Start

```rust
use pulsar_config::{
    Color, ConfigManager, ConfigValue,
    NamespaceSchema, SchemaEntry, Validator,
};

// ── 1. Create the manager ─────────────────────────────────────────────────────
//    Typically held by your engine or app root. Cheap to clone and share.
let manager = ConfigManager::new();

// ── 2. A plugin declares its schema ──────────────────────────────────────────
let schema = NamespaceSchema::new("Shadow Renderer", "Controls shadow rendering")
    .setting("enabled",
        SchemaEntry::new("Enable shadow rendering", true))
    .setting("max_distance",
        SchemaEntry::new("Max draw distance (world units)", 500.0_f64)
            .validator(Validator::float_range(0.0, 10_000.0))
            .tag("performance"))
    .setting("quality",
        SchemaEntry::new("Quality preset", "high")
            .validator(Validator::string_one_of(["low", "medium", "high", "ultra"]))
            .tags(["rendering", "quality"]))
    .setting("tint",
        SchemaEntry::new("Shadow tint color", Color::rgba(0, 0, 0, 128)));

// ── 3. Register and receive a scoped handle ───────────────────────────────────
let shadows = manager.register_namespace("renderer.shadows", schema).unwrap();

// ── 4. Read — always succeeds (defaults are pre-loaded) ───────────────────────
let enabled      = shadows.get_bool("enabled").unwrap();
let max_distance = shadows.get_float("max_distance").unwrap();

// ── 5. Write — validated against the schema ───────────────────────────────────
shadows.set("max_distance", 1_000.0_f64).unwrap();
shadows.set("max_distance", 99_999.0_f64).unwrap_err(); // out of range

// ── 6. Subscribe to changes ───────────────────────────────────────────────────
//    The listener is removed automatically when `_guard` is dropped.
let _guard = shadows.on_change("enabled", |event| {
    println!("shadows.enabled changed to {:?}", event.new_value);
}).unwrap();

// ── 7. Cross-plugin read (read-only — no handle required) ─────────────────────
let quality = manager.get("renderer.shadows", "quality").unwrap();

// ── 8. Search across every namespace ─────────────────────────────────────────
let results = manager.search("performance"); // matches key names, descriptions, tags
```

---

## Core Concepts

### `ConfigManager`

The root object. Create one per application and share it (it is `Clone + Send + Sync`).

```rust
let manager = ConfigManager::new();

// Cross-namespace read
let v = manager.get("renderer.shadows", "quality")?;

// Enumerate all registered namespaces
let ids: Vec<String> = manager.list_namespaces();

// Enumerate all settings in a namespace, with metadata
let infos: Vec<SettingInfo> = manager.list_settings("renderer.shadows").unwrap();

// Subscribe to every change across every namespace
let _guard = manager.on_any_change(|event| {
    println!("{}::{} changed", event.namespace, event.key);
});
```

### `NamespaceHandle`

The write handle a plugin holds. Scoped to its own namespace — writing to another namespace is not possible.

```rust
// Typed reads
let vol  = handle.get_int("master_volume")?;
let name = handle.get_string("display_name")?;
let tint = handle.get_color("ui_tint")?;

// Write (validated)
handle.set("master_volume", 75_i64)?;

// Reset a key to its schema default
handle.reset_to_default("master_volume")?;

// Namespace-scoped change listener
let _guard = handle.on_any_change(|e| println!("{} → {}", e.key, e.new_value));
```

### `NamespaceSchema` & `SchemaEntry`

Schemas are built with a fluent API and passed once to `register_namespace`.

```rust
let schema = NamespaceSchema::new("Audio", "Audio engine settings")
    .setting("master_volume",
        SchemaEntry::new("Master volume (0–100)", 80_i64)
            .validator(Validator::int_range(0, 100))
            .tag("audio"))
    .setting("sample_rate",
        SchemaEntry::new("Output sample rate (Hz)", 48_000_i64)
            .read_only())                          // rejects runtime writes
    .setting("reverb",
        SchemaEntry::new("Enable reverb", true)
            .tags(["audio", "quality"]));
```

### Validators

Multiple validators can be chained on a single key. All must pass or the write is rejected.

| Validator | Description |
|-----------|-------------|
| `Validator::int_range(min, max)` | Inclusive integer bounds |
| `Validator::float_range(min, max)` | Inclusive float bounds |
| `Validator::string_max_length(n)` | Maximum byte length |
| `Validator::string_one_of([...])` | Exact-match allowlist |
| `Validator::custom(fn)` | Arbitrary `Send + Sync` closure |

```rust
SchemaEntry::new("Thread count", 4_i64)
    .validator(Validator::int_range(1, 64))
    .validator(Validator::custom(|v| {
        let n = v.as_int()?;
        if n.count_ones() == 1 { Ok(()) } else { Err("must be a power of two".into()) }
    }));
```

### `ConfigValue` — Supported Types

| Variant | Rust type | Typed accessor |
|---------|-----------|----------------|
| `Bool` | `bool` | `as_bool()` |
| `Int` | `i64` | `as_int()` |
| `Float` | `f64` | `as_float()` (also accepts `Int`) |
| `String` | `String` | `as_str()` |
| `Color` | [`Color`](#color) | `as_color()` |
| `Array` | `Vec<ConfigValue>` | `as_array()` |

All standard Rust primitives (`bool`, `i32`, `i64`, `u32`, `f32`, `f64`, `&str`, `String`) as well as `Color` convert into `ConfigValue` via `From` automatically.

### `Color`

An sRGB RGBA color value (8 bits per channel). Includes helpers for renderer interop.

```rust
let red   = Color::rgb(255, 0, 0);
let tint  = Color::rgba(0, 0, 0, 128);
let white = Color::from_hex(0xFFFFFFFF);

let packed: u32    = red.to_hex();
let linear: [f32; 4] = tint.to_linear_f32(); // sRGB → linear, for GPU upload
```

### Change Listeners & `ListenerId`

Listeners are automatically cleaned up when their `ListenerId` guard is dropped — no manual deregistration required.

```rust
// Key-level listener
let _key_guard = handle.on_change("enabled", |e| { /* ... */ })?;

// Namespace-level listener
let _ns_guard = handle.on_any_change(|e| { /* ... */ });

// Global listener (all namespaces)
let _global_guard = manager.on_any_change(|e| { /* ... */ });

// Listener fires while guard is alive; dropped → removed automatically.
drop(_key_guard);
```

`ChangeEvent` carries `namespace`, `key`, `old_value: Option<ConfigValue>`, and `new_value: ConfigValue`.

---

## Plugin Registration Pattern

Inspired by VS Code's extension contribution model: each plugin calls `register_namespace` exactly once at startup and holds the returned handle for its lifetime.

```rust
use pulsar_config::{ConfigManager, NamespaceHandle, NamespaceSchema, SchemaEntry, Validator};

pub struct RenderPlugin {
    config: NamespaceHandle,
}

impl RenderPlugin {
    pub fn new(manager: &ConfigManager) -> Self {
        let schema = NamespaceSchema::new("Renderer", "Core rendering settings")
            .setting("vsync",   SchemaEntry::new("Enable VSync", true))
            .setting("max_fps", SchemaEntry::new("Frame rate cap", 144_i64)
                .validator(Validator::int_range(1, 500))
                .tag("performance"));

        let config = manager
            .register_namespace("renderer", schema)
            .expect("renderer namespace registered twice");

        Self { config }
    }

    pub fn vsync_enabled(&self) -> bool {
        self.config.get_bool("vsync").unwrap()
    }

    pub fn set_max_fps(&self, fps: i64) -> Result<(), pulsar_config::ConfigError> {
        self.config.set("max_fps", fps)
    }
}
```

---

## Persistence

`ConfigStore` wraps a `ConfigManager` and serializes each namespace to a separate human-editable TOML file. The config directory is resolved automatically via the OS convention.

### Platform paths

| Platform | Default location |
|----------|-----------------|
| Linux    | `~/.config/<app_name>/` |
| macOS    | `~/Library/Application Support/<app_name>/` |
| Windows  | `%APPDATA%\<app_name>\` |

### File layout

One `.toml` file per namespace, named after the namespace ID:

```
~/.config/my_game/
  renderer.shadows.toml
  audio.toml
  input.toml
```

### Example file

```toml
# PulsarConfig — renderer.shadows
# Edit this file to override application defaults.
# Missing keys fall back to the schema default.
# Read-only settings are managed by the application and are not listed here.

enabled = true
max_distance = 1000.0
quality = "high"

[tint]
r = 0
g = 0
b = 0
a = 128
```

### Usage

```rust
use pulsar_config::{ConfigManager, ConfigStore, NamespaceSchema, SchemaEntry, Validator, Color};

let manager = ConfigManager::new();

// Register namespaces as usual.
let audio_schema = NamespaceSchema::new("Audio", "Audio engine settings")
    .setting("master_volume", SchemaEntry::new("Master volume (0–100)", 80_i64)
        .validator(Validator::int_range(0, 100)))
    .setting("reverb", SchemaEntry::new("Enable reverb", true));

let shadows_schema = NamespaceSchema::new("Shadows", "Shadow rendering settings")
    .setting("enabled", SchemaEntry::new("Enable shadows", true))
    .setting("tint", SchemaEntry::new("Shadow tint color", Color::rgba(0, 0, 0, 128)));

let audio   = manager.register_namespace("audio", audio_schema).unwrap();
let shadows = manager.register_namespace("renderer.shadows", shadows_schema).unwrap();

// Wrap the manager in a ConfigStore.
let store = ConfigStore::new(manager, "my_game").unwrap();

// On startup: load persisted overrides on top of schema defaults.
// Returns Ok(false) on first run — schema defaults are used transparently.
store.load(&audio).unwrap();
store.load(&shadows).unwrap();

// Or load all handles at once. Returns the IDs with no persisted file yet.
let first_run_ids = store.load_all([&audio, &shadows]).unwrap();

// ... application runs ...

// On shutdown: save all namespaces.
store.save_all().unwrap();

// Or save a single namespace (e.g. after a settings screen is closed).
store.save("audio").unwrap();
```

### Alternative: explicit config directory

For portable or embedded deployments:

```rust
let store = ConfigStore::with_dir(manager, "./config").unwrap();
```

### Resilience guarantees

| Situation | Behaviour |
|-----------|-----------|
| No file (first run) | Schema defaults used; `load` returns `false` |
| Key in file, not in schema | Silently skipped (schema change between versions) |
| Key in schema, not in file | Schema default retained (new setting added) |
| Type mismatch in file | Silently skipped; schema default retained |
| Value fails validation | Silently skipped; schema default retained |
| Read-only setting | Never written to file; always sourced from schema |

---

## Error Handling

All fallible operations return a typed `Result`. Config operations use `ConfigError`; persistence operations use `PersistError`.

### `ConfigError`

| Variant | When |
|---------|------|
| `NamespaceAlreadyRegistered(id)` | Registering a namespace ID that already exists |
| `NamespaceNotFound(id)` | Accessing an unregistered namespace |
| `UnknownKey { namespace, key }` | Key not declared in the schema |
| `ValidationFailed { namespace, key, reason }` | One or more validators rejected the value |
| `ReadOnly { namespace, key }` | Writing to a key marked `.read_only()` |
| `TypeMismatch { expected, got }` | Typed accessor called on the wrong variant |

### `PersistError`

| Variant | When |
|---------|------|
| `NoPlatformConfigDir` | `$HOME` (or equivalent) is unset — config dir cannot be determined |
| `Io(e)` | File read or write failed |
| `TomlParse(e)` | The `.toml` file contains invalid TOML |
| `TomlSerialize(e)` | A value could not be converted to TOML (extremely rare) |
| `Config(e)` | A `ConfigError` occurred while applying a loaded value |

---

## License

Licensed under the [MIT License](LICENSE).
