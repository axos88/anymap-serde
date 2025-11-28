# anymap-serde

A serializable type-indexed map for Rust — a Serde-backed version of `anymap`

## 🚀 What is this & why use it

Typical usage of a map in Rust involves specifying key and value types; e.g. HashMap<String, String> or HashMap<usize, MyType>. But sometimes you want a type-indexed map: one where you store exactly one value per type, and retrieve by specifying the type — not by a string key or other runtime key. 

This pattern is popularized by the anymap crate. 

However — the classical type-indexed map does not support (de)serialization out-of-the-box, because it uses runtime TypeId + Any + downcasting, which don’t map directly to a serializable representation.

The anymap-serde crate fills exactly this gap: it provides an AnyMap-style container whose stored values are automatically converted to a serializable form (using serde), making it easy to persist, send over network, or otherwise (de)serialize your heterogeneous data bag.

## Use it when you want:
  - A flexible, type-safe heterogeneous container: store one value per type, keyed by stable type identifier.
  - The ability to serialize/deserialize the contents as a whole — e.g. to JSON, TOML, binary formats — via serde.
  - No prior knowledge or registry of allowable types is required like in ad-hoc “map-of-enum-or-trait-object” hacks.


## 📦 How it works (conceptually)

Internally, anymap-serde stores each value as a `serde_value::Value` (or otherwise serde-serializable representation), along with an `Option<Box<dyn Any>>`. This allows the entire map to implement Serialize and Deserialize using serde, because serde_value::Value is itself (de)serializable.

When you insert a value of some type T: Serialize + 'static, the crate serializes it to an intermediate serde_value::Value , stores it under the “type key” for T. When retrieving (or deserializing), the crate attempts to reconstruct the original type(s) as needed, but also writes the deserialized value back into the internal cache for future accesses.


This approach bypasses the limitations many have pointed out when trying to serialize a classic AnyMap — e.g., the problem that TypeId is not stable across compilations / Rust versions, and cannot easily be serialized. 

## ✅ Basic Usage
```
    use anymap_serde::SerializableAnyMap;
    use serde::{Serialize, Deserialize};
    
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Config {
        name: String,
        verbose: bool,
    }
    
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Stats {
        hits: u64,
        misses: u64,
    }
    
    fn main() -> Result<(), Box<dyn std::error::Error>> {
        let mut map = SerializableAnyMap::new();
    
        map.insert(Config { name: "app".into(), verbose: true });
        map.insert(Stats { hits: 10, misses: 2 });
    
        // Serialize to JSON
        let s = serde_json::to_string(&map)?;
        println!("Serialized: {}", s);
    
        // Deserialize back
        let deserialized: SerializableAnyMap = serde_json::from_str(&s)?;

        let cfg = deserialized.get::<Config>().unwrap();
        let stats = deserialized.get::<Stats>().unwrap();
    
        assert_eq!(cfg.name, "app");
        assert_eq!(stats.hits, 10);
    
        Ok(())
    }
```

Example based on the crate’s design: store different types, serialize the bag, then later recover.

You can also store standard Rust types, nested data structures, or anything that implements Serialize + Deserialize + 'static.

⚠️ Limitations & Notes

  - All stored types must implement Serialize + Deserialize (from serde).
  - Because values are stored in a serialized / type-erased form internally — the crate cannot preserve e.g. type identity stubbornly across profound changes. For example, if you rename a type or change its structure between serialization and deserialization, deserialization may fail or misinterpret the data.
  - Since there is no type registry, values are deserialized on access only - meaning deserialization errors may not occur immediately when deserializing the container, only at access time, when the code path has access to the `Deserialize` implementation of the requested type.
  - Each value is re-serialized and deserialized on first access / each modification, because the container cannot rely on knowledge on how to serialize each type erased `dyn Any` at the time the container is serialized. This is a significant runtime cost that may not be acceptable in some use cases and hot paths.
  - This is not a reflection or “downcasting” mechanism for arbitrary dyn Any objects: serialization strength comes from requiring Serialize + Deserialize.


## 📚 Comparison with related crates

  - The original anymap crate provides a type-indexed map but without serialization. API parity for a drop-in replacement was desired, but dropped, because of the need to be able to extract lazy deserialization errors. Still the API is supposed to be similar enough so that migration is easy.

  - Other crates like serde_anymap provide similar ideas: “map-like data structures that can store values of different types and be (de)serializable”, but it requires a registry.

  - anymap-serde aims for minimal overhead and “just works” when your types are Serialize + Deserialize, with a convenient API inspired by the original anymap.


## 🧩 When (and when not) to use this

### Use this crate if you want:

  - A simple way to build a heterogeneous configuration bag, context store, plugin registry, or similar — where each type is unique and you don’t want ad-hoc runtime keys, and can pay the runtime cost of serialization/deserialization after each modification.

  - To persist / transmit that bag via serde (JSON, YAML, binary, etc.) without writing custom serialization logic for each combination manually.


### Avoid this crate if you need:

  - Maximum performance/zero-cost for hot loops (because serialization/deserialization costs are significant if you modify contained elements a lot).

  - “Any” dynamic dispatch with type discovery — this crate expects concrete, Serialize + Deserialize types, and does not work as a generic “downcast arbitrarily” container.

## 📄 Contributing

Contributions, bug reports, feature requests are welcome (e.g. via GitHub issues / pull requests).

Please install push hooks via ./setup.sh to ensure code formatting, linting and green test suite.
