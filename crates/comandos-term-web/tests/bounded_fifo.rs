//! Ring queue retains the original bounded map's observable FIFO contract.
use comandos_term_web::canvas::Bounded;
use std::{
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;
#[derive(Debug, Clone)]
struct Original<K, V> {
    cap: usize,
    map: HashMap<K, V>,
    order: VecDeque<K>,
}
impl<K: Hash + Eq + Clone, V> Original<K, V> {
    fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }
    fn insert(&mut self, k: K, v: V) {
        if self.map.insert(k.clone(), v).is_none() {
            self.order.push_back(k);
        }
        while self.map.len() > self.cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }
    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key(u32);
impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0u8.hash(state);
    }
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn collisions_updates_clones_clear_and_capacity_match_original() {
    for cap in [0, 1, 2, 3, 7, 64, 128, 8192] {
        let mut old = Original::new(cap);
        let mut now = Bounded::new(cap);
        // All colliding keys need not quadratic-probe 8192; the larger case uses
        // a bounded rotating set, while the separate test fills the full cap.
        for n in 0..4096u32 {
            let k = Key((n.wrapping_mul(37)) % 257);
            old.insert(k, n);
            now.insert(k, n);
            if n % 3 == 0 {
                let k = Key(n % 257);
                old.insert(k, n + 1);
                now.insert(k, n + 1);
            }
            for k in 0..257 {
                assert_eq!(now.get(&Key(k)), old.map.get(&Key(k)));
            }
            assert_eq!(now.len(), old.map.len());
            assert_eq!(now.is_empty(), old.map.is_empty());
            if n % 127 == 0 {
                let mut cloned = now.clone();
                assert_eq!(format!("{cloned:?}"), format!("{now:?}"));
                cloned.insert(Key(999), n);
                assert_eq!(now.get(&Key(999)), None);
            }
            if n % 997 == 0 {
                old.clear();
                now.clear();
            }
        }
        // Keep Debug's public fields and logical order without the new cursor.
        let debug = format!("{now:?}");
        let order = format!("order: {:?}", old.order);
        assert!(debug.contains(&order));
        assert!(!debug.contains("oldest"));
    }
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn full_capacity_wraps_without_changing_lookup_or_eviction() {
    let mut old = Original::new(8192);
    let mut now = Bounded::new(8192);
    for n in 0..20000u32 {
        old.insert(n, n + 1);
        now.insert(n, n + 1);
        if n % 97 == 0 {
            old.insert(n / 2, n + 3);
            now.insert(n / 2, n + 3);
        }
        assert_eq!(now.len(), old.map.len());
        if n % 1000 == 0 {
            for k in 0..=n {
                assert_eq!(now.get(&k), old.map.get(&k));
            }
        }
    }
    for k in 0..20000 {
        assert_eq!(now.get(&k), old.map.get(&k));
    }
    let order = format!("order: {:?}", old.order);
    assert!(format!("{now:?}").contains(&order));
    old.clear();
    now.clear();
    assert!(now.is_empty());
    old.insert(1, 7);
    now.insert(1, 7);
    assert_eq!(now.get(&1), old.map.get(&1));
}
