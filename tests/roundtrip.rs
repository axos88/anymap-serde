#[cfg(test)]
mod tests {
    use anymap_serde::SerializableAnyMap;

    #[test]
    fn roundtrip_basic() {
        let mut m = SerializableAnyMap::new();
        m.insert::<String>("hello".to_string());
        m.insert::<usize>(99usize);

        let json = serde_json::to_string(&m).unwrap();
        let mut m2: SerializableAnyMap = serde_json::from_str(&json).unwrap();

        assert_eq!(m2.get_mut::<usize>().map(|v| *v.unwrap()), Some(99usize));
        assert_eq!(
            m2.get_mut::<String>().map(|v| v.unwrap().clone()),
            Some("hello".to_string())
        );
    }
}
