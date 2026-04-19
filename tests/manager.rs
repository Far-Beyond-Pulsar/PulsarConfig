//! Tests for `ConfigManager` — registration, cross-owner access, discovery, search.

use pulsar_config::{ConfigError, ConfigManager, ConfigValue, NamespaceSchema, SchemaEntry, Validator};

fn basic_schema() -> NamespaceSchema {
    NamespaceSchema::new("Test", "Test schema")
        .setting("count", SchemaEntry::new("A counter", 0_i64).validator(Validator::int_range(0, 1000)))
        .setting("label", SchemaEntry::new("A label", "default").validator(Validator::string_max_length(32)))
        .setting("active", SchemaEntry::new("Active flag", true))
        .setting("ratio", SchemaEntry::new("A ratio", 1.0_f64).validator(Validator::float_range(0.0, 1.0)))
        .setting("version", SchemaEntry::new("Version string", "1.0.0").read_only())
}

// ── Construction ─────────────────────────────────────────────────────────────

#[test]
fn new_creates_empty_manager() {
    let m = ConfigManager::new();
    assert!(m.list_namespaces().is_empty());
    assert!(m.list_all_owners().is_empty());
}

#[test]
fn default_creates_empty_manager() {
    let m = ConfigManager::default();
    assert!(m.list_namespaces().is_empty());
}

// ── register ─────────────────────────────────────────────────────────────────

#[test]
fn register_returns_handle_with_correct_namespace() {
    let m = ConfigManager::new();
    let h = m.register("editor", "plugin/audio", basic_schema()).unwrap();
    assert_eq!(h.namespace(), "editor");
}

#[test]
fn register_returns_handle_with_correct_owner_segments() {
    let m = ConfigManager::new();
    let h = m.register("editor", "plugin/audio", basic_schema()).unwrap();
    assert_eq!(h.owner(), &["plugin", "audio"]);
}

#[test]
fn register_returns_handle_with_correct_owner_path() {
    let m = ConfigManager::new();
    let h = m.register("editor", "subsystem/physics/main", basic_schema()).unwrap();
    assert_eq!(h.owner_path(), "subsystem/physics/main");
}

#[test]
fn register_seeds_all_defaults() {
    let m = ConfigManager::new();
    let h = m.register("ns", "o", basic_schema()).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 0);
    assert_eq!(h.get_string("label").unwrap(), "default");
    assert!(h.get_bool("active").unwrap());
    assert!((h.get_float("ratio").unwrap() - 1.0).abs() < 1e-9);
}

#[test]
fn register_rejects_duplicate_same_namespace_and_owner() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    let result = m.register("ns", "o", basic_schema());
    assert!(matches!(result, Err(ConfigError::OwnerAlreadyRegistered { .. })));
}

#[test]
fn register_allows_same_owner_in_different_namespace() {
    let m = ConfigManager::new();
    assert!(m.register("editor", "plugin/audio", basic_schema()).is_ok());
    assert!(m.register("project", "plugin/audio", basic_schema()).is_ok());
}

#[test]
fn register_allows_different_owners_same_namespace() {
    let m = ConfigManager::new();
    let s1 = NamespaceSchema::new("A", "").setting("x", SchemaEntry::new("", 1_i64));
    let s2 = NamespaceSchema::new("B", "").setting("x", SchemaEntry::new("", 2_i64));
    assert!(m.register("editor", "plugin/a", s1).is_ok());
    assert!(m.register("editor", "plugin/b", s2).is_ok());
}

#[test]
fn register_rejects_null_byte_in_namespace() {
    let m = ConfigManager::new();
    let result = m.register("bad\0ns", "o", basic_schema());
    assert!(matches!(result, Err(ConfigError::InvalidIdentifier(_))));
}

#[test]
fn register_rejects_null_byte_in_owner_segment() {
    let m = ConfigManager::new();
    let result = m.register("ns", "plug\0in/audio", basic_schema());
    assert!(matches!(result, Err(ConfigError::InvalidIdentifier(_))));
}

#[test]
fn register_empty_schema_succeeds() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("Empty", "No settings");
    assert!(m.register("ns", "empty", s).is_ok());
}

#[test]
fn register_multiple_times_with_different_owners_and_keys() {
    let m = ConfigManager::new();
    for i in 0..10 {
        let s = NamespaceSchema::new("T", "")
            .setting("k", SchemaEntry::new("", i as i64));
        m.register("ns", &format!("owner/{i}"), s).unwrap();
    }
    assert_eq!(m.list_owners("ns").len(), 10);
}

// ── owner_handle ─────────────────────────────────────────────────────────────

#[test]
fn owner_handle_returns_some_for_registered() {
    let m = ConfigManager::new();
    m.register("ns", "plugin/audio", basic_schema()).unwrap();
    assert!(m.owner_handle("ns", "plugin/audio").is_some());
}

#[test]
fn owner_handle_returns_none_for_unknown_namespace() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    assert!(m.owner_handle("other", "o").is_none());
}

#[test]
fn owner_handle_returns_none_for_unknown_owner() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    assert!(m.owner_handle("ns", "does/not/exist").is_none());
}

#[test]
fn owner_handle_handle_can_read_values() {
    let m = ConfigManager::new();
    let h1 = m.register("ns", "o", basic_schema()).unwrap();
    h1.set("count", 42_i64).unwrap();
    let h2 = m.owner_handle("ns", "o").unwrap();
    assert_eq!(h2.get_int("count").unwrap(), 42);
}

#[test]
fn owner_handle_path_with_leading_slash() {
    let m = ConfigManager::new();
    m.register("ns", "a/b", basic_schema()).unwrap();
    // Leading slash is stripped, so "/a/b" and "a/b" are the same owner
    assert!(m.owner_handle("ns", "/a/b").is_some());
}

// ── ConfigManager::get ───────────────────────────────────────────────────────

#[test]
fn manager_get_returns_default_value() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    assert_eq!(m.get("ns", "o", "count").unwrap(), ConfigValue::Int(0));
}

#[test]
fn manager_get_returns_overridden_value() {
    let m = ConfigManager::new();
    let h = m.register("ns", "o", basic_schema()).unwrap();
    h.set("count", 77_i64).unwrap();
    assert_eq!(m.get("ns", "o", "count").unwrap(), ConfigValue::Int(77));
}

#[test]
fn manager_get_owner_not_found() {
    let m = ConfigManager::new();
    let err = m.get("ns", "missing", "k").unwrap_err();
    assert!(matches!(err, ConfigError::OwnerNotFound { .. }));
}

#[test]
fn manager_get_unknown_key() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    let err = m.get("ns", "o", "no_such_key").unwrap_err();
    assert!(matches!(err, ConfigError::UnknownKey { .. }));
}

#[test]
fn manager_get_wrong_namespace() {
    let m = ConfigManager::new();
    m.register("editor", "o", basic_schema()).unwrap();
    let err = m.get("project", "o", "count").unwrap_err();
    assert!(matches!(err, ConfigError::OwnerNotFound { .. }));
}

// ── list_namespaces ──────────────────────────────────────────────────────────

#[test]
fn list_namespaces_empty_on_new_manager() {
    let m = ConfigManager::new();
    assert!(m.list_namespaces().is_empty());
}

#[test]
fn list_namespaces_contains_registered_namespace() {
    let m = ConfigManager::new();
    m.register("editor", "o", basic_schema()).unwrap();
    let ns = m.list_namespaces();
    assert!(ns.contains(&"editor".to_owned()));
}

#[test]
fn list_namespaces_deduplicates_multiple_owners() {
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("editor", "a", s()).unwrap();
    m.register("editor", "b", s()).unwrap();
    m.register("editor", "c", s()).unwrap();
    let ns = m.list_namespaces();
    assert_eq!(ns.iter().filter(|n| *n == "editor").count(), 1);
}

#[test]
fn list_namespaces_returns_all_distinct() {
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("editor", "o", s()).unwrap();
    m.register("project", "o", s()).unwrap();
    m.register("runtime", "o", s()).unwrap();
    let mut ns = m.list_namespaces();
    ns.sort();
    assert_eq!(ns, vec!["editor", "project", "runtime"]);
}

// ── list_owners ───────────────────────────────────────────────────────────────

#[test]
fn list_owners_contains_registered_owners() {
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("ns", "a/b", s()).unwrap();
    m.register("ns", "c/d", s()).unwrap();
    let owners = m.list_owners("ns");
    assert_eq!(owners.len(), 2);
}

#[test]
fn list_owners_empty_for_unknown_namespace() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    assert!(m.list_owners("other").is_empty());
}

#[test]
fn list_owners_does_not_cross_namespaces() {
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("editor", "p/a", s()).unwrap();
    m.register("project", "p/b", s()).unwrap();
    assert_eq!(m.list_owners("editor").len(), 1);
    assert_eq!(m.list_owners("project").len(), 1);
}

#[test]
fn list_owners_returns_correct_segments() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("ns", "x/y/z", s).unwrap();
    let owners = m.list_owners("ns");
    let segs: Vec<&[String]> = owners.iter().map(|v| v.as_slice()).collect();
    assert!(segs.iter().any(|s| *s == ["x", "y", "z"]));
}

// ── list_all_owners ───────────────────────────────────────────────────────────

#[test]
fn list_all_owners_empty_on_new_manager() {
    let m = ConfigManager::new();
    assert!(m.list_all_owners().is_empty());
}

#[test]
fn list_all_owners_returns_all_pairs() {
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("editor", "a", s()).unwrap();
    m.register("project", "b", s()).unwrap();
    let all = m.list_all_owners();
    assert_eq!(all.len(), 2);
}

// ── list_settings ─────────────────────────────────────────────────────────────

#[test]
fn list_settings_returns_all_keys() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let keys: Vec<&str> = settings.iter().map(|s| s.key.as_str()).collect();
    assert!(keys.contains(&"count"));
    assert!(keys.contains(&"label"));
    assert!(keys.contains(&"active"));
}

#[test]
fn list_settings_returns_none_for_unregistered() {
    let m = ConfigManager::new();
    assert!(m.list_settings("ns", "missing").is_none());
}

#[test]
fn list_settings_shows_current_value() {
    let m = ConfigManager::new();
    let h = m.register("ns", "o", basic_schema()).unwrap();
    h.set("count", 55_i64).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let entry = settings.iter().find(|s| s.key == "count").unwrap();
    assert_eq!(entry.current_value, ConfigValue::Int(55));
}

#[test]
fn list_settings_shows_read_only_flag() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let version = settings.iter().find(|s| s.key == "version").unwrap();
    assert!(version.read_only);
    let count = settings.iter().find(|s| s.key == "count").unwrap();
    assert!(!count.read_only);
}

// ── search ────────────────────────────────────────────────────────────────────

#[test]
fn search_finds_by_exact_key_name() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    let results = m.search("count");
    assert!(results.iter().any(|r| r.key == "count"));
}

#[test]
fn search_finds_by_key_name_substring() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("shadow_quality", SchemaEntry::new("desc", "high"));
    m.register("ns", "o", s).unwrap();
    let results = m.search("quality");
    assert!(!results.is_empty());
}

#[test]
fn search_finds_by_description() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("Gravitational acceleration constant", 9.81_f64));
    m.register("ns", "o", s).unwrap();
    let results = m.search("gravitational");
    assert!(results.iter().any(|r| r.key == "k"));
}

#[test]
fn search_finds_by_tag() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("", 0_i64).tag("performance"));
    m.register("ns", "o", s).unwrap();
    let results = m.search("performance");
    assert!(results.iter().any(|r| r.key == "k"));
}

#[test]
fn search_is_case_insensitive_on_key() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("ShadowQuality", SchemaEntry::new("", "high"));
    m.register("ns", "o", s).unwrap();
    let results = m.search("shadowquality");
    assert!(!results.is_empty());
}

#[test]
fn search_is_case_insensitive_on_description() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("Max DRAW Distance", 100.0_f64));
    m.register("ns", "o", s).unwrap();
    let results = m.search("max draw distance");
    assert!(!results.is_empty());
}

#[test]
fn search_is_case_insensitive_on_tag() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("", 0_i64).tag("PERFORMANCE"));
    m.register("ns", "o", s).unwrap();
    let results = m.search("performance");
    assert!(!results.is_empty());
}

#[test]
fn search_returns_empty_when_no_match() {
    let m = ConfigManager::new();
    m.register("ns", "o", basic_schema()).unwrap();
    let results = m.search("xyzzy_no_match");
    assert!(results.is_empty());
}

#[test]
fn search_result_has_correct_namespace() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("unique_key_abc", SchemaEntry::new("", 0_i64));
    m.register("editor", "o", s).unwrap();
    let results = m.search("unique_key_abc");
    assert_eq!(results[0].namespace, "editor");
}

#[test]
fn search_result_has_correct_owner_segments() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("unique_key_def", SchemaEntry::new("", 0_i64));
    m.register("ns", "sub/system/core", s).unwrap();
    let results = m.search("unique_key_def");
    assert_eq!(results[0].owner, vec!["sub", "system", "core"]);
}

#[test]
fn search_result_has_correct_owner_display_name() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("My Renderer", "").setting("unique_xyz", SchemaEntry::new("", 0_i64));
    m.register("ns", "o", s).unwrap();
    let results = m.search("unique_xyz");
    assert_eq!(results[0].owner_display_name, "My Renderer");
}

#[test]
fn search_result_shows_current_value_not_default() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("unique_val_ghi", SchemaEntry::new("", 0_i64));
    let h = m.register("ns", "o", s).unwrap();
    h.set("unique_val_ghi", 42_i64).unwrap();
    let results = m.search("unique_val_ghi");
    assert_eq!(results[0].current_value, ConfigValue::Int(42));
}

#[test]
fn search_result_owner_path_helper() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("unique_path_jkl", SchemaEntry::new("", 0_i64));
    m.register("ns", "a/b/c", s).unwrap();
    let results = m.search("unique_path_jkl");
    assert_eq!(results[0].owner_path(), "a/b/c");
}
