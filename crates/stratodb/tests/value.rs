//! Integration tests for dynamic `Value` load/store round-trips.

use stratodb::{
    data::Scalar,
    index::{IndexColumn, IndexDef},
    path::SPath,
    StratoDb,
    Value,
};

use std::collections::BTreeMap;

fn mem_db() -> StratoDb {
    StratoDb::create_in_memory().expect("create db")
}

fn node(pairs: Vec<(&str, Value)>) -> Value {
    Value::Node(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[test]
fn store_then_load_roundtrips() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let value = node(vec![
        ("name", Value::Leaf(Scalar::Str("Alice".into()))),
        ("age", Value::Leaf(Scalar::U32(30))),
        (
            "scores",
            Value::List(vec![Value::Leaf(Scalar::I32(1)), Value::Leaf(Scalar::I32(2))]),
        ),
    ]);

    let w = table.write().unwrap();
    w.store_value("user", &value).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load_value("user").unwrap(), Some(value));

    // A nested leaf comes back as a leaf.
    assert_eq!(r.load_value("user/age").unwrap(), Some(Value::Leaf(Scalar::U32(30))));
    assert_eq!(
        r.load_value("user/scores").unwrap().unwrap().at(1).unwrap(),
        &Value::Leaf(Scalar::I32(2))
    );
}

#[test]
fn load_value_is_none_for_a_missing_path() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();
    let r = table.read().unwrap();

    // Empty table: even the root has no node yet.
    assert_eq!(r.load_value("").unwrap(), None);
    assert_eq!(r.load_value("nope").unwrap(), None);
}

#[test]
fn store_value_replaces_the_subtree() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let w = table.write().unwrap();
    w.put("user/old", &1u32).unwrap();
    w.store_value("user", &node(vec![("new", Value::Leaf(Scalar::Bool(true)))]))
        .unwrap();

    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(
        r.load_value("user").unwrap(),
        Some(node(vec![("new", Value::Leaf(Scalar::Bool(true)))])),
    );

    // The prior field is gone (replace, not merge).
    assert!(!r.exists("user/old").unwrap());
}

#[test]
fn store_value_materializes_empty_containers() {
    let db = mem_db();
    let table = db.open_table("data").unwrap();

    let value = node(vec![
        ("list", Value::List(vec![])),
        ("obj", Value::Node(BTreeMap::new())),
    ]);

    let w = table.write().unwrap();
    w.store_value("root", &value).unwrap();
    w.commit().unwrap();

    let r = table.read().unwrap();
    assert_eq!(r.load_value("root").unwrap(), Some(value));
}

#[test]
fn get_value_navigates_and_clones() {
    let value = node(vec![
        ("user", node(vec![("age", Value::Leaf(Scalar::U32(30)))])),
        ("tags", Value::List(vec![Value::Leaf(Scalar::Str("x".into()))])),
    ]);

    assert_eq!(value.get_value("user/age"), Some(Value::Leaf(Scalar::U32(30))));
    assert_eq!(value.get_value("tags[0]"), Some(Value::Leaf(Scalar::Str("x".into()))));
    assert_eq!(
        value.get_value("user"),
        Some(node(vec![("age", Value::Leaf(Scalar::U32(30)))]))
    );

    // The root returns the whole value; missing or out-of-range paths return None.
    assert_eq!(value.get_value(""), Some(value.clone()));
    assert_eq!(value.get_value("user/ghost"), None);
    assert_eq!(value.get_value("tags[5]"), None);
}

#[test]
fn set_value_creates_paths_but_never_destroys_leaves() {
    let mut value = node(vec![("a", Value::Leaf(Scalar::I32(1)))]);

    // Creates a fresh nested path.
    assert!(value.set_value("x/y", Value::Leaf(Scalar::I32(9))));
    assert_eq!(value.get_value("x/y"), Some(Value::Leaf(Scalar::I32(9))));

    // Replaces the value at the destination itself.
    assert!(value.set_value("a", Value::Leaf(Scalar::I32(2))));
    assert_eq!(value.get_value("a"), Some(Value::Leaf(Scalar::I32(2))));

    // Refuses to traverse through an existing leaf, changing nothing.
    let before = value.clone();
    assert!(!value.set_value("a/b", Value::Leaf(Scalar::I32(3))));
    assert_eq!(value, before);

    // A list grows only at its end.
    let mut list = Value::new_empty_list();
    assert!(list.set_value("[0]", Value::Leaf(Scalar::I32(10))));
    assert!(!list.set_value("[5]", Value::Leaf(Scalar::I32(50))));
    assert_eq!(list, Value::List(vec![Value::Leaf(Scalar::I32(10))]));
}

#[test]
fn store_value_maintains_indexes() {
    let db = mem_db();
    let users = db.open_table("users").unwrap();
    users
        .create_index(&IndexDef::new(
            String::from("by_age"),
            String::from("users/*"),
            vec![IndexColumn::asc(SPath::parse("age").unwrap())],
            false,
        ))
        .unwrap();

    let count_at = |age: i32| {
        users
            .read()
            .unwrap()
            .find::<BTreeMap<String, i32>>("by_age", &[Scalar::I32(age)])
            .unwrap()
            .len()
    };

    // A single dynamic `store_value` of several entities must index each child,
    // exactly as the typed `store` does — the write goes through `WriteCursor`.
    let everyone = node(vec![
        ("alice", node(vec![("age", Value::Leaf(Scalar::I32(30)))])),
        ("bob", node(vec![("age", Value::Leaf(Scalar::I32(30)))])),
        ("carol", node(vec![("age", Value::Leaf(Scalar::I32(40)))])),
    ]);

    let w = users.write().unwrap();
    w.store_value("users", &everyone).unwrap();
    w.commit().unwrap();

    assert_eq!(count_at(30), 2);
    assert_eq!(count_at(40), 1);

    // Replacing the subtree re-maintains the index: the prior entries are deleted
    // before the new ones are inserted (delete-then-insert bracketing).
    let w = users.write().unwrap();
    w.store_value(
        "users",
        &node(vec![("alice", node(vec![("age", Value::Leaf(Scalar::I32(40)))]))]),
    )
    .unwrap();
    w.commit().unwrap();

    assert_eq!(count_at(30), 0);
    assert_eq!(count_at(40), 1);
}
