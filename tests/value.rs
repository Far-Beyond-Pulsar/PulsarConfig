//! Tests for `ConfigValue` and `Color`.

use pulsar_config::{Color, ConfigValue};

// ── Bool ─────────────────────────────────────────────────────────────────────

#[test]
fn bool_true_stores_and_retrieves() {
    assert_eq!(ConfigValue::Bool(true).as_bool().unwrap(), true);
}

#[test]
fn bool_false_stores_and_retrieves() {
    assert_eq!(ConfigValue::Bool(false).as_bool().unwrap(), false);
}

#[test]
fn bool_from_into() {
    let v: ConfigValue = true.into();
    assert_eq!(v, ConfigValue::Bool(true));
}

#[test]
fn bool_type_mismatch_from_int() {
    assert!(ConfigValue::Int(1).as_bool().is_err());
}

#[test]
fn bool_type_mismatch_from_float() {
    assert!(ConfigValue::Float(1.0).as_bool().is_err());
}

#[test]
fn bool_type_mismatch_from_string() {
    assert!(ConfigValue::String("true".into()).as_bool().is_err());
}

#[test]
fn bool_type_mismatch_from_color() {
    assert!(ConfigValue::Color(Color::BLACK).as_bool().is_err());
}

#[test]
fn bool_type_mismatch_from_array() {
    assert!(ConfigValue::Array(vec![]).as_bool().is_err());
}

// ── Int ──────────────────────────────────────────────────────────────────────

#[test]
fn int_as_int_positive() {
    assert_eq!(ConfigValue::Int(42).as_int().unwrap(), 42);
}

#[test]
fn int_as_int_negative() {
    assert_eq!(ConfigValue::Int(-99).as_int().unwrap(), -99);
}

#[test]
fn int_as_int_zero() {
    assert_eq!(ConfigValue::Int(0).as_int().unwrap(), 0);
}

#[test]
fn int_as_int_max() {
    assert_eq!(ConfigValue::Int(i64::MAX).as_int().unwrap(), i64::MAX);
}

#[test]
fn int_as_int_min() {
    assert_eq!(ConfigValue::Int(i64::MIN).as_int().unwrap(), i64::MIN);
}

#[test]
fn int_from_i32() {
    let v: ConfigValue = 42_i32.into();
    assert_eq!(v, ConfigValue::Int(42));
}

#[test]
fn int_from_u32() {
    let v: ConfigValue = 100_u32.into();
    assert_eq!(v, ConfigValue::Int(100));
}

#[test]
fn int_from_i64() {
    let v: ConfigValue = 999_i64.into();
    assert_eq!(v, ConfigValue::Int(999));
}

#[test]
fn int_widens_to_float() {
    assert!((ConfigValue::Int(7).as_float().unwrap() - 7.0).abs() < 1e-9);
}

#[test]
fn int_type_mismatch_from_bool() {
    assert!(ConfigValue::Bool(true).as_int().is_err());
}

#[test]
fn int_type_mismatch_from_float() {
    assert!(ConfigValue::Float(3.14).as_int().is_err());
}

#[test]
fn int_type_mismatch_from_string() {
    assert!(ConfigValue::String("1".into()).as_int().is_err());
}

// ── Float ─────────────────────────────────────────────────────────────────────

#[test]
fn float_as_float_positive() {
    assert!((ConfigValue::Float(3.14).as_float().unwrap() - 3.14).abs() < 1e-9);
}

#[test]
fn float_as_float_negative() {
    assert!((ConfigValue::Float(-2.5).as_float().unwrap() + 2.5).abs() < 1e-9);
}

#[test]
fn float_as_float_zero() {
    assert_eq!(ConfigValue::Float(0.0).as_float().unwrap(), 0.0);
}

#[test]
fn float_from_f32() {
    let v: ConfigValue = 1.5_f32.into();
    if let ConfigValue::Float(f) = v {
        assert!((f - 1.5).abs() < 1e-5);
    } else {
        panic!("expected Float variant");
    }
}

#[test]
fn float_from_f64() {
    let v: ConfigValue = 2.718_f64.into();
    assert_eq!(v, ConfigValue::Float(2.718));
}

#[test]
fn float_widens_from_int() {
    assert_eq!(ConfigValue::Int(5).as_float().unwrap(), 5.0);
}

#[test]
fn float_type_mismatch_from_bool() {
    assert!(ConfigValue::Bool(false).as_float().is_err());
}

#[test]
fn float_type_mismatch_from_string() {
    assert!(ConfigValue::String("3.14".into()).as_float().is_err());
}

#[test]
fn float_type_mismatch_from_color() {
    assert!(ConfigValue::Color(Color::WHITE).as_float().is_err());
}

// ── String ───────────────────────────────────────────────────────────────────

#[test]
fn string_as_str_basic() {
    assert_eq!(ConfigValue::String("hello".into()).as_str().unwrap(), "hello");
}

#[test]
fn string_empty() {
    assert_eq!(ConfigValue::String("".into()).as_str().unwrap(), "");
}

#[test]
fn string_unicode() {
    let s = "🦀 Ferris 🦀";
    assert_eq!(ConfigValue::String(s.into()).as_str().unwrap(), s);
}

#[test]
fn string_from_str_ref() {
    let v: ConfigValue = "world".into();
    assert_eq!(v.as_str().unwrap(), "world");
}

#[test]
fn string_from_string_owned() {
    let v: ConfigValue = String::from("owned").into();
    assert_eq!(v.as_str().unwrap(), "owned");
}

#[test]
fn string_type_mismatch_from_bool() {
    assert!(ConfigValue::Bool(true).as_str().is_err());
}

#[test]
fn string_type_mismatch_from_int() {
    assert!(ConfigValue::Int(1).as_str().is_err());
}

#[test]
fn string_type_mismatch_from_float() {
    assert!(ConfigValue::Float(0.0).as_str().is_err());
}

// ── Color value ───────────────────────────────────────────────────────────────

#[test]
fn color_value_as_color_ok() {
    let c = Color::rgba(10, 20, 30, 40);
    assert_eq!(ConfigValue::Color(c).as_color().unwrap(), c);
}

#[test]
fn color_value_from_color() {
    let v: ConfigValue = Color::WHITE.into();
    assert_eq!(v.as_color().unwrap(), Color::WHITE);
}

#[test]
fn color_value_type_mismatch_from_bool() {
    assert!(ConfigValue::Bool(false).as_color().is_err());
}

#[test]
fn color_value_type_mismatch_from_int() {
    assert!(ConfigValue::Int(0).as_color().is_err());
}

#[test]
fn color_value_type_mismatch_from_string() {
    assert!(ConfigValue::String("#FFFFFF".into()).as_color().is_err());
}

// ── Array ─────────────────────────────────────────────────────────────────────

#[test]
fn array_as_array_ok() {
    let items = vec![ConfigValue::Int(1), ConfigValue::Int(2)];
    let v = ConfigValue::Array(items.clone());
    assert_eq!(v.as_array().unwrap(), items.as_slice());
}

#[test]
fn array_empty() {
    let v = ConfigValue::Array(vec![]);
    assert_eq!(v.as_array().unwrap().len(), 0);
}

#[test]
fn array_from_vec() {
    let v: ConfigValue = vec![ConfigValue::Bool(true)].into();
    assert_eq!(v.as_array().unwrap().len(), 1);
}

#[test]
fn array_nested() {
    let inner = ConfigValue::Array(vec![ConfigValue::Int(99)]);
    let outer = ConfigValue::Array(vec![inner.clone()]);
    assert_eq!(outer.as_array().unwrap()[0], inner);
}

#[test]
fn array_mixed_types() {
    let v = ConfigValue::Array(vec![
        ConfigValue::Bool(true),
        ConfigValue::Int(1),
        ConfigValue::Float(2.0),
        ConfigValue::String("hi".into()),
        ConfigValue::Color(Color::BLACK),
    ]);
    assert_eq!(v.as_array().unwrap().len(), 5);
}

#[test]
fn array_type_mismatch_from_bool() {
    assert!(ConfigValue::Bool(false).as_array().is_err());
}

#[test]
fn array_type_mismatch_from_string() {
    assert!(ConfigValue::String("[]".into()).as_array().is_err());
}

// ── type_name ─────────────────────────────────────────────────────────────────

#[test]
fn type_name_bool() {
    assert_eq!(ConfigValue::Bool(true).type_name(), "bool");
}

#[test]
fn type_name_int() {
    assert_eq!(ConfigValue::Int(0).type_name(), "int");
}

#[test]
fn type_name_float() {
    assert_eq!(ConfigValue::Float(0.0).type_name(), "float");
}

#[test]
fn type_name_string() {
    assert_eq!(ConfigValue::String("".into()).type_name(), "string");
}

#[test]
fn type_name_color() {
    assert_eq!(ConfigValue::Color(Color::BLACK).type_name(), "color");
}

#[test]
fn type_name_array() {
    assert_eq!(ConfigValue::Array(vec![]).type_name(), "array");
}

// ── Display ───────────────────────────────────────────────────────────────────

#[test]
fn display_bool_true() {
    assert_eq!(format!("{}", ConfigValue::Bool(true)), "true");
}

#[test]
fn display_bool_false() {
    assert_eq!(format!("{}", ConfigValue::Bool(false)), "false");
}

#[test]
fn display_int_positive() {
    assert_eq!(format!("{}", ConfigValue::Int(42)), "42");
}

#[test]
fn display_int_negative() {
    assert_eq!(format!("{}", ConfigValue::Int(-7)), "-7");
}

#[test]
fn display_float() {
    assert_eq!(format!("{}", ConfigValue::Float(3.5)), "3.5");
}

#[test]
fn display_string() {
    assert_eq!(format!("{}", ConfigValue::String("hello".into())), "hello");
}

#[test]
fn display_empty_array() {
    assert_eq!(format!("{}", ConfigValue::Array(vec![])), "[]");
}

#[test]
fn display_array_with_items() {
    let v = ConfigValue::Array(vec![ConfigValue::Int(1), ConfigValue::Int(2)]);
    assert_eq!(format!("{v}"), "[1, 2]");
}

#[test]
fn display_color() {
    assert_eq!(
        format!("{}", ConfigValue::Color(Color::rgba(1, 2, 3, 4))),
        "rgba(1, 2, 3, 4)"
    );
}

// ── Clone + PartialEq ─────────────────────────────────────────────────────────

#[test]
fn clone_equality_int() {
    let v = ConfigValue::Int(100);
    assert_eq!(v.clone(), v);
}

#[test]
fn clone_equality_array() {
    let v = ConfigValue::Array(vec![ConfigValue::Bool(true)]);
    assert_eq!(v.clone(), v);
}

#[test]
fn value_inequality_same_type() {
    assert_ne!(ConfigValue::Int(1), ConfigValue::Int(2));
}

#[test]
fn value_inequality_different_types() {
    assert_ne!(ConfigValue::Int(1), ConfigValue::Bool(true));
}

#[test]
fn value_float_equality() {
    assert_eq!(ConfigValue::Float(1.5), ConfigValue::Float(1.5));
}

// ── Color type ────────────────────────────────────────────────────────────────

#[test]
fn color_rgba_fields() {
    let c = Color::rgba(10, 20, 30, 40);
    assert_eq!((c.r, c.g, c.b, c.a), (10, 20, 30, 40));
}

#[test]
fn color_rgb_sets_alpha_255() {
    let c = Color::rgb(1, 2, 3);
    assert_eq!(c.a, 255);
}

#[test]
fn color_white_constant() {
    assert_eq!(Color::WHITE, Color::rgba(255, 255, 255, 255));
}

#[test]
fn color_black_constant() {
    assert_eq!(Color::BLACK, Color::rgba(0, 0, 0, 255));
}

#[test]
fn color_transparent_constant() {
    assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0, 0, 0));
}

#[test]
fn color_from_hex_components() {
    let c = Color::from_hex(0xFF804020);
    assert_eq!((c.r, c.g, c.b, c.a), (0xFF, 0x80, 0x40, 0x20));
}

#[test]
fn color_from_hex_roundtrip() {
    let hex = 0xDEAD_BEEFu32;
    assert_eq!(Color::from_hex(hex).to_hex(), hex);
}

#[test]
fn color_to_hex_white() {
    assert_eq!(Color::WHITE.to_hex(), 0xFFFF_FFFFu32);
}

#[test]
fn color_to_hex_black() {
    assert_eq!(Color::BLACK.to_hex(), 0x0000_00FFu32);
}

#[test]
fn color_to_hex_transparent() {
    assert_eq!(Color::TRANSPARENT.to_hex(), 0x0000_0000u32);
}

#[test]
fn color_to_linear_black_channels() {
    let lin = Color::BLACK.to_linear_f32();
    assert!(lin[0].abs() < 1e-5, "r should be near 0");
    assert!(lin[1].abs() < 1e-5, "g should be near 0");
    assert!(lin[2].abs() < 1e-5, "b should be near 0");
    assert!((lin[3] - 1.0).abs() < 1e-5, "alpha should be 1.0");
}

#[test]
fn color_to_linear_white_channels() {
    let lin = Color::WHITE.to_linear_f32();
    assert!((lin[0] - 1.0).abs() < 1e-3, "r should be near 1.0");
    assert!((lin[1] - 1.0).abs() < 1e-3, "g should be near 1.0");
    assert!((lin[2] - 1.0).abs() < 1e-3, "b should be near 1.0");
    assert!((lin[3] - 1.0).abs() < 1e-5, "alpha should be 1.0");
}

#[test]
fn color_to_linear_transparent_alpha() {
    let lin = Color::TRANSPARENT.to_linear_f32();
    assert!(lin[3].abs() < 1e-5, "alpha of transparent should be 0.0");
}

#[test]
fn color_display_format() {
    assert_eq!(format!("{}", Color::rgba(1, 2, 3, 4)), "rgba(1, 2, 3, 4)");
}

#[test]
fn color_equality_same() {
    assert_eq!(Color::rgb(1, 2, 3), Color::rgb(1, 2, 3));
}

#[test]
fn color_inequality_different_channel() {
    assert_ne!(Color::rgb(1, 2, 3), Color::rgb(1, 2, 4));
}

#[test]
fn color_inequality_alpha() {
    assert_ne!(Color::rgba(0, 0, 0, 255), Color::rgba(0, 0, 0, 0));
}
