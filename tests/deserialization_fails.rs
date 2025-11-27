#[cfg(test)]
mod tests {
  use std::fmt::Debug;
  use serde_value::Unexpected::Str;
  use anymap_serde::SerializableAnyMap;
  #[derive(Debug)]
  struct EqDebug<T>(T);

  impl<T: Debug> PartialEq for EqDebug<T> {
    fn eq(&self, other: &Self) -> bool {
      format!("{:?}", self.0) == format!("{:?}", other.0)
    }
  }

  impl<T: Debug> Eq for EqDebug<T> {}


  #[test]
  fn entry_should_be_removed_if_deserialization_fails() {
    let json = r#"{ "usize": "foo" }"#.to_string();
    let mut m: SerializableAnyMap = serde_json::from_str(&json).unwrap();

    assert!(m.contains::<usize>()); // Entry exists before deserialization, we can't know it's invalid yet
    assert!(m.entry::<usize>().into_occupied().is_none()); // Once deserialization is attempted, it will fail and we know it was invalid
    assert!(!m.contains::<usize>()); // Entry is removed after failed deserialization
  }
}