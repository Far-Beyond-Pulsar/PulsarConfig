//! Tests for `Validator`, `SchemaEntry`, and `NamespaceSchema`.
//! Validators are tested indirectly through `OwnerHandle::set`.

use pulsar_config::{ConfigManager, ConfigValue, NamespaceSchema, SchemaEntry, Validator};

fn mgr() -> ConfigManager {
    ConfigManager::new()
}

fn schema_with(key: &str, entry: SchemaEntry) -> NamespaceSchema {
    NamespaceSchema::new("Test", "Test schema").setting(key, entry)
}

// ── IntRange ─────────────────────────────────────────────────────────────────

#[test]
fn int_range_accepts_min_bound() {
    let m = mgr();
    let h = m
        .register("ns", "owner", schema_with("v", SchemaEntry::new("", 0_i64).validator(Validator::int_range(0, 100))))
        .unwrap();
    assert!(h.set("v", 0_i64).is_ok());
}

#[test]
fn int_range_accepts_max_bound() {
    let m = mgr();
    let h = m
        .register("ns", "owner", schema_with("v", SchemaEntry::new("", 50_i64).validator(Validator::int_range(0, 100))))
        .unwrap();
    assert!(h.set("v", 100_i64).is_ok());
}

#[test]
fn int_range_accepts_midpoint() {
    let m = mgr();
    let h = m
        .register("ns", "owner", schema_with("v", SchemaEntry::new("", 50_i64).validator(Validator::int_range(0, 100))))
        .unwrap();
    assert!(h.set("v", 50_i64).is_ok());
}

#[test]
fn int_range_rejects_below_min() {
    let m = mgr();
    let h = m
        .register("ns", "owner", schema_with("v", SchemaEntry::new("", 50_i64).validator(Validator::int_range(0, 100))))
        .unwrap();
    assert!(h.set("v", -1_i64).is_err());
}

#[test]
fn int_range_rejects_above_max() {
    let m = mgr();
    let h = m
        .register("ns", "owner", schema_with("v", SchemaEntry::new("", 50_i64).validator(Validator::int_range(0, 100))))
        .unwrap();
    assert!(h.set("v", 101_i64).is_err());
}

#[test]
fn int_range_rejects_wrong_type() {
    // Passing a float where an int_range validator is declared fails with ValidationFailed
    let m = mgr();
    let h = m
        .register(
            "ns",
            "owner",
            schema_with("v", SchemaEntry::new("", 50_i64).validator(Validator::int_range(0, 100))),
        )
        .unwrap();
    // Float isn't an int — the validator calls as_int() which will fail
    assert!(h.set("v", 50.0_f64).is_err());
}

#[test]
fn int_range_value_unchanged_on_failure() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 10_i64).validator(Validator::int_range(0, 20))))
        .unwrap();
    h.set("v", 15_i64).unwrap();
    assert!(h.set("v", 999_i64).is_err());
    assert_eq!(h.get_int("v").unwrap(), 15);
}

// ── FloatRange ───────────────────────────────────────────────────────────────

#[test]
fn float_range_accepts_in_range() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", 5.0_f64).is_ok());
}

#[test]
fn float_range_accepts_min_bound() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", 0.0_f64).is_ok());
}

#[test]
fn float_range_accepts_max_bound() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", 10.0_f64).is_ok());
}

#[test]
fn float_range_rejects_below_min() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", -0.001_f64).is_err());
}

#[test]
fn float_range_rejects_above_max() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", 10.001_f64).is_err());
}

#[test]
fn float_range_widens_int_input() {
    // Validator::FloatRange calls as_float() which widens int — should succeed.
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", 3_i64).is_ok());
}

#[test]
fn float_range_wrong_type_fails() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 5.0_f64).validator(Validator::float_range(0.0, 10.0))))
        .unwrap();
    assert!(h.set("v", "five").is_err());
}

// ── StringMaxLength ───────────────────────────────────────────────────────────

#[test]
fn string_max_length_accepts_short() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", "hi").validator(Validator::string_max_length(10))))
        .unwrap();
    assert!(h.set("v", "short").is_ok());
}

#[test]
fn string_max_length_accepts_exactly_max() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", "").validator(Validator::string_max_length(5))))
        .unwrap();
    assert!(h.set("v", "abcde").is_ok());
}

#[test]
fn string_max_length_rejects_over_max() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", "").validator(Validator::string_max_length(3))))
        .unwrap();
    assert!(h.set("v", "toolong").is_err());
}

#[test]
fn string_max_length_zero_accepts_empty() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", "").validator(Validator::string_max_length(0))))
        .unwrap();
    assert!(h.set("v", "").is_ok());
}

#[test]
fn string_max_length_zero_rejects_nonempty() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", "").validator(Validator::string_max_length(0))))
        .unwrap();
    assert!(h.set("v", "x").is_err());
}

#[test]
fn string_max_length_wrong_type_fails() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", "").validator(Validator::string_max_length(10))))
        .unwrap();
    assert!(h.set("v", 5_i64).is_err());
}

// ── StringOneOf ───────────────────────────────────────────────────────────────

#[test]
fn string_one_of_accepts_valid_option() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", "low").validator(Validator::string_one_of(["low", "medium", "high"])),
            ),
        )
        .unwrap();
    assert!(h.set("v", "high").is_ok());
}

#[test]
fn string_one_of_accepts_first_option() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", "a").validator(Validator::string_one_of(["a", "b"])),
            ),
        )
        .unwrap();
    assert!(h.set("v", "a").is_ok());
}

#[test]
fn string_one_of_rejects_unknown_value() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", "low").validator(Validator::string_one_of(["low", "medium", "high"])),
            ),
        )
        .unwrap();
    assert!(h.set("v", "ultra").is_err());
}

#[test]
fn string_one_of_is_case_sensitive() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", "low").validator(Validator::string_one_of(["low", "medium"])),
            ),
        )
        .unwrap();
    // "Low" (capital L) is not in the list
    assert!(h.set("v", "Low").is_err());
}

#[test]
fn string_one_of_rejects_empty_string_not_in_list() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", "a").validator(Validator::string_one_of(["a", "b"])),
            ),
        )
        .unwrap();
    assert!(h.set("v", "").is_err());
}

#[test]
fn string_one_of_wrong_type_fails() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", "low").validator(Validator::string_one_of(["low"])),
            ),
        )
        .unwrap();
    // Int is not a string
    assert!(h.set("v", 0_i64).is_err());
}

// ── Custom validator ──────────────────────────────────────────────────────────

#[test]
fn custom_validator_accepts_valid() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", 0_i64).validator(Validator::custom(|v| {
                    if v.as_int().unwrap_or(-1) % 2 == 0 {
                        Ok(())
                    } else {
                        Err("must be even".into())
                    }
                })),
            ),
        )
        .unwrap();
    assert!(h.set("v", 4_i64).is_ok());
}

#[test]
fn custom_validator_rejects_invalid() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", 0_i64).validator(Validator::custom(|v| {
                    if v.as_int().unwrap_or(-1) % 2 == 0 {
                        Ok(())
                    } else {
                        Err("must be even".into())
                    }
                })),
            ),
        )
        .unwrap();
    assert!(h.set("v", 3_i64).is_err());
}

// ── Multiple validators ───────────────────────────────────────────────────────

#[test]
fn multiple_validators_first_fails() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", 5_i64)
                    .validator(Validator::int_range(0, 10))
                    .validator(Validator::custom(|_| Ok(()))), // always passes
            ),
        )
        .unwrap();
    assert!(h.set("v", 99_i64).is_err()); // first validator fails
}

#[test]
fn multiple_validators_second_fails() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", 5_i64)
                    .validator(Validator::int_range(0, 100))
                    .validator(Validator::custom(|v| {
                        if v.as_int().unwrap_or(0) < 50 {
                            Ok(())
                        } else {
                            Err("must be < 50".into())
                        }
                    })),
            ),
        )
        .unwrap();
    assert!(h.set("v", 75_i64).is_err()); // second validator fails
}

#[test]
fn both_validators_pass() {
    let m = mgr();
    let h = m
        .register(
            "ns",
            "o",
            schema_with(
                "v",
                SchemaEntry::new("", 10_i64)
                    .validator(Validator::int_range(0, 100))
                    .validator(Validator::custom(|v| {
                        if v.as_int().unwrap_or(0) % 2 == 0 {
                            Ok(())
                        } else {
                            Err("odd".into())
                        }
                    })),
            ),
        )
        .unwrap();
    assert!(h.set("v", 20_i64).is_ok());
}

#[test]
fn no_validators_always_passes() {
    let m = mgr();
    let h = m
        .register("ns", "o", schema_with("v", SchemaEntry::new("", 0_i64)))
        .unwrap();
    // No validators — any int value accepted
    assert!(h.set("v", i64::MAX).is_ok());
    assert!(h.set("v", i64::MIN).is_ok());
}

// ── SchemaEntry metadata ─────────────────────────────────────────────────────

#[test]
fn schema_entry_tag_stored() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("desc", 0_i64).tag("perf"));
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let entry = settings.iter().find(|s| s.key == "k").unwrap();
    assert!(entry.tags.contains(&"perf".to_owned()));
}

#[test]
fn schema_entry_multiple_tags() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("", 0_i64).tags(["a", "b", "c"]));
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let e = settings.iter().find(|s| s.key == "k").unwrap();
    assert_eq!(e.tags.len(), 3);
}

#[test]
fn schema_entry_tags_via_chained_tag_calls() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("", 0_i64).tag("x").tag("y"));
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let e = settings.iter().find(|s| s.key == "k").unwrap();
    assert_eq!(e.tags.len(), 2);
}

#[test]
fn schema_entry_read_only_flag_stored() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "")
        .setting("ro", SchemaEntry::new("", 0_i64).read_only());
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let e = settings.iter().find(|s| s.key == "ro").unwrap();
    assert!(e.read_only);
}

#[test]
fn schema_entry_not_read_only_by_default() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let e = settings.iter().find(|s| s.key == "k").unwrap();
    assert!(!e.read_only);
}

#[test]
fn schema_entry_description_stored() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("My description", 0_i64));
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    let e = settings.iter().find(|s| s.key == "k").unwrap();
    assert_eq!(e.description, "My description");
}

// ── NamespaceSchema ───────────────────────────────────────────────────────────

#[test]
fn namespace_schema_stores_display_name_in_search() {
    let m = mgr();
    let s = NamespaceSchema::new("My Display Name", "desc")
        .setting("k", SchemaEntry::new("Some key", 1_i64).tag("unique_tag_xyz"));
    m.register("ns", "owner", s).unwrap();
    let results = m.search("unique_tag_xyz");
    assert_eq!(results[0].owner_display_name, "My Display Name");
}

#[test]
fn namespace_schema_multiple_settings() {
    let m = mgr();
    let s = NamespaceSchema::new("T", "")
        .setting("a", SchemaEntry::new("", 1_i64))
        .setting("b", SchemaEntry::new("", 2_i64))
        .setting("c", SchemaEntry::new("", 3_i64));
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    assert_eq!(settings.len(), 3);
}

#[test]
fn namespace_schema_duplicate_key_overwrites() {
    // In HashMap, inserting with same key overwrites.
    let m = mgr();
    let s = NamespaceSchema::new("T", "")
        .setting("k", SchemaEntry::new("first", 10_i64))
        .setting("k", SchemaEntry::new("second", 20_i64)); // overwrites
    m.register("ns", "o", s).unwrap();
    let settings = m.list_settings("ns", "o").unwrap();
    // Only one setting with key "k" should exist
    let matches: Vec<_> = settings.iter().filter(|s| s.key == "k").collect();
    assert_eq!(matches.len(), 1);
    // The second definition wins
    assert_eq!(matches[0].default_value, ConfigValue::Int(20));
}
