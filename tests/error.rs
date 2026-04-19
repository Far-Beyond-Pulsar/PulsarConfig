//! Tests for `ConfigError` — display formatting, equality, cloning, and Debug.

use pulsar_config::ConfigError;

// ── Display ───────────────────────────────────────────────────────────────────

#[test]
fn owner_already_registered_display_contains_namespace() {
    let err = ConfigError::OwnerAlreadyRegistered {
        namespace: "editor".to_owned(),
        owner: vec!["plugin".to_owned(), "audio".to_owned()],
    };
    let s = err.to_string();
    assert!(s.contains("editor"), "display was: {s}");
}

#[test]
fn owner_already_registered_display_contains_owner() {
    let err = ConfigError::OwnerAlreadyRegistered {
        namespace: "editor".to_owned(),
        owner: vec!["plugin".to_owned(), "audio".to_owned()],
    };
    let s = err.to_string();
    assert!(s.contains("plugin"), "display was: {s}");
}

#[test]
fn owner_not_found_display_contains_namespace_and_owner() {
    let err = ConfigError::OwnerNotFound {
        namespace: "project".to_owned(),
        owner: vec!["missing".to_owned()],
    };
    let s = err.to_string();
    assert!(s.contains("project"), "display was: {s}");
    assert!(s.contains("missing"), "display was: {s}");
}

#[test]
fn unknown_key_display_contains_key_name() {
    let err = ConfigError::UnknownKey {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "my_key".to_owned(),
    };
    let s = err.to_string();
    assert!(s.contains("my_key"), "display was: {s}");
}

#[test]
fn validation_failed_display_contains_key_and_reason() {
    let err = ConfigError::ValidationFailed {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "count".to_owned(),
        reason: "must be positive".to_owned(),
    };
    let s = err.to_string();
    assert!(s.contains("count"), "display was: {s}");
    assert!(s.contains("must be positive"), "display was: {s}");
}

#[test]
fn read_only_display_contains_key() {
    let err = ConfigError::ReadOnly {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "version".to_owned(),
    };
    let s = err.to_string();
    assert!(s.contains("version"), "display was: {s}");
}

#[test]
fn type_mismatch_display_contains_expected_and_found() {
    let err = ConfigError::TypeMismatch {
        expected: "int",
        got: "bool",
    };
    let s = err.to_string();
    assert!(s.contains("int"), "display was: {s}");
    assert!(s.contains("bool"), "display was: {s}");
}

#[test]
fn invalid_identifier_display_contains_the_identifier() {
    let err = ConfigError::InvalidIdentifier("bad\0id".to_owned());
    let s = err.to_string();
    assert!(s.contains("bad"), "display was: {s}");
}

// ── Debug ─────────────────────────────────────────────────────────────────────

#[test]
fn debug_format_includes_variant_name_owner_already_registered() {
    let err = ConfigError::OwnerAlreadyRegistered {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
    };
    let s = format!("{err:?}");
    assert!(s.contains("OwnerAlreadyRegistered"), "debug was: {s}");
}

#[test]
fn debug_format_includes_variant_name_unknown_key() {
    let err = ConfigError::UnknownKey {
        namespace: "ns".to_owned(),
        owner: vec![],
        key: "k".to_owned(),
    };
    let s = format!("{err:?}");
    assert!(s.contains("UnknownKey"), "debug was: {s}");
}

#[test]
fn debug_format_includes_field_values() {
    let err = ConfigError::ValidationFailed {
        namespace: "editor".to_owned(),
        owner: vec!["plugin".to_owned()],
        key: "gravity".to_owned(),
        reason: "out of range".to_owned(),
    };
    let s = format!("{err:?}");
    assert!(s.contains("gravity"), "debug was: {s}");
    assert!(s.contains("out of range"), "debug was: {s}");
}

// ── Clone ─────────────────────────────────────────────────────────────────────

#[test]
fn config_error_clone_is_equal() {
    let err = ConfigError::ReadOnly {
        namespace: "ns".to_owned(),
        owner: vec!["a".to_owned(), "b".to_owned()],
        key: "version".to_owned(),
    };
    let cloned = err.clone();
    assert_eq!(err, cloned);
}

#[test]
fn config_error_clone_is_independent() {
    let err = ConfigError::InvalidIdentifier("original".to_owned());
    let _ = err.clone(); // must not panic
}

// ── PartialEq ────────────────────────────────────────────────────────────────

#[test]
fn same_variant_same_fields_are_equal() {
    let e1 = ConfigError::UnknownKey {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "k".to_owned(),
    };
    let e2 = e1.clone();
    assert_eq!(e1, e2);
}

#[test]
fn same_variant_different_fields_are_not_equal() {
    let e1 = ConfigError::UnknownKey {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "k1".to_owned(),
    };
    let e2 = ConfigError::UnknownKey {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "k2".to_owned(),
    };
    assert_ne!(e1, e2);
}

#[test]
fn different_variants_are_not_equal() {
    let e1 = ConfigError::ReadOnly {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "k".to_owned(),
    };
    let e2 = ConfigError::UnknownKey {
        namespace: "ns".to_owned(),
        owner: vec!["o".to_owned()],
        key: "k".to_owned(),
    };
    assert_ne!(e1, e2);
}

// ── std::error::Error ────────────────────────────────────────────────────────

#[test]
fn config_error_implements_std_error() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<ConfigError>();
}

#[test]
fn type_mismatch_source_is_none() {
    let err = ConfigError::TypeMismatch {
        expected: "int",
        got: "bool",
    };
    use std::error::Error;
    assert!(err.source().is_none());
}

// ── from_registration roundtrip (integration) ─────────────────────────────────

#[test]
fn registration_error_has_correct_owner_segments() {
    use pulsar_config::{ConfigManager, NamespaceSchema, SchemaEntry};
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("ns", "a/b/c", s()).unwrap();
    let err = m.register("ns", "a/b/c", s()).err().expect("expected error");
    if let ConfigError::OwnerAlreadyRegistered { owner, .. } = err {
        assert_eq!(owner, vec!["a", "b", "c"]);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn unknown_key_error_has_correct_fields() {
    use pulsar_config::{ConfigManager, NamespaceSchema, SchemaEntry};
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h = m.register("ns", "o", s).unwrap();
    let err = h.get("nonexistent").unwrap_err();
    if let ConfigError::UnknownKey { namespace, owner, key } = err {
        assert_eq!(namespace, "ns");
        assert_eq!(owner, vec!["o"]);
        assert_eq!(key, "nonexistent");
    } else {
        panic!("wrong variant");
    }
}
