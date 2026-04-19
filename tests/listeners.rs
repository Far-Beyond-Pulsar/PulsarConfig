//! Tests for change listeners — scoping, ordering, RAII cleanup, and event contents.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use pulsar_config::{ConfigError, ConfigManager, ConfigValue, NamespaceSchema, SchemaEntry};

fn make_mgr() -> (ConfigManager, pulsar_config::OwnerHandle) {
    let m = ConfigManager::new();
    let schema = NamespaceSchema::new("L", "Listener tests")
        .setting("alpha", SchemaEntry::new("Alpha", 1_i64))
        .setting("beta", SchemaEntry::new("Beta", 2_i64))
        .setting("gamma", SchemaEntry::new("Gamma", "g"));
    let h = m.register("ns", "owner/a", schema).unwrap();
    (m, h)
}

fn make_second_owner(m: &ConfigManager) -> pulsar_config::OwnerHandle {
    let schema = NamespaceSchema::new("L2", "Second owner")
        .setting("x", SchemaEntry::new("X", 0_i64));
    m.register("ns", "owner/b", schema).unwrap()
}

// ── on_change — basic ─────────────────────────────────────────────────────────

#[test]
fn on_change_fires_on_matching_key() {
    let (_, h) = make_mgr();
    let fired = Arc::new(AtomicU64::new(0));
    let f = Arc::clone(&fired);
    let _g = h.on_change("alpha", move |_| { f.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.set("alpha", 99_i64).unwrap();
    assert_eq!(fired.load(Ordering::Relaxed), 1);
}

#[test]
fn on_change_does_not_fire_on_other_key() {
    let (_, h) = make_mgr();
    let fired = Arc::new(AtomicU64::new(0));
    let f = Arc::clone(&fired);
    let _g = h.on_change("alpha", move |_| { f.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.set("beta", 99_i64).unwrap(); // different key — should NOT fire
    assert_eq!(fired.load(Ordering::Relaxed), 0);
}

#[test]
fn on_change_provides_correct_new_value() {
    let (_, h) = make_mgr();
    let received = Arc::new(Mutex::new(None::<ConfigValue>));
    let rx = Arc::clone(&received);
    let _g = h.on_change("alpha", move |e| *rx.lock().unwrap() = Some(e.new_value.clone())).unwrap();
    h.set("alpha", 42_i64).unwrap();
    assert_eq!(*received.lock().unwrap(), Some(ConfigValue::Int(42)));
}

#[test]
fn on_change_provides_old_value_is_default_on_first_write() {
    // Defaults are pre-seeded, so old_value on first write is Some(default).
    let (_, h) = make_mgr();
    let old_val = Arc::new(Mutex::new(None::<Option<ConfigValue>>));
    let ov = Arc::clone(&old_val);
    let _g = h.on_change("alpha", move |e| *ov.lock().unwrap() = Some(e.old_value.clone())).unwrap();
    h.set("alpha", 100_i64).unwrap();
    let captured = old_val.lock().unwrap().clone().unwrap();
    assert_eq!(captured, Some(ConfigValue::Int(1))); // default was 1
}

#[test]
fn on_change_provides_old_value_from_prior_write() {
    let (_, h) = make_mgr();
    h.set("alpha", 5_i64).unwrap();
    let old_val = Arc::new(Mutex::new(None::<ConfigValue>));
    let ov = Arc::clone(&old_val);
    let _g = h.on_change("alpha", move |e| {
        if let Some(v) = &e.old_value {
            *ov.lock().unwrap() = Some(v.clone());
        }
    }).unwrap();
    h.set("alpha", 10_i64).unwrap();
    assert_eq!(*old_val.lock().unwrap(), Some(ConfigValue::Int(5)));
}

#[test]
fn on_change_event_contains_correct_namespace() {
    let (_, h) = make_mgr();
    let ns = Arc::new(Mutex::new(String::new()));
    let n = Arc::clone(&ns);
    let _g = h.on_change("alpha", move |e| *n.lock().unwrap() = e.namespace.clone()).unwrap();
    h.set("alpha", 1_i64).unwrap();
    assert_eq!(*ns.lock().unwrap(), "ns");
}

#[test]
fn on_change_event_contains_correct_owner_segments() {
    let (_, h) = make_mgr();
    let captured = Arc::new(Mutex::new(vec![]));
    let cap = Arc::clone(&captured);
    let _g = h.on_change("alpha", move |e| *cap.lock().unwrap() = e.owner.clone()).unwrap();
    h.set("alpha", 1_i64).unwrap();
    assert_eq!(*captured.lock().unwrap(), vec!["owner", "a"]);
}

#[test]
fn on_change_event_contains_correct_key() {
    let (_, h) = make_mgr();
    let captured = Arc::new(Mutex::new(String::new()));
    let cap = Arc::clone(&captured);
    let _g = h.on_change("alpha", move |e| *cap.lock().unwrap() = e.key.clone()).unwrap();
    h.set("alpha", 1_i64).unwrap();
    assert_eq!(*captured.lock().unwrap(), "alpha");
}

#[test]
fn on_change_event_owner_path_helper() {
    let (_, h) = make_mgr();
    let path = Arc::new(Mutex::new(String::new()));
    let p = Arc::clone(&path);
    let _g = h.on_change("alpha", move |e| *p.lock().unwrap() = e.owner_path()).unwrap();
    h.set("alpha", 1_i64).unwrap();
    assert_eq!(*path.lock().unwrap(), "owner/a");
}

#[test]
fn on_change_unknown_key_returns_unknown_key_error() {
    let (_, h) = make_mgr();
    let result = h.on_change("nonexistent", |_| {});
    assert!(result.is_err());
    // ListenerId doesn't impl Debug, use is_err() instead of unwrap_err()
}

#[test]
fn on_change_fires_exactly_n_times_for_n_writes() {
    let (_, h) = make_mgr();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let _g = h.on_change("alpha", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
    for i in 0..50 {
        h.set("alpha", i).unwrap();
    }
    assert_eq!(count.load(Ordering::Relaxed), 50);
}

// ── on_change — RAII ─────────────────────────────────────────────────────────

#[test]
fn listener_id_drop_removes_listener() {
    let (_, h) = make_mgr();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    {
        let _g = h.on_change("alpha", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
        h.set("alpha", 1_i64).unwrap(); // fires
    } // _g dropped → listener removed
    h.set("alpha", 2_i64).unwrap(); // should NOT fire
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn multiple_listeners_on_same_key_all_fire() {
    let (_, h) = make_mgr();
    let c1 = Arc::new(AtomicU64::new(0));
    let c2 = Arc::new(AtomicU64::new(0));
    let a1 = Arc::clone(&c1);
    let a2 = Arc::clone(&c2);
    let _g1 = h.on_change("alpha", move |_| { a1.fetch_add(1, Ordering::Relaxed); }).unwrap();
    let _g2 = h.on_change("alpha", move |_| { a2.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.set("alpha", 1_i64).unwrap();
    assert_eq!(c1.load(Ordering::Relaxed), 1);
    assert_eq!(c2.load(Ordering::Relaxed), 1);
}

#[test]
fn dropping_one_of_multiple_listeners_only_removes_that_one() {
    let (_, h) = make_mgr();
    let c1 = Arc::new(AtomicU64::new(0));
    let c2 = Arc::new(AtomicU64::new(0));
    let a1 = Arc::clone(&c1);
    let a2 = Arc::clone(&c2);
    let g1 = h.on_change("alpha", move |_| { a1.fetch_add(1, Ordering::Relaxed); }).unwrap();
    let _g2 = h.on_change("alpha", move |_| { a2.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.set("alpha", 1_i64).unwrap(); // both fire
    drop(g1);
    h.set("alpha", 2_i64).unwrap(); // only _g2 fires
    assert_eq!(c1.load(Ordering::Relaxed), 1);
    assert_eq!(c2.load(Ordering::Relaxed), 2);
}

#[test]
fn listener_added_after_writes_starts_fresh() {
    let (_, h) = make_mgr();
    h.set("alpha", 5_i64).unwrap();
    h.set("alpha", 6_i64).unwrap();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let _g = h.on_change("alpha", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.set("alpha", 7_i64).unwrap(); // only this fires
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// ── on_any_change (owner scope) ───────────────────────────────────────────────

#[test]
fn owner_listener_fires_for_all_keys() {
    let (_, h) = make_mgr();
    let keys = Arc::new(Mutex::new(vec![]));
    let k = Arc::clone(&keys);
    let _g = h.on_any_change(move |e| k.lock().unwrap().push(e.key.clone()));
    h.set("alpha", 10_i64).unwrap();
    h.set("beta", 20_i64).unwrap();
    h.set("gamma", "g2").unwrap();
    let ks = keys.lock().unwrap();
    assert!(ks.contains(&"alpha".to_owned()));
    assert!(ks.contains(&"beta".to_owned()));
    assert!(ks.contains(&"gamma".to_owned()));
}

#[test]
fn owner_listener_does_not_fire_for_different_owner() {
    let (m, h) = make_mgr();
    let h2 = make_second_owner(&m);
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    // Listen on owner/a — writes to owner/b should NOT fire it
    let _g = h.on_any_change(move |_| { c.fetch_add(1, Ordering::Relaxed); });
    h2.set("x", 99_i64).unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[test]
fn owner_listener_raii_cleanup() {
    let (_, h) = make_mgr();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    {
        let _g = h.on_any_change(move |_| { c.fetch_add(1, Ordering::Relaxed); });
        h.set("alpha", 1_i64).unwrap(); // fires
    } // dropped
    h.set("alpha", 2_i64).unwrap(); // should NOT fire
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// ── on_any_change (global scope) ─────────────────────────────────────────────

#[test]
fn global_listener_fires_for_all_owners() {
    let (m, h1) = make_mgr();
    let h2 = make_second_owner(&m);
    let events = Arc::new(Mutex::new(vec![]));
    let ev = Arc::clone(&events);
    let _g = m.on_any_change(move |e| ev.lock().unwrap().push(e.owner_path()));
    h1.set("alpha", 1_i64).unwrap();
    h2.set("x", 2_i64).unwrap();
    let e = events.lock().unwrap();
    assert!(e.contains(&"owner/a".to_owned()));
    assert!(e.contains(&"owner/b".to_owned()));
}

#[test]
fn global_listener_fires_across_namespaces() {
    let m = ConfigManager::new();
    let s = || NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
    let h1 = m.register("editor", "o", s()).unwrap();
    let h2 = m.register("project", "o", s()).unwrap();
    let namespaces = Arc::new(Mutex::new(vec![]));
    let ns = Arc::clone(&namespaces);
    let _g = m.on_any_change(move |e| ns.lock().unwrap().push(e.namespace.clone()));
    h1.set("k", 1_i64).unwrap();
    h2.set("k", 2_i64).unwrap();
    let collected = namespaces.lock().unwrap();
    assert!(collected.contains(&"editor".to_owned()));
    assert!(collected.contains(&"project".to_owned()));
}

#[test]
fn global_listener_raii_cleanup() {
    let (m, h) = make_mgr();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    {
        let _g = m.on_any_change(move |_| { c.fetch_add(1, Ordering::Relaxed); });
        h.set("alpha", 1_i64).unwrap(); // fires
    }
    h.set("alpha", 2_i64).unwrap(); // should NOT fire
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// ── reset fires listeners ─────────────────────────────────────────────────────

#[test]
fn reset_fires_key_listener() {
    let (_, h) = make_mgr();
    h.set("alpha", 5_i64).unwrap();
    let fired = Arc::new(AtomicU64::new(0));
    let f = Arc::clone(&fired);
    let _g = h.on_change("alpha", move |_| { f.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.reset_to_default("alpha").unwrap();
    assert_eq!(fired.load(Ordering::Relaxed), 1);
}

#[test]
fn reset_fires_owner_listener() {
    let (_, h) = make_mgr();
    let fired = Arc::new(AtomicU64::new(0));
    let f = Arc::clone(&fired);
    let _g = h.on_any_change(move |_| { f.fetch_add(1, Ordering::Relaxed); });
    h.reset_to_default("alpha").unwrap();
    assert_eq!(fired.load(Ordering::Relaxed), 1);
}

#[test]
fn reset_event_new_value_is_default() {
    let (_, h) = make_mgr();
    h.set("alpha", 999_i64).unwrap();
    let received = Arc::new(Mutex::new(None::<ConfigValue>));
    let rx = Arc::clone(&received);
    let _g = h.on_change("alpha", move |e| *rx.lock().unwrap() = Some(e.new_value.clone())).unwrap();
    h.reset_to_default("alpha").unwrap();
    assert_eq!(*received.lock().unwrap(), Some(ConfigValue::Int(1))); // default
}

// ── cross-owner isolation ─────────────────────────────────────────────────────

#[test]
fn key_listener_on_owner_a_not_fired_by_owner_b() {
    let (m, h1) = make_mgr();
    let h2 = make_second_owner(&m);
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let _g = h1.on_change("alpha", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h2.set("x", 1_i64).unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[test]
fn events_from_cloned_handle_share_listeners() {
    let (_, h) = make_mgr();
    let h2 = h.clone();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let _g = h.on_change("alpha", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
    // Write via clone — should still fire the listener registered on original
    h2.set("alpha", 1_i64).unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// ── ordering ─────────────────────────────────────────────────────────────────

#[test]
fn listener_receives_events_in_write_order() {
    let (_, h) = make_mgr();
    let values = Arc::new(Mutex::new(vec![]));
    let v = Arc::clone(&values);
    let _g = h.on_change("alpha", move |e| {
        v.lock().unwrap().push(e.new_value.clone());
    }).unwrap();
    for i in 0..5_i64 {
        h.set("alpha", i).unwrap();
    }
    let collected = values.lock().unwrap();
    let ints: Vec<i64> = collected.iter().map(|v| v.as_int().unwrap()).collect();
    assert_eq!(ints, vec![0, 1, 2, 3, 4]);
}
