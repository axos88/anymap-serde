#[cfg(test)]
mod tests {
    use anymap_serde::SerializableAnyMap;

    #[test]
    fn modifications_should_be_reflected_after_serialization() {
        let mut m = SerializableAnyMap::new();
        m.insert::<usize>(0usize);
        *m.get_mut::<usize>().unwrap().unwrap() = 1usize;

        let json = serde_json::to_string(&m).unwrap();
        let mut m2: SerializableAnyMap = serde_json::from_str(&json).unwrap();

        assert_eq!(m2.get_mut::<usize>().map(|v| *v.unwrap()), Some(1usize));
    }

    #[test]
    fn into_ref_on_guard_correctly_re_serializes() {
        let mut m = SerializableAnyMap::new();
        m.insert::<usize>(0usize);

        let mut guard = m.get_mut::<usize>().unwrap().unwrap();
        *guard = 1usize;
        let _ref: &usize = guard.into_ref();

        let json = serde_json::to_string(&m).unwrap();
        let mut m2: SerializableAnyMap = serde_json::from_str(&json).unwrap();

        assert_eq!(m2.get_mut::<usize>().map(|v| *v.unwrap()), Some(1usize));
    }
}
