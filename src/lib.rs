//! # PulsarConfig
//!
//! A high-performance, in-memory settings database designed for applications of
//! all shapes and sizes — from game engines to desktop tools to long-running
//! services.
//!
//! PulsarConfig acts as a **centralized source of truth** for every setting in
//! your application. It is built specifically for large-scale settings screens
//! in complex applications: the kind where dozens of plugins or subsystems each
//! own a slice of the configuration, users can search and filter thousands of
//! keys, and multiple threads need to read values simultaneously without paying
//! a synchronisation tax.
//!
//! ## What PulsarConfig does for you
//!
//! - **Centralizes configuration** — one [`ConfigManager`] owns every setting.
//!   No more scattered `static` variables or global mutable state.
//! - **Handles all storage complexity** — namespacing, defaults, validation,
//!   type coercion, and change notification are built in. You declare a schema;
//!   PulsarConfig handles the rest.
//! - **Scales with your codebase** — register as many namespaces as you like.
//!   Each plugin or subsystem gets an isolated [`NamespaceHandle`]; it cannot
//!   accidentally write to another subsystem's settings.
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
//! | **Schema-first** | Plugins declare keys, types, defaults, and validators upfront |
//! | **Plugin isolation** | Each plugin holds a [`NamespaceHandle`] — cross-namespace writes are impossible |
//! | **Concurrent** | [`DashMap`](dashmap::DashMap) sharding; reads are effectively lock-free |
//! | **Reactive** | RAII [`ListenerId`] tokens wire up change callbacks with automatic cleanup |
//! | **Type-safe** | Typed getters (`get_bool`, `get_int`, …) return `Result`, never panic |
//! | **Zero overhead defaults** | All values are pre-seeded at registration; `get` is a single map lookup |
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
//! // 2. A plugin (or subsystem) declares its schema and registers it.
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
//! let shadows = manager.register_namespace("renderer.shadows", schema).unwrap();
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
//!     println!("shadows.enabled → {:?}", event.new_value);
//! }).unwrap();
//!
//! // 6. Cross-plugin read (read-only, no handle required).
//! let v = manager.get("renderer.shadows", "quality").unwrap();
//!
//! // 7. Search across all namespaces.
//! let results = manager.search("performance");
//! ```
//!
//! ## Plugin Registration Pattern
//!
//! Inspired by how VS Code extensions contribute settings, each plugin calls
//! [`ConfigManager::register_namespace`] exactly once (typically at startup or
//! plugin load time). The returned [`NamespaceHandle`] is the plugin's only
//! write path — other plugins can only read cross-namespace values through
//! [`ConfigManager::get`].
//!
//! ```rust
//! use pulsar_config::{ConfigManager, NamespaceSchema, SchemaEntry, Validator};
//!
//! pub struct AudioPlugin {
//!     config: pulsar_config::NamespaceHandle,
//! }
//!
//! impl AudioPlugin {
//!     pub fn new(manager: &ConfigManager) -> Self {
//!         let schema = NamespaceSchema::new("Audio", "Audio engine settings")
//!             .setting("master_volume", SchemaEntry::new("Master volume (0–100)", 80_i64)
//!                 .validator(Validator::int_range(0, 100)))
//!             .setting("sample_rate",  SchemaEntry::new("Output sample rate", 48_000_i64)
//!                 .validator(Validator::string_one_of(["44100", "48000", "96000"]))
//!                 .read_only())
//!             .setting("reverb",       SchemaEntry::new("Enable reverb", true)
//!                 .tag("quality"));
//!
//!         let config = manager
//!             .register_namespace("audio", schema)
//!             .expect("audio namespace already registered");
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
pub use manager::{ChangeEvent, ConfigManager, ListenerId, NamespaceHandle, SearchResult, SettingInfo};
pub use persist::{ConfigStore, PersistError};
pub use schema::{NamespaceSchema, SchemaEntry, Validator, ValidatorFn};
pub use value::{Color, ConfigValue};
