//! Scheduling of thumbnail loads, independent of the UI and of what a
//! thumbnail is: which wanted keys to start, at most `max_running` at a
//! time, only those still wanted, plus a bounded LRU of the results.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::thumbnails::SizeClass;

/// Failed keys remembered at most.
const MAX_FAILED: usize = 2048;

/// A thumbnail identity: changing the file or the size asks for a new one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub path: PathBuf,
    /// Modification time in whole seconds.
    pub mtime: i64,
    pub class: SizeClass,
}

/// Least-recently-used map of at most `capacity` values.
pub struct Lru<V> {
    capacity: usize,
    /// Value and the tick it was last used at.
    map: HashMap<Key, (V, u64)>,
    tick: u64,
}

impl<V> Lru<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            map: HashMap::new(),
            tick: 0,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// The value without marking it as used (usable while rendering).
    pub fn peek(&self, key: &Key) -> Option<&V> {
        self.map.get(key).map(|(value, _)| value)
    }

    /// Marks `key` as just used; false if absent.
    pub fn touch(&mut self, key: &Key) -> bool {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|slot| slot.1 = tick).is_some()
    }

    /// Stores `value`, returning what it displaced or evicted (the caller
    /// may need to release resources tied to it).
    pub fn insert(&mut self, key: Key, value: V) -> Vec<V> {
        self.tick += 1;
        let mut gone = Vec::new();
        if let Some((old, _)) = self.map.insert(key, (value, self.tick)) {
            gone.push(old);
        }
        while self.map.len() > self.capacity {
            let oldest = self
                .map
                .iter()
                .min_by_key(|(_, (_, tick))| *tick)
                .map(|(key, _)| key.clone());
            match oldest {
                Some(key) => gone.extend(self.map.remove(&key).map(|(value, _)| value)),
                None => break,
            }
        }
        gone
    }

    /// Removes and returns every value.
    pub fn drain(&mut self) -> Vec<V> {
        self.map.drain().map(|(_, (value, _))| value).collect()
    }
}

/// Decides which thumbnails to load: the wanted ones (visible cells) that
/// are neither loaded, failed nor already loading.
pub struct Queue<V> {
    loaded: Lru<V>,
    /// Keys that produced nothing; not asked again.
    failed: HashSet<Key>,
    running: HashSet<Key>,
    waiting: VecDeque<Key>,
    max_running: usize,
}

impl<V> Queue<V> {
    pub fn new(capacity: usize, max_running: usize) -> Self {
        Self {
            loaded: Lru::new(capacity),
            failed: HashSet::new(),
            running: HashSet::new(),
            waiting: VecDeque::new(),
            max_running: max_running.max(1),
        }
    }

    pub fn get(&self, key: &Key) -> Option<&V> {
        self.loaded.peek(key)
    }

    #[cfg(test)]
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Replaces what is wanted (in priority order): waiting keys that are
    /// no longer wanted are dropped, loaded ones are marked as used.
    pub fn want(&mut self, keys: impl IntoIterator<Item = Key>) {
        self.waiting.clear();
        let mut seen = HashSet::new();
        for key in keys {
            if !seen.insert(key.clone()) || self.failed.contains(&key) {
                continue;
            }
            if !self.loaded.touch(&key) && !self.running.contains(&key) {
                self.waiting.push_back(key);
            }
        }
    }

    /// Keys to start now, up to the concurrency bound.
    pub fn start(&mut self) -> Vec<Key> {
        let mut started = Vec::new();
        while self.running.len() < self.max_running {
            let Some(key) = self.waiting.pop_front() else {
                break;
            };
            self.running.insert(key.clone());
            started.push(key);
        }
        started
    }

    /// A started key is done, with its thumbnail or without one. Returns the
    /// values evicted from the cache.
    pub fn finish(&mut self, key: Key, value: Option<V>) -> Vec<V> {
        self.running.remove(&key);
        match value {
            Some(value) => self.loaded.insert(key, value),
            None => {
                // Bounded: a long session must not collect failures forever
                // (they are recorded on disk, so asking again is cheap).
                if self.failed.len() >= MAX_FAILED {
                    self.failed.clear();
                }
                self.failed.insert(key);
                Vec::new()
            }
        }
    }

    /// A started key is done without a result that should be remembered
    /// (a transient I/O error): it may be asked for again.
    pub fn abandon(&mut self, key: &Key) {
        self.running.remove(key);
    }

    /// Forgets the waiting keys (the folder changed); running ones finish.
    pub fn cancel_waiting(&mut self) {
        self.waiting.clear();
    }

    /// Removes and returns every loaded value (the grid is going away).
    pub fn drain(&mut self) -> Vec<V> {
        self.waiting.clear();
        self.failed.clear();
        self.loaded.drain()
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.running.is_empty() && self.waiting.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, Queue};
    use crate::thumbnails::SizeClass;

    fn key(name: &str) -> Key {
        Key {
            path: name.into(),
            mtime: 1,
            class: SizeClass::Normal,
        }
    }

    #[test]
    fn starts_at_most_the_bound_in_order() {
        let mut queue: Queue<u8> = Queue::new(10, 2);
        queue.want(["a", "b", "c"].map(key));
        assert_eq!(queue.start(), vec![key("a"), key("b")]);
        assert!(queue.start().is_empty());
        queue.finish(key("a"), Some(1));
        assert_eq!(queue.start(), vec![key("c")]);
        assert_eq!(queue.get(&key("a")), Some(&1));
    }

    #[test]
    fn deduplicates_loaded_running_and_failed() {
        let mut queue: Queue<u8> = Queue::new(10, 2);
        queue.want(["a", "a", "b", "c"].map(key));
        assert_eq!(queue.start(), vec![key("a"), key("b")]);
        queue.finish(key("b"), None);
        queue.finish(key("a"), Some(1));
        queue.want(["a", "b", "c", "a"].map(key));
        assert_eq!(queue.start(), vec![key("c")]);
        queue.want(["c"].map(key));
        assert!(queue.start().is_empty());
        // Another mtime or size is another thumbnail.
        let mut changed = key("a");
        changed.mtime = 2;
        queue.want([changed.clone()]);
        assert_eq!(queue.start(), vec![changed]);
    }

    #[test]
    fn scrolled_away_keys_are_dropped_before_starting() {
        let mut queue: Queue<u8> = Queue::new(10, 1);
        queue.want(["a", "b", "c"].map(key));
        assert_eq!(queue.start(), vec![key("a")]);
        queue.want(["x", "y"].map(key));
        queue.finish(key("a"), Some(1));
        assert_eq!(queue.start(), vec![key("x")]);
        assert!(!queue.is_idle());
        queue.finish(key("x"), Some(1));
        assert_eq!(queue.start(), vec![key("y")]);
        queue.finish(key("y"), Some(1));
        assert!(queue.is_idle());
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut queue: Queue<u8> = Queue::new(2, 4);
        queue.want(["a", "b"].map(key));
        queue.start();
        queue.finish(key("a"), Some(1));
        queue.finish(key("b"), Some(2));
        // Scrolling back to "a" makes it recent; "c" evicts "b".
        queue.want(["a"].map(key));
        queue.want(["c"].map(key));
        queue.start();
        assert_eq!(queue.finish(key("c"), Some(3)), vec![2]);
        assert_eq!(queue.loaded_count(), 2);
        assert!(queue.get(&key("a")).is_some());
        assert!(queue.get(&key("b")).is_none());
    }

    #[test]
    fn abandoned_keys_are_asked_again_and_waiting_can_be_cancelled() {
        let mut queue: Queue<u8> = Queue::new(10, 1);
        queue.want(["a", "b"].map(key));
        assert_eq!(queue.start(), vec![key("a")]);
        queue.cancel_waiting();
        queue.abandon(&key("a"));
        assert!(queue.start().is_empty());
        queue.want(["a"].map(key));
        assert_eq!(queue.start(), vec![key("a")]);
        queue.finish(key("a"), Some(1));
        assert_eq!(queue.drain(), vec![1]);
        assert_eq!(queue.loaded_count(), 0);
    }
}
