//! Multi-threaded tests verifying `ConfigManager` and `OwnerHandle` are `Send + Sync`.

use std::{
    sync::{Arc, Barrier, Mutex},
    thread,
};
use std::sync::atomic::{AtomicU64, Ordering};

use pulsar_config::{ConfigManager, ConfigValue, NamespaceSchema, SchemaEntry};

fn make_manager() -> (ConfigManager, pulsar_config::OwnerHandle) {
    let m = ConfigManager::new();
    let schema = NamespaceSchema::new("T", "")
        .setting("a", SchemaEntry::new("", 0_i64))
        .setting("b", SchemaEntry::new("", 0_i64))
        .setting("c", SchemaEntry::new("", "default"));
    let h = m.register("ns", "owner", schema).unwrap();
    (m, h)
}

// ── Concurrent reads ──────────────────────────────────────────────────────────

#[test]
fn concurrent_reads_from_eight_threads() {
    let (_, h) = make_manager();
    h.set("a", 42_i64).unwrap();
    let h = Arc::new(h);
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let h = Arc::clone(&h);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            let v = h.get_int("a").unwrap();
            assert_eq!(v, 42);
        }));
    }
    for jh in handles { jh.join().unwrap(); }
}

// ── Concurrent writes to different keys ──────────────────────────────────────

#[test]
fn concurrent_writes_to_different_keys() {
    let (_, h) = make_manager();
    let ha = Arc::new(h.clone());
    let hb = Arc::new(h.clone());
    let t1 = thread::spawn(move || { ha.set("a", 1_i64).unwrap(); });
    let t2 = thread::spawn(move || { hb.set("b", 2_i64).unwrap(); });
    t1.join().unwrap();
    t2.join().unwrap();
    assert_eq!(h.get_int("a").unwrap(), 1);
    assert_eq!(h.get_int("b").unwrap(), 2);
}

// ── Concurrent writes to same key — no panic ──────────────────────────────────

#[test]
fn concurrent_writes_to_same_key_no_panic() {
    let (_, h) = make_manager();
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for i in 0..8_i64 {
        let h = h.clone();
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            let _ = h.set("a", i); // last write wins; we only care there's no panic/deadlock
        }));
    }
    for jh in handles { jh.join().unwrap(); }
    // Value should be a valid i64 (one of 0..8)
    let v = h.get_int("a").unwrap();
    assert!((0..8).contains(&v));
}

// ── Concurrent register ───────────────────────────────────────────────────────

#[test]
fn concurrent_register_different_owners() {
    let m = ConfigManager::new();
    let m = Arc::new(m);
    let mut handles = Vec::new();
    for i in 0..16 {
        let m = Arc::clone(&m);
        handles.push(thread::spawn(move || {
            let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", i as i64));
            m.register("ns", &format!("owner/{i}"), s).unwrap();
        }));
    }
    for jh in handles { jh.join().unwrap(); }
    assert_eq!(m.list_owners("ns").len(), 16);
}

// ── Clone shares state across threads ────────────────────────────────────────

#[test]
fn clone_shares_state_across_thread_boundary() {
    let (_, h) = make_manager();
    let h2 = h.clone();
    let t = thread::spawn(move || { h2.set("a", 99_i64).unwrap(); });
    t.join().unwrap();
    assert_eq!(h.get_int("a").unwrap(), 99);
}

#[test]
fn manager_clone_shares_state_across_thread_boundary() {
    let (m, h) = make_manager();
    let m2 = m.clone();
    h.set("a", 55_i64).unwrap();
    let t = thread::spawn(move || m2.get("ns", "owner", "a").unwrap());
    let v = t.join().unwrap();
    assert_eq!(v, ConfigValue::Int(55));
}

// ── Global listener fires from write thread ───────────────────────────────────

#[test]
fn global_listener_fires_from_write_thread() {
    let (m, h) = make_manager();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let _g = m.on_any_change(move |_| { c.fetch_add(1, Ordering::Relaxed); });
    let t = thread::spawn(move || { h.set("a", 1_i64).unwrap(); });
    t.join().unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// ── ListenerId dropped from a different thread ────────────────────────────────

#[test]
fn listener_id_dropped_on_different_thread() {
    let (_, h) = make_manager();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let g = h.on_change("a", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
    h.set("a", 1_i64).unwrap(); // fires
    let t = thread::spawn(move || drop(g)); // drop on another thread
    t.join().unwrap();
    h.set("a", 2_i64).unwrap(); // should NOT fire
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

// ── Concurrent list operations ────────────────────────────────────────────────

#[test]
fn concurrent_list_owners_while_registering() {
    let m = Arc::new(ConfigManager::new());
    // Pre-register some owners
    for i in 0..8 {
        let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", 0_i64));
        m.register("ns", &format!("pre/{i}"), s).unwrap();
    }
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let m = Arc::clone(&m);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            let _ = m.list_owners("ns");
        }));
    }
    for jh in handles { jh.join().unwrap(); }
}

#[test]
fn concurrent_search_while_writing() {
    let (m, h) = make_manager();
    let m = Arc::new(m);
    let h = Arc::new(h);
    let barrier = Arc::new(Barrier::new(2));
    let m2 = Arc::clone(&m);
    let h2 = Arc::clone(&h);
    let b1 = Arc::clone(&barrier);
    let b2 = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        b1.wait();
        for i in 0..50_i64 { let _ = h2.set("a", i); }
    });
    let searcher = thread::spawn(move || {
        b2.wait();
        for _ in 0..10 { let _ = m2.search("k"); }
    });
    writer.join().unwrap();
    searcher.join().unwrap();
}

// ── High-frequency writes — listener fires exactly N times ────────────────────

#[test]
fn high_frequency_writes_listener_count() {
    let (_, h) = make_manager();
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let _g = h.on_change("a", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap();
    for i in 0..500_i64 {
        h.set("a", i).unwrap();
    }
    assert_eq!(count.load(Ordering::Relaxed), 500);
}

// ── Multiple threads each hold a ListenerId ───────────────────────────────────

#[test]
fn multiple_threads_each_hold_listener_id() {
    let (_, h) = make_manager();
    let count = Arc::new(AtomicU64::new(0));
    let guards: Vec<_> = (0..4)
        .map(|_| {
            let c = Arc::clone(&count);
            h.on_change("a", move |_| { c.fetch_add(1, Ordering::Relaxed); }).unwrap()
        })
        .collect();
    h.set("a", 1_i64).unwrap(); // all 4 fire
    assert_eq!(count.load(Ordering::Relaxed), 4);
    drop(guards);
    h.set("a", 2_i64).unwrap(); // none fire
    assert_eq!(count.load(Ordering::Relaxed), 4);
}

// ── Concurrent register + read ────────────────────────────────────────────────

#[test]
fn concurrent_register_and_read_different_namespaces() {
    let m = Arc::new(ConfigManager::new());
    let errors = Arc::new(Mutex::new(vec![]));
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = Vec::new();
    for i in 0..5 {
        let m = Arc::clone(&m);
        let e = Arc::clone(&errors);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let s = NamespaceSchema::new("T", "").setting("k", SchemaEntry::new("", i as i64));
            b.wait();
            if let Err(err) = m.register("ns_concurrent", &format!("owner/{i}"), s) {
                e.lock().unwrap().push(format!("{err:?}"));
            }
        }));
    }
    for i in 0..5 {
        let m = Arc::clone(&m);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            let _ = m.list_namespaces();
            let _ = m.list_owners(&format!("ns_{i}"));
        }));
    }
    for jh in handles { jh.join().unwrap(); }
    assert!(errors.lock().unwrap().is_empty());
}
