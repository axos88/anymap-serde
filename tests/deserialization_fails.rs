#[cfg(test)]
mod tests {
    use anymap_serde::SerializableAnyMap;

    #[test]
    fn entry_should_be_removed_if_deserialization_fails() {
        let json = r#"{ "usize": "foo" }"#.to_string();
        let mut m: SerializableAnyMap = serde_json::from_str(&json).unwrap();

        assert!(m.contains::<usize>()); // Entry exists before deserialization, we can't know it's invalid yet
        assert!(m.entry::<usize>().0.into_occupied().is_none()); // Once deserialization is attempted, it will fail and we know it was invalid
        assert!(!m.contains::<usize>()); // Entry is removed after failed deserialization
    }
}
