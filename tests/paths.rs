//! Tests for N-level owner path parsing and isolation.

use pulsar_config::{ConfigManager, ConfigValue, NamespaceSchema, SchemaEntry};

fn s() -> NamespaceSchema {
    NamespaceSchema::new("T", "").setting("v", SchemaEntry::new("", 0_i64))
}

// ── Path parsing ─────────────────────────────────────────────────────────────

#[test]
fn single_segment_owner() {
    let m = ConfigManager::new();
    let h = m.register("ns", "audio", s()).unwrap();
    assert_eq!(h.owner(), &["audio"]);
    assert_eq!(h.owner_path(), "audio");
}

#[test]
fn two_segment_owner() {
    let m = ConfigManager::new();
    let h = m.register("ns", "plugin/audio", s()).unwrap();
    assert_eq!(h.owner(), &["plugin", "audio"]);
    assert_eq!(h.owner_path(), "plugin/audio");
}

#[test]
fn three_segment_owner() {
    let m = ConfigManager::new();
    let h = m.register("ns", "subsystem/physics/main", s()).unwrap();
    assert_eq!(h.owner(), &["subsystem", "physics", "main"]);
}

#[test]
fn ten_segment_owner() {
    let m = ConfigManager::new();
    let path = "a/b/c/d/e/f/g/h/i/j";
    let h = m.register("ns", path, s()).unwrap();
    assert_eq!(h.owner().len(), 10);
    assert_eq!(h.owner_path(), path);
}

#[test]
fn twenty_segment_owner() {
    let m = ConfigManager::new();
    let segs: Vec<_> = (0..20).map(|i| format!("seg{i}")).collect();
    let path = segs.join("/");
    let h = m.register("ns", &path, s()).unwrap();
    assert_eq!(h.owner().len(), 20);
    assert_eq!(h.owner_path(), path);
}

#[test]
fn leading_slash_is_stripped() {
    let m = ConfigManager::new();
    let h = m.register("ns", "/a/b", s()).unwrap();
    assert_eq!(h.owner(), &["a", "b"]);
    assert_eq!(h.owner_path(), "a/b");
}

#[test]
fn trailing_slash_is_stripped() {
    let m = ConfigManager::new();
    let h = m.register("ns", "a/b/", s()).unwrap();
    assert_eq!(h.owner(), &["a", "b"]);
}

#[test]
fn consecutive_slashes_are_collapsed() {
    let m = ConfigManager::new();
    let h = m.register("ns", "a//b///c", s()).unwrap();
    assert_eq!(h.owner(), &["a", "b", "c"]);
}

#[test]
fn leading_and_trailing_slashes_stripped() {
    let m = ConfigManager::new();
    let h = m.register("ns", "/a/b/", s()).unwrap();
    assert_eq!(h.owner(), &["a", "b"]);
}

#[test]
fn owner_path_roundtrip() {
    let m = ConfigManager::new();
    let original = "deep/nested/path/segments/here";
    let h = m.register("ns", original, s()).unwrap();
    assert_eq!(h.owner_path(), original);
}

// ── Isolation across namespaces ───────────────────────────────────────────────

#[test]
fn same_path_in_two_namespaces_is_independent() {
    let m = ConfigManager::new();
    let h1 = m.register("editor", "plugin/audio", s()).unwrap();
    let h2 = m.register("project", "plugin/audio", s()).unwrap();
    h1.set("v", 10_i64).unwrap();
    h2.set("v", 20_i64).unwrap();
    assert_eq!(h1.get_int("v").unwrap(), 10);
    assert_eq!(h2.get_int("v").unwrap(), 20);
}

#[test]
fn different_paths_same_namespace_are_independent() {
    let m = ConfigManager::new();
    let h1 = m.register("ns", "owner/x", s()).unwrap();
    let h2 = m.register("ns", "owner/y", s()).unwrap();
    h1.set("v", 1_i64).unwrap();
    h2.set("v", 2_i64).unwrap();
    assert_eq!(h1.get_int("v").unwrap(), 1);
    assert_eq!(h2.get_int("v").unwrap(), 2);
}

#[test]
fn two_owners_with_common_prefix_are_independent() {
    let m = ConfigManager::new();
    let h1 = m.register("ns", "sub/core", s()).unwrap();
    let h2 = m.register("ns", "sub/core/extra", s()).unwrap();
    h1.set("v", 111_i64).unwrap();
    h2.set("v", 222_i64).unwrap();
    assert_eq!(h1.get_int("v").unwrap(), 111);
    assert_eq!(h2.get_int("v").unwrap(), 222);
}

// ── Path segment content ──────────────────────────────────────────────────────

#[test]
fn path_segment_with_numbers() {
    let m = ConfigManager::new();
    let h = m.register("ns", "player1/team42", s()).unwrap();
    assert_eq!(h.owner(), &["player1", "team42"]);
}

#[test]
fn path_segment_with_underscores() {
    let m = ConfigManager::new();
    let h = m.register("ns", "my_plugin/my_feature", s()).unwrap();
    assert_eq!(h.owner(), &["my_plugin", "my_feature"]);
}

#[test]
fn path_segment_with_hyphens() {
    let m = ConfigManager::new();
    let h = m.register("ns", "my-plugin/my-feature", s()).unwrap();
    assert_eq!(h.owner(), &["my-plugin", "my-feature"]);
}

// ── Lookup with normalized paths ─────────────────────────────────────────────

#[test]
fn owner_handle_lookup_with_equivalent_paths() {
    let m = ConfigManager::new();
    m.register("ns", "a/b/c", s()).unwrap();
    // "a/b/c" and "/a/b/c/" resolve to the same owner
    assert!(m.owner_handle("ns", "/a/b/c/").is_some());
}

#[test]
fn manager_get_with_normalized_path() {
    let m = ConfigManager::new();
    let h = m.register("ns", "x/y/z", s()).unwrap();
    h.set("v", 77_i64).unwrap();
    // "/x/y/z/" should resolve to the same owner
    let val = m.get("ns", "/x/y/z/", "v").unwrap();
    assert_eq!(val, ConfigValue::Int(77));
}

#[test]
fn list_owners_returns_segments_not_joined_path() {
    let m = ConfigManager::new();
    m.register("ns", "a/b/c", s()).unwrap();
    let owners = m.list_owners("ns");
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0], vec!["a", "b", "c"]);
}

#[test]
fn deep_path_write_and_read() {
    let m = ConfigManager::new();
    let schema = NamespaceSchema::new("T", "").setting("val", SchemaEntry::new("", 0_i64));
    let h = m.register("ns", "level1/level2/level3/level4/level5", schema).unwrap();
    h.set("val", 12345_i64).unwrap();
    assert_eq!(h.get_int("val").unwrap(), 12345);
    assert_eq!(m.get("ns", "level1/level2/level3/level4/level5", "val").unwrap(), ConfigValue::Int(12345));
}

#[test]
fn duplicate_registration_error_carries_correct_segments() {
    let m = ConfigManager::new();
    m.register("ns", "a/b/c", s()).unwrap();
    let err = m.register("ns", "a/b/c", s()).err().expect("expected error");
    if let pulsar_config::ConfigError::OwnerAlreadyRegistered { namespace, owner } = err {
        assert_eq!(namespace, "ns");
        assert_eq!(owner, vec!["a", "b", "c"]);
    } else {
        panic!("expected OwnerAlreadyRegistered");
    }
}

#[test]
fn owner_not_found_error_carries_correct_segments() {
    let m = ConfigManager::new();
    let err = m.get("ns", "x/y/z", "k").unwrap_err();
    if let pulsar_config::ConfigError::OwnerNotFound { namespace, owner } = err {
        assert_eq!(namespace, "ns");
        assert_eq!(owner, vec!["x", "y", "z"]);
    } else {
        panic!("expected OwnerNotFound");
    }
}
