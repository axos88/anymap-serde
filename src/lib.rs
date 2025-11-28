//! A serializable AnyMap-like container keyed by `type_name::<T>()`.
//! A serializable AnyMap-like container keyed by `type_name::<T>()`.
//!
//! This crate provides `SerializableAnyMap`, an AnyMap-compatible container that
//! stores values in a serializable form (`serde_value::Value`) and optionally
//! caches a deserialized `Box<dyn Any>` for fast access. Keys are the stable
//! string returned by `StableTypeId::for_type::<T>()`, not `TypeId`, which makes
//! the map suitable for sending over the network and reconstructing on another
//! process / compilation unit.
//!
//! Key properties:
//! - The on-disk / on-wire representation is a `HashMap<String, serde_value::Value>`.
//! - Deserialized values are stored in-memory only and are not serialized.
//! - Inserting a value serializes it immediately and stores the value in both forms.
//! - Deserialization reconstructs only the serialized representation; the cache is empty
//!   until a lazy access triggers deserialization, since we don't have access to type information
//!   at collection deserialization time
//!
//! # Motivation
//!
//! This is useful for extension holders (for example request extensions) where
//! heterogeneous typed data must be transported and reconstructed remotely.
//!
//! Example (illustrative):
//! ```rust
//! # use serde::{Serialize, Deserialize};
//! # use serde_json;
//! # use anymap_serde::SerializableAnyMap;
//! #[derive(Serialize, Deserialize, Debug, PartialEq)]
//! struct Ext { a: u32 }
//!
//! let mut map = SerializableAnyMap::new();
//! map.insert(Ext { a: 10 });
//!
//! // Serialize to JSON
//! let json = serde_json::to_string(&map).unwrap();
//!
//! // On the other side, deserialize and access the stored type by `type_name::<T>()`
//! let mut map2: SerializableAnyMap = serde_json::from_str(&json).unwrap();
//! let value: &Ext = map2.get::<Ext>().unwrap().unwrap();
//! assert_eq!(value.a, 10);
//! ```

#![deny(missing_docs, unused, warnings)]

mod entry;
mod stable_type_id;
mod write_guard;

use crate::entry::{Entry, OccupiedEntry, OccupiedError, VacantEntry};
use crate::write_guard::WriteGuard;
use serde::{Deserialize, Serialize};
use serde_value::{Value, to_value};
pub use stable_type_id::StableTypeId;
use std::collections::{TryReserveError, hash_map};
use std::marker::PhantomData;
use std::{any::Any, collections::HashMap};

/// A serializable heterogeneous-map keyed by a stable value derived from the type.
///
/// `SerializableAnyMap` behaves like `anymap3` for most basic use-cases but
/// stores values in serialized form so the entire map can be serialized and
/// sent across process boundaries. The map implements `Serialize` and
/// `Deserialize` where only the serialized form is persisted; the cached
/// deserialized values are not persisted.
///
/// Inserted types must implement `Serialize + Deserialize<'de> + 'static`.
///
/// See method docs for usage patterns (`insert`, `get`, `get_mut`, `entry`, …).
///
/// Safety notes:
/// - Some escape hatches (`as_raw_mut`, `from_raw`) are `unsafe` — the caller
///   must ensure the key string matches the type stored under it.
///
/// A stored entry containing the serialized representation and an optional
/// cached runtime value.
///
#[derive(Default, Serialize, Deserialize, Debug, Clone)]
#[serde(transparent)]
pub struct SerializableAnyMap {
    raw: HashMap<StableTypeId, RawItem>,
}

impl SerializableAnyMap {
    /// Create a new empty `SerializableAnyMap`.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::new();
    /// map.insert(42u32);
    /// assert_eq!(map.get::<u32>().unwrap().unwrap(), &42u32);
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self {
            raw: HashMap::new(),
        }
    }

    /// Creates a new empty map with preallocated space for `capacity` elements.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::with_capacity(8);
    /// assert!(map.capacity() >= 8);
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// assert_eq!(map.len(), 3);
    /// assert!(map.capacity() >= 8);
    /// ```
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            raw: HashMap::with_capacity(capacity),
        }
    }

    /// Returns the number of elements the map can hold without reallocating.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::new();
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// assert_eq!(map.len(), 3);
    /// assert!(map.capacity() >= 3);
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.raw.capacity()
    }

    /// Reserves capacity for at least `additional` more elements to be inserted.
    /// The collection may reserve more space to avoid frequent reallocations.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::new();
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// map.reserve(5);
    /// assert!(map.capacity() >= 8);
    ///
    /// ```
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.raw.reserve(additional);
    }

    /// Tries to reserve capacity for at least additional more elements to be inserted in the HashMap.
    /// The collection may reserve more space to speculatively avoid frequent reallocations.
    /// After calling try_reserve, capacity will be greater than or equal to self.len() + additional
    /// if it returns Ok(()). Does nothing if capacity is already sufficient.
    ///
    /// # Errors
    ///
    /// If the capacity overflows, or the allocator reports a failure, then an error is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::new();
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// map.try_reserve(5).expect("not to fail");
    /// assert!(map.capacity() >= 8);
    ///
    /// ```
    #[inline]
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.raw.try_reserve(additional)
    }

    /// Shrinks the capacity of the collection as much as possible. It will drop
    /// down as much as possible while maintaining the internal rules
    /// and possibly leaving some space in accordance with the resize policy.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::with_capacity(128);
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// map.shrink_to_fit();
    /// assert_eq!(map.len(), 3);
    /// assert!(map.capacity() < 128);
    ///
    /// ```
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.raw.shrink_to_fit()
    }

    /// Shrinks the capacity of the map with a lower limit. It will drop down no lower than the
    /// supplied limit while maintaining the internal rules and possibly leaving some space in accordance with the resize policy.
    /// If the current capacity is less than the lower limit, this is a no-op.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::with_capacity(128);
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// map.shrink_to(64);
    /// assert_eq!(map.len(), 3);
    /// assert!(map.capacity() < 128 );
    /// assert!(map.capacity() >= 64 );
    ///
    /// ```
    #[inline]
    pub fn shrink_to(&mut self, min_capcity: usize) {
        self.raw.shrink_to(min_capcity)
    }

    /// Returns the number of elements in the map. Note that this may not be the exact number of
    /// retrievable items, if deserialization of some items fail.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::with_capacity(128);
    /// map.insert(42u32);
    /// map.insert(42u16);
    /// map.insert(42u8);
    /// assert_eq!(map.len(), 3);
    ///
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// Returns true if there are no items in the collection.
    /// Returns the number of elements in the map. Note that this may not be the exact number of
    /// retrievable items, if deserialization of some items fail.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::with_capacity(128);
    /// assert_eq!(map.is_empty(), true);
    /// map.insert(42u16);
    /// assert_eq!(map.is_empty(), false);
    ///
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Removes all items from the collection. Keeps the allocated memory for reuse.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map = SerializableAnyMap::with_capacity(128);
    /// map.insert(42u32);
    /// map.clear();
    /// assert_eq!(map.is_empty(), true)
    ///
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.raw.clear()
    }

    /// Get an immutable reference to a cached value of type `T` if it exists AND was deserialized.
    ///
    /// # Notes
    ///
    /// This does NOT lazily deserialize, use [`Self::get`] if you want lazy deserialization.
    /// This may return None even if the type is present in the map, but not deserialized yet, which
    /// may happen if the map was deserialized from a serialized form and the type has not yet been accessed.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    /// map.insert(42u16);
    /// assert_eq!(map.try_get::<u32>(), None); // not deserialized yet
    /// assert_eq!(map.try_get::<u16>(), Some(&42u16));
    /// assert_eq!(map.get::<u32>().unwrap().unwrap(), &42u32); // deserialization happens now
    /// assert_eq!(map.try_get::<u32>(), Some(&42u32)); // can now be access via try_get as well.
    /// ```
    ///
    #[inline]
    pub fn try_get<T>(&self) -> Option<&T>
    where
        T: for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = StableTypeId::for_type::<T>();
        self.raw.get(&key)?.try_get()
    }

    /// Get an immutable reference to a value of type `T`, lazily deserializing if necessary.
    ///
    /// # Notes
    ///
    /// This requires a `&mut self` since it may need to deserialize and modify the entry.
    /// If the item comes from deserializing the map, this may fail if the deserialization of the item fails.
    /// In such case the item is removed from subsequent accesses.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom" })).unwrap();
    /// map.insert(42u16);
    /// assert_eq!(map.len(), 2);
    /// assert!(matches!(map.get::<u32>(), Some(Err(_)))); // deserialization failed, removed from the map.
    /// assert_eq!(map.len(), 1);
    /// assert!(matches!(map.get::<u32>(), None)); // not present in the map anymore
    /// assert_eq!(map.get::<u16>().unwrap().ok(), Some(&42u16)); // deserialization successful
    /// ```
    #[inline]
    pub fn get<T>(&mut self) -> Option<Result<&T, serde_value::DeserializerError>>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        match self.entry::<T>() {
            (Entry::Occupied(inner), None) => Some(inner.into_mut().map(|g| g.into_ref())),
            (Entry::Vacant(_), None) => None,
            (_, Some(e)) => Some(Err(e)),
        }
    }

    /// Gets a copy value of type `T`, by deserializing the value in the map. Note that this will
    /// *always* return a copy of the item contained in the map, and may lose infomration that is `#[serde(skip)]`.
    ///
    /// # Example
    ///
    /// ```
    /// use serde::{Deserialize, Serialize};
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::from_value;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// map.insert(42u32);
    /// assert_eq!(map.get_deserialized_copy::<u32>(), Some(42u32));
    ///
    /// #[derive(Debug, PartialEq, Serialize, Deserialize)]
    /// struct Foo { bar: u32, #[serde(skip)] baz: u32 }
    /// map.insert(Foo { bar: 42, baz: 43 });
    /// assert_eq!(map.get_deserialized_copy::<Foo>().unwrap(), Foo { bar: 42, baz: 0}); // deserialization looses `baz` value
    /// ```
    #[inline]
    pub fn get_deserialized_copy<T>(&self) -> Option<T>
    where
        T: for<'de> Deserialize<'de> + Any + 'static,
    {
        self.get_serialized_value::<T>()
            .and_then(|v| T::deserialize(v.clone()).ok())
    }

    /// Get a mutable reference to a value of type `T`, lazily deserializing into the cache if necessary.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom" })).unwrap();
    /// map.insert(42u16);
    /// assert_eq!(map.len(), 2);
    /// assert!(matches!(map.get_mut::<u32>(), Some(Err(_)))); // deserialization failed, removed from the map.
    /// assert_eq!(map.len(), 1);
    /// assert!(matches!(map.get_mut::<u32>(), None)); // not present in the map anymore
    /// assert_eq!(*map.get_mut::<u16>().unwrap().unwrap(), 42u16); // deserialization successful.
    /// ```
    pub fn get_mut<T>(
        &mut self,
    ) -> Option<Result<WriteGuard<'_, T>, serde_value::DeserializerError>>
    where
        T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        match self.entry::<T>() {
            (Entry::Occupied(inner), None) => Some(inner.into_mut()),
            (Entry::Vacant(_), None) => None,
            (_, Some(e)) => Some(Err(e)),
        }
    }

    /// Get the serialized [`serde_value::Value`] representation for type `T`
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// map.insert(42u16);
    /// assert_eq!(map.get_serialized_value::<u16>().unwrap(), &serde_value::Value::U16(42));
    /// ```
    #[inline]
    pub fn get_serialized_value<T>(&self) -> Option<&Value>
    where
        T: 'static,
    {
        let key = StableTypeId::for_type::<T>();
        self.raw.get(&key).map(|e| &e.serialized)
    }

    /// Insert by type name key.
    ///
    /// This is lower-level and useful if you already have a Value, but do not want to defer
    /// deserialization to first access.
    ///
    /// # Notes
    /// The provided Value needs to match the given match the type name, otherwise the insert will
    /// appear to succeed, but will not return the expected value on access.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom", "u16": 42 })).unwrap();
    /// map.insert_only_serialized::<u32>(serde_value::to_value(json!("boom")).unwrap());
    /// map.insert_only_serialized::<u16>(serde_value::to_value(json!(42)).unwrap());
    ///
    /// assert_eq!(map.len(), 2);
    /// assert!(matches!(map.get_mut::<u32>(), Some(Err(_)))); // deserialization failed, removed from the map.
    /// assert_eq!(map.len(), 1);
    /// assert!(matches!(map.get_mut::<u32>(), None)); // not present in the map anymore
    /// assert_eq!(map.get_mut::<u16>().unwrap().unwrap().into_ref(), &42u16); // deserialization successful.
    /// ```
    #[inline]
    pub fn insert_only_serialized<T>(&mut self, value: Value) -> Option<Item<T>> {
        let key = StableTypeId::for_type::<T>();

        let e = RawItem {
            serialized: value,
            value: None,
        };
        self.raw.insert(key, e).map(RawItem::wrap)
    }

    /// Insert a value of type `T`. Returns the previous value of that type if present.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// assert!(map.insert::<u32>(1u32).is_none());
    /// assert_eq!(map.insert::<u32>(2u32).unwrap().get().ok(), Some(&1u32));
    /// ```
    #[inline]
    pub fn insert<T>(&mut self, value: T) -> Option<Item<T>>
    where
        T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = StableTypeId::for_type::<T>();
        let serialized = to_value(&value).expect("serialization failed");

        let new_entry = RawItem {
            serialized,
            value: Some(Box::new(value)),
        };
        self.raw.insert(key, new_entry).map(RawItem::wrap)
    }

    /// Tries to insert a value into the map, and returns
    /// a mutable reference to the value if successful.
    ///
    /// If the map already had this type of value present, nothing is updated, and
    /// an error containing the occupied entry and the value is returned.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// assert_eq!(map.try_insert::<u32>(1u32).map(|e| e.into_ref()).ok(), Some(&1u32));
    /// assert_eq!(map.try_insert::<u32>(1u32).is_err(), true);
    /// ```
    pub fn try_insert<'a, T>(
        &'a mut self,
        value: T,
    ) -> Result<WriteGuard<'a, T>, OccupiedError<'a, T>>
    where
        T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        match self.entry::<T>().0 {
            Entry::Occupied(entry) => Err(OccupiedError { entry, value }),
            Entry::Vacant(entry) => Ok(entry.insert(value)),
        }
    }

    /// Remove the stored value of type `T`, returning it if was present, or None if it was not.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// map.insert::<u32>(1u32);
    /// assert_eq!(map.remove::<u32>(), Some(1u32));
    /// assert_eq!(map.remove::<u32>(), None);
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom" })).unwrap();
    /// assert_eq!(map.remove::<u32>(), None); // Deserialization fails, so it is treated as if it is not present.
    ///
    /// ```
    #[inline]
    pub fn remove<T>(&mut self) -> Option<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = StableTypeId::for_type::<T>();
        self.raw
            .remove(&key)
            .and_then(|item| item.into_inner().ok())
    }

    /// Returns true if a value of type `T` exists in the map (regardless whether already deserialized or not).
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// assert!(!map.contains::<u32>());
    /// map.insert(1u32);
    /// assert!(map.contains::<u32>());
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom", "u16": 42 })).unwrap();
    /// assert!(map.contains::<u32>());
    /// assert!(map.contains::<u16>());
    ///
    /// ```
    #[inline]
    pub fn contains<T>(&self) -> bool
    where
        T: 'static,
    {
        let key = StableTypeId::for_type::<T>();
        self.raw.contains_key(&key)
    }

    /// Returns true if a value of type `T` exists in the map and has already been deserialized
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// assert!(!map.contains_deserialized::<u32>());
    /// map.insert(1u32);
    /// assert!(map.contains_deserialized::<u32>());
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom", "u16": 42 })).unwrap();
    /// assert!(!map.contains_deserialized::<u32>());
    /// assert!(!map.contains_deserialized::<u16>());
    /// map.get_mut::<u16>(); // deserialization happens here
    /// assert!(map.contains_deserialized::<u16>());
    ///
    /// ```
    #[inline]
    pub fn contains_deserialized<T>(&self) -> bool
    where
        T: 'static,
    {
        let key = StableTypeId::for_type::<T>();
        self.raw
            .get(&key)
            .map(|e| e.value.is_some())
            .unwrap_or(false)
    }

    /// Entry API similar to `HashMap::entry` / `anymap3::entry::<T>()`.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// map.insert(1u32);
    /// map.entry::<u32>().0.and_modify(|mut v| *v += 1);
    /// map.entry::<u16>().0.or_insert(42u16);
    /// *map.entry::<u8>().0.or_default().unwrap() += 1;
    ///
    /// assert_eq!(map.get::<u32>().unwrap().unwrap(), &2u32);
    /// assert_eq!(map.get::<u16>().unwrap().unwrap(), &42u16);
    /// assert_eq!(map.get::<u8>().unwrap().unwrap(), &1u8);
    /// ```
    #[inline]
    pub fn entry<'a, T>(&'a mut self) -> (Entry<'a, T>, Option<serde_value::DeserializerError>)
    where
        T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = StableTypeId::for_type::<T>();

        let err = match self.raw.entry(key.clone()) {
            hash_map::Entry::Occupied(mut inner) => inner.get_mut().get_mut::<T>().err(),
            _ => None,
        };

        if err.is_some() {
            self.raw.remove(&key);
        }

        match self.raw.entry(key) {
            hash_map::Entry::Occupied(inner) => (Entry::Occupied(OccupiedEntry::new(inner)), err),
            hash_map::Entry::Vacant(inner) => (Entry::Vacant(VacantEntry::new(inner)), err),
        }
    }

    /// Return an iterator over type-name keys.
    ///
    /// Probably not very useful since the keys are opaque `StableTypeId`s, but here it is anyway.
    /// If anyone has a valid use-case for this, please open an issue.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use anymap_serde::StableTypeId;
    ///
    /// let mut map: SerializableAnyMap = SerializableAnyMap::new();
    /// map.insert(1u32);
    ///
    /// assert_eq!(map.keys().next().unwrap(), &StableTypeId::for_type::<u32>());
    /// ```
    pub fn keys(&self) -> impl Iterator<Item = &StableTypeId> {
        self.raw.keys()
    }

    /// Get access to the raw hash map that backs this.
    ///
    /// This will seldom be useful, but it’s conceivable that you could wish to iterate
    /// over all the items in the collection, and this lets you do that.
    ///
    /// Provided to be on parity with anymap3
    #[inline]
    pub fn as_raw(&self) -> &HashMap<StableTypeId, RawItem> {
        &self.raw
    }

    /// Get mutable access to the raw hash map that backs this.
    ///
    /// This will seldom be useful, but it’s conceivable that you could wish to iterate
    /// over all the items in the collection mutably, or drain or something, or *possibly*
    /// even batch insert, and this lets you do that.
    ///
    /// Provided to be on parity with anymap3
    ///
    /// # Safety
    ///
    /// If you insert any deserialized values to the raw map, the key must match the
    /// value’s type name as returned by [`StableTypeId::for_type::<T>()`], or *undefined behaviour* will occur when you access those values.
    ///
    /// (*Removing* entries is perfectly safe.)
    /// (*Inserting only serialiazed values* is perfectly safe - but may disappear if they cannot be deserialized later on access)
    ///
    #[inline]
    pub unsafe fn as_raw_mut(&mut self) -> &mut HashMap<StableTypeId, RawItem> {
        &mut self.raw
    }

    /// Convert this into the raw hash map that backs this.
    ///
    /// This will seldom be useful, but it’s conceivable that you could wish to consume all
    /// the items in the collection and do *something* with some or all of them, and this
    /// lets you do that, without the `unsafe` that `.as_raw_mut().drain()` would require.
    ///
    /// Provided to be on parity with anymap3
    #[inline]
    pub fn into_raw(self) -> HashMap<StableTypeId, RawItem> {
        self.raw
    }

    /// Construct a map from a collection of raw values.
    ///
    /// You know what? I can’t immediately think of any legitimate use for this.
    ///
    /// Perhaps this will be most practical as `unsafe { SerializableAnyMap::from_raw(iter.collect()) }`,
    /// `iter` being an iterator over `(String, Box<dyn Any + Serialize + Deserialize + 'static>)` pairs.
    /// Eh, this method provides symmetry with `into_raw`, so I don’t care if literally no one ever uses it. I’m not
    /// even going to write a test for it, it’s so trivial.
    ///
    /// Provided to be on parity with anymap3
    ///
    /// # Safety
    ///
    /// For all entries in the raw map, the key (a `String`) must match the value’s type as returned by any::type_name(),
    /// or *undefined behaviour* will occur when you access that entry.
    #[inline]
    pub unsafe fn from_raw(raw: HashMap<StableTypeId, RawItem>) -> SerializableAnyMap {
        Self { raw }
    }
}

impl<A: Any + Serialize + for<'de> Deserialize<'de>> Extend<Box<A>> for SerializableAnyMap {
    #[inline]
    fn extend<T: IntoIterator<Item = Box<A>>>(&mut self, iter: T) {
        for item in iter {
            let serialized = to_value(&*item).expect("serialization failed");
            let _ = self.raw.insert(
                StableTypeId::for_type::<T>(),
                RawItem {
                    serialized,
                    value: Some(item),
                },
            );
        }
    }
}

/// A stored entry containing the serialized representation and an optional
/// cached runtime value.
///
/// - `serialized`: the `serde_value::Value` used for Serialize/Deserialize of the map.
/// - `value`: an optional `Box<dyn Any>` holding the deserialized value. This field is not
///   serialized and is cleared on `Clone` and `Deserialize`.
///
/// Semantics:
/// - On `insert`, both `serialized` and `value` are populated.
/// - On access (`get`/`get_mut`), the entry will lazily deserialize `serialized` into
///   `value` if the cache is empty.
/// - `into_inner` attempts to extract the concrete type from the cache or by deserializing.
///
/// Notes:
/// - The cloning, serialization and deserialization happens based on the serialized `serde_value::Value`, so
///   any modifications to fields that are marked `#[serde(skip)]` will not be lost after a serialization
///   round-trip, or after cloning the map.
/// - Currently any modifications to the cached `value` are not reflected back into the `serialized` form, so
///   serialization WILL reflect the original value only. This is a known limitation and is planned to be
///   addressed in a future version by returning a Guard object that will update the serialized value on Drop.
#[derive(Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct RawItem {
    serialized: Value,

    /// value runtime representation; skipped during (de)serialization.
    #[serde(skip)]
    #[serde(default)]
    value: Option<Box<dyn Any>>,
}

impl Clone for RawItem {
    fn clone(&self) -> Self {
        RawItem {
            serialized: self.serialized.clone(),
            value: None, // do not clone cached value
        }
    }
}

///TODO Docuemnt
pub struct Item<T>(RawItem, PhantomData<T>);

impl<T> Item<T> {
    /// Attempt to get an immutable reference to the deserialized value of type `T`. Will return None
    /// if the value has not yet been deserialized, as we need a mutable reference to mutate the
    /// entry to save the deserialized value.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u16": 42 })).unwrap();
    /// let mut old = map.insert(43u16).unwrap();
    ///
    /// assert_eq!(old.try_get(), None); // not deserialized yet
    /// old.get(); // Trigger deserialization
    /// assert_eq!(old.try_get(), Some(&42));
    /// ```
    ///
    pub fn try_get(&self) -> Option<&T>
    where
        T: for<'de> Deserialize<'de> + 'static,
    {
        self.0.value.as_ref()?.downcast_ref::<T>()
    }

    /// Attempt to get an immutable reference to the deserialized value of type `T`,
    /// lazily deserializing if necessary. We need a mutable reference in order to save the
    /// deserialized value to the entry
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    /// let mut old = map.insert(43u32).unwrap();
    ///
    /// assert_eq!(old.get().ok(), Some(&42));
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": "boom" })).unwrap();
    /// let mut old = map.insert(43u32).unwrap();
    ///
    /// assert_eq!(old.get().is_err(), true);
    /// assert_eq!(old.get().is_err(), true);
    /// ```
    pub fn get(&mut self) -> Result<&T, serde_value::DeserializerError>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        self.0.get_mut().map(|x: WriteGuard<T>| x.into_ref())
    }

    /// Attempt to an mutable reference to the deserialized value of type `T`,
    /// lazily deserializing if necessary.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    /// use std::ops::DerefMut;
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u16": 42 })).unwrap();
    ///
    /// let mut old = map.insert(43u16).unwrap();
    ///
    /// *old.get_mut().unwrap() += 100;
    ///
    /// assert_eq!(old.get_mut().unwrap().deref_mut(), &mut 142u16);
    /// ```
    pub fn get_mut<'a>(&'a mut self) -> Result<WriteGuard<'a, T>, serde_value::DeserializerError>
    where
        T: for<'de> Deserialize<'de> + Serialize + 'static,
    {
        if self.0.value.is_none() {
            let deserialized = T::deserialize(self.0.serialized.clone())?;
            self.0.value = Some(Box::new(deserialized));
        }

        Ok(WriteGuard::new(&mut self.0))
    }

    /// Attempt to extract the inner value by deserializing if necessary.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    /// let item = map.insert(41u32).unwrap();
    ///
    /// assert_eq!(item.into_inner().unwrap(), 42u32);
    /// ```
    pub fn into_inner(mut self) -> Result<T, serde_value::DeserializerError>
    where
        T: for<'de> Deserialize<'de> + 'static,
    {
        self.0
            .value
            .take()
            .map(|a| Ok(*a.downcast::<T>().unwrap()))
            .unwrap_or_else(|| T::deserialize(self.0.serialized))
    }
}

impl RawItem {
    /// Attempt to get an immutable reference to the deserialized value of type `T`. Will return None
    /// if the value has not yet been deserialized, as we need a mutable reference to mutate the
    /// entry to save the deserialized value.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    ///
    /// let mut item = map.insert(43u32).unwrap();
    ///
    /// assert_eq!(item.try_get(), None); // Not yet deserialized
    /// item.get(); // Trigger deserialization
    /// assert_eq!(item.try_get(), Some(&42u32));
    /// ```
    ///
    fn try_get<T>(&self) -> Option<&T>
    where
        T: for<'de> Deserialize<'de> + 'static,
    {
        self.value.as_ref()?.downcast_ref::<T>()
    }

    /// Attempt to get an immutable reference to the deserialized value of type `T`,
    /// lazily deserializing if necessary. We need a mutable reference in order to save the
    /// deserialized value to the entry
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    /// let mut item = map.insert(43u32).unwrap();
    ///
    /// assert_eq!(item.get().unwrap(), &42); // Deserializes value on demand
    /// ```
    fn get<T>(&mut self) -> Result<&T, serde_value::DeserializerError>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        self.get_mut().map(|x: WriteGuard<T>| x.into_ref())
    }

    /// Attempt to an mutable reference to the deserialized value of type `T`,
    /// lazily deserializing if necessary.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    ///
    /// let mut item = map.insert(43u32).unwrap();
    ///
    /// *item.get_mut().unwrap() += 100;
    ///
    /// assert_eq!(item.get().unwrap(), &mut 142); // Deserializes value on demand
    /// ```
    fn get_mut<'a, T>(&'a mut self) -> Result<WriteGuard<'a, T>, serde_value::DeserializerError>
    where
        T: for<'de> Deserialize<'de> + Serialize + 'static,
    {
        if self.value.is_none() {
            let deserialized = T::deserialize(self.serialized.clone())?;
            self.value = Some(Box::new(deserialized));
        }

        Ok(WriteGuard::new(self))
    }

    /// Attempt to extract the inner value by deserializing if necessary.
    ///
    /// # Example
    ///
    /// ```
    /// use anymap_serde::SerializableAnyMap;
    /// use serde_json::json;
    ///
    /// let mut map: SerializableAnyMap = serde_json::from_value(json!({ "u32": 42 })).unwrap();
    /// let item = map.insert(42u32).unwrap();
    ///
    /// assert_eq!(item.into_inner().unwrap(), 42);
    /// ```
    fn into_inner<T>(mut self) -> Result<T, serde_value::DeserializerError>
    where
        T: for<'de> Deserialize<'de> + 'static,
    {
        self.value
            .take()
            .map(|a| Ok(*a.downcast::<T>().unwrap()))
            .unwrap_or_else(|| T::deserialize(self.serialized))
    }

    fn wrap<T>(self) -> Item<T> {
        Item(self, PhantomData)
    }
}
