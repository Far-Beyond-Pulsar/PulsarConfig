use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::error::ConfigError;
use crate::schema::NamespaceSchema;
use crate::value::{Color, ConfigValue};

// ─── Change events ───────────────────────────────────────────────────────────

/// Describes a single value change. Passed to every matching change listener.
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    /// The namespace ID that contains the changed key.
    pub namespace: String,
    /// The key that changed.
    pub key: String,
    /// The previous value, or `None` if this is the first write (should not
    /// occur in normal usage since defaults are written at registration time).
    pub old_value: Option<ConfigValue>,
    /// The new value.
    pub new_value: ConfigValue,
}

// ─── Search / listing types ──────────────────────────────────────────────────

/// A single result returned by [`ConfigManager::search`].
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub namespace_id: String,
    pub namespace_display_name: String,
    pub key: String,
    pub description: String,
    pub tags: Vec<String>,
    pub current_value: ConfigValue,
}

/// A snapshot of one setting's metadata and current value.
///
/// Returned by [`ConfigManager::list_settings`] and
/// [`NamespaceHandle::list_settings`].
#[derive(Debug, Clone)]
pub struct SettingInfo {
    pub key: String,
    pub description: String,
    pub current_value: ConfigValue,
    pub default_value: ConfigValue,
    pub tags: Vec<String>,
    pub read_only: bool,
}

// ─── Listener internals ──────────────────────────────────────────────────────

type ListenerCallback = Arc<dyn Fn(&ChangeEvent) + Send + Sync>;

struct Listener {
    id: u64,
    callback: ListenerCallback,
}

/// A RAII guard for a change listener.
///
/// The listener is **automatically removed** when this value is dropped.
/// Store it for as long as you want the listener to be active.
///
/// ```rust
/// # use pulsar_config::*;
/// # let manager = ConfigManager::new();
/// # let schema = NamespaceSchema::new("P","d").setting("k", SchemaEntry::new("d", true));
/// # let handle = manager.register_namespace("p", schema).unwrap();
/// let _guard = handle.on_change("k", |e| println!("{:?}", e.new_value)).unwrap();
/// // listener fires until `_guard` is dropped
/// ```
pub struct ListenerId {
    id: u64,
    /// The compound key used in `Inner::listeners` (`"namespace::key"` or `"namespace::*"` or `"*"`).
    listener_key: String,
    inner: Arc<Inner>,
}

impl Drop for ListenerId {
    fn drop(&mut self) {
        if let Some(entry) = self.inner.listeners.get(&self.listener_key) {
            entry.write().retain(|l| l.id != self.id);
        }
    }
}

// ─── Internal state ──────────────────────────────────────────────────────────

struct RegisteredNamespace {
    display_name: String,
    description: String,
    entries: HashMap<String, crate::schema::SchemaEntry>,
}

struct Inner {
    /// Schema registry — written once at registration, then read-only.
    namespaces: DashMap<String, RegisteredNamespace>,

    /// Live values — one inner DashMap per namespace, pre-seeded with defaults.
    values: DashMap<String, DashMap<String, ConfigValue>>,

    /// Listener registry.
    ///
    /// Keys:
    /// - `"ns::key"` — specific-key listeners
    /// - `"ns::*"`   — namespace-wide listeners
    /// - `"*"`       — global listeners (all namespaces)
    listeners: DashMap<String, RwLock<Vec<Listener>>>,

    next_id: AtomicU64,
}

impl Inner {
    /// Collect all matching callbacks (releasing every lock first) then invoke them.
    ///
    /// Callbacks are called with **no internal locks held**, which means a
    /// callback is free to call `get` or `set` on the manager without risk of
    /// deadlock.
    fn fire_change(&self, event: &ChangeEvent) {
        let mut callbacks: Vec<ListenerCallback> = Vec::new();

        let keys = [
            format!("{}::{}", event.namespace, event.key), // specific
            format!("{}::*", event.namespace),             // namespace-wide
            "*".to_owned(),                                 // global
        ];

        for lk in &keys {
            if let Some(entry) = self.listeners.get(lk) {
                let guard = entry.read();
                for l in guard.iter() {
                    callbacks.push(Arc::clone(&l.callback));
                }
            }
        }

        // All locks released — safe to call user code.
        for cb in callbacks {
            cb(event);
        }
    }

    fn get_value(&self, namespace: &str, key: &str) -> Result<ConfigValue, ConfigError> {
        if self.namespaces.get(namespace).is_none() {
            return Err(ConfigError::NamespaceNotFound(namespace.to_owned()));
        }
        let ns = self.namespaces.get(namespace).unwrap();
        if !ns.entries.contains_key(key) {
            return Err(ConfigError::UnknownKey {
                namespace: namespace.to_owned(),
                key: key.to_owned(),
            });
        }
        // Values map is always kept in sync with the schema.
        let ns_values = self.values.get(namespace).unwrap();
        Ok(ns_values.get(key).unwrap().clone())
    }

    fn set_value(
        &self,
        namespace: &str,
        key: &str,
        value: ConfigValue,
    ) -> Result<(), ConfigError> {
        // --- validate key + schema constraints ---------------------------------
        let (read_only, validation_result) = {
            let ns = self
                .namespaces
                .get(namespace)
                .ok_or_else(|| ConfigError::NamespaceNotFound(namespace.to_owned()))?;
            let entry = ns.entries.get(key).ok_or_else(|| ConfigError::UnknownKey {
                namespace: namespace.to_owned(),
                key: key.to_owned(),
            })?;

            (entry.read_only, entry.validate(&value))
        }; // `ns` dropped here — shard unlocked

        if read_only {
            return Err(ConfigError::ReadOnly {
                namespace: namespace.to_owned(),
                key: key.to_owned(),
            });
        }
        validation_result.map_err(|reason| ConfigError::ValidationFailed {
            namespace: namespace.to_owned(),
            key: key.to_owned(),
            reason,
        })?;

        // --- write the value ---------------------------------------------------
        let old_value = {
            let ns_values = self.values.get(namespace).unwrap();
            let old = ns_values.get(key).map(|v| v.clone());
            ns_values.insert(key.to_owned(), value.clone());
            old
        }; // `ns_values` dropped — shard unlocked

        // --- notify listeners (no locks held) ----------------------------------
        self.fire_change(&ChangeEvent {
            namespace: namespace.to_owned(),
            key: key.to_owned(),
            old_value,
            new_value: value,
        });

        Ok(())
    }

    /// Register a listener under `listener_key` and return its ID.
    fn add_listener(
        &self,
        listener_key: String,
        callback: ListenerCallback,
    ) -> ListenerId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.listeners
            .entry(listener_key.clone())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback,
            });
        ListenerId {
            id,
            listener_key,
            inner: Arc::new(Inner {
                // Share the listener map via a wrapper — see ConfigManager::clone instead.
                // This field is never used; we rebuild the Arc below.
                namespaces: DashMap::new(),
                values: DashMap::new(),
                listeners: DashMap::new(),
                next_id: AtomicU64::new(0),
            }),
        }
    }
}

// ─── ConfigManager ───────────────────────────────────────────────────────────

/// The root configuration manager.
///
/// Cheap to clone — all clones share the same underlying state.
/// Typically owned by the engine or application and handed to subsystems.
///
/// # Thread Safety
///
/// `ConfigManager` is `Send + Sync`. All operations use fine-grained shard
/// locks via [`DashMap`] and [`parking_lot::RwLock`], making concurrent reads
/// effectively lock-free in the common case.
#[derive(Clone)]
pub struct ConfigManager {
    inner: Arc<Inner>,
}

impl ConfigManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                namespaces: DashMap::new(),
                values: DashMap::new(),
                listeners: DashMap::new(),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    // ── Plugin registration ──────────────────────────────────────────────────

    /// Register a configuration namespace and receive a scoped [`NamespaceHandle`].
    ///
    /// This is the primary entry point for plugins. Call once at startup.
    ///
    /// Returns [`ConfigError::NamespaceAlreadyRegistered`] if `id` has already
    /// been registered.
    pub fn register_namespace(
        &self,
        id: impl Into<String>,
        schema: NamespaceSchema,
    ) -> Result<NamespaceHandle, ConfigError> {
        let id: String = id.into();

        if self.inner.namespaces.contains_key(&id) {
            return Err(ConfigError::NamespaceAlreadyRegistered(id));
        }

        // Seed all values from schema defaults.
        let ns_values: DashMap<String, ConfigValue> = schema
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), v.default.clone()))
            .collect();
        self.inner.values.insert(id.clone(), ns_values);

        self.inner.namespaces.insert(
            id.clone(),
            RegisteredNamespace {
                display_name: schema.display_name,
                description: schema.description,
                entries: schema.entries,
            },
        );

        Ok(NamespaceHandle {
            namespace_id: id,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Retrieve a handle to an already-registered namespace.
    ///
    /// Useful for cross-plugin reads or if the original handle was not retained.
    pub fn namespace_handle(&self, id: &str) -> Option<NamespaceHandle> {
        if self.inner.namespaces.contains_key(id) {
            Some(NamespaceHandle {
                namespace_id: id.to_owned(),
                inner: Arc::clone(&self.inner),
            })
        } else {
            None
        }
    }

    // ── Cross-namespace reads ────────────────────────────────────────────────

    /// Read a value from any registered namespace.
    pub fn get(&self, namespace: &str, key: &str) -> Result<ConfigValue, ConfigError> {
        self.inner.get_value(namespace, key)
    }

    // ── Discovery ────────────────────────────────────────────────────────────

    /// Search all namespaces by key name, description text, or tag.
    ///
    /// The query is matched case-insensitively against all three fields.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let q = query.to_lowercase();
        let mut results = Vec::new();

        for ns_ref in self.inner.namespaces.iter() {
            let ns_id = ns_ref.key();
            let ns = ns_ref.value();

            let ns_values = match self.inner.values.get(ns_id) {
                Some(v) => v,
                None => continue,
            };

            for (key, entry) in &ns.entries {
                let hit = key.to_lowercase().contains(&q)
                    || entry.description.to_lowercase().contains(&q)
                    || entry.tags.iter().any(|t| t.to_lowercase().contains(&q));

                if hit {
                    let current_value = ns_values
                        .get(key)
                        .map(|v| v.clone())
                        .unwrap_or_else(|| entry.default.clone());

                    results.push(SearchResult {
                        namespace_id: ns_id.clone(),
                        namespace_display_name: ns.display_name.clone(),
                        key: key.clone(),
                        description: entry.description.clone(),
                        tags: entry.tags.clone(),
                        current_value,
                    });
                }
            }
        }

        results
    }

    /// Return the IDs of all registered namespaces.
    pub fn list_namespaces(&self) -> Vec<String> {
        self.inner
            .namespaces
            .iter()
            .map(|e| e.key().clone())
            .collect()
    }

    /// Return metadata and current values for every key in a namespace.
    ///
    /// Returns `None` if the namespace is not registered.
    pub fn list_settings(&self, namespace: &str) -> Option<Vec<SettingInfo>> {
        let ns = self.inner.namespaces.get(namespace)?;
        let ns_values = self.inner.values.get(namespace)?;

        let infos = ns
            .entries
            .iter()
            .map(|(key, entry)| SettingInfo {
                key: key.clone(),
                description: entry.description.clone(),
                current_value: ns_values
                    .get(key)
                    .map(|v| v.clone())
                    .unwrap_or_else(|| entry.default.clone()),
                default_value: entry.default.clone(),
                tags: entry.tags.clone(),
                read_only: entry.read_only,
            })
            .collect();

        Some(infos)
    }

    // ── Global listeners ─────────────────────────────────────────────────────

    /// Subscribe to **every** value change across all namespaces.
    ///
    /// The returned [`ListenerId`] removes the listener when dropped.
    pub fn on_any_change<F>(&self, callback: F) -> ListenerId
    where
        F: Fn(&ChangeEvent) + Send + Sync + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let key = "*".to_owned();
        self.inner
            .listeners
            .entry(key.clone())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback: Arc::new(callback),
            });
        ListenerId {
            id,
            listener_key: key,
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── NamespaceHandle ─────────────────────────────────────────────────────────

/// A scoped handle to one registered configuration namespace.
///
/// Plugins receive this from [`ConfigManager::register_namespace`] and use it
/// for all subsequent reads, writes, and listener registrations.
///
/// Handles are cheap to clone and share across threads — all copies are backed
/// by the same `Arc<Inner>`.
#[derive(Clone)]
pub struct NamespaceHandle {
    namespace_id: String,
    inner: Arc<Inner>,
}

impl NamespaceHandle {
    /// The namespace ID this handle is scoped to.
    pub fn namespace_id(&self) -> &str {
        &self.namespace_id
    }

    // ── Reads ────────────────────────────────────────────────────────────────

    /// Read any value from this namespace.
    pub fn get(&self, key: &str) -> Result<ConfigValue, ConfigError> {
        self.inner.get_value(&self.namespace_id, key)
    }

    /// Read a `bool` value.
    pub fn get_bool(&self, key: &str) -> Result<bool, ConfigError> {
        self.get(key)?.as_bool()
    }

    /// Read an `i64` value.
    pub fn get_int(&self, key: &str) -> Result<i64, ConfigError> {
        self.get(key)?.as_int()
    }

    /// Read an `f64` value. Integer-typed settings are implicitly widened.
    pub fn get_float(&self, key: &str) -> Result<f64, ConfigError> {
        self.get(key)?.as_float()
    }

    /// Read a `String` value.
    pub fn get_string(&self, key: &str) -> Result<String, ConfigError> {
        Ok(self.get(key)?.as_str()?.to_owned())
    }

    /// Read a [`Color`] value.
    pub fn get_color(&self, key: &str) -> Result<Color, ConfigError> {
        self.get(key)?.as_color()
    }

    // ── Writes ───────────────────────────────────────────────────────────────

    /// Write a value. The value must pass all validators declared in the schema.
    ///
    /// Accepts any type that implements `Into<ConfigValue>` (e.g. `true`,
    /// `42_i64`, `"medium"`, [`Color`]).
    pub fn set(&self, key: &str, value: impl Into<ConfigValue>) -> Result<(), ConfigError> {
        self.inner
            .set_value(&self.namespace_id, key, value.into())
    }

    /// Reset a setting to its schema-defined default, firing change listeners.
    ///
    /// Works even on read-only settings (since the default is authoritative).
    pub fn reset_to_default(&self, key: &str) -> Result<(), ConfigError> {
        let default = {
            let ns = self
                .inner
                .namespaces
                .get(&self.namespace_id)
                .ok_or_else(|| ConfigError::NamespaceNotFound(self.namespace_id.clone()))?;
            let entry = ns.entries.get(key).ok_or_else(|| ConfigError::UnknownKey {
                namespace: self.namespace_id.clone(),
                key: key.to_owned(),
            })?;
            entry.default.clone()
        }; // `ns` dropped — shard unlocked

        let old_value = {
            let ns_values = self.inner.values.get(&self.namespace_id).unwrap();
            let old = ns_values.get(key).map(|v| v.clone());
            ns_values.insert(key.to_owned(), default.clone());
            old
        }; // `ns_values` dropped — shard unlocked

        self.inner.fire_change(&ChangeEvent {
            namespace: self.namespace_id.clone(),
            key: key.to_owned(),
            old_value,
            new_value: default,
        });

        Ok(())
    }

    // ── Listeners ────────────────────────────────────────────────────────────

    /// Subscribe to changes for a single key in this namespace.
    ///
    /// Returns [`ConfigError::UnknownKey`] if `key` is not in the schema.
    /// The listener is removed automatically when the returned [`ListenerId`]
    /// is dropped.
    pub fn on_change<F>(&self, key: &str, callback: F) -> Result<ListenerId, ConfigError>
    where
        F: Fn(&ChangeEvent) + Send + Sync + 'static,
    {
        // Validate that the key is in the schema.
        {
            let ns = self
                .inner
                .namespaces
                .get(&self.namespace_id)
                .ok_or_else(|| ConfigError::NamespaceNotFound(self.namespace_id.clone()))?;
            if !ns.entries.contains_key(key) {
                return Err(ConfigError::UnknownKey {
                    namespace: self.namespace_id.clone(),
                    key: key.to_owned(),
                });
            }
        }

        let listener_key = format!("{}::{}", self.namespace_id, key);
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);

        self.inner
            .listeners
            .entry(listener_key.clone())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback: Arc::new(callback),
            });

        Ok(ListenerId {
            id,
            listener_key,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Subscribe to **all** changes in this namespace.
    ///
    /// Useful for persistence layers or debugging. The listener is removed
    /// automatically when the returned [`ListenerId`] is dropped.
    pub fn on_any_change<F>(&self, callback: F) -> ListenerId
    where
        F: Fn(&ChangeEvent) + Send + Sync + 'static,
    {
        let listener_key = format!("{}::*", self.namespace_id);
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);

        self.inner
            .listeners
            .entry(listener_key.clone())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback: Arc::new(callback),
            });

        ListenerId {
            id,
            listener_key,
            inner: Arc::clone(&self.inner),
        }
    }

    // ── Discovery ────────────────────────────────────────────────────────────

    /// Return metadata and current values for every key in this namespace.
    pub fn list_settings(&self) -> Vec<SettingInfo> {
        let Some(ns) = self.inner.namespaces.get(&self.namespace_id) else {
            return Vec::new();
        };
        let Some(ns_values) = self.inner.values.get(&self.namespace_id) else {
            return Vec::new();
        };

        ns.entries
            .iter()
            .map(|(key, entry)| SettingInfo {
                key: key.clone(),
                description: entry.description.clone(),
                current_value: ns_values
                    .get(key)
                    .map(|v| v.clone())
                    .unwrap_or_else(|| entry.default.clone()),
                default_value: entry.default.clone(),
                tags: entry.tags.clone(),
                read_only: entry.read_only,
            })
            .collect()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };

    use parking_lot::Mutex;

    use super::*;
    use crate::{
        schema::{NamespaceSchema, SchemaEntry, Validator},
        value::ConfigValue,
    };

    fn make_manager() -> (ConfigManager, NamespaceHandle) {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Test Plugin", "A test plugin namespace")
            .setting(
                "count",
                SchemaEntry::new("Item count", 42_i64)
                    .validator(Validator::int_range(0, 100)),
            )
            .setting(
                "name",
                SchemaEntry::new("Plugin name", "default")
                    .validator(Validator::string_max_length(50)),
            )
            .setting("enabled", SchemaEntry::new("Plugin enabled", true))
            .setting(
                "quality",
                SchemaEntry::new("Quality preset", "high")
                    .validator(Validator::string_one_of(["low", "medium", "high", "ultra"])),
            );
        let handle = manager.register_namespace("test", schema).unwrap();
        (manager, handle)
    }

    #[test]
    fn defaults_are_readable() {
        let (_, h) = make_manager();
        assert_eq!(h.get_int("count").unwrap(), 42);
        assert_eq!(h.get_string("name").unwrap(), "default");
        assert!(h.get_bool("enabled").unwrap());
    }

    #[test]
    fn set_and_get() {
        let (_, h) = make_manager();
        h.set("count", 77_i64).unwrap();
        assert_eq!(h.get_int("count").unwrap(), 77);
    }

    #[test]
    fn validation_rejects_out_of_range() {
        let (_, h) = make_manager();
        assert!(h.set("count", 200_i64).is_err());
        // Value must not have changed.
        assert_eq!(h.get_int("count").unwrap(), 42);
    }

    #[test]
    fn validation_rejects_unknown_option() {
        let (_, h) = make_manager();
        assert!(h.set("quality", "extreme").is_err());
        assert_eq!(h.get_string("quality").unwrap(), "high");
    }

    #[test]
    fn unknown_key_returns_error() {
        let (_, h) = make_manager();
        assert!(matches!(
            h.get("nonexistent"),
            Err(ConfigError::UnknownKey { .. })
        ));
    }

    #[test]
    fn duplicate_namespace_returns_error() {
        let manager = ConfigManager::new();
        let s1 = NamespaceSchema::new("A", "a").setting("k", SchemaEntry::new("v", 1_i64));
        let s2 = NamespaceSchema::new("A", "a").setting("k", SchemaEntry::new("v", 1_i64));
        assert!(manager.register_namespace("ns", s1).is_ok());
        assert!(matches!(
            manager.register_namespace("ns", s2),
            Err(ConfigError::NamespaceAlreadyRegistered(_))
        ));
    }

    #[test]
    fn change_listener_fires_on_set() {
        let (_, h) = make_manager();
        let received: Arc<Mutex<Vec<ConfigValue>>> = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::clone(&received);

        let _guard = h
            .on_change("count", move |e| rx.lock().push(e.new_value.clone()))
            .unwrap();

        h.set("count", 10_i64).unwrap();
        h.set("count", 20_i64).unwrap();

        let values = received.lock();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], ConfigValue::Int(10));
        assert_eq!(values[1], ConfigValue::Int(20));
    }

    #[test]
    fn listener_is_removed_on_drop() {
        let (_, h) = make_manager();
        let count = Arc::new(AtomicU64::new(0));
        let c = Arc::clone(&count);

        {
            let _guard = h
                .on_change("count", move |_| {
                    c.fetch_add(1, Ordering::Relaxed);
                })
                .unwrap();
            h.set("count", 10_i64).unwrap(); // fires
        } // _guard dropped — listener removed

        h.set("count", 20_i64).unwrap(); // must not fire
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn namespace_wide_listener() {
        let (_, h) = make_manager();
        let keys: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let ks = Arc::clone(&keys);

        let _guard = h.on_any_change(move |e| ks.lock().push(e.key.clone()));

        h.set("count", 1_i64).unwrap();
        h.set("enabled", false).unwrap();

        let k = keys.lock();
        assert!(k.contains(&"count".to_owned()));
        assert!(k.contains(&"enabled".to_owned()));
    }

    #[test]
    fn reset_to_default() {
        let (_, h) = make_manager();
        h.set("count", 99_i64).unwrap();
        h.reset_to_default("count").unwrap();
        assert_eq!(h.get_int("count").unwrap(), 42);
    }

    #[test]
    fn cross_namespace_read_via_manager() {
        let (manager, _) = make_manager();
        let v = manager.get("test", "count").unwrap();
        assert_eq!(v, ConfigValue::Int(42));
    }

    #[test]
    fn search_finds_by_key() {
        let (manager, _) = make_manager();
        let results = manager.search("count");
        assert!(results.iter().any(|r| r.key == "count"));
    }

    #[test]
    fn search_finds_by_tag() {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Renderer", "Rendering settings").setting(
            "shadows",
            SchemaEntry::new("Enable shadows", true).tag("graphics"),
        );
        manager.register_namespace("renderer", schema).unwrap();

        let results = manager.search("graphics");
        assert!(!results.is_empty());
    }

    #[test]
    fn global_listener_fires_for_all_namespaces() {
        let manager = ConfigManager::new();
        let s1 = NamespaceSchema::new("A", "").setting("x", SchemaEntry::new("", 0_i64));
        let s2 = NamespaceSchema::new("B", "").setting("y", SchemaEntry::new("", 0_i64));

        let h1 = manager.register_namespace("ns_a", s1).unwrap();
        let h2 = manager.register_namespace("ns_b", s2).unwrap();

        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let ev = Arc::clone(&events);
        let _guard = manager.on_any_change(move |e| {
            ev.lock().push(format!("{}::{}", e.namespace, e.key));
        });

        h1.set("x", 1_i64).unwrap();
        h2.set("y", 2_i64).unwrap();

        let e = events.lock();
        assert!(e.contains(&"ns_a::x".to_owned()));
        assert!(e.contains(&"ns_b::y".to_owned()));
    }

    #[test]
    fn read_only_rejects_set() {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Engine", "Engine internals").setting(
            "version",
            SchemaEntry::new("Engine version string", "1.0.0").read_only(),
        );
        let handle = manager.register_namespace("engine", schema).unwrap();

        assert!(matches!(
            handle.set("version", "2.0.0"),
            Err(ConfigError::ReadOnly { .. })
        ));
        // reset_to_default must still work
        assert!(handle.reset_to_default("version").is_ok());
    }
}
