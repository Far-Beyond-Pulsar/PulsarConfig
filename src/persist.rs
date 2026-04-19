use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use toml::map::Map as TomlMap;

use crate::error::ConfigError;
use crate::manager::{ConfigManager, NamespaceHandle};
use crate::value::{Color, ConfigValue};

// ─── PersistError ─────────────────────────────────────────────────────────────

/// Errors that can arise during persistence operations.
#[derive(Debug)]
pub enum PersistError {
    /// The platform-specific config directory could not be determined.
    ///
    /// This typically means the `HOME` environment variable is unset.
    NoPlatformConfigDir,

    /// An I/O error occurred while reading or writing a file.
    Io(std::io::Error),

    /// The TOML file could not be parsed.
    TomlParse(toml::de::Error),

    /// A value could not be serialized to TOML.
    TomlSerialize(toml::ser::Error),

    /// A configuration error occurred while applying a loaded value.
    Config(ConfigError),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlatformConfigDir => {
                write!(f, "could not determine the platform config directory (is $HOME set?)")
            }
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::TomlParse(e) => write!(f, "TOML parse error: {e}"),
            Self::TomlSerialize(e) => write!(f, "TOML serialization error: {e}"),
            Self::Config(e) => write!(f, "config error: {e}"),
        }
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::TomlParse(e) => Some(e),
            Self::TomlSerialize(e) => Some(e),
            Self::Config(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PersistError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<toml::de::Error> for PersistError {
    fn from(e: toml::de::Error) -> Self {
        Self::TomlParse(e)
    }
}
impl From<toml::ser::Error> for PersistError {
    fn from(e: toml::ser::Error) -> Self {
        Self::TomlSerialize(e)
    }
}
impl From<ConfigError> for PersistError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

// ─── ConfigStore ──────────────────────────────────────────────────────────────

/// Wraps a [`ConfigManager`] with TOML-based persistence.
///
/// Each registered namespace is stored as a separate `.toml` file inside a
/// platform-appropriate config directory. On the first launch no files exist
/// and schema defaults are used transparently. On subsequent launches the saved
/// values are layered on top of the defaults, so adding a new setting to the
/// schema always has a safe fallback.
///
/// # Platform directories
///
/// The config root is resolved via [`dirs::config_dir`]:
///
/// | Platform | Default path |
/// |----------|--------------|
/// | Linux    | `~/.config/<app_name>/` |
/// | macOS    | `~/Library/Application Support/<app_name>/` |
/// | Windows  | `%APPDATA%\<app_name>\` |
///
/// # File layout
///
/// ```text
/// <config_dir>/<app_name>/
///   renderer.shadows.toml
///   audio.toml
///   input.toml
/// ```
///
/// Namespace IDs are used directly as file names (forward-slashes in IDs are
/// replaced with underscores to produce valid file names).
///
/// # File format
///
/// Each file is plain TOML. Scalar values map to their natural TOML
/// equivalents; `Color` values are stored as inline tables:
///
/// ```toml
/// # PulsarConfig — renderer.shadows
/// # Edit this file to override application defaults.
/// # Missing keys fall back to the schema default.
///
/// enabled = true
/// max_distance = 1000.0
/// quality = "high"
///
/// [tint]
/// r = 0
/// g = 0
/// b = 0
/// a = 128
/// ```
///
/// # Example
///
/// ```rust,no_run
/// use pulsar_config::{ConfigManager, NamespaceSchema, SchemaEntry};
/// use pulsar_config::persist::ConfigStore;
///
/// let manager = ConfigManager::new();
///
/// let schema = NamespaceSchema::new("Audio", "Audio settings")
///     .setting("volume", SchemaEntry::new("Master volume", 80_i64));
/// let handle = manager.register_namespace("audio", schema).unwrap();
///
/// let store = ConfigStore::new(manager, "my_app").unwrap();
///
/// // Load persisted values — no-op on first run, defaults are used.
/// store.load(&handle).unwrap();
///
/// // ... application runs, settings may change ...
///
/// // Persist on shutdown.
/// store.save("audio").unwrap();
/// ```
pub struct ConfigStore {
    manager: ConfigManager,
    config_dir: PathBuf,
}

impl ConfigStore {
    /// Create a store for `app_name` rooted at the platform config directory.
    ///
    /// The directory is created automatically if it does not exist.
    pub fn new(manager: ConfigManager, app_name: &str) -> Result<Self, PersistError> {
        let base = dirs::config_dir().ok_or(PersistError::NoPlatformConfigDir)?;
        let config_dir = base.join(app_name);
        fs::create_dir_all(&config_dir)?;
        Ok(Self { manager, config_dir })
    }

    /// Create a store rooted at an explicit directory.
    ///
    /// Useful for portable deployments, embedded use-cases, or tests where
    /// writing to the real user config dir is undesirable.
    ///
    /// ```rust,no_run
    /// use pulsar_config::{ConfigManager, persist::ConfigStore};
    ///
    /// let store = ConfigStore::with_dir(ConfigManager::new(), "/tmp/my_app_test").unwrap();
    /// ```
    pub fn with_dir(
        manager: ConfigManager,
        config_dir: impl Into<PathBuf>,
    ) -> Result<Self, PersistError> {
        let config_dir = config_dir.into();
        fs::create_dir_all(&config_dir)?;
        Ok(Self { manager, config_dir })
    }

    /// Return a reference to the underlying [`ConfigManager`].
    pub fn manager(&self) -> &ConfigManager {
        &self.manager
    }

    /// Return the directory where config files are stored.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    // ── Path helpers ─────────────────────────────────────────────────────────

    /// Map a namespace ID to its `.toml` file path.
    ///
    /// Forward-slashes in IDs are replaced with underscores so that
    /// `"renderer/shadows"` and `"renderer.shadows"` both become safe filenames.
    pub fn toml_path(&self, namespace_id: &str) -> PathBuf {
        let filename = namespace_id.replace('/', "_");
        self.config_dir.join(format!("{filename}.toml"))
    }

    // ── Save ─────────────────────────────────────────────────────────────────

    /// Persist the current values for `namespace_id` to its `.toml` file.
    ///
    /// All non-read-only settings are saved, including those equal to their
    /// schema default. This ensures the file is self-documenting — users can
    /// open it, see every available knob, and edit freely.
    ///
    /// Read-only settings are never written; they are always resolved from
    /// the schema at startup.
    ///
    /// Returns [`PersistError::Config`] wrapping
    /// [`ConfigError::NamespaceNotFound`] if the namespace is not registered.
    pub fn save(&self, namespace_id: &str) -> Result<(), PersistError> {
        let settings = self
            .manager
            .list_settings(namespace_id)
            .ok_or_else(|| ConfigError::NamespaceNotFound(namespace_id.to_owned()))?;

        let mut table = TomlMap::new();
        for info in &settings {
            if info.read_only {
                // Read-only values are always sourced from the schema; there is
                // no point persisting them and doing so could confuse users.
                continue;
            }
            table.insert(info.key.clone(), config_value_to_toml(&info.current_value));
        }

        let header = format!(
            "# PulsarConfig — {namespace_id}\n\
             # Edit this file to override application defaults.\n\
             # Missing keys fall back to the schema default.\n\
             # Read-only settings are managed by the application and are not listed here.\n\n"
        );

        let body = toml::to_string(&toml::Value::Table(table))?;
        let path = self.toml_path(namespace_id);
        fs::write(&path, format!("{header}{body}"))?;
        Ok(())
    }

    /// Save every registered namespace.
    ///
    /// Equivalent to calling [`save`](Self::save) for each value returned by
    /// [`ConfigManager::list_namespaces`].
    pub fn save_all(&self) -> Result<(), PersistError> {
        for id in self.manager.list_namespaces() {
            self.save(&id)?;
        }
        Ok(())
    }

    // ── Load ─────────────────────────────────────────────────────────────────

    /// Load persisted values for `handle`'s namespace and apply them on top of
    /// the schema defaults.
    ///
    /// Returns `true` if a file was found and applied, `false` if no file
    /// exists yet (first run — schema defaults remain in effect).
    ///
    /// **Resilience guarantees:**
    /// - Keys present in the file but absent from the current schema are
    ///   silently ignored (the schema may have changed since the file was
    ///   written).
    /// - Keys present in the schema but absent from the file keep their
    ///   schema defaults (newly added settings always have a safe value).
    /// - Values that fail type conversion or schema validation are silently
    ///   skipped; the schema default is used instead.
    pub fn load(&self, handle: &NamespaceHandle) -> Result<bool, PersistError> {
        let path = self.toml_path(handle.namespace_id());
        if !path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(&path)?;
        let table: toml::Table = toml::from_str(&content)?;

        // Build a map of key → schema default so we know the expected type.
        let schema: std::collections::HashMap<String, ConfigValue> = handle
            .list_settings()
            .into_iter()
            .map(|info| (info.key, info.default_value))
            .collect();

        for (key, toml_val) in &table {
            let Some(default) = schema.get(key) else {
                // Key no longer in schema — skip gracefully.
                continue;
            };

            if let Ok(value) = toml_to_config_value(toml_val, default) {
                // Validation failures (e.g. schema range narrowed) keep the
                // default — the `let _` discards the error intentionally.
                let _ = handle.set(key, value);
            }
            // Type mismatch → keep schema default, no error propagated.
        }

        Ok(true)
    }

    /// Load persisted values for every handle in `handles`.
    ///
    /// Returns the namespace IDs for which no persisted file existed (first
    /// run for those namespaces). All other error conditions follow the same
    /// resilience guarantees as [`load`](Self::load).
    pub fn load_all<'a>(
        &self,
        handles: impl IntoIterator<Item = &'a NamespaceHandle>,
    ) -> Result<Vec<String>, PersistError> {
        let mut first_run = Vec::new();
        for handle in handles {
            if !self.load(handle)? {
                first_run.push(handle.namespace_id().to_owned());
            }
        }
        Ok(first_run)
    }
}

// ─── ConfigValue ↔ toml::Value conversions ───────────────────────────────────

/// Convert a [`ConfigValue`] to a [`toml::Value`] for serialization.
///
/// Type mapping:
///
/// | `ConfigValue`    | `toml::Value`                                  |
/// |------------------|------------------------------------------------|
/// | `Bool(b)`        | `Boolean(b)`                                   |
/// | `Int(i)`         | `Integer(i)`                                   |
/// | `Float(f)`       | `Float(f)`                                     |
/// | `String(s)`      | `String(s)`                                    |
/// | `Color{r,g,b,a}` | `Table { r: Integer, g: Integer, … }`          |
/// | `Array(v)`       | `Array(v)` (elements converted recursively)    |
pub fn config_value_to_toml(v: &ConfigValue) -> toml::Value {
    match v {
        ConfigValue::Bool(b) => toml::Value::Boolean(*b),
        ConfigValue::Int(i) => toml::Value::Integer(*i),
        ConfigValue::Float(f) => toml::Value::Float(*f),
        ConfigValue::String(s) => toml::Value::String(s.clone()),
        ConfigValue::Color(c) => {
            let mut t = TomlMap::new();
            t.insert("r".into(), toml::Value::Integer(c.r as i64));
            t.insert("g".into(), toml::Value::Integer(c.g as i64));
            t.insert("b".into(), toml::Value::Integer(c.b as i64));
            t.insert("a".into(), toml::Value::Integer(c.a as i64));
            toml::Value::Table(t)
        }
        ConfigValue::Array(arr) => {
            toml::Value::Array(arr.iter().map(config_value_to_toml).collect())
        }
    }
}

/// Convert a [`toml::Value`] to a [`ConfigValue`], using the schema's default
/// value to determine the expected type.
///
/// Returns `Err(reason)` on type mismatch. The caller decides whether to
/// propagate or ignore this (typically the latter, keeping the schema default).
fn toml_to_config_value(
    toml_val: &toml::Value,
    expected: &ConfigValue,
) -> Result<ConfigValue, String> {
    match expected {
        ConfigValue::Bool(_) => match toml_val {
            toml::Value::Boolean(b) => Ok(ConfigValue::Bool(*b)),
            other => Err(format!("expected boolean, got {}", other.type_str())),
        },

        ConfigValue::Int(_) => match toml_val {
            toml::Value::Integer(i) => Ok(ConfigValue::Int(*i)),
            other => Err(format!("expected integer, got {}", other.type_str())),
        },

        // Integers in the TOML file are widened to float for convenience.
        ConfigValue::Float(_) => match toml_val {
            toml::Value::Float(f) => Ok(ConfigValue::Float(*f)),
            toml::Value::Integer(i) => Ok(ConfigValue::Float(*i as f64)),
            other => Err(format!("expected float or integer, got {}", other.type_str())),
        },

        ConfigValue::String(_) => match toml_val {
            toml::Value::String(s) => Ok(ConfigValue::String(s.clone())),
            other => Err(format!("expected string, got {}", other.type_str())),
        },

        ConfigValue::Color(_) => match toml_val {
            toml::Value::Table(t) => {
                let r = u8_from_toml_table(t, "r")?;
                let g = u8_from_toml_table(t, "g")?;
                let b = u8_from_toml_table(t, "b")?;
                let a = u8_from_toml_table(t, "a")?;
                Ok(ConfigValue::Color(Color::rgba(r, g, b, a)))
            }
            other => Err(format!(
                "expected color table {{r, g, b, a}}, got {}",
                other.type_str()
            )),
        },

        ConfigValue::Array(defaults) => match toml_val {
            toml::Value::Array(arr) => {
                // Use the first default element to guide element-type conversion.
                // If the default array is empty, fall back to unguided conversion.
                let elem_default = defaults.first();
                let items: Result<Vec<ConfigValue>, String> = arr
                    .iter()
                    .map(|item| match elem_default {
                        Some(d) => toml_to_config_value(item, d),
                        None => toml_value_unguided(item),
                    })
                    .collect();
                Ok(ConfigValue::Array(items?))
            }
            other => Err(format!("expected array, got {}", other.type_str())),
        },
    }
}

/// Best-effort conversion with no schema guidance.
///
/// Used for array elements when the schema default array is empty. TOML tables
/// with exactly `{r, g, b, a}` integer keys are recognized as `Color`.
fn toml_value_unguided(v: &toml::Value) -> Result<ConfigValue, String> {
    match v {
        toml::Value::Boolean(b) => Ok(ConfigValue::Bool(*b)),
        toml::Value::Integer(i) => Ok(ConfigValue::Int(*i)),
        toml::Value::Float(f) => Ok(ConfigValue::Float(*f)),
        toml::Value::String(s) => Ok(ConfigValue::String(s.clone())),
        toml::Value::Array(arr) => {
            let items: Result<Vec<_>, _> = arr.iter().map(toml_value_unguided).collect();
            Ok(ConfigValue::Array(items?))
        }
        toml::Value::Table(t) => {
            // Heuristic: a four-field table with r/g/b/a integer keys is a Color.
            if t.len() == 4 {
                if let (Some(r), Some(g), Some(b), Some(a)) = (
                    t.get("r").and_then(toml::Value::as_integer),
                    t.get("g").and_then(toml::Value::as_integer),
                    t.get("b").and_then(toml::Value::as_integer),
                    t.get("a").and_then(toml::Value::as_integer),
                ) {
                    return Ok(ConfigValue::Color(Color::rgba(
                        r as u8, g as u8, b as u8, a as u8,
                    )));
                }
            }
            Err("unexpected TOML table in unguided array element".into())
        }
        toml::Value::Datetime(_) => Err("TOML Datetime values are not supported".into()),
    }
}

/// Extract a `u8` from a TOML table field, returning a descriptive error on failure.
fn u8_from_toml_table(t: &TomlMap<String, toml::Value>, key: &str) -> Result<u8, String> {
    t.get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|i| u8::try_from(i).ok())
        .ok_or_else(|| format!("color field '{key}' must be an integer in [0, 255]"))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        manager::ConfigManager,
        schema::{NamespaceSchema, SchemaEntry, Validator},
        value::{Color, ConfigValue},
    };

    fn make_store() -> (ConfigStore, NamespaceHandle) {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Test", "Test namespace")
            .setting(
                "volume",
                SchemaEntry::new("Master volume", 80_i64)
                    .validator(Validator::int_range(0, 100)),
            )
            .setting("name", SchemaEntry::new("Player name", "hero"))
            .setting("enabled", SchemaEntry::new("Feature flag", true))
            .setting(
                "brightness",
                SchemaEntry::new("Screen brightness", 0.8_f64),
            )
            .setting(
                "tint",
                SchemaEntry::new("UI tint", Color::rgba(255, 255, 255, 255)),
            )
            .setting("version", SchemaEntry::new("Build version", "1.0.0").read_only());

        let handle = manager.register_namespace("test", schema).unwrap();
        let tmp = std::env::temp_dir().join(format!("pulsar_config_test_{}", std::process::id()));
        let store = ConfigStore::with_dir(manager, &tmp).unwrap();
        (store, handle)
    }

    #[test]
    fn save_and_load_round_trip() {
        let (store, handle) = make_store();

        handle.set("volume", 42_i64).unwrap();
        handle.set("name", "ferris").unwrap();
        handle.set("enabled", false).unwrap();
        handle.set("brightness", 0.5_f64).unwrap();
        handle.set("tint", Color::rgba(10, 20, 30, 200)).unwrap();

        store.save("test").unwrap();

        // Reset all values to defaults so we verify load actually changes them.
        handle.reset_to_default("volume").unwrap();
        handle.reset_to_default("name").unwrap();
        handle.reset_to_default("enabled").unwrap();
        handle.reset_to_default("brightness").unwrap();
        handle.reset_to_default("tint").unwrap();

        let found = store.load(&handle).unwrap();
        assert!(found, "expected the file to exist");

        assert_eq!(handle.get_int("volume").unwrap(), 42);
        assert_eq!(handle.get_string("name").unwrap(), "ferris");
        assert!(!handle.get_bool("enabled").unwrap());
        assert!((handle.get_float("brightness").unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(handle.get_color("tint").unwrap(), Color::rgba(10, 20, 30, 200));
    }

    #[test]
    fn load_returns_false_on_first_run() {
        let (store, handle) = make_store();
        // No file written — must return Ok(false).
        assert!(!store.load(&handle).unwrap());
    }

    #[test]
    fn load_ignores_unknown_keys() {
        let (store, handle) = make_store();

        // Write a TOML file with an extra key not in the schema.
        let path = store.toml_path("test");
        fs::write(&path, "volume = 55\nunknown_setting = \"oops\"\n").unwrap();

        store.load(&handle).unwrap();
        assert_eq!(handle.get_int("volume").unwrap(), 55);
        // No panic — unknown key was silently ignored.
    }

    #[test]
    fn load_keeps_default_for_type_mismatch() {
        let (store, handle) = make_store();
        let path = store.toml_path("test");
        // `volume` expects an integer, but file has a string.
        fs::write(&path, "volume = \"not_a_number\"\n").unwrap();

        store.load(&handle).unwrap();
        // Type mismatch → default (80) retained.
        assert_eq!(handle.get_int("volume").unwrap(), 80);
    }

    #[test]
    fn load_keeps_default_for_validation_failure() {
        let (store, handle) = make_store();
        let path = store.toml_path("test");
        // `volume` validator requires [0, 100] — 999 must be rejected.
        fs::write(&path, "volume = 999\n").unwrap();

        store.load(&handle).unwrap();
        assert_eq!(handle.get_int("volume").unwrap(), 80);
    }

    #[test]
    fn read_only_not_saved() {
        let (store, handle) = make_store();
        store.save("test").unwrap();

        let content = fs::read_to_string(store.toml_path("test")).unwrap();
        assert!(!content.contains("version"), "read-only key must not appear in the file");
        drop(handle);
    }

    #[test]
    fn save_all_and_load_all() {
        let manager = ConfigManager::new();
        let s1 = NamespaceSchema::new("A", "").setting("x", SchemaEntry::new("", 1_i64));
        let s2 = NamespaceSchema::new("B", "").setting("y", SchemaEntry::new("", 2_i64));
        let h1 = manager.register_namespace("ns_a", s1).unwrap();
        let h2 = manager.register_namespace("ns_b", s2).unwrap();

        let tmp = std::env::temp_dir()
            .join(format!("pulsar_config_all_{}", std::process::id()));
        let store = ConfigStore::with_dir(manager, &tmp).unwrap();

        h1.set("x", 10_i64).unwrap();
        h2.set("y", 20_i64).unwrap();
        store.save_all().unwrap();

        h1.reset_to_default("x").unwrap();
        h2.reset_to_default("y").unwrap();

        let first_run = store.load_all([&h1, &h2]).unwrap();
        assert!(first_run.is_empty());
        assert_eq!(h1.get_int("x").unwrap(), 10);
        assert_eq!(h2.get_int("y").unwrap(), 20);
    }
}
