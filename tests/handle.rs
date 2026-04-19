//! Tests for `OwnerHandle` — typed reads, writes, validation, reset, listing, accessors.

use pulsar_config::{
    Color, ConfigError, ConfigManager, ConfigValue, NamespaceSchema, SchemaEntry, Validator,
};

fn mgr_with_schema() -> (ConfigManager, pulsar_config::OwnerHandle) {
    let m = ConfigManager::new();
    let schema = NamespaceSchema::new("Physics", "Physics settings")
        .setting("enabled", SchemaEntry::new("Enable physics", true))
        .setting(
            "gravity",
            SchemaEntry::new("Gravity", 9.81_f64).validator(Validator::float_range(0.0, 100.0)),
        )
        .setting(
            "count",
            SchemaEntry::new("Object count", 0_i64).validator(Validator::int_range(0, 10_000)),
        )
        .setting(
            "preset",
            SchemaEntry::new("Quality preset", "medium")
                .validator(Validator::string_one_of(["low", "medium", "high"])),
        )
        .setting(
            "name",
            SchemaEntry::new("Name", "default").validator(Validator::string_max_length(20)),
        )
        .setting("tint", SchemaEntry::new("Color tint", Color::rgba(255, 255, 255, 255)))
        .setting("version", SchemaEntry::new("Engine version", "1.0.0").read_only());
    let h = m.register("editor", "subsystem/physics", schema).unwrap();
    (m, h)
}

// ── Typed reads ───────────────────────────────────────────────────────────────

#[test]
fn get_returns_default() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.get("enabled").unwrap(), ConfigValue::Bool(true));
}

#[test]
fn get_bool_correct_type() {
    let (_, h) = mgr_with_schema();
    assert!(h.get_bool("enabled").unwrap());
}

#[test]
fn get_bool_type_mismatch() {
    let (_, h) = mgr_with_schema();
    // "gravity" is a float, not a bool
    assert!(matches!(h.get_bool("gravity"), Err(ConfigError::TypeMismatch { .. })));
}

#[test]
fn get_int_correct_type() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.get_int("count").unwrap(), 0);
}

#[test]
fn get_int_type_mismatch() {
    let (_, h) = mgr_with_schema();
    assert!(matches!(h.get_int("enabled"), Err(ConfigError::TypeMismatch { .. })));
}

#[test]
fn get_float_correct_type() {
    let (_, h) = mgr_with_schema();
    assert!((h.get_float("gravity").unwrap() - 9.81).abs() < 1e-9);
}

#[test]
fn get_float_widens_int() {
    let m = ConfigManager::new();
    let s = NamespaceSchema::new("T", "").setting("v", SchemaEntry::new("", 5_i64));
    let h = m.register("ns", "o", s).unwrap();
    // Default is Int(5); get_float should widen it
    assert_eq!(h.get_float("v").unwrap(), 5.0);
}

#[test]
fn get_float_type_mismatch_from_string() {
    let (_, h) = mgr_with_schema();
    assert!(matches!(h.get_float("preset"), Err(ConfigError::TypeMismatch { .. })));
}

#[test]
fn get_string_correct_type() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.get_string("preset").unwrap(), "medium");
}

#[test]
fn get_string_type_mismatch() {
    let (_, h) = mgr_with_schema();
    assert!(matches!(h.get_string("enabled"), Err(ConfigError::TypeMismatch { .. })));
}

#[test]
fn get_color_correct_type() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.get_color("tint").unwrap(), Color::rgba(255, 255, 255, 255));
}

#[test]
fn get_color_type_mismatch() {
    let (_, h) = mgr_with_schema();
    assert!(matches!(h.get_color("enabled"), Err(ConfigError::TypeMismatch { .. })));
}

#[test]
fn get_unknown_key_returns_error() {
    let (_, h) = mgr_with_schema();
    assert!(matches!(h.get("nonexistent"), Err(ConfigError::UnknownKey { .. })));
}

// ── Writes ────────────────────────────────────────────────────────────────────

#[test]
fn set_writes_bool_value() {
    let (_, h) = mgr_with_schema();
    h.set("enabled", false).unwrap();
    assert!(!h.get_bool("enabled").unwrap());
}

#[test]
fn set_writes_int_value() {
    let (_, h) = mgr_with_schema();
    h.set("count", 42_i64).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 42);
}

#[test]
fn set_writes_float_value() {
    let (_, h) = mgr_with_schema();
    h.set("gravity", 1.62_f64).unwrap();
    assert!((h.get_float("gravity").unwrap() - 1.62).abs() < 1e-9);
}

#[test]
fn set_writes_string_value() {
    let (_, h) = mgr_with_schema();
    h.set("preset", "high").unwrap();
    assert_eq!(h.get_string("preset").unwrap(), "high");
}

#[test]
fn set_writes_color_value() {
    let (_, h) = mgr_with_schema();
    let new_color = Color::rgba(10, 20, 30, 128);
    h.set("tint", new_color).unwrap();
    assert_eq!(h.get_color("tint").unwrap(), new_color);
}

#[test]
fn set_rejects_read_only() {
    let (_, h) = mgr_with_schema();
    let err = h.set("version", "2.0.0").unwrap_err();
    assert!(matches!(err, ConfigError::ReadOnly { .. }));
}

#[test]
fn set_preserves_old_value_on_read_only_rejection() {
    let (_, h) = mgr_with_schema();
    h.set("version", "2.0.0").unwrap_err();
    assert_eq!(h.get_string("version").unwrap(), "1.0.0");
}

#[test]
fn set_rejects_int_out_of_range() {
    let (_, h) = mgr_with_schema();
    assert!(h.set("count", 99_999_i64).is_err());
}

#[test]
fn set_preserves_value_on_int_range_rejection() {
    let (_, h) = mgr_with_schema();
    h.set("count", 100_i64).unwrap();
    h.set("count", 99_999_i64).unwrap_err();
    assert_eq!(h.get_int("count").unwrap(), 100);
}

#[test]
fn set_rejects_float_out_of_range() {
    let (_, h) = mgr_with_schema();
    assert!(h.set("gravity", 999.0_f64).is_err());
}

#[test]
fn set_preserves_value_on_float_range_rejection() {
    let (_, h) = mgr_with_schema();
    h.set("gravity", 5.0_f64).unwrap();
    h.set("gravity", 999.0_f64).unwrap_err();
    assert!((h.get_float("gravity").unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn set_rejects_invalid_one_of_option() {
    let (_, h) = mgr_with_schema();
    assert!(h.set("preset", "ultra").is_err());
}

#[test]
fn set_preserves_value_on_one_of_rejection() {
    let (_, h) = mgr_with_schema();
    h.set("preset", "high").unwrap();
    h.set("preset", "ultra").unwrap_err();
    assert_eq!(h.get_string("preset").unwrap(), "high");
}

#[test]
fn set_rejects_string_too_long() {
    let (_, h) = mgr_with_schema();
    let too_long = "a".repeat(21);
    assert!(h.set("name", too_long.as_str()).is_err());
}

#[test]
fn set_unknown_key_returns_error() {
    let (_, h) = mgr_with_schema();
    assert!(matches!(h.set("no_such_key", 0_i64), Err(ConfigError::UnknownKey { .. })));
}

#[test]
fn set_is_visible_via_manager_get() {
    let (m, h) = mgr_with_schema();
    h.set("count", 500_i64).unwrap();
    assert_eq!(m.get("editor", "subsystem/physics", "count").unwrap(), ConfigValue::Int(500));
}

#[test]
fn set_multiple_times_accumulates() {
    let (_, h) = mgr_with_schema();
    for i in 0..=10 {
        h.set("count", i as i64).unwrap();
    }
    assert_eq!(h.get_int("count").unwrap(), 10);
}

// ── reset_to_default ──────────────────────────────────────────────────────────

#[test]
fn reset_restores_default() {
    let (_, h) = mgr_with_schema();
    h.set("gravity", 1.0_f64).unwrap();
    h.reset_to_default("gravity").unwrap();
    assert!((h.get_float("gravity").unwrap() - 9.81).abs() < 1e-9);
}

#[test]
fn reset_works_on_read_only_key() {
    // reset_to_default bypasses the read-only check
    let (_, h) = mgr_with_schema();
    assert!(h.reset_to_default("version").is_ok());
    assert_eq!(h.get_string("version").unwrap(), "1.0.0");
}

#[test]
fn reset_fires_change_event() {
    use std::sync::{Arc, Mutex};
    let (_, h) = mgr_with_schema();
    h.set("gravity", 5.0_f64).unwrap();
    let fired = Arc::new(Mutex::new(false));
    let f = Arc::clone(&fired);
    let _g = h.on_change("gravity", move |_| *f.lock().unwrap() = true).unwrap();
    h.reset_to_default("gravity").unwrap();
    assert!(*fired.lock().unwrap());
}

#[test]
fn reset_unknown_key_returns_error() {
    let (_, h) = mgr_with_schema();
    assert!(h.reset_to_default("no_such_key").is_err());
}

// ── list_settings ─────────────────────────────────────────────────────────────

#[test]
fn handle_list_settings_all_keys_present() {
    let (_, h) = mgr_with_schema();
    let settings = h.list_settings();
    let keys: std::collections::HashSet<_> = settings.iter().map(|s| s.key.as_str()).collect();
    assert!(keys.contains("enabled"));
    assert!(keys.contains("gravity"));
    assert!(keys.contains("count"));
    assert!(keys.contains("preset"));
    assert!(keys.contains("tint"));
    assert!(keys.contains("version"));
}

#[test]
fn handle_list_settings_shows_current_value() {
    let (_, h) = mgr_with_schema();
    h.set("count", 99_i64).unwrap();
    let settings = h.list_settings();
    let e = settings.iter().find(|s| s.key == "count").unwrap();
    assert_eq!(e.current_value, ConfigValue::Int(99));
}

#[test]
fn handle_list_settings_shows_default_value() {
    let (_, h) = mgr_with_schema();
    let settings = h.list_settings();
    let e = settings.iter().find(|s| s.key == "gravity").unwrap();
    assert_eq!(e.default_value, ConfigValue::Float(9.81));
}

#[test]
fn handle_list_settings_shows_read_only_flag() {
    let (_, h) = mgr_with_schema();
    let settings = h.list_settings();
    let v = settings.iter().find(|s| s.key == "version").unwrap();
    assert!(v.read_only);
    let c = settings.iter().find(|s| s.key == "count").unwrap();
    assert!(!c.read_only);
}

// ── Accessors ────────────────────────────────────────────────────────────────

#[test]
fn handle_namespace_accessor() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.namespace(), "editor");
}

#[test]
fn handle_owner_accessor_returns_segments() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.owner(), &["subsystem", "physics"]);
}

#[test]
fn handle_owner_path_accessor_joins_with_slash() {
    let (_, h) = mgr_with_schema();
    assert_eq!(h.owner_path(), "subsystem/physics");
}

// ── Clone ─────────────────────────────────────────────────────────────────────

#[test]
fn handle_clone_shares_state_reads() {
    let (_, h) = mgr_with_schema();
    let h2 = h.clone();
    h.set("count", 77_i64).unwrap();
    assert_eq!(h2.get_int("count").unwrap(), 77);
}

#[test]
fn handle_clone_shares_state_writes() {
    let (_, h) = mgr_with_schema();
    let h2 = h.clone();
    h2.set("count", 88_i64).unwrap();
    assert_eq!(h.get_int("count").unwrap(), 88);
}
