use crate::RawItem;
use serde::Serialize;
use serde_value::to_value;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

// RAII guard returned for mutable access. On Drop it re-serializes the concrete value
// back into the entry's `serialized` field.
pub struct WriteGuard<'a, T>
where
    T: Serialize + 'static,
{
    entry: Option<&'a mut RawItem>,
    ghost: PhantomData<T>,
}

impl<'a, T> WriteGuard<'a, T>
where
    T: Serialize + 'static,
{
    pub(crate) fn new(entry: &'a mut RawItem) -> Self {
        WriteGuard {
            entry: Some(entry),
            ghost: PhantomData,
        }
    }

    pub fn commit(&mut self) {
        // Re-serialize the potentially-mutated concrete value into serialized.
        if let Some(ref mut entry) = self.entry
            && let Some(ref boxed) = entry.value
        {
            let concrete_ref = boxed
                .downcast_ref::<T>()
                .expect("wrong type in serialization");
            entry.serialized = to_value(concrete_ref).expect("serialization failed");
        }
    }
    pub fn into_ref(mut self) -> &'a T {
        self.commit();
        self.entry
            .take()
            .unwrap()
            .value
            .as_ref()
            .unwrap()
            .downcast_ref()
            .unwrap()
    }
}

impl<'a, T> Deref for WriteGuard<'a, T>
where
    T: Serialize + 'static,
{
    type Target = T;
    fn deref(&self) -> &T {
        self.entry
            .as_ref()
            .unwrap()
            .value
            .as_ref()
            .and_then(|b| b.downcast_ref::<T>())
            .expect("wrong type in serialization")
    }
}

impl<'a, T> DerefMut for WriteGuard<'a, T>
where
    T: Serialize + 'static,
{
    fn deref_mut(&mut self) -> &mut T {
        self.entry
            .as_mut()
            .unwrap()
            .value
            .as_mut()
            .and_then(|b| b.downcast_mut::<T>())
            .expect("wrong type in serialization")
    }
}

impl<'a, T> Drop for WriteGuard<'a, T>
where
    T: Serialize + 'static,
{
    fn drop(&mut self) {
        self.commit()
    }
}
