//! End-to-end secondary indexes against a real `#[derive(SData)]` entity: the
//! query recomposes heterogeneous structs from their stable keys.
//!
//! Behind the `derive` feature, like the rest of the derive tests
//! (`cargo test --features derive`).
#![cfg(feature = "derive")]

use stratodb::{
    data::Scalar,
    index::{IndexColumn, IndexDef},
    path::SPath,
    SData,
    StratoDb,
};

#[derive(SData, Debug, PartialEq)]
struct User {
    age:  i32,
    name: String,
}

#[test]
fn find_returns_typed_entities() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("idx_typed.stratodb")).unwrap();
    let users = db.open_table("users").unwrap();

    users
        .create_index(&IndexDef::new(
            String::from("by_age"),
            String::from("users/*"),
            vec![IndexColumn::asc(SPath::parse("age").unwrap())],
            false,
        ))
        .unwrap();

    let alice = User {
        age:  30,
        name: String::from("Alice"),
    };
    let bob = User {
        age:  30,
        name: String::from("Bob"),
    };
    let carol = User {
        age:  40,
        name: String::from("Carol"),
    };

    let w = users.write().unwrap();
    w.store("users/alice", &alice).unwrap();
    w.store("users/bob", &bob).unwrap();
    w.store("users/carol", &carol).unwrap();
    w.commit().unwrap();

    let r = users.read().unwrap();

    // Two users are 30; both come back as fully recomposed structs.
    let mut at_30: Vec<User> = r.find("by_age", &[Scalar::I32(30)]).unwrap();
    at_30.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(at_30, vec![alice, bob]);

    let at_40: Vec<User> = r.find("by_age", &[Scalar::I32(40)]).unwrap();
    assert_eq!(at_40, vec![carol]);
}
