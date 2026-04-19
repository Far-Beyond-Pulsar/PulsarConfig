//! Persistence tests — save/load round-trips using temporary directories.

use std::sync::atomic::{AtomicU64, Ordering};

use pulsar_config::{
    Color, ConfigManager, ConfigStore, ConfigValue, NamespaceSchema, SchemaEntry, Validator,
};

fn unique_tmp(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "pulsar_cfg_{}_{}_{:06}",
        label,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
    ))
}

fn make_store(label: &str) -> (ConfigStore, pulsar_config::OwnerHandle) {
    let m = ConfigManager::new();
    let schema = NamespaceSchema::new("Test", "")
        .setting("count", SchemaEntry::new("Count", 0_i64))
        .setting("label", SchemaEntry::new("Label", "default"))
        .setting("active", SchemaEntry::new("Active", true))
        .setting("ratio", SchemaEntry::new("Ratio", 0.5_f64))
        .setting("tint", SchemaEntry::new("Tint", Color::rgba(0, 0, 0, 255)))
        .setting("version", SchemaEntry::new("Version", "1.0").read_only());
    let h = m.register("editor", "plugin/test", schema).unwrap();
    let store = ConfigStore::with_dir(m, unique_tmp(label)).unwrap();
    (store, h)
}

// ── ConfigStore construction ──────────────────────────────────────────────────

#[test]
fn with_dir_creates_directory() {
    let dir = unique_tmp("ctor");
    assert!(!dir.exists());
    let m = ConfigManager::new();
    let _store = ConfigStore::with_dir(m, &dir).unwrap();
    assert!(dir.exists());
}

#[test]
fn with_dir_creates_nested_directory() {
    let dir = unique_tmp("ctor_nested").join("a").join("b").join("c");
    let m = ConfigManager::new();
    let _store = ConfigStore::with_dir(m, &dir).unwrap();
    assert!(dir.exists());
}

#[test]
fn config_dir_accessor() {
    let dir = unique_tmp("accessor");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    assert_eq!(store.config_dir(), dir.as_path());
}

#[test]
fn manager_accessor_allows_registration() {
    let dir = unique_tmp("mgr_access");
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let store = ConfigStore::with_dir(m, dir).unwrap();
    assert!(store.manager().register("ns", "o", s).is_ok());
}

// ── toml_path ────────────────────────────────────────────────────────────────

#[test]
fn toml_path_single_segment_owner() {
    let dir = unique_tmp("path1");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let segs = vec!["audio".to_owned()];
    let path = store.toml_path("editor", &segs);
    assert_eq!(path, dir.join("editor").join("audio.toml"));
}

#[test]
fn toml_path_two_segments() {
    let dir = unique_tmp("path2");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let segs: Vec<String> = ["plugin", "audio"].iter().map(|s| s.to_string()).collect();
    let path = store.toml_path("editor", &segs);
    assert_eq!(path, dir.join("editor").join("plugin").join("audio.toml"));
}

#[test]
fn toml_path_three_segments() {
    let dir = unique_tmp("path3");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let segs: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
    let path = store.toml_path("ns", &segs);
    assert_eq!(path, dir.join("ns").join("a").join("b").join("c.toml"));
}

#[test]
fn toml_path_empty_owner_uses_root() {
    let dir = unique_tmp("path_root");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let path = store.toml_path("editor", &[]);
    assert_eq!(path, dir.join("editor").join("_root.toml"));
}

#[test]
fn toml_path_namespace_is_first_dir() {
    let dir = unique_tmp("path_ns");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let segs = vec!["owner".to_owned()];
    let path = store.toml_path("mynamespace", &segs);
    assert!(path.starts_with(dir.join("mynamespace")));
}

// ── save ─────────────────────────────────────────────────────────────────────

#[test]
fn save_creates_file() {
    let (store, _) = make_store("save_creates");
    store.save("editor", "plugin/test").unwrap();
    let h = store.manager().owner_handle("editor", "plugin/test").unwrap();
    let path = store.toml_path("editor", h.owner());
    assert!(path.exists());
}

#[test]
fn save_creates_parent_directories() {
    let dir = unique_tmp("save_mkdir");
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("", 0_i64));
    m.register("ns", "a/b/c/d", s).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    store.save("ns", "a/b/c/d").unwrap();
    assert!(dir.join("ns").join("a").join("b").join("c").join("d.toml").exists());
}

#[test]
fn save_file_contains_header_comment() {
    let (store, _) = make_store("save_header");
    store.save("editor", "plugin/test").unwrap();
    let h = store.manager().owner_handle("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    assert!(content.contains("PulsarConfig"));
}

#[test]
fn save_roundtrip_int() {
    let (store, h) = make_store("rt_int");
    h.set("count", 42_i64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(
        store.toml_path("editor", h.owner())
    ).unwrap();
    assert!(content.contains("count = 42"));
}

#[test]
fn save_roundtrip_bool() {
    let (store, h) = make_store("rt_bool");
    h.set("active", false).unwrap();
    store.save("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    assert!(content.contains("active = false"));
}

#[test]
fn save_roundtrip_float() {
    let (store, h) = make_store("rt_float");
    h.set("ratio", 0.25_f64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    assert!(content.contains("ratio"));
    assert!(content.contains("0.25"));
}

#[test]
fn save_roundtrip_string() {
    let (store, h) = make_store("rt_str");
    h.set("label", "hello world").unwrap();
    store.save("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    assert!(content.contains("hello world"));
}

#[test]
fn save_roundtrip_color() {
    let (store, h) = make_store("rt_color");
    h.set("tint", Color::rgba(10, 20, 30, 40)).unwrap();
    store.save("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    // Colors are stored as TOML tables with r/g/b/a fields
    assert!(content.contains("[tint]") || content.contains("tint"));
}

#[test]
fn save_does_not_write_read_only_keys() {
    let (store, _) = make_store("save_ro");
    store.save("editor", "plugin/test").unwrap();
    let h = store.manager().owner_handle("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    assert!(!content.contains("version"));
}

#[test]
fn save_unregistered_owner_returns_error() {
    let dir = unique_tmp("save_missing");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, dir).unwrap();
    let err = store.save("ns", "does/not/exist");
    assert!(err.is_err());
}

#[test]
fn save_second_save_overwrites_first() {
    let (store, h) = make_store("save_overwrite");
    h.set("count", 1_i64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    h.set("count", 2_i64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    let content = std::fs::read_to_string(store.toml_path("editor", h.owner())).unwrap();
    assert!(content.contains("count = 2"));
    assert!(!content.contains("count = 1"));
}

// ── load ─────────────────────────────────────────────────────────────────────

#[test]
fn load_returns_false_when_no_file() {
    let (store, h) = make_store("load_no_file");
    assert!(!store.load(&h).unwrap());
}

#[test]
fn load_returns_true_when_file_exists() {
    let (store, h) = make_store("load_exists");
    store.save("editor", "plugin/test").unwrap();
    assert!(store.load(&h).unwrap());
}

#[test]
fn load_applies_int_on_top_of_default() {
    let (store, h) = make_store("load_int");
    h.set("count", 99_i64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    // Reset to default, then reload
    h.reset_to_default("count").unwrap();
    assert_eq!(h.get_int("count").unwrap(), 0);
    store.load(&h).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 99);
}

#[test]
fn load_applies_bool_on_top_of_default() {
    let (store, h) = make_store("load_bool");
    h.set("active", false).unwrap();
    store.save("editor", "plugin/test").unwrap();
    h.reset_to_default("active").unwrap();
    assert!(h.get_bool("active").unwrap()); // default is true
    store.load(&h).unwrap();
    assert!(!h.get_bool("active").unwrap()); // loaded false
}

#[test]
fn load_applies_string_on_top_of_default() {
    let (store, h) = make_store("load_str");
    h.set("label", "loaded").unwrap();
    store.save("editor", "plugin/test").unwrap();
    h.reset_to_default("label").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_string("label").unwrap(), "loaded");
}

#[test]
fn load_ignores_unknown_keys_in_file() {
    let (store, h) = make_store("load_unknown");
    let path = store.toml_path("editor", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "count = 5\nunknown_key = true\n").unwrap();
    assert!(store.load(&h).is_ok());
    assert_eq!(h.get_int("count").unwrap(), 5);
}

#[test]
fn load_keeps_default_for_type_mismatch_in_file() {
    let (store, h) = make_store("load_type_mismatch");
    let path = store.toml_path("editor", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // count expects Int, but file has a String — should keep default 0
    std::fs::write(&path, "count = \"not_a_number\"\n").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 0);
}

#[test]
fn load_keeps_default_for_validation_failure_in_file() {
    let dir = unique_tmp("load_val_fail");
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("v", SchemaEntry::new("", 50_i64).validator(Validator::int_range(0, 100)));
    let h = m.register("ns", "o", s).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let path = store.toml_path("ns", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "v = 999\n").unwrap(); // out of range
    store.load(&h).unwrap();
    assert_eq!(h.get_int("v").unwrap(), 50); // default retained
}

#[test]
fn load_integer_to_float_widening() {
    let dir = unique_tmp("load_int_float");
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "")
        .setting("ratio", SchemaEntry::new("", 0.5_f64));
    let h = m.register("ns", "o", s).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    let path = store.toml_path("ns", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // TOML integer should widen to float
    std::fs::write(&path, "ratio = 1\n").unwrap();
    store.load(&h).unwrap();
    assert!((h.get_float("ratio").unwrap() - 1.0).abs() < 1e-9);
}

#[test]
fn load_partial_file_keeps_defaults_for_missing_keys() {
    let (store, h) = make_store("load_partial");
    let path = store.toml_path("editor", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Only write count — other keys should remain at their defaults
    std::fs::write(&path, "count = 7\n").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 7);
    assert_eq!(h.get_string("label").unwrap(), "default"); // unchanged
    assert!(h.get_bool("active").unwrap()); // unchanged
}

#[test]
fn load_skips_read_only_keys_in_file() {
    let (store, h) = make_store("load_skip_ro");
    let path = store.toml_path("editor", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Try to set read-only key through the file — should be skipped
    std::fs::write(&path, "version = \"99.99\"\n").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_string("version").unwrap(), "1.0"); // unchanged
}

#[test]
fn load_malformed_toml_returns_error() {
    let (store, h) = make_store("load_malformed");
    let path = store.toml_path("editor", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "this is not [ valid toml ===\n").unwrap();
    let result = store.load(&h);
    assert!(result.is_err());
}

#[test]
fn load_empty_file_returns_true_with_all_defaults() {
    let (store, h) = make_store("load_empty");
    let path = store.toml_path("editor", h.owner());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    assert!(store.load(&h).unwrap());
    // All defaults should be intact
    assert_eq!(h.get_int("count").unwrap(), 0);
    assert_eq!(h.get_string("label").unwrap(), "default");
}

#[test]
fn save_then_load_full_roundtrip() {
    let (store, h) = make_store("full_rt");
    h.set("count", 123_i64).unwrap();
    h.set("label", "saved").unwrap();
    h.set("active", false).unwrap();
    h.set("ratio", 0.75_f64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    // Reset everything
    h.reset_to_default("count").unwrap();
    h.reset_to_default("label").unwrap();
    h.reset_to_default("active").unwrap();
    h.reset_to_default("ratio").unwrap();
    // Now load and verify
    store.load(&h).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 123);
    assert_eq!(h.get_string("label").unwrap(), "saved");
    assert!(!h.get_bool("active").unwrap());
    assert!((h.get_float("ratio").unwrap() - 0.75).abs() < 1e-9);
}

#[test]
fn load_color_roundtrip() {
    let (store, h) = make_store("rt_color_load");
    let target = Color::rgba(10, 20, 30, 40);
    h.set("tint", target).unwrap();
    store.save("editor", "plugin/test").unwrap();
    h.reset_to_default("tint").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_color("tint").unwrap(), target);
}

// ── save_namespace ────────────────────────────────────────────────────────────

#[test]
fn save_namespace_saves_all_owners_in_that_namespace() {
    let dir = unique_tmp("save_ns");
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h1 = m.register("editor", "a", s()).unwrap();
    let h2 = m.register("editor", "b", s()).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    store.save_namespace("editor").unwrap();
    assert!(store.toml_path("editor", h1.owner()).exists());
    assert!(store.toml_path("editor", h2.owner()).exists());
}

#[test]
fn save_namespace_does_not_touch_other_namespaces() {
    let dir = unique_tmp("save_ns_iso");
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h_project = m.register("project", "owner", s()).unwrap();
    m.register("editor", "owner", s()).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    store.save_namespace("editor").unwrap();
    // project namespace file should NOT exist
    assert!(!store.toml_path("project", h_project.owner()).exists());
}

// ── save_all ──────────────────────────────────────────────────────────────────

#[test]
fn save_all_saves_every_owner() {
    let dir = unique_tmp("save_all");
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h1 = m.register("editor", "a", s()).unwrap();
    let h2 = m.register("project", "b", s()).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    store.save_all().unwrap();
    assert!(store.toml_path("editor", h1.owner()).exists());
    assert!(store.toml_path("project", h2.owner()).exists());
}

#[test]
fn save_all_on_empty_manager_is_noop() {
    let dir = unique_tmp("save_all_empty");
    let m = ConfigManager::new();
    let store = ConfigStore::with_dir(m, dir).unwrap();
    assert!(store.save_all().is_ok());
}

// ── load_all ──────────────────────────────────────────────────────────────────

#[test]
fn load_all_returns_empty_vec_when_all_files_exist() {
    let dir = unique_tmp("load_all_exist");
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h1 = m.register("editor", "a", s()).unwrap();
    let h2 = m.register("project", "b", s()).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    store.save("editor", "a").unwrap();
    store.save("project", "b").unwrap();
    let missing = store.load_all([&h1, &h2]).unwrap();
    assert!(missing.is_empty());
}

#[test]
fn load_all_returns_missing_pairs() {
    let dir = unique_tmp("load_all_missing");
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h1 = m.register("editor", "a", s()).unwrap();
    let h2 = m.register("project", "b", s()).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    // Only save h1 — h2 has no file
    store.save("editor", "a").unwrap();
    let missing = store.load_all([&h1, &h2]).unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].0, "project");
    assert_eq!(missing[0].1, vec!["b"]);
}

#[test]
fn load_all_applies_all_values() {
    let dir = unique_tmp("load_all_apply");
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h1 = m.register("editor", "a", s()).unwrap();
    let h2 = m.register("project", "b", s()).unwrap();
    h1.set("k", 10_i64).unwrap();
    h2.set("k", 20_i64).unwrap();
    let store = ConfigStore::with_dir(m, &dir).unwrap();
    store.save("editor", "a").unwrap();
    store.save("project", "b").unwrap();
    h1.reset_to_default("k").unwrap();
    h2.reset_to_default("k").unwrap();
    store.load_all([&h1, &h2]).unwrap();
    assert_eq!(h1.get_int("k").unwrap(), 10);
    assert_eq!(h2.get_int("k").unwrap(), 20);
}

#[test]
fn save_then_load_then_save_again_roundtrip() {
    let (store, h) = make_store("triple_rt");
    h.set("count", 77_i64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    h.reset_to_default("count").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 77);
    h.set("count", 99_i64).unwrap();
    store.save("editor", "plugin/test").unwrap();
    h.reset_to_default("count").unwrap();
    store.load(&h).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 99);
}
