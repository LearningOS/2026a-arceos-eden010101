//! Collection types (`alloc::collections` re-exports plus `HashMap`).

pub use alloc::collections::*;

use alloc::vec::Vec;
use core::borrow::Borrow;
use core::hash::{BuildHasher, Hash, Hasher};

/// Simple FNV-1a / multiply-xor-rotate hasher seeded from `ax_random()`.
#[derive(Clone)]
pub struct RandomState {
    k0: u64,
    k1: u64,
}

impl RandomState {
    #[inline]
    pub fn new() -> Self {
        let r = arceos_api::sys::ax_random();
        Self {
            k0: (r as u64) ^ 0xcbf2_9ce4_8422_2325,
            k1: ((r >> 64) as u64) ^ 0x9e37_79b9_7f4a_7c15,
        }
    }
}

impl Default for RandomState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DefaultHasher {
    state: u64,
    k1: u64,
}

impl Hasher for DefaultHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= b as u64;
            self.state = self.state.wrapping_mul(0x100_0000_01b3);
        }
    }

    #[inline]
    fn finish(&self) -> u64 {
        let mut h = self.state.wrapping_add(self.k1);
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^ (h >> 31)
    }
}

impl BuildHasher for RandomState {
    type Hasher = DefaultHasher;

    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        DefaultHasher {
            state: self.k0,
            k1: self.k1,
        }
    }
}

const INITIAL_BUCKETS: usize = 16;

/// A hash map implemented with bucket chaining and seeded via `axhal::misc::random()`.
pub struct HashMap<K, V, S = RandomState> {
    buckets: Vec<Vec<(K, V)>>,
    len: usize,
    hash_builder: S,
}

impl<K, V> HashMap<K, V, RandomState> {
    pub fn new() -> Self {
        Self::with_hasher(RandomState::new())
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_and_hasher(capacity, RandomState::new())
    }
}

impl<K, V, S> HashMap<K, V, S> {
    pub fn with_hasher(hash_builder: S) -> Self {
        Self {
            buckets: Vec::new(),
            len: 0,
            hash_builder,
        }
    }

    pub fn with_capacity_and_hasher(capacity: usize, hash_builder: S) -> Self {
        let num_buckets = if capacity == 0 {
            0
        } else {
            capacity.next_power_of_two().max(INITIAL_BUCKETS)
        };
        let mut buckets = Vec::with_capacity(num_buckets);
        for _ in 0..num_buckets {
            buckets.push(Vec::new());
        }
        Self {
            buckets,
            len: 0,
            hash_builder,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.buckets
            .iter()
            .flat_map(|b| b.iter().map(|(k, v)| (k, v)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.buckets
            .iter_mut()
            .flat_map(|b| b.iter_mut().map(|(k, v)| (&*k, v)))
    }
}

impl<K: Eq + Hash, V, S: BuildHasher> HashMap<K, V, S> {
    #[inline]
    fn bucket_index<Q: Hash + ?Sized>(&self, k: &Q, num_buckets: usize) -> usize {
        let mut hasher = self.hash_builder.build_hasher();
        k.hash(&mut hasher);
        (hasher.finish() as usize) & (num_buckets - 1)
    }

    fn resize(&mut self, new_num_buckets: usize) {
        let mut new_buckets = Vec::with_capacity(new_num_buckets);
        for _ in 0..new_num_buckets {
            new_buckets.push(Vec::new());
        }
        let old_buckets = core::mem::replace(&mut self.buckets, new_buckets);
        for bucket in old_buckets {
            for (k, v) in bucket {
                let idx = self.bucket_index(&k, new_num_buckets);
                self.buckets[idx].push((k, v));
            }
        }
    }

    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        if self.buckets.is_empty() {
            self.resize(INITIAL_BUCKETS);
        } else if self.len >= self.buckets.len() {
            self.resize(self.buckets.len() * 2);
        }
        let idx = self.bucket_index(&k, self.buckets.len());
        let bucket = &mut self.buckets[idx];
        for (ek, ev) in bucket.iter_mut() {
            if *ek == k {
                return Some(core::mem::replace(ev, v));
            }
        }
        bucket.push((k, v));
        self.len += 1;
        None
    }

    pub fn get<Q>(&self, k: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if self.buckets.is_empty() {
            return None;
        }
        let idx = self.bucket_index(k, self.buckets.len());
        self.buckets[idx]
            .iter()
            .find(|(ek, _)| ek.borrow() == k)
            .map(|(_, ev)| ev)
    }

    pub fn get_mut<Q>(&mut self, k: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if self.buckets.is_empty() {
            return None;
        }
        let idx = self.bucket_index(k, self.buckets.len());
        self.buckets[idx]
            .iter_mut()
            .find(|(ek, _)| (*ek).borrow() == k)
            .map(|(_, ev)| ev)
    }

    pub fn contains_key<Q>(&self, k: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.get(k).is_some()
    }

    pub fn remove<Q>(&mut self, k: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if self.buckets.is_empty() {
            return None;
        }
        let idx = self.bucket_index(k, self.buckets.len());
        let bucket = &mut self.buckets[idx];
        let pos = bucket.iter().position(|(ek, _)| ek.borrow() == k)?;
        self.len -= 1;
        Some(bucket.swap_remove(pos).1)
    }
}

impl<K, V, S: Default> Default for HashMap<K, V, S> {
    fn default() -> Self {
        Self::with_hasher(S::default())
    }
}
