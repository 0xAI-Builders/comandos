// Frozen exact GlyphIndex source from 309fe3c (same original 38cc17e).
use comandos_term_web::canvas::{MAX_ENTRIES, MAX_KEY_BYTES};
use std::collections::HashMap;
pub struct OriginalIndex<K, S> {
    single: HashMap<(char, K), S>,
    multi: HashMap<String, HashMap<K, S>>,
    /// Entradas guardadas (incluidas las vacías, que no ocupan página).
    entries: usize,
    key_bytes: usize,
    generation: u32,
}

impl<K, S> Default for OriginalIndex<K, S> {
    fn default() -> Self {
        OriginalIndex {
            single: HashMap::new(),
            multi: HashMap::new(),
            entries: 0,
            key_bytes: 0,
            generation: 0,
        }
    }
}

impl<K: Copy + Eq + std::hash::Hash, S: Copy> OriginalIndex<K, S> {
    pub fn lookup(&self, text: &str, key: &K) -> Option<S> {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => self.single.get(&(c, *key)).copied(),
            _ => self.multi.get(text).and_then(|m| m.get(key)).copied(),
        }
    }

    /// Guardar `text` pasaría de algún tope.
    pub fn needs_room(&self, text: &str) -> bool {
        self.entries >= MAX_ENTRIES || self.key_bytes.saturating_add(text.len()) > MAX_KEY_BYTES
    }

    pub fn insert(&mut self, text: &str, key: K, slot: S) {
        self.entries += 1;
        self.key_bytes = self.key_bytes.saturating_add(text.len());
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => {
                self.single.insert((c, key), slot);
            }
            _ => {
                self.multi
                    .entry(text.to_string())
                    .or_default()
                    .insert(key, slot);
            }
        }
    }

    pub fn clear(&mut self) {
        self.single.clear();
        self.multi.clear();
        self.entries = 0;
        self.key_bytes = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn len(&self) -> usize {
        self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    pub fn key_bytes(&self) -> usize {
        self.key_bytes
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Recorre las entradas guardadas.
    pub fn for_each(&self, mut f: impl FnMut(&str, &K, &S)) {
        let mut buf = [0u8; 4];
        for ((c, k), slot) in &self.single {
            f(c.encode_utf8(&mut buf), k, slot);
        }
        for (text, map) in &self.multi {
            for (k, slot) in map {
                f(text, k, slot);
            }
        }
    }
}
