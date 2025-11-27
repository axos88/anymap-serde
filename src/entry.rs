use crate::Wrapper;
use crate::write_guard::WriteGuard;
use serde::{Deserialize, Serialize};
use serde_value::{Value, to_value};
use std::any::Any;
use std::collections::hash_map;

/// A view into a single location in the map. Mirrors the `HashMap::entry` API so
/// code written against `anymap3::entry::<T>()` can be adapted easily.
///
/// Common patterns:
/// - `map.entry::<T>().or_insert(default)` — insert default and return a mutable ref.
/// - `map.entry::<T>().and_modify(|v| { ... })` — modify if present.
///
/// The generic parameter `T` bounds require `Serialize + Deserialize<'de> + Any + 'static`.
pub enum Entry<'a, T> {
    /// An occupied entry for a type known to exist.
    Occupied(OccupiedEntry<'a, T>),
    /// A vacant entry for a type that is not present.
    Vacant(VacantEntry<'a, T>),
}

/// Occupied entry for a key known to exist.
///
/// Provides:
/// - `try_get` — get cached value if already deserialized (no lazy deserialization).
/// - `get` / `get_mut` — lazily deserialize into the cache and return references.
/// - `insert` — replace the stored value (returns the old value deserialized).
/// - `remove` — remove and return the deserialized value.
///
/// Note: `get_mut` and `into_mut` return results because deserialization can fail.
pub struct OccupiedEntry<'a, T> {
    inner: hash_map::OccupiedEntry<'a, String, Wrapper>,
    _marker: std::marker::PhantomData<T>,
}

/// Vacant entry for a type that is not present.
///
/// Methods:
/// - `insert(value)` — serialize & cache `value`, return a mutable reference to it.
/// - `insert_serialized(value)` (unsafe) — insert a pre-serialized `serde_value::Value`
///   without a cached runtime value. The caller must ensure the `Value` matches `T`.
pub struct VacantEntry<'a, T> {
    inner: hash_map::VacantEntry<'a, String, Wrapper>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> Entry<'a, T>
where
    T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
{
    /// Ensures a value is in the entry by inserting the default if empty, and returns
    /// a mutable reference to the value in the entry.
    #[inline]
    pub fn or_insert(
        self,
        default: T,
    ) -> Result<WriteGuard<'a, T>, serde_value::DeserializerError> {
        match self {
            Entry::Occupied(inner) => inner.into_mut(),
            Entry::Vacant(inner) => Ok(inner.insert(default)),
        }
    }

    /// Ensures a value is in the entry by inserting the result of the default function if
    /// empty, and returns a mutable reference to the value in the entry.
    #[inline]
    pub fn or_insert_with<F: FnOnce() -> T>(
        self,
        default: F,
    ) -> Result<WriteGuard<'a, T>, serde_value::DeserializerError> {
        match self {
            Entry::Occupied(inner) => inner.into_mut(),
            Entry::Vacant(inner) => Ok(inner.insert(default())),
        }
    }

    /// Ensures a value is in the entry by inserting the default value if empty,
    /// and returns a mutable reference to the value in the entry.
    #[inline]
    pub fn or_default(self) -> Result<WriteGuard<'a, T>, serde_value::DeserializerError>
    where
        T: Default,
    {
        match self {
            Entry::Occupied(inner) => inner.into_mut(),
            Entry::Vacant(inner) => Ok(inner.insert(Default::default())),
        }
    }

    /// Provides in-place mutable access to an occupied entry before any potential inserts
    /// into the map.
    #[inline]
    pub fn and_modify<F: FnOnce(WriteGuard<T>)>(self, f: F) -> Self {
        match self {
            Entry::Occupied(mut inner) => {
                let _ = inner.get_mut().map(f);
                Entry::Occupied(inner)
            }
            Entry::Vacant(inner) => Entry::Vacant(inner),
        }
    }

    /// Returns a reference to the occupied entry.
    pub fn into_occupied(self) -> Option<OccupiedEntry<'a, T>> {
        match self {
            Entry::Occupied(e) => Some(e),
            Entry::Vacant(_) => None,
        }
    }

    /// Returns a reference to the vacant entry.
    pub fn into_vacant(self) -> Option<VacantEntry<'a, T>> {
        match self {
            Entry::Vacant(e) => Some(e),
            Entry::Occupied(_) => None,
        }
    }
}

impl<'a, T> OccupiedEntry<'a, T>
where
    T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
{
    pub(crate) fn new(inner: hash_map::OccupiedEntry<'a, String, Wrapper>) -> Self {
        OccupiedEntry {
            inner,
            _marker: std::marker::PhantomData,
        }
    }

    /// Get immutable reference if cached. Does not attempt to deserialize.
    pub fn try_get(&self) -> Option<&T> {
        self.inner.get().try_get::<T>()
    }

    /// Gets a reference to the value in the entry
    #[inline]
    pub fn get(&mut self) -> Result<&T, serde_value::DeserializerError> {
        self.inner.get_mut().get()
    }

    /// Gets a mutable reference to the value in the entry
    #[inline]
    pub fn get_mut<'b>(&'b mut self) -> Result<WriteGuard<'b, T>, serde_value::DeserializerError> {
        self.inner.get_mut().get_mut()
    }

    /// Remove and return the stored value (deserialized) if possible.
    #[inline]
    pub fn remove(self) -> Result<T, serde_value::DeserializerError> {
        self.inner.remove().into_inner()
    }

    /// Converts the OccupiedEntry into a mutable reference to the value in the entry
    /// with a lifetime bound to the collection itself
    #[inline]
    pub fn into_mut(self) -> Result<WriteGuard<'a, T>, serde_value::DeserializerError> {
        self.inner.into_mut().get_mut()
    }

    /// Sets the value of the entry, and returns the entry's old value
    #[inline]
    pub fn insert(&mut self, value: T) -> Result<T, serde_value::DeserializerError> {
        let serialized = to_value(&value).expect("serialization failed");
        self.inner
            .insert(Wrapper {
                serialized,
                value: Some(Box::new(value)),
            })
            .into_inner()
    }
}

impl<'a, T> VacantEntry<'a, T>
where
    T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
{
    pub(crate) fn new(inner: hash_map::VacantEntry<'a, String, Wrapper>) -> Self {
        VacantEntry {
            inner,
            _marker: std::marker::PhantomData,
        }
    }

    /// Insert the value and return a mutable reference to it.
    pub fn insert(self, value: T) -> WriteGuard<'a, T> {
        let serialized = to_value(&value).expect("serialization failed");
        let entry = Wrapper {
            serialized,
            value: Some(Box::new(value)),
        };

        self.inner.insert(entry);

        todo!();
    }

    /// Insert the provided serialized `Value` under this type-name key.
    pub unsafe fn insert_serialized(self, value: Value) {
        self.inner.insert(Wrapper {
            serialized: value,
            value: None,
        });
    }
}

/// Error returned by `try_insert` when an entry is already occupied.
///
/// Contains:
/// - `entry`: the `OccupiedEntry` for the type.
/// - `value`: the value which was not inserted because the entry was occupied.
pub struct OccupiedError<'a, V: 'a> {
    /// The entry in the map that was already occupied.
    pub entry: OccupiedEntry<'a, V>,
    /// The value which was not inserted, because the entry was already occupied.
    pub value: V,
}
