use serde::{Deserialize, Serialize};
use anymap_serde::SerializableAnyMap;


#[derive(Serialize, Deserialize)]
struct Foo {
  a: i32,
  b: String,
}

#[derive(Serialize, Deserialize)]
struct Bar {
  x: f64,
  y: Vec<u8>,
}

fn main() {
  let mut m = SerializableAnyMap::new();
  m.insert("hello".to_string());
  m.insert(Foo { a: 10, b: "world".to_string() });
  m.insert( Bar { x: 3.14, y: vec![1, 2, 3] } );
  m.insert(42usize);
  m.insert(());

  let json = serde_json::to_string_pretty(&m).unwrap();
  println!("{}", json);

  let mut m2: SerializableAnyMap = serde_json::from_str(&json).unwrap();

  // Nothing cached initially
  assert!(m2.try_get::<String>().is_none());

  // Lazy deserialize via get_mut
  assert_eq!(m2.get::<String>().map(|s| s.unwrap().clone()), Some("hello".to_string()));

  m2.entry::<usize>().and_modify(|mut v| *v += 1);

  assert_eq!(m2.try_get::<usize>(), Some(&43usize));
}