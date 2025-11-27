//! A serializable AnyMap-like container keyed by `type_name::<T>()`.
//!
//! - Stored map: `HashMap<String, Entry>`
//! - `Entry` stores a `serde_value::Value` (serialized form) and an optional cached `Box<dyn Any>`.
//! - On `insert`, values are serialized and cached; on Serialize, only the serialized `Value` is saved.
//! - On Deserialize, we only reconstruct the `Value` and leave the cache empty.
//!
//! This aims to be a drop-in replacement for basic `anymap3` use-cases and implements
//! an `entry()` API similar to standard maps.

use serde::{Deserialize, Serialize};
use serde_value::{to_value, Value};
use std::{any::Any, collections::HashMap};
use std::collections::hash_map;
use serde::de::Error;

/// The serializable `SerializableAnyMap` container.
#[derive(Default, Serialize, Deserialize, Debug, Clone)]
#[serde(transparent)]
pub struct SerializableAnyMap {
    raw: HashMap<String, SerializableAnyMapEntry>,
}


impl SerializableAnyMap {
    /// Create a new empty `SerializableAnyMap`.
    #[inline]
    pub fn new() -> Self {
        Self { raw: HashMap::new() }
    }

    /// Create a new `SerializableAnyMap` with the specified capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { raw: HashMap::with_capacity(capacity) }
    }

    /// Returns the number of elements the map can hold without reallocating.
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
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.raw.reserve(additional);
    }

    /// Shrinks the capacity of the collection as much as possible. It will drop
    /// down as much as possible while maintaining the internal rules
    /// and possibly leaving some space in accordance with the resize policy.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.raw.shrink_to_fit()
    }

    // Additional stable methods (as of 1.60.0-nightly) that could be added:
    // try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError>    (1.57.0)
    // shrink_to(&mut self, min_capacity: usize)                                   (1.56.0)

    /// Returns the number of items in the collection.
    #[inline]
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// Returns true if there are no items in the collection.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Removes all items from the collection. Keeps the allocated memory for reuse.
    #[inline]
    pub fn clear(&mut self) {
        self.raw.clear()
    }

    /// Get an immutable reference to a cached value of type `T` if it exists AND was deserialized.
    ///
    /// # Notes
    ///
    /// This does NOT lazily deserialize, use [`Self::get`] if you want lazy deserialization.
    /// This may return None even if the type is present in the map, but not deserialized.
    ///
    #[inline]
    pub fn try_get<T>(&self) -> Option<&T>
    where
      T: for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        self.raw.get(&key)?.try_get()
    }

    /// Get an immutable reference to a value of type `T`, lazily deserializing if necessary.
    ///
    /// # Notes
    ///
    /// This requires a `&mut self` since it may need to deserialize and modify the entry.
    #[inline]
    pub fn get<T>(&mut self) -> Option<Result<&T, serde_value::DeserializerError>>
    where
      T: for<'de> Deserialize<'de> + Any + 'static,
    {
        Self::get_mut(self).map(|r| r.map(|v| &*v))
    }

    /// Gets a copy value of type `T`, by deserializing the value in the map.
    ///
    /// # Warning
    ///
    /// This always runs deserialization, and may return an instance that is not equivalent to the one
    /// in the map if any fields are marked `#[serde(skip)]` for example.
    ///
    #[inline]
    pub fn get_deserialized_copy<T>(&self) -> Option<T>
    where
      T: for<'de> Deserialize<'de> + Any + 'static,
    {
        self.get_serialized_value::<T>().map(|v| T::deserialize(v.clone()).ok()).flatten()
    }


    /// Get a mutable reference to a value of type `T`, lazily deserializing into the cache if necessary.
    pub fn get_mut<T>(&mut self) -> Option<Result<&mut T, serde_value::DeserializerError>>
    where
      T: for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        Some(self.raw.get_mut(&key)?.get_mut())
    }

    /// Get the serialized `serde_value::Value` for type `T` if present.
    #[inline]
    pub fn get_serialized_value<T>(&self) -> Option<&Value>
    where
      T: 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        self.raw.get(&key).map(|e| &e.serialized)
    }

    /// Insert by type name key.
    ///
    /// This is lower-level and useful if you already have a Value.
    ///
    /// # Notes
    /// Marked unsafe, since we cannot verify that the provided Value matches the type name.
    #[inline]
    pub unsafe fn insert_value_by_name(&mut self, type_name: String, value: Value) -> Option<Value> {
        let e = SerializableAnyMapEntry { serialized: value, value: None };
        self.raw.insert(type_name, e).map(|e| e.serialized)
    }

    /// Insert a value of type `T`. Returns the previous value of that type if present.
    #[inline]
    pub fn insert<T>(&mut self, value: T) -> Option<Result<T, serde_value::DeserializerError>>
    where
      T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        let serialized = to_value(&value).expect("serialization failed");

        let new_entry = SerializableAnyMapEntry { serialized, value: Some(Box::new(value)) };
        self.raw.insert(key, new_entry).map(|old| old.into_inner::<T>())
    }


    /// Tries to insert a value into the map, and returns
    /// a mutable reference to the value if successful.
    ///
    /// If the map already had this type of value present, nothing is updated, and
    /// an error containing the occupied entry and the value is returned.
    pub fn try_insert<T>(&mut self, value: T) -> Result<&mut T, OccupiedError<'_, T>> where
        T: Serialize + for<'de> Deserialize<'de> + Any + 'static
    {
        match self.entry::<T>() {
            Entry::Occupied(entry) => Err(OccupiedError { entry, value }),
            Entry::Vacant(entry) => Ok(entry.insert(value)),
        }
    }

    /// Remove the stored value of type `T`, returning it if was present, or None if it was not.
    #[inline]
    pub fn remove<T>(&mut self) -> Option<T>
    where
      T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        self.raw.remove(&key).and_then(|entry| entry.into_inner::<T>().ok())
    }

    /// Returns true if a value of type `T` exists in the map (regardless whether already deserialized or not).
    #[inline]
    pub fn contains<T>(&self) -> bool
    where
      T: 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        self.raw.contains_key(&key)
    }

    /// Returns true if a value of type `T` exists in the map and has already been deserialized
    #[inline]
    pub fn contains_deserialized<T>(&self) -> bool
    where
      T: 'static,
    {
        let key = std::any::type_name::<T>().to_string();
        self.raw.contains_key(&key)
    }

    /// Entry API similar to `HashMap::entry` / `anymap3::entry::<T>()`.
    #[inline]
    pub fn entry<T>(&mut self) -> Entry<'_, T>
    where
      T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
    {
        let key = std::any::type_name::<T>().to_string();

        match self.raw.entry(key) {
            hash_map::Entry::Occupied(inner) => {
                Entry::Occupied(OccupiedEntry { inner, _marker: std::marker::PhantomData })
            },
            hash_map::Entry::Vacant(inner) => {
                Entry::Vacant(VacantEntry { inner, _marker: std::marker::PhantomData })
            },
        }
    }

    /// Return an iterator over type-name keys.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.raw.keys()
    }

    /// Get access to the raw hash map that backs this.
    ///
    /// This will seldom be useful, but it’s conceivable that you could wish to iterate
    /// over all the items in the collection, and this lets you do that.
    #[inline]
    pub fn as_raw(&self) -> &HashMap<String, SerializableAnyMapEntry> {
        &self.raw
    }

    /// Get mutable access to the raw hash map that backs this.
    ///
    /// This will seldom be useful, but it’s conceivable that you could wish to iterate
    /// over all the items in the collection mutably, or drain or something, or *possibly*
    /// even batch insert, and this lets you do that.
    ///
    /// # Safety
    ///
    /// If you insert any values to the raw map, the key (a `String`) must match the
    /// value’s type name as returned by any::type_name(), or *undefined behaviour* will occur when you access those values.
    ///
    /// (*Removing* entries is perfectly safe.)
    #[inline]
    pub unsafe fn as_raw_mut(&mut self) -> &mut HashMap<String, SerializableAnyMapEntry> {
        &mut self.raw
    }

    /// Convert this into the raw hash map that backs this.
    ///
    /// This will seldom be useful, but it’s conceivable that you could wish to consume all
    /// the items in the collection and do *something* with some or all of them, and this
    /// lets you do that, without the `unsafe` that `.as_raw_mut().drain()` would require.
    #[inline]
    pub fn into_raw(self) -> HashMap<String, SerializableAnyMapEntry> {
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
    /// # Safety
    ///
    /// For all entries in the raw map, the key (a `String`) must match the value’s type as returned by any::type_name(),
    /// or *undefined behaviour* will occur when you access that entry.
    #[inline]
    pub unsafe fn from_raw(raw: HashMap<String, SerializableAnyMapEntry>) -> SerializableAnyMap {
        Self { raw }
    }
}

impl<A: ?Sized + Any + Serialize + for <'de>Deserialize<'de>> Extend<Box<A>> for SerializableAnyMap {
    #[inline]
    fn extend<T: IntoIterator<Item = Box<A>>>(&mut self, iter: T) {
        for item in iter {
            let serialized = to_value(&*item).expect("serialization failed");
            let _ = self.raw.insert(std::any::type_name::<T>().to_string(), SerializableAnyMapEntry { serialized, value: Some(item) });
        }
    }
}

/// An entry in the `SerializableAnyMap`, which contains the serialized value and an optional deserialized value.
#[derive(Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct SerializableAnyMapEntry {
    serialized: Value,

    /// value runtime representation; skipped during (de)serialization.
    #[serde(skip)]
    #[serde(default)]
    value: Option<Box<dyn Any>>,
}

impl Clone for SerializableAnyMapEntry {
    fn clone(&self) -> Self {
        SerializableAnyMapEntry {
            serialized: self.serialized.clone(),
            value: None, // do not clone cached value
        }
    }
}

impl SerializableAnyMapEntry {
    pub fn try_get<T: 'static>(&self) -> Option<&T>
    where
      T: for<'de> Deserialize<'de>,
    {
        self.value.as_ref()?.downcast_ref::<T>()
    }

    pub fn get<T>(&mut self) -> Result<&T, serde_value::DeserializerError>
    where
      T: for<'de> Deserialize<'de> + 'static,
    {
        self.get_mut().map(|x| &*x)
    }

    pub fn get_mut<T>(&mut self) -> Result<&mut T, serde_value::DeserializerError>
    where
      T: for<'de> Deserialize<'de> + 'static,
    {
        if self.value.is_none() {
            let deserialized = T::deserialize(self.serialized.clone())?;
            self.value = Some(Box::new(deserialized));
        }

        match self.value {
            Some(ref mut v) => v.downcast_mut::<T>().ok_or(serde_value::DeserializerError::custom("wrong type in serialization")),
            None => unreachable!(),
        }
    }


    /// Attempt to extract the inner value by deserializing if necessary.
    pub fn into_inner<T: 'static>(mut self) -> Result<T, serde_value::DeserializerError>
    where
      T: for<'de> Deserialize<'de>,
    {
        self.value
          .take()
          .map(|a| Ok(*a.downcast::<T>().unwrap()))
          .unwrap_or_else(|| T::deserialize(self.serialized))
    }
}

// ===================== Entry API types =====================

/// A view into a single location in an [`SerializableAnyMap`], which may be vacant or occupied.
pub enum Entry<'a, T> {
    Occupied(OccupiedEntry<'a, T>),
    Vacant(VacantEntry<'a, T>),
}

/// A view into a single occupied location in an [`SerializableAnyMap`].
pub struct OccupiedEntry<'a, T> {
    inner: hash_map::OccupiedEntry<'a, String, SerializableAnyMapEntry>,
    _marker: std::marker::PhantomData<T>,
}

/// A view into a single empty location in an [`SerializableAnyMap`].
pub struct VacantEntry<'a, T> {
    inner: hash_map::VacantEntry<'a, String, SerializableAnyMapEntry>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> Entry<'a, T>
where
  T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
{
    /// Ensures a value is in the entry by inserting the default if empty, and returns
    /// a mutable reference to the value in the entry.
    #[inline]
    pub fn or_insert(self, default: T) -> Result<&'a mut T, serde_value::DeserializerError> {
        match self {
            Entry::Occupied(inner) => inner.into_mut(),
            Entry::Vacant(inner) => Ok(inner.insert(default)),
        }
    }

    /// Ensures a value is in the entry by inserting the result of the default function if
    /// empty, and returns a mutable reference to the value in the entry.
    #[inline]
    pub fn or_insert_with<F: FnOnce() -> T>(self, default: F) -> Result<&'a mut T, serde_value::DeserializerError> {
        match self {
            Entry::Occupied(inner) => inner.into_mut(),
            Entry::Vacant(inner) => Ok(inner.insert(default())),
        }
    }

    /// Ensures a value is in the entry by inserting the default value if empty,
    /// and returns a mutable reference to the value in the entry.
    #[inline]
    pub fn or_default(self) -> Result<&'a mut T, serde_value::DeserializerError> where T: Default {
        match self {
            Entry::Occupied(inner) => inner.into_mut(),
            Entry::Vacant(inner) => Ok(inner.insert(Default::default())),
        }
    }

    /// Provides in-place mutable access to an occupied entry before any potential inserts
    /// into the map.
    #[inline]
    pub fn and_modify<F: FnOnce(&mut T)>(self, f: F) -> Self {
        match self {
            Entry::Occupied(mut inner) => {
                let _ = inner.get_mut().map(f);
                Entry::Occupied(inner)
            },
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
    pub fn get_mut(&mut self) -> Result<&mut T, serde_value::DeserializerError> {
        unsafe { self.inner.get_mut().get_mut() }
    }

    /// Remove and return the stored value (deserialized) if possible.
    #[inline]
    pub fn remove(self) -> Result<T, serde_value::DeserializerError> {
        self.inner.remove().into_inner()
    }


    /// Converts the OccupiedEntry into a mutable reference to the value in the entry
    /// with a lifetime bound to the collection itself
    #[inline]
    pub fn into_mut(self) -> Result<&'a mut T, serde_value::DeserializerError> {
        self.inner.into_mut().get_mut()
    }

    /// Sets the value of the entry, and returns the entry's old value
    #[inline]
    pub fn insert(&mut self, value: T) -> Result<T, serde_value::DeserializerError> {
        let serialized = to_value(&value).expect("serialization failed");
        self.inner.insert(SerializableAnyMapEntry { serialized, value: Some(Box::new(value)) }).into_inner()
    }
}



impl<'a, T> VacantEntry<'a, T>
where
  T: Serialize + for<'de> Deserialize<'de> + Any + 'static,
{
    /// Insert the value and return a mutable reference to it.
    pub fn insert(self, value: T) -> &'a mut T {
        let serialized = to_value(&value).expect("serialization failed");
        let entry = SerializableAnyMapEntry { serialized, value: Some(Box::new(value)) };
        self.inner.insert(entry).value.as_mut().unwrap().downcast_mut().expect("wrong type in serialization")
    }

    /// Insert the provided serialized `Value` under this type-name key.
    pub unsafe fn insert_serialized(self, value: Value) {
        self.inner.insert(SerializableAnyMapEntry { serialized: value, value: None });
    }
}


pub struct OccupiedError<'a, V: 'a> {
    /// The entry in the map that was already occupied.
    pub entry: OccupiedEntry<'a, V>,
    /// The value which was not inserted, because the entry was already occupied.
    pub value: V,
}
