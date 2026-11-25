//! An arena with typed ids.
//!
//! An id is a `u32` slot index. Slots are never reused: removing a value
//! leaves its slot empty for good, so an id kept after its value is removed
//! fails to resolve instead of aliasing a newer value. Iteration is in slot
//! (= creation) order, so everything is deterministic (no hashing anywhere).

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

/// A typed handle to a `T` stored in an [`Arena<T>`].
pub struct Id<T> {
    index: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// An id from its raw index. Only the arena mints ids that resolve;
    /// this exists for deserialization and tests.
    pub const fn from_raw(index: u32) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }

    /// The slot index.
    pub const fn index(self) -> u32 {
        self.index
    }
}

// Manual impls: derives would wrongly require `T: Clone`, `T: Eq`, ...
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for Id<T> {}
impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.index.cmp(&other.index)
    }
}
impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}
impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.index)
    }
}

/// Values of one kind, addressed by [`Id<T>`].
#[derive(Debug, Clone)]
pub struct Arena<T> {
    slots: Vec<Option<T>>,
    len: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
        }
    }
}

impl<T> Arena<T> {
    /// An empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live values.
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` when no value is live.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Stores `value` in a fresh slot and returns its id.
    pub fn insert(&mut self, value: T) -> Id<T> {
        let index = u32::try_from(self.slots.len()).expect("arena exceeds u32::MAX slots");
        self.slots.push(Some(value));
        self.len += 1;
        Id::from_raw(index)
    }

    /// `true` when `id` resolves.
    pub fn contains(&self, id: Id<T>) -> bool {
        self.get(id).is_some()
    }

    /// The value behind `id`, `None` when it was removed or never existed.
    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.slots.get(id.index as usize)?.as_ref()
    }

    /// Mutable access to the value behind `id`.
    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        self.slots.get_mut(id.index as usize)?.as_mut()
    }

    /// Removes and returns the value behind `id`. The slot is never
    /// reused, so `id` stays unresolvable afterwards.
    pub fn remove(&mut self, id: Id<T>) -> Option<T> {
        let value = self.slots.get_mut(id.index as usize)?.take()?;
        self.len -= 1;
        Some(value)
    }

    /// Live values with their ids, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|v| (Id::from_raw(i as u32), v)))
    }

    /// Live ids, in slot order.
    pub fn ids(&self) -> impl Iterator<Item = Id<T>> + '_ {
        self.iter().map(|(id, _)| id)
    }
}

impl<T> core::ops::Index<Id<T>> for Arena<T> {
    type Output = T;

    /// Panics on a stale id; use [`Arena::get`] when the id may be stale.
    fn index(&self, id: Id<T>) -> &T {
        self.get(id).expect("stale id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_ids_do_not_alias() {
        let mut a = Arena::new();
        let x = a.insert(1);
        assert_eq!(a.remove(x), Some(1));
        let y = a.insert(2);
        assert_ne!(x, y);
        assert_eq!(a.get(x), None);
        assert_eq!(a.get(y), Some(&2));
        assert_eq!(a.len(), 1);
    }
}
