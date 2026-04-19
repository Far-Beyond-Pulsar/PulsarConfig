# PulsarConfig

> **A high-performance, in-memory settings database for applications of every shape and size.**

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

## Error Handling

All fallible operations return `Result<_, ConfigError>`. Errors are structured, not stringly-typed:

| Error | When |
|-------|------|
| `NamespaceAlreadyRegistered(id)` | Registering a namespace ID that already exists |
| `NamespaceNotFound(id)` | Accessing an unregistered namespace |
| `UnknownKey { namespace, key }` | Key not declared in the schema |
| `ValidationFailed { namespace, key, reason }` | One or more validators rejected the value |
| `ReadOnly { namespace, key }` | Writing to a key marked `.read_only()` |
| `TypeMismatch { expected, got }` | Typed accessor called on the wrong variant |

---

## License

Licensed under the [MIT License](LICENSE).
