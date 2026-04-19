use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use toml::map::Map as TomlMap;

use crate::error::ConfigError;
use crate::manager::{ConfigManager, OwnerHandle};
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
            Self::NoPlatformConfigDir => write!(
                f,
                "could not determine the platform config directory (is $HOME set?)"
            ),
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
/// Settings are organised into a directory tree that mirrors the three-tier
/// hierarchy — **namespace → owner path → settings file**. This keeps files
/// human-readable and easy to locate with any text editor.
///
/// # Platform directories
///
/// The config root is resolved automatically via [`dirs::config_dir`]:
///
/// | Platform | Default location |
/// |----------|-----------------|
/// | Linux    | `~/.config/<app_name>/` |
/// | macOS    | `~/Library/Application Support/<app_name>/` |
/// | Windows  | `%APPDATA%\<app_name>\` |
///
/// # File layout
///
/// Each `(namespace, owner)` pair maps to exactly one `.toml` file whose path
/// mirrors the owner hierarchy:
///
/// ```text
/// <config_dir>/
///   editor/
///     subsystem/
///       physics/
///         main.toml          ← "editor" / "subsystem/physics/main"
///     renderer/
///       shadows.toml         ← "editor" / "renderer/shadows"
///   project/
///     audio.toml             ← "project" / "audio"
/// ```
///
/// # File format
///
/// ```toml
/// # PulsarConfig — editor/subsystem/physics/main
/// # Edit this file to override application defaults.
/// # Missing keys fall back to the schema default.
/// # Read-only settings are managed by the application and are not listed here.
///
/// gravity = 9.81
/// solver = "medium"
/// enabled = true
///
/// [tint]
/// r = 0
/// g = 0
/// b = 0
/// a = 128
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
    /// Useful for portable deployments, embedded environments, or tests where
    /// writing to the real user config directory is undesirable.
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

    /// Return the root directory where config files are stored.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    // ── Path helpers ─────────────────────────────────────────────────────────

    /// Compute the `.toml` path for a given `(namespace, owner_segments)` pair.
    ///
    /// The namespace becomes the first directory component; each owner segment
    /// becomes a subsequent component; the last segment receives the `.toml`
    /// extension.
    ///
    /// ```text
    /// namespace = "editor"
    /// owner     = ["subsystem", "physics", "main"]
    /// result    = <config_dir>/editor/subsystem/physics/main.toml
    /// ```
    ///
    /// If `owner` is empty (a namespace-level owner with no segments),
    /// the file is `<config_dir>/<namespace>/_root.toml`.
    pub fn toml_path(&self, namespace: &str, owner: &[String]) -> PathBuf {
        let mut path = self.config_dir.join(namespace);
        if owner.is_empty() {
            path.push("_root.toml");
            return path;
        }
        for seg in &owner[..owner.len() - 1] {
            path.push(seg);
        }
        path.push(format!("{}.toml", owner.last().unwrap()));
        path
    }

    // ── Save ─────────────────────────────────────────────────────────────────

    /// Persist the current values for `(namespace, owner)` to its `.toml` file.
    ///
    /// - All non-read-only settings are written, including those equal to their
    ///   schema default. This makes the file self-documenting.
    /// - Read-only settings are never written.
    /// - Parent directories are created automatically.
    ///
    /// Returns [`PersistError::Config`] wrapping [`ConfigError::OwnerNotFound`]
    /// if the owner is not registered.
    pub fn save(&self, namespace: &str, owner: &str) -> Result<(), PersistError> {
        let owner_vec: Vec<String> = owner
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();

        let settings = self
            .manager
            .list_settings(namespace, owner)
            .ok_or_else(|| {
                ConfigError::OwnerNotFound {
                    namespace: namespace.to_owned(),
                    owner: owner_vec.clone(),
                }
            })?;

        let mut table = TomlMap::new();
        for info in &settings {
            if info.read_only {
                continue;
            }
            table.insert(info.key.clone(), config_value_to_toml(&info.current_value));
        }

        let owner_display = if owner_vec.is_empty() {
            namespace.to_owned()
        } else {
            format!("{}/{}", namespace, owner_vec.join("/"))
        };

        let header = format!(
            "# PulsarConfig — {owner_display}\n\
             # Edit this file to override application defaults.\n\
             # Missing keys fall back to the schema default.\n\
             # Read-only settings are managed by the application and are not listed here.\n\n"
        );

        let body = toml::to_string(&toml::Value::Table(table))?;
        let path = self.toml_path(namespace, &owner_vec);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&path, format!("{header}{body}"))?;
        Ok(())
    }

    /// Save every owner registered under `namespace`.
    pub fn save_namespace(&self, namespace: &str) -> Result<(), PersistError> {
        for owner_segs in self.manager.list_owners(namespace) {
            self.save(namespace, &owner_segs.join("/"))?;
        }
        Ok(())
    }

    /// Save every registered `(namespace, owner)` pair.
    pub fn save_all(&self) -> Result<(), PersistError> {
        for (namespace, owner_segs) in self.manager.list_all_owners() {
            self.save(&namespace, &owner_segs.join("/"))?;
        }
        Ok(())
    }

    // ── Load ─────────────────────────────────────────────────────────────────

    /// Load persisted values for `handle`'s owner and apply them on top of the
    /// schema defaults.
    ///
    /// Returns `true` if a file was found and applied, `false` if no file
    /// exists yet (first run — schema defaults remain in effect).
    ///
    /// **Resilience guarantees:**
    ///
    /// | Situation | Behaviour |
    /// |-----------|-----------|
    /// | Key in file, not in schema | Silently skipped |
    /// | Key in schema, not in file | Schema default retained |
    /// | Type mismatch in file | Silently skipped; schema default retained |
    /// | Value fails validation | Silently skipped; schema default retained |
    /// | Read-only key in file | Silently skipped |
    pub fn load(&self, handle: &OwnerHandle) -> Result<bool, PersistError> {
        let path = self.toml_path(handle.namespace(), handle.owner());
        if !path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(&path)?;
        let table: toml::Table = toml::from_str(&content)?;

        // Build a key → default map so we know the expected type for each key.
        let schema: HashMap<String, ConfigValue> = handle
            .list_settings()
            .into_iter()
            .map(|info| (info.key, info.default_value))
            .collect();

        for (key, toml_val) in &table {
            let Some(default) = schema.get(key) else {
                continue;
            };
            if let Ok(value) = toml_to_config_value(toml_val, default) {
                // Validation failures keep the schema default — discard error intentionally.
                let _ = handle.set(key, value);
            }
        }

        Ok(true)
    }

    /// Load persisted values for every handle in `handles`.
    ///
    /// Returns the `(namespace, owner_segments)` pairs for which no persisted
    /// file existed (first run for those owners).
    pub fn load_all<'a>(
        &self,
        handles: impl IntoIterator<Item = &'a OwnerHandle>,
    ) -> Result<Vec<(String, Vec<String>)>, PersistError> {
        let mut first_run = Vec::new();
        for handle in handles {
            if !self.load(handle)? {
                first_run
                    .push((handle.namespace().to_owned(), handle.owner().to_vec()));
            }
        }
        Ok(first_run)
    }
}

// ─── ConfigValue ↔ toml::Value conversions ───────────────────────────────────

/// Convert a [`ConfigValue`] to a [`toml::Value`] for serialization.
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

/// Convert a [`toml::Value`] to a [`ConfigValue`] guided by `expected` (the
/// schema default for this key). Returns an `Err` string on type mismatch.
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
        // Integers in the file are widened to float for user convenience.
        ConfigValue::Float(_) => match toml_val {
            toml::Value::Float(f) => Ok(ConfigValue::Float(*f)),
            toml::Value::Integer(i) => Ok(ConfigValue::Float(*i as f64)),
            other => Err(format!(
                "expected float or integer, got {}",
                other.type_str()
            )),
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

/// Best-effort conversion with no schema guidance (used for empty default arrays).
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
        value::Color,
    };

    fn unique_tmp(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "pulsar_config_{}_{}_{}",
            label,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn make_store() -> (ConfigStore, OwnerHandle) {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Test", "Test owner")
            .setting(
                "volume",
                SchemaEntry::new("Master volume", 80_i64)
                    .validator(Validator::int_range(0, 100)),
            )
            .setting("name", SchemaEntry::new("Player name", "hero"))
            .setting("enabled", SchemaEntry::new("Feature flag", true))
            .setting("brightness", SchemaEntry::new("Screen brightness", 0.8_f64))
            .setting(
                "tint",
                SchemaEntry::new("UI tint", Color::rgba(255, 255, 255, 255)),
            )
            .setting(
                "version",
                SchemaEntry::new("Build version", "1.0.0").read_only(),
            );

        let handle = manager
            .register("editor", "subsystem/test/main", schema)
            .unwrap();
        let store = ConfigStore::with_dir(manager, unique_tmp("persist")).unwrap();
        (store, handle)
    }

    #[test]
    fn toml_path_mirrors_hierarchy() {
        let manager = ConfigManager::new();
        let store = ConfigStore::with_dir(manager, unique_tmp("path")).unwrap();

        let owner = vec![
            "subsystem".to_owned(),
            "physics".to_owned(),
            "main".to_owned(),
        ];
        let path = store.toml_path("editor", &owner);
        assert!(path.ends_with("editor/subsystem/physics/main.toml"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let (store, handle) = make_store();

        handle.set("volume", 42_i64).unwrap();
        handle.set("name", "ferris").unwrap();
        handle.set("enabled", false).unwrap();
        handle.set("brightness", 0.5_f64).unwrap();
        handle.set("tint", Color::rgba(10, 20, 30, 200)).unwrap();

        store.save("editor", "subsystem/test/main").unwrap();

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
        assert_eq!(
            handle.get_color("tint").unwrap(),
            Color::rgba(10, 20, 30, 200)
        );
    }

    #[test]
    fn load_returns_false_on_first_run() {
        let (store, handle) = make_store();
        assert!(!store.load(&handle).unwrap());
    }

    #[test]
    fn load_ignores_unknown_keys() {
        let (store, handle) = make_store();
        let path = store.toml_path("editor", handle.owner());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "volume = 55\nunknown_setting = \"oops\"\n").unwrap();

        store.load(&handle).unwrap();
        assert_eq!(handle.get_int("volume").unwrap(), 55);
    }

    #[test]
    fn load_keeps_default_for_type_mismatch() {
        let (store, handle) = make_store();
        let path = store.toml_path("editor", handle.owner());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "volume = \"not_a_number\"\n").unwrap();

        store.load(&handle).unwrap();
        assert_eq!(handle.get_int("volume").unwrap(), 80);
    }

    #[test]
    fn load_keeps_default_for_validation_failure() {
        let (store, handle) = make_store();
        let path = store.toml_path("editor", handle.owner());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "volume = 999\n").unwrap();

        store.load(&handle).unwrap();
        assert_eq!(handle.get_int("volume").unwrap(), 80);
    }

    #[test]
    fn read_only_not_saved() {
        let (store, handle) = make_store();
        store.save("editor", "subsystem/test/main").unwrap();

        let content =
            fs::read_to_string(store.toml_path("editor", handle.owner())).unwrap();
        assert!(
            !content.contains("version"),
            "read-only key must not appear in the file"
        );
    }

    #[test]
    fn save_all_and_load_all() {
        let manager = ConfigManager::new();
        let s1 = NamespaceSchema::new("A", "").setting("x", SchemaEntry::new("", 1_i64));
        let s2 = NamespaceSchema::new("B", "").setting("y", SchemaEntry::new("", 2_i64));
        let h1 = manager.register("editor", "ns/a", s1).unwrap();
        let h2 = manager.register("project", "ns/b", s2).unwrap();

        let store = ConfigStore::with_dir(manager, unique_tmp("all")).unwrap();

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
