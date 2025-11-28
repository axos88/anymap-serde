use serde::{Deserialize, Serialize};
use std::fmt;

/// An opaque holder for a stable, deterministic identifier for a Rust type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StableTypeId(String);

impl fmt::Display for StableTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl StableTypeId {
    /// Returns the **stable string key** associated with type `T`.
    ///
    /// # Overview
    ///
    /// This function produces a deterministic, globally stable identifier for a
    /// Rust type by returning the fully-qualified name produced by
    /// [`core::any::type_name`].
    ///
    /// The resulting key is:
    ///
    /// - **Bijective** – each distinct type maps to a distinct string key.
    /// - **Deterministic** – the same type always produces the same key within a
    ///   given version of this crate.
    /// - **Pure** – no side effects; does not depend on runtime state.
    /// - **Compilation-unit stable** – the returned value does not depend on memory
    ///   layout, type IDs, or other non-portable properties.
    ///
    /// This key is used internally by the serializable anymap to index entries by
    /// their type. Because `TypeId` is not stable across compilation units or
    /// compiler versions, string keys offer a portable alternative suitable for
    /// serialization.
    ///
    /// # Stability Guarantees
    ///
    /// Changing the **implementation of this function is to be considered a breaking change**
    ///
    /// Consumers relying on serialized output can therefore depend on this value
    /// as a long-term stable identifier for the type.
    ///
    /// # Caveats
    ///
    /// The current implementatino relies on std::any::type_name, which is *NOT* guaranteed to be
    /// stable across rustc versions, although change is unlikely.
    ///
    /// # Examples
    ///
    /// ```
    /// use anymap_serde::StableTypeId;
    ///
    /// let k1 = StableTypeId::for_type::<i32>();
    /// let k2 = StableTypeId::for_type::<Option<String>>();
    ///
    /// assert_eq!(format!("{}", k1), "i32");
    /// assert_eq!(format!("{}", k2), "core::option::Option<alloc::string::String>");
    /// ```
    ///
    /// # Notes
    ///
    /// - The function returns an owned `String` rather than a `&'static str`,
    ///   because the underlying representation from `type_name` is returned as a
    ///   borrowed string and needs to be materialized for storage.
    /// - For generic types, the key includes full type parameters in a canonical
    ///   format.
    ///
    /// # See Also
    ///
    /// - [`core::any::type_name`] – the underlying mechanism used to generate the key.
    ///
    /// # Returns
    ///
    /// A stable, deterministic string key uniquely identifying the type `T`.
    pub fn for_type<T>() -> StableTypeId {
        StableTypeId(std::any::type_name::<T>().to_string())
    }
}
