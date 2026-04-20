use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use dashmap::DashMap;
use parking_lot::RwLock;

use crate::error::ConfigError;
use crate::schema::{FieldType, NamespaceSchema};
use crate::value::{Color, ConfigValue};

// ─── Key helpers ─────────────────────────────────────────────────────────────

/// Build the internal DashMap key for a `(namespace, owner)` pair.
///
/// The null byte `\0` is used as a separator because it cannot appear in
/// normal user-supplied strings and is therefore unambiguous.
///
/// Layout: `"{namespace}\0{owner[0]}\0{owner[1]}\0..."`
pub(crate) fn compound_key(namespace: &str, owner: &[String]) -> String {
    let mut k = namespace.to_owned();
    for seg in owner {
        k.push('\0');
        k.push_str(seg);
    }
    k
}

/// Parse a slash-delimited owner path string into its individual segments.
///
/// Leading, trailing, and consecutive slashes are ignored gracefully.
///
/// `"subsystem/physics/main"` → `["subsystem", "physics", "main"]`
pub(crate) fn parse_owner_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Reject any identifier component that contains the internal separator `\0`.
fn validate_id(s: &str, label: &str) -> Result<(), ConfigError> {
    if s.contains('\0') {
        Err(ConfigError::InvalidIdentifier(format!(
            "{label} must not contain null bytes: {s:?}"
        )))
    } else {
        Ok(())
    }
}

// ─── Listener scope ───────────────────────────────────────────────────────────

/// Identifies the scope of a change listener, used as the DashMap key in the
/// listener registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ListenerScope {
    /// Fires only when `key` changes within the owner identified by `compound`.
    Key { compound: String, key: String },
    /// Fires for any key change within the owner identified by `compound`.
    Owner { compound: String },
    /// Fires for any key change across every owner and namespace.
    Global,
}

// ─── Change events ────────────────────────────────────────────────────────────

/// Describes a single value change passed to every matching listener.
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    /// The top-level namespace (e.g. `"editor"` or `"project"`).
    pub namespace: String,
    /// The N-level owner path segments (e.g. `["subsystem", "physics", "main"]`).
    pub owner: Vec<String>,
    /// The setting key that changed.
    pub key: String,
    /// The value before the change, or `None` on the very first write.
    pub old_value: Option<ConfigValue>,
    /// The new value.
    pub new_value: ConfigValue,
}

impl ChangeEvent {
    /// Convenience — returns the owner as a slash-joined string.
    pub fn owner_path(&self) -> String {
        self.owner.join("/")
    }
}

// ─── Search / listing types ───────────────────────────────────────────────────

/// A single result returned by [`ConfigManager::search`].
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Top-level namespace.
    pub namespace: String,
    /// N-level owner path segments.
    pub owner: Vec<String>,
    /// The display name supplied in the owner's [`NamespaceSchema`].
    pub owner_display_name: String,
    /// The setting key.
    pub key: String,
    pub description: String,
    pub tags: Vec<String>,
    pub current_value: ConfigValue,
}

impl SearchResult {
    /// Convenience — returns the owner as a slash-joined string.
    pub fn owner_path(&self) -> String {
        self.owner.join("/")
    }
}

/// A snapshot of one setting's metadata and current value.
///
/// Returned by [`ConfigManager::list_settings`] and [`OwnerHandle::list_settings`].
#[derive(Debug, Clone)]
pub struct SettingInfo {
    /// The short key within the owner (e.g. `"theme"`).
    pub key: String,
    /// The namespace this setting belongs to (e.g. `"editor"` or `"project"`).
    pub namespace: String,
    /// The owner path this setting belongs to (e.g. `"appearance"`).
    pub owner: String,
    pub label: Option<String>,
    pub page: Option<String>,
    pub description: String,
    pub current_value: ConfigValue,
    pub default_value: ConfigValue,
    pub tags: Vec<String>,
    pub read_only: bool,
    pub field_type: Option<FieldType>,
}

// ─── Listener internals ───────────────────────────────────────────────────────

type ListenerCallback = Arc<dyn Fn(&ChangeEvent) + Send + Sync>;

struct Listener {
    id: u64,
    callback: ListenerCallback,
}

/// A RAII guard for a change listener.
///
/// The listener is **automatically removed** when this value is dropped —
/// no explicit deregistration call is needed.
pub struct ListenerId {
    id: u64,
    scope: ListenerScope,
    inner: Arc<Inner>,
}

impl Drop for ListenerId {
    fn drop(&mut self) {
        if let Some(entry) = self.inner.listeners.get(&self.scope) {
            entry.write().retain(|l| l.id != self.id);
        }
    }
}

// ─── Internal storage ────────────────────────────────────────────────────────

struct RegisteredOwner {
    /// Stored so error messages are self-contained without callers needing to
    /// re-supply namespace/owner on every operation.
    namespace: String,
    owner: Vec<String>,
    display_name: String,
    #[allow(dead_code)]
    description: String,
    entries: HashMap<String, crate::schema::SchemaEntry>,
}

struct Inner {
    /// Schema registry — keyed by `compound_key(namespace, owner)`.
    /// Written once at registration; effectively read-only afterwards.
    owners: DashMap<String, RegisteredOwner>,

    /// Live values — outer key: compound, inner key: setting name.
    values: DashMap<String, DashMap<String, ConfigValue>>,

    /// Listener registry keyed by [`ListenerScope`].
    listeners: DashMap<ListenerScope, RwLock<Vec<Listener>>>,

    next_id: AtomicU64,
}

impl Inner {
    /// Fire all callbacks matching the changed setting.
    ///
    /// Callbacks are invoked **with no internal locks held**, so listeners may
    /// safely call `get` or `set` on the manager without risk of deadlock.
    fn fire_change(&self, event: &ChangeEvent) {
        let compound = compound_key(&event.namespace, &event.owner);

        let scopes = [
            ListenerScope::Key {
                compound: compound.clone(),
                key: event.key.clone(),
            },
            ListenerScope::Owner { compound },
            ListenerScope::Global,
        ];

        let mut callbacks: Vec<ListenerCallback> = Vec::new();
        for scope in &scopes {
            if let Some(entry) = self.listeners.get(scope) {
                for l in entry.read().iter() {
                    callbacks.push(Arc::clone(&l.callback));
                }
            }
        }

        for cb in callbacks {
            cb(event);
        }
    }
}

// ─── ConfigManager ────────────────────────────────────────────────────────────

/// The root configuration manager.
///
/// Cheap to clone — all clones share the same underlying state via `Arc`.
/// Typically created once and distributed to all subsystems and plugins.
///
/// Settings are organised in three tiers:
///
/// 1. **Namespace** — top-level scope, e.g. `"editor"` vs `"project"`.
/// 2. **Owner path** — an N-level slash-delimited path identifying the
///    subsystem or plugin, e.g. `"subsystem/physics/main"` or
///    `"plugin/my_plugin/ui"`. Any depth is supported.
/// 3. **Key** — the individual setting name within the owner's schema.
///
/// # Thread Safety
///
/// `ConfigManager` is `Send + Sync`. All operations use fine-grained shard
/// locks via [`DashMap`] and [`parking_lot::RwLock`]; concurrent reads are
/// effectively contention-free.
#[derive(Clone)]
pub struct ConfigManager {
    inner: Arc<Inner>,
}

impl ConfigManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                owners: DashMap::new(),
                values: DashMap::new(),
                listeners: DashMap::new(),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    // ── Registration ─────────────────────────────────────────────────────────

    /// Register a configuration owner and receive a scoped [`OwnerHandle`].
    ///
    /// - `namespace` — top-level scope (e.g. `"editor"`).
    /// - `owner` — slash-delimited path (e.g. `"subsystem/physics/main"`).
    ///   Any depth is supported.
    ///
    /// Call once per owner, typically at startup or plugin load time.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::OwnerAlreadyRegistered`] — this `(namespace, owner)`
    ///   pair was already registered.
    /// - [`ConfigError::InvalidIdentifier`] — `namespace` or an owner segment
    ///   contains a null byte.
    pub fn register(
        &self,
        namespace: &str,
        owner: &str,
        schema: NamespaceSchema,
    ) -> Result<OwnerHandle, ConfigError> {
        validate_id(namespace, "namespace")?;

        let owner_vec = parse_owner_path(owner);
        for seg in &owner_vec {
            validate_id(seg, "owner segment")?;
        }

        let compound = compound_key(namespace, &owner_vec);

        if self.inner.owners.contains_key(&compound) {
            return Err(ConfigError::OwnerAlreadyRegistered {
                namespace: namespace.to_owned(),
                owner: owner_vec,
            });
        }

        // Pre-seed all values from schema defaults.
        let seed: DashMap<String, ConfigValue> = schema
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), v.default.clone()))
            .collect();
        self.inner.values.insert(compound.clone(), seed);

        self.inner.owners.insert(
            compound.clone(),
            RegisteredOwner {
                namespace: namespace.to_owned(),
                owner: owner_vec.clone(),
                display_name: schema.display_name,
                description: schema.description,
                entries: schema.entries,
            },
        );

        Ok(OwnerHandle {
            namespace: namespace.to_owned(),
            owner: owner_vec,
            compound,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Retrieve a handle to an already-registered owner.
    ///
    /// Returns `None` if the `(namespace, owner)` pair is not registered.
    pub fn owner_handle(&self, namespace: &str, owner: &str) -> Option<OwnerHandle> {
        let owner_vec = parse_owner_path(owner);
        let compound = compound_key(namespace, &owner_vec);
        if self.inner.owners.contains_key(&compound) {
            Some(OwnerHandle {
                namespace: namespace.to_owned(),
                owner: owner_vec,
                compound,
                inner: Arc::clone(&self.inner),
            })
        } else {
            None
        }
    }

    // ── Cross-owner reads ─────────────────────────────────────────────────────

    /// Read a value from any registered owner.
    ///
    /// This is a read-only cross-owner accessor. To write, use an [`OwnerHandle`].
    pub fn get(
        &self,
        namespace: &str,
        owner: &str,
        key: &str,
    ) -> Result<ConfigValue, ConfigError> {
        let owner_vec = parse_owner_path(owner);
        let compound = compound_key(namespace, &owner_vec);

        let owner_data =
            self.inner
                .owners
                .get(&compound)
                .ok_or_else(|| ConfigError::OwnerNotFound {
                    namespace: namespace.to_owned(),
                    owner: owner_vec.clone(),
                })?;

        if !owner_data.entries.contains_key(key) {
            return Err(ConfigError::UnknownKey {
                namespace: namespace.to_owned(),
                owner: owner_vec,
                key: key.to_owned(),
            });
        }
        drop(owner_data); // release shard before values lookup

        Ok(self
            .inner
            .values
            .get(&compound)
            .unwrap()
            .get(key)
            .unwrap()
            .clone())
    }

    // ── Discovery ─────────────────────────────────────────────────────────────

    /// Search every registered owner by key name, description, or tag.
    ///
    /// The query is matched case-insensitively against all three fields.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let q = query.to_lowercase();
        let mut results = Vec::new();

        for owner_ref in self.inner.owners.iter() {
            let compound = owner_ref.key();
            let owner_data = owner_ref.value();

            let Some(values) = self.inner.values.get(compound) else {
                continue;
            };

            for (key, entry) in &owner_data.entries {
                let hit = key.to_lowercase().contains(&q)
                    || entry.description.to_lowercase().contains(&q)
                    || entry.tags.iter().any(|t| t.to_lowercase().contains(&q));

                if hit {
                    let current_value = values
                        .get(key)
                        .map(|v| v.clone())
                        .unwrap_or_else(|| entry.default.clone());

                    results.push(SearchResult {
                        namespace: owner_data.namespace.clone(),
                        owner: owner_data.owner.clone(),
                        owner_display_name: owner_data.display_name.clone(),
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

    /// Return every distinct namespace that has at least one registered owner.
    pub fn list_namespaces(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        for entry in self.inner.owners.iter() {
            seen.insert(entry.value().namespace.clone());
        }
        seen.into_iter().collect()
    }

    /// Return the owner paths of every owner registered under `namespace`.
    pub fn list_owners(&self, namespace: &str) -> Vec<Vec<String>> {
        self.inner
            .owners
            .iter()
            .filter(|e| e.value().namespace == namespace)
            .map(|e| e.value().owner.clone())
            .collect()
    }

    /// Return every `(namespace, owner_segments)` pair registered in the manager.
    pub fn list_all_owners(&self) -> Vec<(String, Vec<String>)> {
        self.inner
            .owners
            .iter()
            .map(|e| (e.value().namespace.clone(), e.value().owner.clone()))
            .collect()
    }

    /// Return metadata and current values for every key belonging to an owner.
    ///
    /// Returns `None` if the `(namespace, owner)` pair is not registered.
    pub fn list_settings(&self, namespace: &str, owner: &str) -> Option<Vec<SettingInfo>> {
        let owner_vec = parse_owner_path(owner);
        let compound = compound_key(namespace, &owner_vec);

        let owner_data = self.inner.owners.get(&compound)?;
        let values = self.inner.values.get(&compound)?;

        Some(
            owner_data
                .entries
                .iter()
                .map(|(key, entry)| SettingInfo {
                    key: key.clone(),
                    namespace: namespace.to_owned(),
                    owner: owner_vec.join("/"),
                    label: entry.label.clone(),
                    page: entry.page.clone(),
                    description: entry.description.clone(),
                    current_value: values
                        .get(key)
                        .map(|v| v.clone())
                        .unwrap_or_else(|| entry.default.clone()),
                    default_value: entry.default.clone(),
                    tags: entry.tags.clone(),
                    read_only: entry.read_only,
                    field_type: entry.field_type.clone(),
                })
                .collect(),
        )
    }

    /// Return every [`SettingInfo`] across all registered owners in all namespaces.
    pub fn list_all_settings(&self) -> Vec<SettingInfo> {
        let mut out = Vec::new();
        for owner_ref in self.inner.owners.iter() {
            let compound = owner_ref.key();
            let owner_data = owner_ref.value();
            let Some(values) = self.inner.values.get(compound) else { continue };
            for (key, entry) in &owner_data.entries {
                out.push(SettingInfo {
                    key: key.clone(),
                    namespace: owner_data.namespace.clone(),
                    owner: owner_data.owner.join("/"),
                    label: entry.label.clone(),
                    page: entry.page.clone(),
                    description: entry.description.clone(),
                    current_value: values.get(key).map(|v| v.clone()).unwrap_or_else(|| entry.default.clone()),
                    default_value: entry.default.clone(),
                    tags: entry.tags.clone(),
                    read_only: entry.read_only,
                    field_type: entry.field_type.clone(),
                });
            }
        }
        out
    }

    /// Return all distinct page names used by settings in a given namespace.
    ///
    /// Useful for building the sidebar of a settings screen.
    pub fn list_pages(&self, namespace: &str) -> Vec<String> {
        let mut pages: Vec<String> = self.inner
            .owners
            .iter()
            .filter(|e| e.value().namespace == namespace)
            .flat_map(|e| e.value().entries.values().filter_map(|ent| ent.page.clone()).collect::<Vec<_>>())
            .collect();
        pages.sort();
        pages.dedup();
        pages
    }

    /// Return every setting in `namespace` that belongs to `page`.
    pub fn list_settings_by_page(&self, namespace: &str, page: &str) -> Vec<SettingInfo> {
        let mut out = Vec::new();
        for owner_ref in self.inner.owners.iter() {
            let owner_data = owner_ref.value();
            if owner_data.namespace != namespace { continue; }
            let compound = owner_ref.key();
            let Some(values) = self.inner.values.get(compound) else { continue };
            for (key, entry) in &owner_data.entries {
                if entry.page.as_deref() == Some(page) {
                    out.push(SettingInfo {
                        key: key.clone(),
                        namespace: owner_data.namespace.clone(),
                        owner: owner_data.owner.join("/"),
                        label: entry.label.clone(),
                        page: entry.page.clone(),
                        description: entry.description.clone(),
                        current_value: values.get(key).map(|v| v.clone()).unwrap_or_else(|| entry.default.clone()),
                        default_value: entry.default.clone(),
                        tags: entry.tags.clone(),
                        read_only: entry.read_only,
                        field_type: entry.field_type.clone(),
                    });
                }
            }
        }
        out
    }

    // ── Global listeners ──────────────────────────────────────────────────────

    /// Subscribe to **every** value change across all namespaces and owners.
    ///
    /// The returned [`ListenerId`] removes the listener when dropped.
    pub fn on_any_change<F>(&self, callback: F) -> ListenerId
    where
        F: Fn(&ChangeEvent) + Send + Sync + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .listeners
            .entry(ListenerScope::Global)
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback: Arc::new(callback),
            });
        ListenerId {
            id,
            scope: ListenerScope::Global,
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── OwnerHandle ─────────────────────────────────────────────────────────────

/// A scoped handle to one registered `(namespace, owner)` pair.
///
/// Plugins receive this from [`ConfigManager::register`] and use it for all
/// subsequent reads, writes, and listener registrations within their scope.
///
/// Handles are cheap to clone — all copies share the same `Arc<Inner>`.
///
/// Address hierarchy:
///
/// ```text
/// namespace  ("editor")
///   └── owner path  ("subsystem/physics/main")   ← any depth
///         └── key   ("gravity")
/// ```
#[derive(Clone)]
pub struct OwnerHandle {
    namespace: String,
    /// Individual path segments, e.g. `["subsystem", "physics", "main"]`.
    owner: Vec<String>,
    /// Cached `compound_key(namespace, owner)` — avoids recomputation on reads.
    compound: String,
    inner: Arc<Inner>,
}

impl OwnerHandle {
    /// The top-level namespace this handle is scoped to.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The owner path segments this handle is scoped to.
    pub fn owner(&self) -> &[String] {
        &self.owner
    }

    /// The owner path as a slash-joined string (e.g. `"subsystem/physics/main"`).
    pub fn owner_path(&self) -> String {
        self.owner.join("/")
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn owner_not_found(&self) -> ConfigError {
        ConfigError::OwnerNotFound {
            namespace: self.namespace.clone(),
            owner: self.owner.clone(),
        }
    }

    fn unknown_key(&self, key: &str) -> ConfigError {
        ConfigError::UnknownKey {
            namespace: self.namespace.clone(),
            owner: self.owner.clone(),
            key: key.to_owned(),
        }
    }

    // ── Reads ─────────────────────────────────────────────────────────────────

    /// Read any value from this owner's settings.
    pub fn get(&self, key: &str) -> Result<ConfigValue, ConfigError> {
        let owner_data = self
            .inner
            .owners
            .get(&self.compound)
            .ok_or_else(|| self.owner_not_found())?;

        if !owner_data.entries.contains_key(key) {
            return Err(self.unknown_key(key));
        }
        drop(owner_data);

        Ok(self
            .inner
            .values
            .get(&self.compound)
            .unwrap()
            .get(key)
            .unwrap()
            .clone())
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

    // ── Writes ────────────────────────────────────────────────────────────────

    /// Write a value. The value must pass all validators declared in the schema.
    ///
    /// Accepts any type that implements `Into<ConfigValue>` — e.g. `true`,
    /// `42_i64`, `"medium"`, [`Color`].
    pub fn set(&self, key: &str, value: impl Into<ConfigValue>) -> Result<(), ConfigError> {
        let value = value.into();

        // --- validate (shard locked) -----------------------------------------
        let (read_only, validation_result) = {
            let owner_data = self
                .inner
                .owners
                .get(&self.compound)
                .ok_or_else(|| self.owner_not_found())?;

            let entry = owner_data
                .entries
                .get(key)
                .ok_or_else(|| self.unknown_key(key))?;

            (entry.read_only, entry.validate(&value))
        }; // shard lock released

        if read_only {
            return Err(ConfigError::ReadOnly {
                namespace: self.namespace.clone(),
                owner: self.owner.clone(),
                key: key.to_owned(),
            });
        }

        validation_result.map_err(|reason| ConfigError::ValidationFailed {
            namespace: self.namespace.clone(),
            owner: self.owner.clone(),
            key: key.to_owned(),
            reason,
        })?;

        // --- write (shard locked) --------------------------------------------
        let old_value = {
            let values = self.inner.values.get(&self.compound).unwrap();
            let old = values.get(key).map(|v| v.clone());
            values.insert(key.to_owned(), value.clone());
            old
        }; // shard lock released

        // --- notify (no locks held) ------------------------------------------
        self.inner.fire_change(&ChangeEvent {
            namespace: self.namespace.clone(),
            owner: self.owner.clone(),
            key: key.to_owned(),
            old_value,
            new_value: value,
        });

        Ok(())
    }

    /// Reset a setting to its schema-defined default, firing change listeners.
    ///
    /// Works even on read-only settings — the schema default is always
    /// authoritative.
    pub fn reset_to_default(&self, key: &str) -> Result<(), ConfigError> {
        let default = {
            let owner_data = self
                .inner
                .owners
                .get(&self.compound)
                .ok_or_else(|| self.owner_not_found())?;
            let entry = owner_data
                .entries
                .get(key)
                .ok_or_else(|| self.unknown_key(key))?;
            entry.default.clone()
        }; // shard lock released

        let old_value = {
            let values = self.inner.values.get(&self.compound).unwrap();
            let old = values.get(key).map(|v| v.clone());
            values.insert(key.to_owned(), default.clone());
            old
        }; // shard lock released

        self.inner.fire_change(&ChangeEvent {
            namespace: self.namespace.clone(),
            owner: self.owner.clone(),
            key: key.to_owned(),
            old_value,
            new_value: default,
        });

        Ok(())
    }

    // ── Listeners ─────────────────────────────────────────────────────────────

    /// Subscribe to changes for a single key within this owner.
    ///
    /// Returns [`ConfigError::UnknownKey`] if `key` is not in the schema.
    /// The listener is removed automatically when the returned [`ListenerId`]
    /// is dropped.
    pub fn on_change<F>(&self, key: &str, callback: F) -> Result<ListenerId, ConfigError>
    where
        F: Fn(&ChangeEvent) + Send + Sync + 'static,
    {
        {
            let owner_data = self
                .inner
                .owners
                .get(&self.compound)
                .ok_or_else(|| self.owner_not_found())?;
            if !owner_data.entries.contains_key(key) {
                return Err(self.unknown_key(key));
            }
        }

        let scope = ListenerScope::Key {
            compound: self.compound.clone(),
            key: key.to_owned(),
        };
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);

        self.inner
            .listeners
            .entry(scope.clone())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback: Arc::new(callback),
            });

        Ok(ListenerId {
            id,
            scope,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Subscribe to **all** changes within this owner's settings.
    ///
    /// The listener is removed automatically when the returned [`ListenerId`]
    /// is dropped.
    pub fn on_any_change<F>(&self, callback: F) -> ListenerId
    where
        F: Fn(&ChangeEvent) + Send + Sync + 'static,
    {
        let scope = ListenerScope::Owner {
            compound: self.compound.clone(),
        };
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);

        self.inner
            .listeners
            .entry(scope.clone())
            .or_insert_with(|| RwLock::new(Vec::new()))
            .write()
            .push(Listener {
                id,
                callback: Arc::new(callback),
            });

        ListenerId {
            id,
            scope,
            inner: Arc::clone(&self.inner),
        }
    }

    // ── Discovery ─────────────────────────────────────────────────────────────

    /// Return metadata and current values for every key in this owner's schema.
    pub fn list_settings(&self) -> Vec<SettingInfo> {
        let Some(owner_data) = self.inner.owners.get(&self.compound) else {
            return Vec::new();
        };
        let Some(values) = self.inner.values.get(&self.compound) else {
            return Vec::new();
        };

        owner_data
            .entries
            .iter()
            .map(|(key, entry)| SettingInfo {
                key: key.clone(),
                namespace: self.namespace.clone(),
                owner: self.owner.join("/"),
                label: entry.label.clone(),
                page: entry.page.clone(),
                description: entry.description.clone(),
                current_value: values
                    .get(key)
                    .map(|v| v.clone())
                    .unwrap_or_else(|| entry.default.clone()),
                default_value: entry.default.clone(),
                tags: entry.tags.clone(),
                read_only: entry.read_only,
                field_type: entry.field_type.clone(),
            })
            .collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

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

    fn make_manager() -> (ConfigManager, OwnerHandle) {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Physics", "Physics subsystem settings")
            .setting(
                "gravity",
                SchemaEntry::new("Gravitational acceleration", 9.81_f64)
                    .validator(Validator::float_range(0.0, 100.0)),
            )
            .setting(
                "solver",
                SchemaEntry::new("Solver preset", "medium")
                    .validator(Validator::string_one_of(["fast", "medium", "accurate"])),
            )
            .setting("enabled", SchemaEntry::new("Enable physics", true))
            .setting(
                "version",
                SchemaEntry::new("Physics engine version", "2.0.0").read_only(),
            );

        let handle = manager
            .register("editor", "subsystem/physics/main", schema)
            .unwrap();
        (manager, handle)
    }

    #[test]
    fn namespace_and_owner_accessible_on_handle() {
        let (_, h) = make_manager();
        assert_eq!(h.namespace(), "editor");
        assert_eq!(h.owner(), &["subsystem", "physics", "main"]);
        assert_eq!(h.owner_path(), "subsystem/physics/main");
    }

    #[test]
    fn defaults_are_readable() {
        let (_, h) = make_manager();
        assert!((h.get_float("gravity").unwrap() - 9.81).abs() < 1e-9);
        assert_eq!(h.get_string("solver").unwrap(), "medium");
        assert!(h.get_bool("enabled").unwrap());
    }

    #[test]
    fn set_and_get() {
        let (_, h) = make_manager();
        h.set("gravity", 1.62_f64).unwrap();
        assert!((h.get_float("gravity").unwrap() - 1.62).abs() < 1e-9);
    }

    #[test]
    fn validation_rejects_out_of_range() {
        let (_, h) = make_manager();
        assert!(h.set("gravity", 9999.0_f64).is_err());
        assert!((h.get_float("gravity").unwrap() - 9.81).abs() < 1e-9);
    }

    #[test]
    fn validation_rejects_unknown_option() {
        let (_, h) = make_manager();
        assert!(h.set("solver", "turbo").is_err());
        assert_eq!(h.get_string("solver").unwrap(), "medium");
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
    fn duplicate_owner_returns_error() {
        let manager = ConfigManager::new();
        let s1 = NamespaceSchema::new("A", "a").setting("k", SchemaEntry::new("v", 1_i64));
        let s2 = NamespaceSchema::new("A", "a").setting("k", SchemaEntry::new("v", 1_i64));
        assert!(manager.register("ns", "owner/path", s1).is_ok());
        assert!(matches!(
            manager.register("ns", "owner/path", s2),
            Err(ConfigError::OwnerAlreadyRegistered { .. })
        ));
    }

    #[test]
    fn different_namespaces_same_owner_path_coexist() {
        let manager = ConfigManager::new();
        let s1 = NamespaceSchema::new("A", "a").setting("x", SchemaEntry::new("v", 1_i64));
        let s2 = NamespaceSchema::new("B", "b").setting("x", SchemaEntry::new("v", 2_i64));
        let h1 = manager.register("editor", "subsystem/audio", s1).unwrap();
        let h2 = manager.register("project", "subsystem/audio", s2).unwrap();
        h1.set("x", 10_i64).unwrap();
        h2.set("x", 20_i64).unwrap();
        assert_eq!(h1.get_int("x").unwrap(), 10);
        assert_eq!(h2.get_int("x").unwrap(), 20);
    }

    #[test]
    fn n_level_deep_owner_path() {
        let manager = ConfigManager::new();
        let schema =
            NamespaceSchema::new("Deep", "").setting("v", SchemaEntry::new("", 42_i64));
        let h = manager.register("editor", "a/b/c/d/e/f", schema).unwrap();
        assert_eq!(h.owner(), &["a", "b", "c", "d", "e", "f"]);
        assert_eq!(h.get_int("v").unwrap(), 42);
    }

    #[test]
    fn change_listener_fires_on_set() {
        let (_, h) = make_manager();
        let received: Arc<Mutex<Vec<ConfigValue>>> = Arc::new(Mutex::new(Vec::new()));
        let rx = Arc::clone(&received);
        let _guard = h
            .on_change("gravity", move |e| rx.lock().push(e.new_value.clone()))
            .unwrap();
        h.set("gravity", 1.0_f64).unwrap();
        h.set("gravity", 2.0_f64).unwrap();
        let values = received.lock();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], ConfigValue::Float(1.0));
        assert_eq!(values[1], ConfigValue::Float(2.0));
    }

    #[test]
    fn listener_carries_owner_path() {
        let (_, h) = make_manager();
        let captured: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
        let cap = Arc::clone(&captured);
        let _guard = h
            .on_change("gravity", move |e| {
                *cap.lock() = Some(e.owner.clone());
            })
            .unwrap();
        h.set("gravity", 5.0_f64).unwrap();
        let locked = captured.lock();
        let got: &[String] = locked.as_deref().unwrap();
        let expected: &[&str] = &["subsystem", "physics", "main"];
        assert_eq!(got.len(), expected.len());
        for (a, b) in got.iter().zip(expected.iter()) {
            assert_eq!(a.as_str(), *b);
        }
    }

    #[test]
    fn listener_is_removed_on_drop() {
        let (_, h) = make_manager();
        let count = Arc::new(AtomicU64::new(0));
        let c = Arc::clone(&count);
        {
            let _guard = h
                .on_change("gravity", move |_| {
                    c.fetch_add(1, Ordering::Relaxed);
                })
                .unwrap();
            h.set("gravity", 1.0_f64).unwrap();
        }
        h.set("gravity", 2.0_f64).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn owner_wide_listener() {
        let (_, h) = make_manager();
        let keys: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let ks = Arc::clone(&keys);
        let _guard = h.on_any_change(move |e| ks.lock().push(e.key.clone()));
        h.set("gravity", 1.0_f64).unwrap();
        h.set("enabled", false).unwrap();
        let k = keys.lock();
        assert!(k.contains(&"gravity".to_owned()));
        assert!(k.contains(&"enabled".to_owned()));
    }

    #[test]
    fn reset_to_default() {
        let (_, h) = make_manager();
        h.set("gravity", 1.62_f64).unwrap();
        h.reset_to_default("gravity").unwrap();
        assert!((h.get_float("gravity").unwrap() - 9.81).abs() < 1e-9);
    }

    #[test]
    fn cross_owner_read_via_manager() {
        let (manager, _) = make_manager();
        let v = manager
            .get("editor", "subsystem/physics/main", "gravity")
            .unwrap();
        assert_eq!(v, ConfigValue::Float(9.81));
    }

    #[test]
    fn owner_not_found_via_manager() {
        let manager = ConfigManager::new();
        assert!(matches!(
            manager.get("editor", "does/not/exist", "key"),
            Err(ConfigError::OwnerNotFound { .. })
        ));
    }

    #[test]
    fn search_finds_by_key() {
        let (manager, _) = make_manager();
        let results = manager.search("gravity");
        assert!(results.iter().any(|r| r.key == "gravity"));
    }

    #[test]
    fn search_result_carries_owner_path() {
        let (manager, _) = make_manager();
        let results = manager.search("gravity");
        let r = results.iter().find(|r| r.key == "gravity").unwrap();
        assert_eq!(r.namespace, "editor");
        assert_eq!(r.owner, ["subsystem", "physics", "main"]);
    }

    #[test]
    fn search_finds_by_tag() {
        let manager = ConfigManager::new();
        let schema = NamespaceSchema::new("Renderer", "Rendering settings").setting(
            "shadows",
            SchemaEntry::new("Enable shadows", true).tag("graphics"),
        );
        manager.register("editor", "renderer/shadows", schema).unwrap();
        let results = manager.search("graphics");
        assert!(!results.is_empty());
    }

    #[test]
    fn list_namespaces_deduplicates() {
        let manager = ConfigManager::new();
        let s =
            || NamespaceSchema::new("X", "").setting("k", SchemaEntry::new("", 0_i64));
        manager.register("editor", "a", s()).unwrap();
        manager.register("editor", "b", s()).unwrap();
        manager.register("project", "a", s()).unwrap();
        let mut ns = manager.list_namespaces();
        ns.sort();
        assert_eq!(ns, ["editor", "project"]);
    }

    #[test]
    fn list_owners_scoped_to_namespace() {
        let manager = ConfigManager::new();
        let s =
            || NamespaceSchema::new("X", "").setting("k", SchemaEntry::new("", 0_i64));
        manager.register("editor", "a/b", s()).unwrap();
        manager.register("editor", "a/c", s()).unwrap();
        manager.register("project", "a/b", s()).unwrap();
        let owners = manager.list_owners("editor");
        assert_eq!(owners.len(), 2);
    }

    #[test]
    fn global_listener_fires_for_all_owners() {
        let manager = ConfigManager::new();
        let s = |v: i64| NamespaceSchema::new("X", "").setting("x", SchemaEntry::new("", v));
        let h1 = manager.register("editor", "a", s(0)).unwrap();
        let h2 = manager.register("project", "b", s(0)).unwrap();
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let ev = Arc::clone(&events);
        let _guard = manager.on_any_change(move |e| {
            ev.lock().push(format!("{}:{}", e.namespace, e.owner_path()));
        });
        h1.set("x", 1_i64).unwrap();
        h2.set("x", 2_i64).unwrap();
        let e = events.lock();
        assert!(e.contains(&"editor:a".to_owned()));
        assert!(e.contains(&"project:b".to_owned()));
    }

    #[test]
    fn read_only_rejects_set() {
        let (_, handle) = make_manager();
        assert!(matches!(
            handle.set("version", "3.0.0"),
            Err(ConfigError::ReadOnly { .. })
        ));
        assert!(handle.reset_to_default("version").is_ok());
    }

    #[test]
    fn owner_handle_retrieval() {
        let manager = ConfigManager::new();
        let schema =
            NamespaceSchema::new("A", "").setting("k", SchemaEntry::new("", 7_i64));
        manager.register("ns", "a/b/c", schema).unwrap();
        let h = manager.owner_handle("ns", "a/b/c").unwrap();
        assert_eq!(h.get_int("k").unwrap(), 7);
        assert!(manager.owner_handle("ns", "x/y/z").is_none());
    }
}
