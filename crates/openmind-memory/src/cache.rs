// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Small bounded caches (std only).
//!
//! * [`Lru`]: capacity-bounded map with least-recently-used eviction and an
//!   optional TTL. O(1) get/insert via a tick counter; eviction scans for the
//!   oldest entry only when full (caps are small: tens to hundreds).
//! * [`FileCache`]: content of a file, re-read only when (mtime, len) change,
//!   with an optional derived value computed once per version.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

struct Entry<V> {
    value: V,
    used: u64,
    born: Instant,
}

pub struct Lru<K, V> {
    cap: usize,
    ttl: Option<Duration>,
    inner: Mutex<(HashMap<K, Entry<V>>, u64)>,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
}

impl<K: Eq + Hash + Clone, V: Clone> Lru<K, V> {
    /// `cap == 0` disables the cache (every get misses, insert is a no-op).
    pub fn new(cap: usize, ttl: Option<Duration>) -> Self {
        Lru {
            cap,
            ttl,
            inner: Mutex::new((HashMap::new(), 0)),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.0.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, k: &K) -> Option<V> {
        if self.cap == 0 {
            self.misses.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.1 += 1;
        let tick = g.1;
        let expired = match g.0.get_mut(k) {
            Some(e) if self.ttl.is_some_and(|t| e.born.elapsed() > t) => true,
            Some(e) => {
                e.used = tick;
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(e.value.clone());
            }
            None => false,
        };
        if expired {
            g.0.remove(k);
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    pub fn insert(&self, k: K, v: V) {
        if self.cap == 0 {
            return;
        }
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.1 += 1;
        let tick = g.1;
        if !g.0.contains_key(&k) && g.0.len() >= self.cap {
            if let Some(old) =
                g.0.iter()
                    .min_by_key(|(_, e)| e.used)
                    .map(|(k, _)| k.clone())
            {
                g.0.remove(&old);
                self.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
        g.0.insert(
            k,
            Entry {
                value: v,
                used: tick,
                born: Instant::now(),
            },
        );
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.0.clear();
        }
    }
}

/// (mtime, len) of a file; `None` when it does not exist.
pub type FileSig = Option<(SystemTime, u64)>;

pub fn file_sig(p: &Path) -> FileSig {
    std::fs::metadata(p)
        .ok()
        .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

/// Caches a value derived from one file; recomputed when the file changes.
pub struct FileCache<T> {
    path: PathBuf,
    slot: Mutex<Option<(FileSig, T)>>,
    pub reloads: AtomicU64,
}

impl<T: Clone> FileCache<T> {
    pub fn new(path: PathBuf) -> Self {
        FileCache {
            path,
            slot: Mutex::new(None),
            reloads: AtomicU64::new(0),
        }
    }

    pub fn sig(&self) -> FileSig {
        file_sig(&self.path)
    }

    /// The derived value for the file's current version. `derive` gets the
    /// file text (None when missing/unreadable).
    pub fn get(&self, derive: impl FnOnce(Option<String>) -> T) -> T {
        let sig = self.sig();
        let mut g = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((s, v)) = g.as_ref() {
            if *s == sig {
                return v.clone();
            }
        }
        let text = if sig.is_some() {
            std::fs::read_to_string(&self.path).ok()
        } else {
            None
        };
        let v = derive(text);
        self.reloads.fetch_add(1, Ordering::Relaxed);
        *g = Some((sig, v.clone()));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_bounded_and_evicts_least_recent() {
        let c: Lru<u32, u32> = Lru::new(2, None);
        c.insert(1, 10);
        c.insert(2, 20);
        assert_eq!(c.get(&1), Some(10)); // 1 is now most recent
        c.insert(3, 30); // evicts 2
        assert_eq!(c.get(&2), None);
        assert_eq!(c.get(&1), Some(10));
        assert_eq!(c.get(&3), Some(30));
        assert_eq!(c.len(), 2);
        assert_eq!(c.evictions.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn lru_ttl_and_disabled() {
        let c: Lru<u32, u32> = Lru::new(4, Some(Duration::from_millis(0)));
        c.insert(1, 1);
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(c.get(&1), None);
        let off: Lru<u32, u32> = Lru::new(0, None);
        off.insert(1, 1);
        assert_eq!(off.get(&1), None);
        assert!(off.is_empty());
    }

    #[test]
    fn file_cache_reloads_on_change() {
        let d = std::env::temp_dir().join(format!("om-fc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("f.txt");
        let _ = std::fs::remove_file(&p);
        let fc: FileCache<usize> = FileCache::new(p.clone());
        assert_eq!(fc.get(|t| t.map(|s| s.len()).unwrap_or(0)), 0);
        std::fs::write(&p, "abc").unwrap();
        assert_eq!(fc.get(|t| t.map(|s| s.len()).unwrap_or(0)), 3);
        assert_eq!(fc.get(|_| 99), 3); // unchanged: cached
        std::fs::write(&p, "abcdef").unwrap();
        assert_eq!(fc.get(|t| t.map(|s| s.len()).unwrap_or(0)), 6);
        let _ = std::fs::remove_dir_all(&d);
    }
}
