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
    SdbError,
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

#[derive(SData, Debug, PartialEq)]
#[sdata(index(name = "people_by_age", columns(age)))]
#[sdata(index(name = "people_by_name", columns(name), unique))]
#[sdata(index(name = "people_by_age_name", columns(age, name desc)))]
struct Person {
    age:  i32,
    name: String,
}

#[test]
fn derived_index_attributes_declare_and_create() {
    let dir = tempfile::tempdir().unwrap();
    let db = StratoDb::create(dir.path().join("idx_derived.stratodb")).unwrap();
    let people = db.open_table("people").unwrap();

    // One call registers every index the type declares, scoped to the pattern.
    people.create_indexes::<Person>("people/*").unwrap();

    // The declared schemas were registered faithfully (name, columns, direction,
    // uniqueness — and the pattern from the call).
    assert_eq!(
        people.index_def("people_by_age").unwrap(),
        Some(IndexDef::new(
            String::from("people_by_age"),
            String::from("people/*"),
            vec![IndexColumn::asc(SPath::parse("age").unwrap())],
            false,
        ))
    );

    assert!(people.index_def("people_by_name").unwrap().unwrap().unique());
    assert_eq!(
        people.index_def("people_by_age_name").unwrap().unwrap().columns(),
        &vec![
            IndexColumn::asc(SPath::parse("age").unwrap()),
            IndexColumn::desc(SPath::parse("name").unwrap()),
        ]
    );

    // Maintenance and typed queries work through the derived indexes.
    let w = people.write().unwrap();
    w.store(
        "people/p1",
        &Person {
            age:  30,
            name: String::from("Alice"),
        },
    )
    .unwrap();
    w.store(
        "people/p2",
        &Person {
            age:  30,
            name: String::from("Bob"),
        },
    )
    .unwrap();
    w.commit().unwrap();

    let mut at_30: Vec<Person> = people
        .read()
        .unwrap()
        .find("people_by_age", &[Scalar::I32(30)])
        .unwrap();
    at_30.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(
        at_30,
        vec![
            Person {
                age:  30,
                name: String::from("Alice"),
            },
            Person {
                age:  30,
                name: String::from("Bob"),
            },
        ]
    );

    // The declared unique index `people_by_name` is enforced.
    let w = people.write().unwrap();
    let err = w
        .store(
            "people/p3",
            &Person {
                age:  99,
                name: String::from("Alice"),
            },
        )
        .unwrap_err();

    assert!(matches!(err, SdbError::UniqueViolation { .. }), "got {err:?}");
}
