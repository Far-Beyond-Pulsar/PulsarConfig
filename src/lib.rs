//! # PulsarConfig
//!
//! A high-performance, in-memory settings database designed for applications of
//! all shapes and sizes — from game engines to desktop tools to long-running
//! services.
//!
//! PulsarConfig acts as a **centralized source of truth** for every setting in
//! your application. Settings are organised in a three-tier N-level hierarchy:
//!
//! ```text
//! namespace  ("editor" vs "project")
//!   └── owner path  ("subsystem/physics/main")   ← any depth
//!         └── key   ("gravity")
//! ```
//!
//! ## What PulsarConfig does for you
//!
//! - **Centralizes configuration** — one [`ConfigManager`] owns every setting.
//!   No more scattered `static` variables or global mutable state.
//! - **Handles all storage complexity** — namespacing, defaults, validation,
//!   type coercion, and change notification are built in. You declare a schema;
//!   PulsarConfig handles the rest.
//! - **Scales with your codebase** — register as many `(namespace, owner)`
//!   pairs as you like. Each plugin or subsystem gets an isolated
//!   [`OwnerHandle`]; it cannot accidentally write to another owner's settings.
//! - **Stays fast under load** — the storage layer is backed by
//!   [`DashMap`](dashmap::DashMap), giving you fine-grained shard locking so
//!   concurrent reads are effectively contention-free.
//! - **Notifies on change** — RAII [`ListenerId`] guards let any thread react
//!   to setting updates. The guard automatically removes the listener when
//!   dropped, preventing stale callbacks.
//!
//! ## Design Goals
//!
//! | Goal | How it's achieved |
//! |------|------------------|
//! | **Schema-first** | Owners declare keys, types, defaults, and validators upfront |
//! | **Plugin isolation** | Each plugin holds an [`OwnerHandle`] — cross-owner writes are impossible |
//! | **Concurrent** | [`DashMap`](dashmap::DashMap) sharding; reads are effectively lock-free |
//! | **Reactive** | RAII [`ListenerId`] tokens wire up change callbacks with automatic cleanup |
//! | **Type-safe** | Typed getters (`get_bool`, `get_int`, …) return `Result`, never panic |
//! | **Zero overhead defaults** | All values are pre-seeded at registration; `get` is a single map lookup |
//! | **N-level paths** | Owner paths can be any depth: `"a"`, `"a/b"`, `"a/b/c/d/e"` |
//!
//! ## Quick Start
//!
//! ```rust
//! use pulsar_config::{
//!     ConfigManager, Color, ConfigValue,
//!     NamespaceSchema, SchemaEntry, Validator,
//! };
//!
//! // 1. Create the manager — typically stored in your engine/app.
//! let manager = ConfigManager::new();
//!
//! // 2. A plugin declares its schema and registers it.
//! //    - "editor" is the namespace (top-level scope).
//! //    - "renderer/shadows" is the owner path (N levels deep).
//! let schema = NamespaceSchema::new("Shadow Renderer", "Controls shadow rendering")
//!     .setting("enabled",      SchemaEntry::new("Enable shadows", true))
//!     .setting("max_distance", SchemaEntry::new("Max draw distance (world units)", 500.0_f64)
//!         .validator(Validator::float_range(0.0, 10_000.0))
//!         .tag("performance"))
//!     .setting("quality",      SchemaEntry::new("Quality preset", "high")
//!         .validator(Validator::string_one_of(["low", "medium", "high", "ultra"]))
//!         .tags(["rendering", "quality"]))
//!     .setting("tint",         SchemaEntry::new("Shadow tint color", Color::rgba(0, 0, 0, 128)));
//!
//! // `shadows` is the plugin's private, scoped handle.
//! let shadows = manager.register("editor", "renderer/shadows", schema).unwrap();
//!
//! // 3. Read settings — always returns a value (defaults are pre-loaded).
//! let enabled      = shadows.get_bool("enabled").unwrap();
//! let max_distance = shadows.get_float("max_distance").unwrap();
//!
//! // 4. Write settings — validated against the schema.
//! shadows.set("max_distance", 1_000.0_f64).unwrap();
//!
//! // 5. Subscribe to changes.  `_guard` removes the listener when dropped.
//! let _guard = shadows.on_change("enabled", |event| {
//!     println!("renderer/shadows.enabled → {:?}", event.new_value);
//! }).unwrap();
//!
//! // 6. Cross-owner read (read-only; no handle required).
//! let v = manager.get("editor", "renderer/shadows", "quality").unwrap();
//!
//! // 7. Search across all owners.
//! let results = manager.search("performance");
//! ```
//!
//! ## Plugin Registration Pattern
//!
//! Inspired by how VS Code extensions contribute settings, each plugin calls
//! [`ConfigManager::register`] exactly once (typically at startup or plugin
//! load time). The returned [`OwnerHandle`] is the plugin's only write path —
//! other code can only read cross-owner values through [`ConfigManager::get`].
//!
//! ```rust
//! use pulsar_config::{ConfigManager, NamespaceSchema, SchemaEntry, Validator, OwnerHandle};
//!
//! pub struct AudioPlugin {
//!     config: OwnerHandle,
//! }
//!
//! impl AudioPlugin {
//!     pub fn new(manager: &ConfigManager) -> Self {
//!         let schema = NamespaceSchema::new("Audio", "Audio engine settings")
//!             .setting("master_volume", SchemaEntry::new("Master volume (0–100)", 80_i64)
//!                 .validator(Validator::int_range(0, 100)))
//!             .setting("sample_rate",  SchemaEntry::new("Output sample rate", 48_000_i64)
//!                 .read_only())
//!             .setting("reverb",       SchemaEntry::new("Enable reverb", true)
//!                 .tag("quality"));
//!
//!         let config = manager
//!             .register("editor", "plugin/audio", schema)
//!             .expect("audio owner already registered");
//!
//!         Self { config }
//!     }
//!
//!     pub fn master_volume(&self) -> i64 {
//!         self.config.get_int("master_volume").unwrap()
//!     }
//! }
//! ```

pub mod error;
pub mod manager;
pub mod persist;
pub mod schema;
pub mod value;

pub use error::ConfigError;
pub use manager::{ChangeEvent, ConfigManager, ListenerId, OwnerHandle, SearchResult, SettingInfo};
pub use persist::{ConfigStore, PersistError};
pub use schema::{DropdownOption, FieldType, NamespaceSchema, SchemaEntry, Validator, ValidatorFn};
pub use value::{Color, ConfigValue};
